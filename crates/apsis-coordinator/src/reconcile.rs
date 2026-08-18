//! The reconcile decision (design §7, FR-004/005): for one file, gate on the
//! change token (skip unchanged/handled without probing), else probe + plan via
//! the engine — the single decision point — and either mark it `Done` (compliant)
//! or claim `Pending` (CAS) and publish a `Job`.

use std::path::PathBuf;

use apsis_common::config::Library;
use apsis_common::{Job, JobPublisher, StateEntry, StateStore, Status, StoreError};
use apsis_engine::{Probe, Profile, parse_probe, plan};
use async_trait::async_trait;
use thiserror::Error;
use time::OffsetDateTime;

#[derive(Debug, Error)]
pub(crate) enum ReconcileError {
    #[error("state store: {0}")]
    Store(#[from] StoreError),
    #[error("probe: {0}")]
    Probe(#[from] std::io::Error),
}

/// What a single-file reconcile did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReconcileOutcome {
    /// Unchanged & already handled — no probe, no work (FR-005).
    Skipped,
    /// Planned; already compliant → recorded `Done`.
    Compliant,
    /// Drift → claimed `Pending` and published a `Job`.
    Enqueued,
    /// Lost the claim to a concurrent actor.
    Claimed,
}

/// Probe a file to its stream layout. The prod impl shells out to ffprobe; tests
/// inject a fake so the reconcile logic is exercised without ffprobe.
#[async_trait]
pub(crate) trait Prober: Send + Sync {
    async fn probe(&self, path: &str) -> std::io::Result<Probe>;
}

pub(crate) struct FfprobeProber {
    pub ffprobe: PathBuf,
}

#[async_trait]
impl Prober for FfprobeProber {
    async fn probe(&self, path: &str) -> std::io::Result<Probe> {
        let out = tokio::process::Command::new(&self.ffprobe)
            .args(["-v", "quiet", "-print_format", "json", "-show_streams"])
            .arg(path)
            .output()
            .await?;
        if !out.status.success() {
            return Err(std::io::Error::other("ffprobe exited non-zero"));
        }
        parse_probe(&String::from_utf8_lossy(&out.stdout))
            .map_err(|e| std::io::Error::other(e.to_string()))
    }
}

pub(crate) struct Reconciler<S, Q, P> {
    pub store: S,
    pub publisher: Q,
    pub prober: P,
}

impl<S: StateStore, Q: JobPublisher, P: Prober> Reconciler<S, Q, P> {
    /// Reconcile one file (already known to be a video under `library`) against
    /// `profile`. `ver` is its current `mtime:size` token.
    #[tracing::instrument(
        name = "reconcile",
        skip_all,
        fields(path, version = ver, outcome = tracing::field::Empty)
    )]
    pub(crate) async fn reconcile_file(
        &self,
        path: &str,
        ver: &str,
        library: &Library,
        profile: &Profile,
    ) -> std::result::Result<ReconcileOutcome, ReconcileError> {
        // Gate: unchanged & already handled → no probe, no-op.
        if let Some((st, _)) = self.store.get(path).await?
            && st.version == ver
            && is_handled(st.status)
        {
            tracing::Span::current().record("outcome", "skipped");
            return Ok(ReconcileOutcome::Skipped);
        }

        // Changed/new → probe + plan (the only decision point).
        let probe = self.prober.probe(path).await?;
        let file_plan = plan(path, &probe, profile);

        if file_plan.should_skip {
            self.store
                .put(path, &entry(Status::Done, ver, None))
                .await?;
            tracing::Span::current().record("outcome", "compliant");
            return Ok(ReconcileOutcome::Compliant);
        }

        // Drift → claim Pending (CAS), then publish the job.
        if !self.claim_pending(path, ver).await? {
            tracing::Span::current().record("outcome", "claimed_elsewhere");
            return Ok(ReconcileOutcome::Claimed);
        }
        let job = Job {
            id: ulid::Ulid::new(),
            path: path.to_string(),
            version: ver.to_string(),
            profile: library.profile.clone(),
            plan: file_plan,
            profile_config: profile.clone(),
            enqueued_at: OffsetDateTime::now_utc(),
        };
        self.publisher.publish(&job).await?;
        tracing::Span::current().record("outcome", "enqueued");
        tracing::info!(job_id = %job.id, "enqueued transcode job");
        Ok(ReconcileOutcome::Enqueued)
    }

    /// CAS the state to `Pending` at `ver` (create if absent). `false` = lost race.
    async fn claim_pending(
        &self,
        path: &str,
        ver: &str,
    ) -> std::result::Result<bool, ReconcileError> {
        let e = entry(Status::Pending, ver, None);
        let result = match self.store.get(path).await? {
            Some((_, rev)) => self.store.update(path, &e, rev).await,
            None => self.store.create(path, &e).await,
        };
        match result {
            Ok(_) => Ok(true),
            Err(StoreError::Conflict(_)) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }
}

fn is_handled(s: Status) -> bool {
    matches!(
        s,
        Status::Done | Status::Pending | Status::InProgress | Status::Failed
    )
}

fn entry(status: Status, ver: &str, last_error: Option<String>) -> StateEntry {
    StateEntry {
        status,
        version: ver.to_string(),
        job_id: None,
        attempts: 0,
        used_fallback: false,
        updated_at: OffsetDateTime::now_utc(),
        last_error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apsis_common::{FakeJobPublisher, FakeStateStore};
    use apsis_engine::StreamInfo;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeProber {
        probe: Probe,
        calls: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl Prober for FakeProber {
        async fn probe(&self, _path: &str) -> std::io::Result<Probe> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.probe.clone())
        }
    }

    fn library() -> Library {
        serde_json::from_str(r#"{"name":"tv","path":"/hdd/tv","profile":"tv"}"#).unwrap()
    }
    fn profile() -> Profile {
        serde_json::from_str(
            r#"{"video":{"codec":"hevc","skip_codecs":["hevc"]},"audio":{},"subtitles":{},"output":{"container":"mkv"}}"#,
        )
        .unwrap()
    }
    fn video(codec: &str) -> Probe {
        Probe {
            video: Some(StreamInfo {
                index: 0,
                codec_type: "video".into(),
                codec: codec.into(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn reconciler(
        probe: Probe,
    ) -> (
        Reconciler<FakeStateStore, FakeJobPublisher, FakeProber>,
        Arc<AtomicUsize>,
    ) {
        let calls = Arc::new(AtomicUsize::new(0));
        let r = Reconciler {
            store: FakeStateStore::default(),
            publisher: FakeJobPublisher::default(),
            prober: FakeProber {
                probe,
                calls: calls.clone(),
            },
        };
        (r, calls)
    }

    #[tokio::test]
    async fn compliant_file_marks_done_and_enqueues_nothing() {
        // hevc is in skip_codecs → compliant.
        let (r, calls) = reconciler(video("hevc"));
        let out = r
            .reconcile_file("/hdd/tv/x.mkv", "1:1", &library(), &profile())
            .await
            .unwrap();
        assert_eq!(out, ReconcileOutcome::Compliant);
        assert!(r.publisher.is_empty(), "no job for a compliant file");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Second pass, same version → gated out, NOT re-probed (FR-005).
        let out = r
            .reconcile_file("/hdd/tv/x.mkv", "1:1", &library(), &profile())
            .await
            .unwrap();
        assert_eq!(out, ReconcileOutcome::Skipped);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "unchanged file not re-probed"
        );
        assert!(r.publisher.is_empty());
    }

    #[tokio::test]
    async fn drift_enqueues_once_then_is_idempotent() {
        // h264 → not in skip_codecs → encode → drift.
        let (r, calls) = reconciler(video("h264"));
        let out = r
            .reconcile_file("/hdd/tv/y.mkv", "2:2", &library(), &profile())
            .await
            .unwrap();
        assert_eq!(out, ReconcileOutcome::Enqueued);
        assert_eq!(r.publisher.len(), 1, "exactly one job for the drift");
        assert_eq!(r.publisher.published()[0].path, "/hdd/tv/y.mkv");

        // Second pass, same version, now Pending → gated, no new job, no re-probe.
        let out = r
            .reconcile_file("/hdd/tv/y.mkv", "2:2", &library(), &profile())
            .await
            .unwrap();
        assert_eq!(out, ReconcileOutcome::Skipped);
        assert_eq!(r.publisher.len(), 1, "idempotent — still one job");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn changed_version_supersedes_and_reprobes() {
        let (r, calls) = reconciler(video("hevc"));
        r.reconcile_file("/hdd/tv/z.mkv", "1:1", &library(), &profile())
            .await
            .unwrap(); // Done@1:1
        // File changed (new token) → gate misses → re-probe + re-plan.
        let out = r
            .reconcile_file("/hdd/tv/z.mkv", "9:9", &library(), &profile())
            .await
            .unwrap();
        assert_eq!(out, ReconcileOutcome::Compliant);
        assert_eq!(calls.load(Ordering::SeqCst), 2, "changed file is re-probed");
    }
}
