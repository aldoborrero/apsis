//! State-store + job-publisher abstractions, with NATS-backed impls and
//! in-memory fakes, so coordinator/worker logic is unit-testable without a
//! broker. The NATS impls are exercised by the gated integration tests.

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
    #[error(transparent)]
    Backend(async_nats::Error),
}

fn to_bytes<T: serde::Serialize>(v: &T) -> Result<Bytes, StoreError> {
    Ok(Bytes::from(serde_json::to_vec(v)?))
}

/// Per-file state (KV `transcode_state`). `get`/`update` carry the revision for
/// compare-and-set; `create` fails if the key already exists.
#[async_trait]
pub trait StateStore: Send + Sync {
    /// Returns `(entry, revision)` or `None` if absent/deleted.
    async fn get(&self, key: &str) -> Result<Option<(StateEntry, u64)>, StoreError>;
    /// Create a new key; [`StoreError::Conflict`] if it already exists.
    async fn create(&self, key: &str, entry: &StateEntry) -> Result<u64, StoreError>;
    /// CAS update at `revision`; [`StoreError::Conflict`] on a stale revision.
    async fn update(&self, key: &str, entry: &StateEntry, revision: u64)
    -> Result<u64, StoreError>;
    /// Unconditional overwrite (used when folding a result).
    async fn put(&self, key: &str, entry: &StateEntry) -> Result<u64, StoreError>;
}

/// Publish a job to a subject.
#[async_trait]
pub trait JobPublisher: Send + Sync {
    async fn publish(&self, subject: &str, job: &Job) -> Result<(), StoreError>;
}

// --- NATS-backed impls (newtypes avoid colliding with kv::Store's inherent
// create/update/put) ---

/// KV-backed [`StateStore`].
pub struct KvStateStore(pub Store);

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
}

/// JetStream-backed [`JobPublisher`].
pub struct NatsPublisher(pub Context);

#[async_trait]
impl JobPublisher for NatsPublisher {
    async fn publish(&self, subject: &str, job: &Job) -> Result<(), StoreError> {
        let bytes = to_bytes(job)?;
        // Await the publish, then the ack, so a job is durably stored before we
        // claim it in KV (contract §invariant 1).
        self.0
            .publish(subject.to_string(), bytes)
            .await
            .map_err(|e| StoreError::Backend(e.into()))?
            .await
            .map_err(|e| StoreError::Backend(e.into()))?;
        Ok(())
    }
}

// --- in-memory fakes (unit tests) ---

/// In-memory [`StateStore`] with the same CAS semantics as the KV impl.
#[derive(Default, Clone)]
pub struct FakeStateStore {
    inner: Arc<Mutex<HashMap<String, (StateEntry, u64)>>>,
}

#[async_trait]
impl StateStore for FakeStateStore {
    async fn get(&self, key: &str) -> Result<Option<(StateEntry, u64)>, StoreError> {
        Ok(self.inner.lock().unwrap().get(key).cloned())
    }

    async fn create(&self, key: &str, entry: &StateEntry) -> Result<u64, StoreError> {
        let mut g = self.inner.lock().unwrap();
        if g.contains_key(key) {
            return Err(StoreError::Conflict(key.to_string()));
        }
        g.insert(key.to_string(), (entry.clone(), 1));
        Ok(1)
    }

    async fn update(
        &self,
        key: &str,
        entry: &StateEntry,
        revision: u64,
    ) -> Result<u64, StoreError> {
        let mut g = self.inner.lock().unwrap();
        match g.get(key) {
            Some((_, rev)) if *rev == revision => {
                let next = revision + 1;
                g.insert(key.to_string(), (entry.clone(), next));
                Ok(next)
            }
            _ => Err(StoreError::Conflict(key.to_string())),
        }
    }

    async fn put(&self, key: &str, entry: &StateEntry) -> Result<u64, StoreError> {
        let mut g = self.inner.lock().unwrap();
        let next = g.get(key).map_or(1, |(_, rev)| rev + 1);
        g.insert(key.to_string(), (entry.clone(), next));
        Ok(next)
    }
}

/// In-memory [`JobPublisher`] that records everything published.
#[derive(Default, Clone)]
pub struct FakeQueue {
    inner: Arc<Mutex<Vec<(String, Job)>>>,
}

impl FakeQueue {
    /// Every `(subject, job)` published so far.
    ///
    /// # Panics
    /// If the internal lock is poisoned (a prior panic while holding it).
    #[must_use]
    pub fn published(&self) -> Vec<(String, Job)> {
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
impl JobPublisher for FakeQueue {
    async fn publish(&self, subject: &str, job: &Job) -> Result<(), StoreError> {
        self.inner
            .lock()
            .unwrap()
            .push((subject.to_string(), job.clone()));
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
        }
    }

    #[tokio::test]
    async fn create_then_get_then_cas_update() {
        let s = FakeStateStore::default();
        let rev = s
            .create("a.mkv", &entry(Status::Pending, "1:1"))
            .await
            .unwrap();
        assert_eq!(rev, 1);
        let (got, r) = s.get("a.mkv").await.unwrap().unwrap();
        assert_eq!(got.status, Status::Pending);
        assert_eq!(r, 1);

        // CAS with the right revision succeeds and bumps it.
        let rev2 = s
            .update("a.mkv", &entry(Status::InProgress, "1:1"), r)
            .await
            .unwrap();
        assert_eq!(rev2, 2);
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
    async fn fake_queue_records_publishes() {
        let q = FakeQueue::default();
        assert!(q.is_empty());
        let job = Job {
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
            enqueued_at: OffsetDateTime::UNIX_EPOCH,
        };
        q.publish("jobs.transcode.local", &job).await.unwrap();
        let pubs = q.published();
        assert_eq!(pubs.len(), 1);
        assert_eq!(pubs[0].0, "jobs.transcode.local");
        assert_eq!(pubs[0].1.path, "/x.mkv");
    }
}
