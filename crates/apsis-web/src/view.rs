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
