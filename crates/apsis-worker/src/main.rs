//! apsis-worker — pull a job, run ffmpeg, verify, atomically replace (spec 002 US1).
//!
//! Config from `APSIS_WORKER_CONFIG` (default `worker.toml`); NATS from `NATS_URL`
//! (default the process-compose local server). Sequential (AMD-VCN concurrency 1);
//! `max_ack_pending` bounds in-flight jobs broker-side.

mod fallback;
mod replace;
mod run;
mod verify;
mod worker;

use std::path::Path;
use std::process::ExitCode;

use apsis_common::{KvStateStore, connect, ensure_topology, load_worker};

use crate::worker::Worker;

type Fatal = Box<dyn std::error::Error + Send + Sync>;

#[tokio::main]
async fn main() -> ExitCode {
    // Box the whole worker future: the real weight is `worker::process`, which holds
    // a `Job` (embedding the full `FilePlan` + `Profile`, grown by spec 004) by value
    // across its awaits in the pull loop. Boxing at the root heap-allocates that state
    // transitively (clippy `large_futures`); the principled fix is to shrink `Job`.
    match Box::pin(serve()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("apsis-worker: fatal: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve() -> Result<(), Fatal> {
    let cfg_path =
        std::env::var("APSIS_WORKER_CONFIG").unwrap_or_else(|_| "worker.toml".to_string());
    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());

    apsis_common::init_tracing();
    install_metrics("0.0.0.0:9101")?;

    let cfg = load_worker(Path::new(&cfg_path))?;
    let (client, ctx) = connect(&nats_url).await?;
    let tuning = cfg.consumer.tuning(cfg.concurrency);
    let kv = ensure_topology(&ctx, &tuning).await?;
    let worker = Worker::new(client, ctx, KvStateStore::new(kv), &cfg);
    worker.run(&tuning).await
}

/// Transcodes span seconds (a small clip) to hours (a 4K feature); bucket to 4h so
/// the tail is visible and the histogram aggregates across workers at the hub.
const TRANSCODE_BUCKETS: &[f64] = &[
    5.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1200.0, 1800.0, 3600.0, 7200.0, 14400.0,
];

/// Install the Prometheus exporter (scrape endpoint at `APSIS_METRICS_ADDR`) and
/// register `# HELP`/`# TYPE` descriptions for every worker metric.
///
/// `apsis_transcode_seconds` gets explicit histogram buckets so it aggregates across
/// workers at the hub (the exporter's default would emit a per-instance summary).
fn install_metrics(default_addr: &str) -> Result<(), Fatal> {
    use metrics::Unit;
    use metrics_exporter_prometheus::Matcher;
    let addr: std::net::SocketAddr = std::env::var("APSIS_METRICS_ADDR")
        .unwrap_or_else(|_| default_addr.to_string())
        .parse()?;
    metrics_exporter_prometheus::PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("apsis_transcode_seconds".to_string()),
            TRANSCODE_BUCKETS,
        )?
        .with_http_listener(addr)
        .install()?;

    metrics::describe_histogram!(
        "apsis_transcode_seconds",
        Unit::Seconds,
        "Wall time of one transcode (claim to atomic replace)"
    );
    metrics::describe_counter!(
        "apsis_jobs_total",
        Unit::Count,
        "Transcode jobs finished, by outcome (done/failed)"
    );
    metrics::describe_counter!(
        "apsis_used_fallback_total",
        Unit::Count,
        "Transcodes that fell back from the primary backend (VAAPI) to CPU"
    );
    metrics::describe_counter!(
        "apsis_bytes_saved_total",
        Unit::Bytes,
        "Cumulative bytes saved (input minus output) across successful transcodes"
    );
    metrics::describe_counter!(
        "apsis_verify_failures_total",
        Unit::Count,
        "Outputs rejected by post-transcode verification (bad duration / bloated)"
    );
    Ok(())
}
