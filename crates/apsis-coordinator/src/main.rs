//! apsis-coordinator — scan, reconcile, enqueue (spec 002 US2).
//!
//! Config from `APSIS_SCHEDULER_CONFIG` (default `scheduler.toml`); NATS from
//! `NATS_URL`. Each `scan_interval`, walk every library, and for each stable
//! video decide via the engine and enqueue only drift (the reconcile loop).
//! inotify is a future add; the periodic walk guarantees convergence.

mod discover;
mod profile_match;
mod reconcile;

use std::collections::HashSet;
use std::path::Path;
use std::process::ExitCode;

use apsis_common::{
    ConsumerTuning, KvStateStore, NatsPublisher, Outcome, SchedulerConfig, TranscodeResult,
    connect, ensure_topology, load_scheduler, version_token,
};
use futures::StreamExt;

use std::time::Duration;

use crate::discover::{discover_videos, sweep_temps};
use crate::profile_match::match_library;
use crate::reconcile::{FfprobeProber, ReconcileOutcome, Reconciler};

/// Orphan temps older than this are crash leftovers, safe to sweep (well beyond
/// any plausible single-file transcode).
const TEMP_ORPHAN_AGE: Duration = Duration::from_hours(6);

type Fatal = Box<dyn std::error::Error + Send + Sync>;
type Coordinator = Reconciler<KvStateStore, NatsPublisher, FfprobeProber>;

#[tokio::main]
async fn main() -> ExitCode {
    match serve().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("apsis-coordinator: fatal: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve() -> Result<(), Fatal> {
    let cfg_path =
        std::env::var("APSIS_SCHEDULER_CONFIG").unwrap_or_else(|_| "scheduler.toml".to_string());
    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());
    let ffprobe = std::env::var("APSIS_FFPROBE").unwrap_or_else(|_| "ffprobe".to_string());

    apsis_common::init_tracing();
    install_metrics("0.0.0.0:9100")?;

    let cfg = load_scheduler(Path::new(&cfg_path))?;
    let (client, ctx) = connect(&nats_url).await?;
    let tuning = ConsumerTuning::for_concurrency(1);
    let kv = ensure_topology(&ctx, &tuning).await?;

    // Consume the worker's result feed for central metrics + a completion log
    // (T023). The worker owns the terminal KV write — it records `Failed@version`
    // itself on dead-letter, so this is the observability half, not a 2nd writer.
    tokio::spawn(consume_results(client));

    // Startup: sweep crash-orphaned temps (FR-008) before the first reconcile.
    for lib in &cfg.libraries {
        let swept = sweep_temps(Path::new(&lib.path), TEMP_ORPHAN_AGE);
        if swept > 0 {
            tracing::info!(count = swept, path = %lib.path, "swept crash-orphaned temps");
        }
    }

    let reconciler = Reconciler {
        store: KvStateStore::new(kv),
        publisher: NatsPublisher::new(ctx),
        prober: FfprobeProber {
            ffprobe: ffprobe.into(),
        },
    };

    loop {
        reconcile_all(&reconciler, &cfg).await;
        tokio::time::sleep(cfg.reconcile.scan_interval).await;
    }
}

/// One reconcile pass over every library. Files under overlapping libraries are
/// deduplicated and matched to their longest-prefix library for the profile.
async fn reconcile_all(reconciler: &Coordinator, cfg: &SchedulerConfig) {
    let started = std::time::Instant::now();
    let mut enqueued: u64 = 0;
    let mut seen = HashSet::new();
    for lib in &cfg.libraries {
        for path in discover_videos(
            Path::new(&lib.path),
            &lib.extensions,
            cfg.reconcile.debounce,
        ) {
            if !seen.insert(path.clone()) {
                continue;
            }
            // The path is the KV key and Job.path; a lossy conversion would mint a
            // key the worker can't open. Skip non-UTF8 paths loudly instead.
            let Some(file) = path.to_str() else {
                tracing::warn!(path = %path.display(), "skipping non-UTF8 path");
                continue;
            };
            let Some(matched) = match_library(&cfg.libraries, file) else {
                continue;
            };
            let Some(profile) = cfg.profiles.get(&matched.profile) else {
                continue;
            };
            let Ok(ver) = version_token(&path) else {
                continue;
            };
            match reconciler
                .reconcile_file(file, &ver, matched, profile)
                .await
            {
                Ok(ReconcileOutcome::Enqueued) => enqueued += 1,
                Ok(_) => {}
                Err(e) => tracing::warn!(file, error = %e, "reconcile failed"),
            }
        }
    }
    metrics::histogram!("apsis_reconcile_seconds").record(started.elapsed().as_secs_f64());
    metrics::counter!("apsis_reconcile_enqueued_total").increment(enqueued);
}

/// Subscribe to the worker's `jobs.result` core subject and fold each result into
/// coordinator-side metrics + a structured completion log — "history without a
/// separate DB" (contract §fold-result). Terminal KV state is the worker's to
/// write; this never writes KV, so there is no second terminal writer.
///
/// A subscription/decode error is logged, not fatal: losing the result feed costs
/// observability, never correctness (the worker's KV write stands).
async fn consume_results(client: async_nats::Client) {
    let mut sub = match client.subscribe(apsis_common::nats::SUBJECT_RESULT).await {
        Ok(sub) => sub,
        Err(e) => {
            tracing::error!(error = %e, "could not subscribe to jobs.result; no completion feed");
            return;
        }
    };
    while let Some(msg) = sub.next().await {
        let Ok(result) = serde_json::from_slice::<TranscodeResult>(&msg.payload) else {
            tracing::warn!("undecodable transcode result dropped");
            continue;
        };
        let outcome = match result.outcome {
            Outcome::Done => "done",
            Outcome::Failed => "failed",
        };
        metrics::counter!("apsis_results_total", "outcome" => outcome).increment(1);
        tracing::info!(
            job_id = %result.job_id,
            path = %result.path,
            outcome,
            used_fallback = result.used_fallback,
            in_bytes = result.input_bytes,
            out_bytes = result.output_bytes,
            secs = result.duration_secs,
            "transcode result",
        );
    }
    tracing::warn!("jobs.result subscription ended");
}

/// Install the Prometheus exporter (scrape endpoint at `APSIS_METRICS_ADDR`).
fn install_metrics(default_addr: &str) -> Result<(), Fatal> {
    let addr: std::net::SocketAddr = std::env::var("APSIS_METRICS_ADDR")
        .unwrap_or_else(|_| default_addr.to_string())
        .parse()?;
    metrics_exporter_prometheus::PrometheusBuilder::new()
        .with_http_listener(addr)
        .install()?;
    Ok(())
}
