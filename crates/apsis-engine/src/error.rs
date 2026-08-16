//! Engine error type. All fallible entry points return `Result<_, EngineError>`
//! — the engine never panics on bad input (FR-010).

use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EngineError {
    #[error("failed to parse ffprobe JSON or profile: {0}")]
    ParseJson(#[from] serde_json::Error),

    #[cfg(feature = "probe-exec")]
    #[error("ffprobe I/O error: {0}")]
    Ffprobe(#[from] std::io::Error),

    #[cfg(feature = "probe-exec")]
    #[error("ffprobe exited unsuccessfully (code {code:?}): {stderr}")]
    FfprobeStatus { code: Option<i32>, stderr: String },
}
