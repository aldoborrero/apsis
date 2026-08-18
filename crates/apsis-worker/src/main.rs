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
    match serve().await {
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
