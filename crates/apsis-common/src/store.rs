//! State-store + job-publisher abstractions, with NATS-backed impls and
//! in-memory fakes, so coordinator/worker logic is unit-testable without a
//! broker. The unit tests below exercise the fakes; the NATS-backed impls get
//! integration coverage against a real `nats-server` in the worker phase (US3).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_nats::jetstream::Context;
use async_nats::jetstream::kv::{CreateErrorKind, Operation, Store, UpdateErrorKind};
use async_trait::async_trait;
use bytes::Bytes;
use thiserror::Error;

use crate::schema::{Job, StateEntry};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StoreError {
    #[error("serialize/deserialize: {0}")]
    Serde(#[from] serde_json::Error),
    /// A compare-and-set lost: the key was created/updated concurrently.
    #[error("CAS conflict on key {0:?}")]
    Conflict(String),
    /// Any other backend failure. Boxed as `dyn Error` (not an async-nats type)
    /// so the *error surface* isn't coupled to the broker crate's version.
    /// (Constructors like [`KvStateStore::new`] and the [`crate::nats`] fns still
    /// take/return async-nats types by design — async-nats is centralized here.)
    /// Note: this is opaque, so a caller cannot yet classify retriable-vs-fatal;
    /// that classification is added in the worker phase (US3).
    #[error(transparent)]
    Backend(Box<dyn std::error::Error + Send + Sync>),
}

fn to_bytes<T: serde::Serialize>(v: &T) -> Result<Bytes, StoreError> {
    Ok(Bytes::from(serde_json::to_vec(v)?))
}

/// Per-file state (KV `transcode_state`).
///
/// The `revision` `u64` is the KV **stream sequence** (a bucket-global,
/// monotonic-but-non-contiguous token). It is unrelated to
/// [`StateEntry::version`] (the `mtime:size` change token) — never do arithmetic
/// on it; round-trip the opaque value from `get`/`create` into the next `update`.
///
/// **Claim protocol.** A caller with `get` == `None` uses `create`; `get` ==
/// `Some(rev)` uses `update(rev)`. A [`StoreError::Conflict`] on *either* arm
/// means another actor won the race — re-`get` and act on the fresh state (a
/// `create` conflict → the key now exists → switch to `update`). In the
/// single-node coordinator, the reconcile pass and inotify feed one serialized
/// task, so this race is in-process and easily avoided; the CAS is the backstop
/// for the distributed case (spec 003).
#[async_trait]
pub trait StateStore: Send + Sync {
    /// Returns `(entry, revision)`, or `None` if the key is absent. A
    /// deleted/purged key also reads as `None` — but this crate never deletes, so
    /// that tombstone path is not exercised here (would land against real NATS).
    async fn get(&self, key: &str) -> Result<Option<(StateEntry, u64)>, StoreError>;
    /// Create a new key; [`StoreError::Conflict`] if it already exists.
    async fn create(&self, key: &str, entry: &StateEntry) -> Result<u64, StoreError>;
    /// CAS update at `revision`; [`StoreError::Conflict`] on a stale revision.
    async fn update(&self, key: &str, entry: &StateEntry, revision: u64)
    -> Result<u64, StoreError>;
    /// Unconditional overwrite (used when folding a result into `Done`/`Failed`).
    async fn put(&self, key: &str, entry: &StateEntry) -> Result<u64, StoreError>;
    /// Delete a key so the change-gate misses it (spec 005 re-queue/retry). Absent is Ok.
    async fn delete(&self, key: &str) -> Result<(), StoreError>;
}

/// Publish a job. The subject is fixed to `jobs.transcode.local` (single-node);
/// spec 003 will add backend routing. Keeping the subject out of the API removes
/// the mis-routing footgun (a typo'd subject that no consumer filters).
#[async_trait]
pub trait JobPublisher: Send + Sync {
    async fn publish(&self, job: &Job) -> Result<(), StoreError>;
}

// --- NATS-backed impls (newtypes avoid colliding with kv::Store's inherent
// create/update/put) ---

/// KV-backed [`StateStore`]. The inner `Store` is private so callers go through
/// the trait (and its CAS error mapping), not the raw kv API.
#[derive(Clone)]
pub struct KvStateStore(Store);

impl KvStateStore {
    #[must_use]
    pub fn new(store: Store) -> Self {
        Self(store)
    }

    /// Read the effective [`crate::control::PauseState`] (spec 005). Absent = not paused.
    ///
    /// # Errors
    /// Backend or deserialization failure.
    pub async fn get_pause(&self) -> Result<crate::control::PauseState, StoreError> {
        let entry = self
            .0
            .entry(crate::nats::KV_CONTROL_PAUSE)
            .await
            .map_err(|e| StoreError::Backend(e.into()))?;
        match entry {
            Some(e) if e.operation == Operation::Put => Ok(serde_json::from_slice(&e.value)?),
            _ => Ok(crate::control::PauseState::default()),
        }
    }

    /// Every file-path key in the bucket (excludes the `__control__/*` control keys).
    /// Used by the read-only console (spec 006) to list tracked files.
    ///
    /// # Errors
    /// Backend failure.
    pub async fn list_keys(&self) -> Result<Vec<String>, StoreError> {
        use futures::TryStreamExt;
        let keys = self
            .0
            .keys()
            .await
            .map_err(|e| StoreError::Backend(e.into()))?;
        let all: Vec<String> = keys
            .try_collect()
            .await
            .map_err(|e| StoreError::Backend(e.into()))?;
        Ok(all
            .into_iter()
            .filter(|k| !k.starts_with("__control__"))
            .collect())
    }

    /// Persist the pause state. **Coordinator-only writer** (spec 005 FR-014).
    ///
    /// # Errors
    /// Serialization or backend failure.
    pub async fn put_pause(&self, state: &crate::control::PauseState) -> Result<(), StoreError> {
        let bytes = to_bytes(state)?;
        self.0
            .put(crate::nats::KV_CONTROL_PAUSE, bytes)
            .await
            .map_err(|e| StoreError::Backend(e.into()))?;
        Ok(())
    }
}

#[async_trait]
impl StateStore for KvStateStore {
    async fn get(&self, key: &str) -> Result<Option<(StateEntry, u64)>, StoreError> {
        let entry = self
            .0
            .entry(key)
            .await
            .map_err(|e| StoreError::Backend(e.into()))?;
        match entry {
            Some(e) if e.operation == Operation::Put => {
                Ok(Some((serde_json::from_slice(&e.value)?, e.revision)))
            }
            _ => Ok(None), // absent, deleted, or purged
        }
    }

    async fn create(&self, key: &str, entry: &StateEntry) -> Result<u64, StoreError> {
        let bytes = to_bytes(entry)?;
        self.0.create(key, bytes).await.map_err(|e| {
            if e.kind() == CreateErrorKind::AlreadyExists {
                StoreError::Conflict(key.to_string())
            } else {
                StoreError::Backend(e.into())
            }
        })
    }

    async fn update(
        &self,
        key: &str,
        entry: &StateEntry,
        revision: u64,
    ) -> Result<u64, StoreError> {
        let bytes = to_bytes(entry)?;
        self.0.update(key, bytes, revision).await.map_err(|e| {
            if e.kind() == UpdateErrorKind::WrongLastRevision {
                StoreError::Conflict(key.to_string())
            } else {
                StoreError::Backend(e.into())
            }
        })
    }

    async fn put(&self, key: &str, entry: &StateEntry) -> Result<u64, StoreError> {
        let bytes = to_bytes(entry)?;
        self.0
            .put(key, bytes)
            .await
            .map_err(|e| StoreError::Backend(e.into()))
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.0
            .delete(key)
            .await
            .map_err(|e| StoreError::Backend(e.into()))
    }
}

/// JetStream-backed [`JobPublisher`].
pub struct NatsPublisher(Context);

impl NatsPublisher {
    #[must_use]
    pub fn new(ctx: Context) -> Self {
        Self(ctx)
    }
}

#[async_trait]
impl JobPublisher for NatsPublisher {
    async fn publish(&self, job: &Job) -> Result<(), StoreError> {
        let bytes = to_bytes(job)?;
        // Fixed subject: the job MUST land on the work stream's filtered subject,
        // or it would ack into the stream yet reach no consumer (silent drop).
        // Await the publish, then the ack, so it's durably stored (contract §inv 1).
        self.0
            .publish(crate::nats::SUBJECT_LOCAL, bytes)
            .await
            .map_err(|e| StoreError::Backend(e.into()))?
            .await
            .map_err(|e| StoreError::Backend(e.into()))?;
        Ok(())
    }
}

/// Publish a [`crate::schema::TranscodeResult`] to the result subject — a
/// fire-and-forget **core** event (not in the work stream) the coordinator folds
/// into KV + metrics.
///
/// # Errors
/// Serialization or publish/flush failure.
pub async fn publish_result(
    client: &async_nats::Client,
    result: &crate::schema::TranscodeResult,
) -> Result<(), StoreError> {
    let bytes = to_bytes(result)?;
    client
        .publish(crate::nats::SUBJECT_RESULT, bytes)
        .await
        .map_err(|e| StoreError::Backend(e.into()))?;
    client
        .flush()
        .await
        .map_err(|e| StoreError::Backend(e.into()))?;
    Ok(())
}

// --- in-memory fakes (unit tests) ---

/// In-memory [`StateStore`] with the same CAS *semantics* as the KV impl.
///
/// Revisions come from a **bucket-global** counter, mirroring NATS KV where the
/// revision is the stream sequence rather than a per-key counter. To make the
/// distinction impossible to miss, the counter advances by **2** on every write:
/// a key's revisions are therefore always non-contiguous (`2, 4, …`), so any
/// coordinator code that does `rev + 1` arithmetic fails here — even on a quiet,
/// single-key bucket where real NATS *would* happen to be contiguous. A stricter
/// fake than the backend, deliberately, to catch the bug earlier.
#[derive(Default)]
struct FakeState {
    map: HashMap<String, (StateEntry, u64)>,
    /// Bucket-global sequence (see [`FakeStateStore`]).
    seq: u64,
}

impl FakeState {
    fn write(&mut self, key: &str, entry: &StateEntry) -> u64 {
        self.seq += 2; // non-contiguous on purpose — see FakeStateStore docs
        self.map.insert(key.to_string(), (entry.clone(), self.seq));
        self.seq
    }
}

#[derive(Default, Clone)]
pub struct FakeStateStore {
    inner: Arc<Mutex<FakeState>>,
}

#[async_trait]
impl StateStore for FakeStateStore {
    async fn get(&self, key: &str) -> Result<Option<(StateEntry, u64)>, StoreError> {
        Ok(self.inner.lock().unwrap().map.get(key).cloned())
    }

    async fn create(&self, key: &str, entry: &StateEntry) -> Result<u64, StoreError> {
        let mut st = self.inner.lock().unwrap();
        if st.map.contains_key(key) {
            return Err(StoreError::Conflict(key.to_string()));
        }
        Ok(st.write(key, entry))
    }

    async fn update(
        &self,
        key: &str,
        entry: &StateEntry,
        revision: u64,
    ) -> Result<u64, StoreError> {
        let mut st = self.inner.lock().unwrap();
        match st.map.get(key) {
            Some((_, rev)) if *rev == revision => Ok(st.write(key, entry)),
            _ => Err(StoreError::Conflict(key.to_string())),
        }
    }

    async fn put(&self, key: &str, entry: &StateEntry) -> Result<u64, StoreError> {
        Ok(self.inner.lock().unwrap().write(key, entry))
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.inner.lock().unwrap().map.remove(key);
        Ok(())
    }
}

/// In-memory [`JobPublisher`] that records every published job.
#[derive(Default, Clone)]
pub struct FakeJobPublisher {
    inner: Arc<Mutex<Vec<Job>>>,
}

impl FakeJobPublisher {
    /// Every job published so far.
    ///
    /// # Panics
    /// If the internal lock is poisoned (a prior panic while holding it).
    #[must_use]
    pub fn published(&self) -> Vec<Job> {
        self.inner.lock().unwrap().clone()
    }

    /// How many jobs were published.
    ///
    /// # Panics
    /// If the internal lock is poisoned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[async_trait]
impl JobPublisher for FakeJobPublisher {
    async fn publish(&self, job: &Job) -> Result<(), StoreError> {
        self.inner.lock().unwrap().push(job.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Status;
    use time::OffsetDateTime;

    fn entry(status: Status, version: &str) -> StateEntry {
        StateEntry {
            status,
            version: version.to_string(),
            job_id: None,
            attempts: 0,
            used_fallback: false,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            last_error: None,
            decision: None,
        }
    }

    #[tokio::test]
    async fn create_then_get_then_cas_update() {
        let s = FakeStateStore::default();
        // Revisions are opaque tokens — assert behavior, not literal values.
        let rev = s
            .create("a.mkv", &entry(Status::Pending, "1:1"))
            .await
            .unwrap();
        let (got, r) = s.get("a.mkv").await.unwrap().unwrap();
        assert_eq!(got.status, Status::Pending);
        assert_eq!(r, rev, "get returns the create revision");

        // CAS with the right revision succeeds and advances it.
        let rev2 = s
            .update("a.mkv", &entry(Status::InProgress, "1:1"), r)
            .await
            .unwrap();
        assert!(rev2 > r, "a successful write advances the revision");
    }

    #[tokio::test]
    async fn revisions_are_bucket_global_and_non_contiguous() {
        // Mirrors real NATS KV: a key's revisions are NOT n, n+1 — writes to
        // other keys advance the shared sequence. Guards against arithmetic.
        let s = FakeStateStore::default();
        let a1 = s.create("a", &entry(Status::Pending, "v")).await.unwrap();
        let _b = s.create("b", &entry(Status::Pending, "v")).await.unwrap();
        let a2 = s.update("a", &entry(Status::Done, "v"), a1).await.unwrap();
        assert!(
            a2 > a1 + 1,
            "b's write must sit between a's revisions: {a1} -> {a2}"
        );
    }

    #[tokio::test]
    async fn create_conflicts_and_stale_update_conflicts() {
        let s = FakeStateStore::default();
        s.create("a", &entry(Status::Pending, "1:1")).await.unwrap();
        // second create → conflict
        assert!(matches!(
            s.create("a", &entry(Status::Pending, "1:1")).await,
            Err(StoreError::Conflict(_))
        ));
        // update at the wrong (stale) revision → conflict
        assert!(matches!(
            s.update("a", &entry(Status::Done, "1:1"), 999).await,
            Err(StoreError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn get_absent_is_none() {
        let s = FakeStateStore::default();
        assert!(s.get("nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn put_folds_result_unconditionally() {
        let s = FakeStateStore::default();
        // `put` on an absent key creates it; on a present key overwrites, no CAS.
        let r1 = s.put("a", &entry(Status::Done, "v")).await.unwrap();
        let r2 = s.put("a", &entry(Status::Failed, "v")).await.unwrap();
        assert!(r2 > r1);
        let (got, _) = s.get("a").await.unwrap().unwrap();
        assert_eq!(got.status, Status::Failed);
    }

    #[tokio::test]
    async fn concurrent_cas_has_exactly_one_winner() {
        // Two tasks both read rev R and both update at R: exactly one wins, the
        // other Conflicts. This is the FR-005 double-enqueue guard.
        let s = FakeStateStore::default();
        let r = s.create("a", &entry(Status::Pending, "v")).await.unwrap();
        let (s1, s2) = (s.clone(), s.clone());
        let t1 =
            tokio::spawn(async move { s1.update("a", &entry(Status::InProgress, "v"), r).await });
        let t2 = tokio::spawn(async move { s2.update("a", &entry(Status::Done, "v"), r).await });
        let (a, b) = (t1.await.unwrap(), t2.await.unwrap());
        assert_eq!(
            [a.is_ok(), b.is_ok()].iter().filter(|x| **x).count(),
            1,
            "exactly one CAS at the same revision must win"
        );
        assert!(a.is_err() || b.is_err());
    }

    fn sample_job() -> Job {
        Job {
            id: ulid::Ulid::from_parts(1, 1),
            path: "/x.mkv".into(),
            version: "1:1".into(),
            profile: "tv".into(),
            plan: serde_json::from_str(
                r#"{"status":"changes_required","compliant":false,"should_skip":false,
                    "source_probe":{"video":null,"audio":[],"subtitles":[]},
                    "output":{"input_path":"/x.mkv","output_path":"/x.mkv","replace_original":true,
                              "source_container":"mkv","target_container":"mkv"},
                    "video":{"source_index":null,"source_codec":null,"target_codec":"hevc","action":"unsupported"},
                    "audio":[],"subtitles":[],
                    "reasons":[{"code":"no_video_stream","message":"no video stream present","scope":"video"}]}"#,
            )
            .unwrap(),
            profile_config: serde_json::from_str(
                r#"{"video":{"codec":"hevc"},"audio":{},"subtitles":{},"output":{}}"#,
            )
            .unwrap(),
            enqueued_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[tokio::test]
    async fn fake_publisher_records_jobs() {
        let q = FakeJobPublisher::default();
        assert!(q.is_empty());
        q.publish(&sample_job()).await.unwrap();
        let pubs = q.published();
        assert_eq!(pubs.len(), 1);
        assert_eq!(pubs[0].path, "/x.mkv");
    }
}
