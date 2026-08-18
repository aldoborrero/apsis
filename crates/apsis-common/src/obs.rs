//! Observability init shared by both daemons.
//!
//! Metrics are wired per-binary (`metrics-exporter-prometheus`); this is the
//! logging half: a `tracing` subscriber so the per-job / per-file spans and events
//! reach stderr with structured fields.

use tracing_subscriber::{EnvFilter, fmt};

/// Install a `tracing` subscriber: `RUST_LOG` (default `info`) selects levels,
/// formatted lines go to stderr. ANSI colour is off — these daemons log to
/// journald/files where escape codes are noise (and it keeps fields greppable).
/// Idempotent — a second call, or a test that already installed one, is a silent
/// no-op rather than a panic.
pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
}
