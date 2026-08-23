//! Operator control & introspection wire protocol (spec 005).
//!
//! All control travels over NATS subjects (see `contracts/control-subjects.md`): operators
//! publish/request; the worker and coordinator are the sole executors. These are the message
//! schemas + subject constants — pure data, no I/O.

use serde::{Deserialize, Serialize};

/// Publish a [`PauseIntent`] here; the coordinator persists it.
pub const SUBJECT_CONTROL_PAUSE: &str = "apsis.control.pause";
/// Request/reply [`CancelRequest`] → [`CancelReply`]; the worker running the job acts.
pub const SUBJECT_CONTROL_CANCEL: &str = "apsis.control.cancel";
/// Request/reply [`StateControlRequest`] → [`StateControlReply`]; the coordinator executes.
pub const SUBJECT_CONTROL_STATE: &str = "apsis.control.state";

/// Ephemeral per-job progress subject (`apsis.progress.<job_id>`).
#[must_use]
pub fn subject_progress(job_id: &str) -> String {
    format!("apsis.progress.{job_id}")
}

// --- Pause (US1) ---

/// Which workers a pause applies to. Serializes as `"global"` or `{"worker": "<id>"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseScope {
    Global,
    Worker(String),
}

/// Soft = finish in-flight, withhold new claims. Hard = also cancel the in-flight transcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseMode {
    Soft,
    Hard,
}

/// Set or clear a pause for a scope. `set = false` resumes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PauseIntent {
    pub scope: PauseScope,
    pub mode: PauseMode,
    pub set: bool,
}

/// The effective pause set, persisted by the coordinator in the `KV_CONTROL_PAUSE` key and
/// watched read-only by workers. Defaults to "not paused" — losing it is safe (Principle I).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PauseState {
    /// A global pause applying to every worker, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global: Option<PauseMode>,
    /// Per-worker pauses (drain one host).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub workers: std::collections::BTreeMap<String, PauseMode>,
}

impl PauseState {
    /// Fold an intent into the set.
    pub fn apply(&mut self, intent: &PauseIntent) {
        match (&intent.scope, intent.set) {
            (PauseScope::Global, true) => self.global = Some(intent.mode),
            (PauseScope::Global, false) => self.global = None,
            (PauseScope::Worker(id), true) => {
                self.workers.insert(id.clone(), intent.mode);
            }
            (PauseScope::Worker(id), false) => {
                self.workers.remove(id);
            }
        }
    }

    /// The effective pause for a worker — the stronger (Hard > Soft) of the global and the
    /// worker-specific pause, or `None` if neither applies.
    #[must_use]
    pub fn effective(&self, worker_id: &str) -> Option<PauseMode> {
        let w = self.workers.get(worker_id).copied();
        match (self.global, w) {
            (Some(PauseMode::Hard), _) | (_, Some(PauseMode::Hard)) => Some(PauseMode::Hard),
            (Some(PauseMode::Soft), _) | (_, Some(PauseMode::Soft)) => Some(PauseMode::Soft),
            (None, None) => None,
        }
    }
}

// --- Cancel (US2) ---

/// What becomes of a file whose transcode was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    /// Clear state → the next reconcile re-plans it.
    Defer,
    /// Write the recoverable on-disk ignore marker → not retried until the file changes.
    Ignore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelRequest {
    pub job_id: String,
    pub disposition: Disposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelOutcome {
    /// The worker was running it and aborted it (source left byte-identical).
    Cancelled,
    /// No worker is running that job.
    NotRunning,
    /// The transcode had already committed (past atomic replace).
    AlreadyDone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelReply {
    pub outcome: CancelOutcome,
}

// --- State control (US3) ---

/// A manual state transition, executed by the coordinator serialized with reconcile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateOp {
    /// Clear the entry → the change-gate misses → re-plan.
    Requeue,
    /// Clear a `Failed` entry → re-try next reconcile.
    Retry,
    /// Record `Done@version` → never transcoded until the file changes.
    MarkDone,
    /// Enqueue a transcode via the plan-level `force` override, despite `should_skip`.
    Force,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateControlRequest {
    pub path: String,
    pub op: StateOp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateOutcome {
    Applied,
    NotFound,
    Noop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateControlReply {
    pub outcome: StateOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// --- Progress (US2/US3) ---

/// Live transcode progress. `speed`/`eta`/`out_time` from ffmpeg `-progress`; no `percent`
/// (the source `duration` is not yet carried by the probe).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub job_id: String,
    pub speed: f64,
    pub eta_s: u64,
    pub out_time_s: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round<T>(v: &T) -> T
    where
        T: Serialize + serde::de::DeserializeOwned,
    {
        serde_json::from_str(&serde_json::to_string(v).unwrap()).unwrap()
    }

    #[test]
    fn pause_scope_wire_shapes() {
        // `global` is a bare string; a worker scope is `{"worker": "<id>"}`.
        assert_eq!(
            serde_json::to_string(&PauseScope::Global).unwrap(),
            "\"global\""
        );
        assert_eq!(
            serde_json::to_string(&PauseScope::Worker("rhea".into())).unwrap(),
            "{\"worker\":\"rhea\"}"
        );
        assert_eq!(
            round(&PauseScope::Worker("io".into())),
            PauseScope::Worker("io".into())
        );
    }

    #[test]
    fn messages_round_trip() {
        let p = PauseIntent {
            scope: PauseScope::Global,
            mode: PauseMode::Hard,
            set: true,
        };
        assert_eq!(round(&p), p);

        let c = CancelRequest {
            job_id: "01J".into(),
            disposition: Disposition::Ignore,
        };
        assert_eq!(round(&c), c);
        assert_eq!(
            round(&CancelReply {
                outcome: CancelOutcome::NotRunning
            })
            .outcome,
            CancelOutcome::NotRunning
        );

        let s = StateControlRequest {
            path: "/hdd/tv/x.mkv".into(),
            op: StateOp::Force,
        };
        assert_eq!(round(&s), s);

        let ev = ProgressEvent {
            job_id: "01J".into(),
            speed: 3.2,
            eta_s: 812,
            out_time_s: 415.0,
        };
        assert_eq!(round(&ev), ev);
    }

    #[test]
    fn state_reply_omits_absent_detail() {
        let r = StateControlReply {
            outcome: StateOutcome::Applied,
            detail: None,
        };
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            "{\"outcome\":\"applied\"}"
        );
    }

    #[test]
    fn progress_subject() {
        assert_eq!(subject_progress("01JABC"), "apsis.progress.01JABC");
    }
}
