//! The reconcile decision (design §7, FR-004/005): for one file, gate on the
//! change token (skip unchanged/handled without probing), else probe + plan via
//! the engine — the single decision point — and either mark it `Done` (compliant)
//! or claim `Pending` (CAS) and publish a `Job`.

use std::path::PathBuf;

use apsis_common::config::Library;
use apsis_common::{
    Decision, DecisionKind, Job, JobPublisher, StateEntry, StateStore, Status, StoreError,
    has_ignore_marker,
};
use apsis_engine::{
    FileFacts, Probe, Profile, SkipReason, build_context, parse_probe, resolve_effective_profile,
};
use async_trait::async_trait;
use thiserror::Error;
use time::OffsetDateTime;

#[derive(Debug, Error)]
pub(crate) enum ReconcileError {
    #[error("state store: {0}")]
    Store(#[from] StoreError),
    #[error("probe: {0}")]
    Probe(#[from] std::io::Error),
    #[error("profile-rule overrides: {0}")]
    Override(#[from] apsis_engine::EngineError),
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
    /// A profile-rule override failed for this file — recorded `Failed` so the
    /// change-gate suppresses re-probing until the file itself changes (no loop).
    OverrideFailed,
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
        self.reconcile_file_opts(path, ver, library, profile, false)
            .await
    }

    /// [`reconcile_file`] with an operator `force` (spec 005): bypass the change-gate and
    /// plan with `should_skip` suppressed, so a compliant/already-handled file is transcoded.
    pub(crate) async fn reconcile_file_opts(
        &self,
        path: &str,
        ver: &str,
        library: &Library,
        profile: &Profile,
        force: bool,
    ) -> std::result::Result<ReconcileOutcome, ReconcileError> {
        // Ignore marker (spec 005 FR-016): an operator "never transcode this" — a recoverable
        // on-disk marker beside the media. Honored before anything else (even force — clear the
        // marker to un-ignore), so an ignored file is never probed and survives a KV wipe.
        if has_ignore_marker(std::path::Path::new(path)) {
            tracing::Span::current().record("outcome", "ignored");
            return Ok(ReconcileOutcome::Skipped);
        }

        // Gate: unchanged & already handled → no probe, no-op. Force bypasses it.
        if !force
            && let Some((st, _)) = self.store.get(path).await?
            && st.version == ver
            && is_handled(st.status)
        {
            tracing::Span::current().record("outcome", "skipped");
            return Ok(ReconcileOutcome::Skipped);
        }

        // Changed/new → probe, resolve profile-rule overrides, then plan (the only
        // decision point). Skip-evaluation inside plan() sees the EFFECTIVE profile,
        // so a rule may set a value a gate reads (data-model §resolution order).
        let probe = self.prober.probe(path).await?;
        let effective = if profile.rules.is_empty() {
            profile.clone()
        } else {
            // File facts the CEL context needs beyond the probe: `size` rides on the
            // `mtime:size` version token; `container` is the path's extension, lowercased
            // to match the contract; `duration` is not yet carried by the probe (contract:
            // 0.0 = unknown).
            let container = std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let size = ver
                .rsplit(':')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let facts = FileFacts {
                path,
                container: &container,
                duration: 0.0,
                size,
            };
            let resolved = build_context(&probe, &facts)
                .and_then(|ctx| resolve_effective_profile(profile, &profile.rules, &ctx));
            match resolved {
                Ok(p) => p,
                Err(e) => {
                    // A per-file CEL/override error is deterministic for a fixed probe, so
                    // record Failed@ver: the change-gate then suppresses re-probing until
                    // the file changes (never an every-pass re-probe loop — Principle IV).
                    // Static rule errors will be caught at load in US3.
                    self.store
                        .put(path, &entry(Status::Failed, ver, Some(e.to_string())))
                        .await?;
                    tracing::Span::current().record("outcome", "override_failed");
                    tracing::warn!(error = %e, "profile-rule override failed; marked Failed");
                    return Ok(ReconcileOutcome::OverrideFailed);
                }
            }
        };
        let file_plan = apsis_engine::plan_with(
            path,
            &probe,
            &effective,
            &apsis_engine::PlanOptions { force },
        );

        if file_plan.should_skip {
            // Persist the positive skip decision (spec 005 FR-011) so "why skipped" is
            // queryable from the KV without a re-probe.
            let mut e = entry(Status::Done, ver, None);
            e.decision = decision_from(file_plan.skip_reason.as_ref());
            self.store.put(path, &e).await?;
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
            // The worker materializes its command from the EFFECTIVE profile (rules
            // already applied); it never sees rules (Constitution III).
            profile_config: effective,
            enqueued_at: OffsetDateTime::now_utc(),
        };
        self.publisher.publish(&job).await?;
        tracing::Span::current().record("outcome", "enqueued");
        tracing::info!(job_id = %job.id, "enqueued transcode job");
        Ok(ReconcileOutcome::Enqueued)
    }

    /// Execute an operator state-control op (spec 005 T014). The caller serializes this with
    /// the reconcile loop (FR-015). Returns the [`StateOutcome`] to reply with.
    pub(crate) async fn apply_state_op(
        &self,
        req: &apsis_common::control::StateControlRequest,
        cfg: &apsis_common::config::SchedulerConfig,
    ) -> apsis_common::control::StateOutcome {
        use apsis_common::control::{StateOp, StateOutcome};
        let p = std::path::Path::new(&req.path);
        match req.op {
            // Clear the entry → the change-gate misses → the next reconcile re-plans.
            StateOp::Requeue | StateOp::Retry => match self.store.get(&req.path).await {
                Ok(Some(_)) => match self.store.delete(&req.path).await {
                    Ok(()) => StateOutcome::Applied,
                    Err(_) => StateOutcome::Noop,
                },
                Ok(None) => StateOutcome::NotFound,
                Err(_) => StateOutcome::Noop,
            },
            StateOp::MarkDone => {
                let Ok(ver) = apsis_common::version_token(p) else {
                    return StateOutcome::NotFound; // file missing
                };
                match self
                    .store
                    .put(&req.path, &entry(Status::Done, &ver, None))
                    .await
                {
                    Ok(_) => StateOutcome::Applied,
                    Err(_) => StateOutcome::Noop,
                }
            }
            StateOp::Force => {
                let Ok(ver) = apsis_common::version_token(p) else {
                    return StateOutcome::NotFound;
                };
                let Some(library) = crate::profile_match::match_library(&cfg.libraries, &req.path)
                else {
                    return StateOutcome::NotFound;
                };
                let Some(profile) = cfg.profiles.get(&library.profile) else {
                    return StateOutcome::NotFound;
                };
                match self
                    .reconcile_file_opts(&req.path, &ver, library, profile, true)
                    .await
                {
                    Ok(ReconcileOutcome::Enqueued) => StateOutcome::Applied,
                    Ok(_) | Err(_) => StateOutcome::Noop,
                }
            }
        }
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

/// Map the engine's positive [`SkipReason`] into the persisted [`Decision`] (spec 005 FR-011).
fn decision_from(skip: Option<&SkipReason>) -> Option<Decision> {
    skip.map(|s| match s {
        SkipReason::CompliantCodec(c) => Decision {
            kind: DecisionKind::CompliantCodec,
            detail: c.clone(),
        },
        SkipReason::ResolutionBelow(r) => Decision {
            kind: DecisionKind::ResolutionBelow,
            detail: r.clone(),
        },
        SkipReason::BitrateBelow(b) => Decision {
            kind: DecisionKind::BitrateBelow,
            detail: b.clone(),
        },
    })
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
        decision: None,
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

    fn profile_with_rule() -> Profile {
        // A rule that fires on an h264 source and retargets the codec to AV1.
        serde_json::from_str(
            r#"{"video":{"codec":"hevc","skip_codecs":["hevc"]},"audio":{},"subtitles":{},
                "output":{"container":"mkv"},
                "rule":[{"when":"video != null && video.codec == 'h264'",
                         "set":{"video.codec":"av1"}}]}"#,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn rule_override_reaches_enqueued_job() {
        // The h264 source triggers the rule → the effective profile targets AV1, and
        // the enqueued job carries that EFFECTIVE profile_config (the worker never
        // sees rules — Constitution III).
        let (r, _) = reconciler(video("h264"));
        let out = r
            .reconcile_file("/hdd/tv/r.mkv", "3:3", &library(), &profile_with_rule())
            .await
            .unwrap();
        assert_eq!(out, ReconcileOutcome::Enqueued);
        assert_eq!(
            r.publisher.published()[0].profile_config.video.codec,
            apsis_engine::VideoCodec::Av1,
            "rule-set codec reached the job"
        );
    }

    #[tokio::test]
    async fn failing_rule_marks_failed_and_stops_reprobing() {
        // A rule whose predicate references an unknown context field errors at eval.
        // The file must be recorded Failed@ver and NOT re-probed next pass (no loop).
        let bad = serde_json::from_str::<Profile>(
            r#"{"video":{"codec":"hevc","skip_codecs":["hevc"]},"audio":{},"subtitles":{},
                "output":{"container":"mkv"},
                "rule":[{"when":"video.bogus > 0","set":{"video.codec":"av1"}}]}"#,
        )
        .unwrap();
        let (r, calls) = reconciler(video("h264"));
        let out = r
            .reconcile_file("/hdd/tv/bad.mkv", "5:5", &library(), &bad)
            .await
            .unwrap();
        assert_eq!(out, ReconcileOutcome::OverrideFailed);
        assert!(r.publisher.is_empty(), "no job for a failed override");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Second pass, same version → gated by the Failed entry, NOT re-probed.
        let out = r
            .reconcile_file("/hdd/tv/bad.mkv", "5:5", &library(), &bad)
            .await
            .unwrap();
        assert_eq!(out, ReconcileOutcome::Skipped);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "failed file not re-probed until it changes"
        );
    }

    // --- US3 state-control ops (spec 005 T016, unit level) ---

    fn sched_cfg(dir: &std::path::Path) -> apsis_common::config::SchedulerConfig {
        serde_json::from_value(serde_json::json!({
            "library": [{ "name": "tv", "path": dir.to_str().unwrap(), "profile": "tv" }],
            "profiles": { "tv": {
                "video": {"codec":"hevc","skip_codecs":["hevc"]},
                "audio": {}, "subtitles": {}, "output": {"container":"mkv"}
            }},
        }))
        .unwrap()
    }

    fn temp_media() -> (std::path::PathBuf, String) {
        let dir = std::env::temp_dir().join(format!(
            "apsis-sop-{}-{}",
            std::process::id(),
            ulid::Ulid::new()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x.mkv");
        std::fs::write(&f, b"x").unwrap();
        let path = f.to_str().unwrap().to_string();
        (dir, path)
    }

    #[tokio::test]
    async fn state_op_mark_done_then_requeue() {
        use apsis_common::control::{StateControlRequest, StateOp, StateOutcome};
        let (r, _) = reconciler(video("h264"));
        let (dir, path) = temp_media();
        let cfg = sched_cfg(&dir);

        let mark = StateControlRequest {
            path: path.clone(),
            op: StateOp::MarkDone,
        };
        assert_eq!(r.apply_state_op(&mark, &cfg).await, StateOutcome::Applied);
        assert_eq!(
            r.store.get(&path).await.unwrap().unwrap().0.status,
            Status::Done
        );

        let requeue = StateControlRequest {
            path: path.clone(),
            op: StateOp::Requeue,
        };
        assert_eq!(
            r.apply_state_op(&requeue, &cfg).await,
            StateOutcome::Applied
        );
        assert!(
            r.store.get(&path).await.unwrap().is_none(),
            "requeue cleared it"
        );

        // Requeue of an absent key → NotFound.
        let absent = StateControlRequest {
            path: "/nope.mkv".into(),
            op: StateOp::Requeue,
        };
        assert_eq!(
            r.apply_state_op(&absent, &cfg).await,
            StateOutcome::NotFound
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn state_op_force_enqueues_a_compliant_file() {
        use apsis_common::control::{StateControlRequest, StateOp, StateOutcome};
        // hevc source + hevc in skip_codecs → normally skipped; force enqueues it anyway.
        let (r, _) = reconciler(video("hevc"));
        let (dir, path) = temp_media();
        let cfg = sched_cfg(&dir);

        let force = StateControlRequest {
            path: path.clone(),
            op: StateOp::Force,
        };
        assert_eq!(r.apply_state_op(&force, &cfg).await, StateOutcome::Applied);
        assert_eq!(r.publisher.len(), 1, "force enqueued the compliant file");

        std::fs::remove_dir_all(&dir).ok();
    }
}
