//! View types crossing the server-function boundary. Client-safe (no `apsis-common` /
//! NATS deps — those would not compile to wasm); the server converts from `apsis-common`.

use serde::{Deserialize, Serialize};

/// One row in the file table — a projection of a KV `StateEntry` + the ignore marker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRow {
    pub path: String,
    /// `Done` / `Pending` / `InProgress` / `Failed` / `Unknown` (stringified server-side).
    pub status: String,
    pub version: String,
    /// The positive skip reason (why this file was left alone), if any — spec 005 FR-011.
    pub decision: Option<String>,
    pub ignored: bool,
    /// Set while `InProgress` (for cancel + progress correlation).
    pub job_id: Option<String>,
}

/// A live progress sample for one job — the client-safe mirror of
/// `apsis_common::control::ProgressEvent`, deserialized from the `/progress` SSE frames.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub job_id: String,
    pub speed: f64,
    pub eta_s: u64,
    pub out_time_s: f64,
}

/// The per-file detail view (US2): everything the KV `StateEntry` holds, so the operator can
/// see *why* a file is in its current state — the decision, the last error, retry count.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDetail {
    pub path: String,
    pub status: String,
    pub version: String,
    pub job_id: Option<String>,
    pub attempts: u32,
    /// RFC 3339 timestamp of the last state write.
    pub updated_at: String,
    pub last_error: Option<String>,
    pub decision: Option<String>,
    pub ignored: bool,
}
