//! Engine error type. All fallible entry points return `Result<_, EngineError>`
//! — the engine never panics on bad input (FR-010).

use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EngineError {
    #[error("failed to parse ffprobe JSON or profile: {0}")]
    ParseJson(#[from] serde_json::Error),

    /// A `FilePlan` references a stream absent from its own `source_probe` (or a
    /// track plan lacks `source_index`). Mirrors the Python engine raising
    /// (`AssertionError`/`ValueError`) rather than silently mis-mapping a stream.
    #[error("plan/probe mismatch: {0}")]
    PlanProbeMismatch(String),

    /// A profile option is accepted by the schema but not yet materializable by
    /// the backend (e.g. `quality.mode = "vmaf"` / `AutoCRF` — deferred, spec 004 R5).
    #[error("unsupported profile option: {0}")]
    Unsupported(String),

    /// A profile-rule CEL expression failed to compile (syntax error) or to
    /// evaluate/convert against the file context, or a rule's `set` targets an
    /// unknown/invalid field (US2 override resolution).
    #[error("profile-rule override error: {0}")]
    Override(String),

    #[cfg(feature = "probe-exec")]
    #[error("ffprobe I/O error: {0}")]
    Ffprobe(#[from] std::io::Error),

    #[cfg(feature = "probe-exec")]
    #[error("ffprobe exited unsuccessfully (code {code:?}): {stderr}")]
    FfprobeStatus { code: Option<i32>, stderr: String },
}
