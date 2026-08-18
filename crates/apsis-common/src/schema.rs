//! Wire schemas shared by the coordinator and worker (JSON on NATS).
//!
//! See `specs/002-single-node-transcode/data-model.md` and
//! `contracts/nats-protocol.md` — these types ARE the contract. Timestamps are
//! RFC3339 UTC; ids are ULIDs (sortable).
//!
//! These top-level wire structs deliberately do **not** use `deny_unknown_fields`,
//! so an additive field on `Job`/`StateEntry`/`TranscodeResult` itself survives a
//! rolling upgrade (old consumer ignores it).
//!
//! **Caveat — this only holds at the top level.** The nested `FilePlan` and
//! `Profile` (and their sub-types) ARE strict (`deny_unknown_fields`, for
//! oracle-parity), so an additive field *inside* the plan or profile in spec 003
//! WILL break an old consumer. Additive changes there require a coordinated worker
//! upgrade, or a permissive wire-copy of those types distinct from the strict
//! fixture-facing ones. The forward-compat guarantee is one level deep.

use apsis_engine::{FilePlan, Profile};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use ulid::Ulid;

/// Per-file lifecycle status (KV `transcode_state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Unknown,
    Pending,
    InProgress,
    Done,
    Failed,
}

/// Terminal outcome of a transcode job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Done,
    Failed,
}

/// One file's transcode unit — published to `jobs.transcode.*`.
///
/// Carries the abstract [`FilePlan`] (engine output), not an ffmpeg command: the
/// worker materializes the command for its own backend. Media stays on the shared
/// filesystem; the job is metadata only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub id: Ulid,
    /// Coordinator-space absolute path (worker applies its `path_map`).
    pub path: String,
    /// Change token `"mtime:size"` this job was planned for.
    pub version: String,
    /// Library's profile name (logging / future routing).
    pub profile: String,
    pub plan: FilePlan,
    /// The full profile — the worker's ffmpeg command builder needs quality,
    /// bitrate and encoder, which the plan doesn't carry. (A future refinement
    /// could fold those into the plan and drop this.)
    pub profile_config: Profile,
    #[serde(with = "time::serde::rfc3339")]
    pub enqueued_at: OffsetDateTime,
}

/// KV `transcode_state` value, keyed by the file's coordinator-space path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateEntry {
    pub status: Status,
    /// The `"mtime:size"` this state refers to; a newer token supersedes it.
    pub version: String,
    pub job_id: Option<Ulid>,
    /// Reserved. The live retry count that drives dead-lettering (FR-009) is the
    /// `JetStream` consumer's `delivered` count, not this field — it is only a
    /// coarse hint written on state transitions, not read by the retry logic.
    pub attempts: u32,
    pub used_fallback: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub last_error: Option<String>,
}

/// Worker → `jobs.result` (core publish). The coordinator folds it into metrics +
/// a completion log; the worker owns the terminal KV write, so this is the
/// observability channel, not a second `transcode_state` writer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscodeResult {
    pub job_id: Ulid,
    pub path: String,
    pub version: String,
    pub outcome: Outcome,
    pub used_fallback: bool,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub duration_secs: f64,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_entry_round_trips() {
        let e = StateEntry {
            status: Status::InProgress,
            version: "1723800000:1048576000".into(),
            job_id: Some(Ulid::from_parts(1, 2)),
            attempts: 1,
            used_fallback: false,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            last_error: None,
        };
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<StateEntry>(&json).unwrap(), e);
        // status serializes snake_case per the contract.
        assert!(json.contains("\"in_progress\""));
    }

    #[test]
    fn result_outcome_is_snake_case() {
        let r = TranscodeResult {
            job_id: Ulid::from_parts(0, 0),
            path: "/x.mkv".into(),
            version: "0:0".into(),
            outcome: Outcome::Done,
            used_fallback: true,
            input_bytes: 100,
            output_bytes: 40,
            duration_secs: 1.5,
            error: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"outcome\":\"done\""));
        assert_eq!(serde_json::from_str::<TranscodeResult>(&json).unwrap(), r);
    }
}
