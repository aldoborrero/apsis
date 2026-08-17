//! The worker loop: pull a job, claim it (KV CAS → `InProgress`), transcode
//! (primary → CPU fallback), verify, atomically replace, and publish the result.
//! Ties together run/fallback/verify/replace with the shared NATS state store.
//!
//! Concurrency is the AMD-VCN default of 1 (sequential); `max_ack_pending` bounds
//! in-flight jobs broker-side. Parallel workers (>1) land with the Semaphore fan-out.

use std::path::{Path, PathBuf};
use std::time::Instant;

use apsis_common::config::{BackendConfig, BackendKind, VerifyConfig, WorkerConfig};
use apsis_common::{
    ConsumerTuning, Job, KvStateStore, Outcome, PathMap, StateEntry, StateStore, Status,
    StoreError, TranscodeResult, bind_job_consumer, publish_result,
};
use apsis_engine::{Backend, CpuBackend, VaapiBackend};
use async_nats::jetstream::AckKind;
use async_nats::jetstream::Context;
use futures::StreamExt;
use time::OffsetDateTime;

use crate::fallback::transcode;
use crate::verify::{check, probe_output};

/// Top-level worker error: at the daemon boundary we log and nak, so a boxed
/// dyn error keeps the `?` composition simple across NATS/IO/store/serde.
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub(crate) struct Worker {
    ctx: Context,
    client: async_nats::Client,
    kv: KvStateStore,
    primary: Box<dyn Backend>,
    fallback: Option<Box<dyn Backend>>,
    path_map: PathMap,
    verify: VerifyConfig,
    ffprobe: PathBuf,
}

fn build_backend(bc: &BackendConfig, ffmpeg: &str) -> Box<dyn Backend> {
    match bc.kind {
        BackendKind::Vaapi => Box::new(VaapiBackend {
            hardware: bc.hardware.clone(),
            vaapi_device: bc
                .device
                .clone()
                .unwrap_or_else(|| "/dev/dri/renderD128".to_string()),
            ffmpeg_path: ffmpeg.to_string(),
        }),
        BackendKind::Cpu => Box::new(CpuBackend {
            ffmpeg_path: ffmpeg.to_string(),
        }),
    }
}

impl Worker {
    /// Build a worker from validated config. `cfg.backends` is non-empty
    /// (config validation guarantees it); the first is primary, the second (if
    /// any) is the fallback.
    pub(crate) fn new(
        client: async_nats::Client,
        ctx: Context,
        kv: KvStateStore,
        cfg: &WorkerConfig,
    ) -> Self {
        let ffmpeg = cfg.ffmpeg.to_string_lossy();
        let mut it = cfg.backends.iter().map(|b| build_backend(b, &ffmpeg));
        let primary = it.next().expect("config guarantees >= 1 backend");
        let fallback = it.next();
        Self {
            ctx,
            client,
            kv,
            primary,
            fallback,
            path_map: PathMap::new(cfg.path_map.clone()),
            verify: cfg.verify.clone(),
            ffprobe: cfg.ffprobe.clone(),
        }
    }

    /// Serve jobs until the stream ends. Each message: process, then ack on a
    /// terminal outcome (done/failed-recorded) or nak on a retriable error.
    ///
    /// # Errors
    /// If the consumer can't be bound or the message stream errors fatally.
    pub(crate) async fn run(&self, tuning: &ConsumerTuning) -> Result<()> {
        let consumer = bind_job_consumer(&self.ctx, tuning).await?;
        let mut messages = consumer.messages().await?;
        while let Some(msg) = messages.next().await {
            let msg = msg?;
            let Ok(job) = serde_json::from_slice::<Job>(&msg.payload) else {
                eprintln!("apsis-worker: undecodable job dropped");
                msg.ack().await?; // will never decode → drain
                continue;
            };
            // 1-based delivery attempt; the last one dead-letters on failure.
            let attempt = msg.info().map_or(1, |i| i.delivered.max(1));
            let max_deliver = tuning.max_deliver.max(1);

            // Extend the ack deadline while a long transcode runs (T025): heartbeat
            // Progress until process() finishes, so NATS doesn't redeliver mid-encode.
            let outcome = {
                let hb = heartbeat(&msg, tuning.ack_wait);
                tokio::pin!(hb);
                tokio::select! {
                    r = self.process(&job) => r,
                    () = &mut hb => Err("heartbeat ended (connection lost) mid-process".into()),
                }
            };

            match outcome {
                Ok(_) => msg.ack().await?, // terminal (done or recorded-failed) → drain
                Err(e) if attempt >= max_deliver => {
                    // Dead-letter (FR-009): record Failed@version so the version gate
                    // suppresses re-queue until the file changes, then drain.
                    eprintln!(
                        "apsis-worker: job {} dead-lettered after {attempt}: {e}",
                        job.id
                    );
                    let _ = self
                        .finish(
                            &job,
                            &job.path,
                            Outcome::Failed,
                            false,
                            0,
                            0,
                            0.0,
                            Some(e.to_string()),
                        )
                        .await;
                    msg.ack().await?;
                }
                Err(e) => {
                    eprintln!(
                        "apsis-worker: job {} retriable ({attempt}/{max_deliver}): {e}",
                        job.id
                    );
                    msg.ack_with(AckKind::Nak(None)).await?; // redeliver
                }
            }
        }
        Ok(())
    }

    /// Process one job end-to-end. Returns the terminal [`Outcome`]; a retriable
    /// error (broker/IO) is returned as `Err` so the caller naks.
    #[allow(clippy::too_many_lines)] // a faithful sequential pipeline reads best whole
    pub(crate) async fn process(&self, job: &Job) -> Result<Outcome> {
        let key = job.path.as_str();
        if !self.claim(key, job).await? {
            return Ok(Outcome::Failed); // stale version or lost claim → drop
        }

        let started = Instant::now();
        let mut plan = job.plan.clone();
        let local_in = self.path_map.translate(&plan.output.input_path);
        let local_out = self.path_map.translate(&plan.output.output_path);
        plan.output.input_path.clone_from(&local_in);
        plan.output.output_path.clone_from(&local_out);
        let in_bytes = std::fs::metadata(&local_in).map_or(0, |m| m.len());

        let t = transcode(
            self.primary.as_ref(),
            self.fallback.as_deref(),
            &plan,
            &job.profile_config,
        )
        .await?;

        // Transcode failed on all backends → terminal, record + drop the temp.
        if !t.outcome.success {
            let _ = std::fs::remove_file(&t.outcome.temp);
            let secs = started.elapsed().as_secs_f64();
            self.finish(
                job,
                key,
                Outcome::Failed,
                t.used_fallback,
                in_bytes,
                0,
                secs,
                Some(t.outcome.stderr_tail),
            )
            .await?;
            return Ok(Outcome::Failed);
        }

        // Verify the output before touching the original.
        let out_bytes = std::fs::metadata(&t.outcome.temp).map_or(0, |m| m.len());
        let (probe, out_dur) = probe_output(&self.ffprobe, &t.outcome.temp)
            .await
            .unwrap_or_default();
        let (_src, in_dur) = probe_output(&self.ffprobe, Path::new(&local_in))
            .await
            .unwrap_or_default();
        if let Err(vf) = check(
            &probe,
            &plan,
            in_bytes,
            out_bytes,
            in_dur,
            out_dur,
            &self.verify,
        ) {
            let _ = std::fs::remove_file(&t.outcome.temp);
            metrics::counter!("apsis_verify_failures_total").increment(1);
            let secs = started.elapsed().as_secs_f64();
            self.finish(
                job,
                key,
                Outcome::Failed,
                t.used_fallback,
                in_bytes,
                out_bytes,
                secs,
                Some(vf.to_string()),
            )
            .await?;
            return Ok(Outcome::Failed);
        }

        // Verified → atomically install, preserving the original's metadata.
        if let Err(e) = crate::replace::atomic_replace(
            Path::new(&local_in),
            &t.outcome.temp,
            Path::new(&local_out),
        ) {
            let _ = std::fs::remove_file(&t.outcome.temp);
            let secs = started.elapsed().as_secs_f64();
            self.finish(
                job,
                key,
                Outcome::Failed,
                t.used_fallback,
                in_bytes,
                out_bytes,
                secs,
                Some(e.to_string()),
            )
            .await?;
            return Ok(Outcome::Failed);
        }

        let secs = started.elapsed().as_secs_f64();
        self.finish(
            job,
            key,
            Outcome::Done,
            t.used_fallback,
            in_bytes,
            out_bytes,
            secs,
            None,
        )
        .await?;
        Ok(Outcome::Done)
    }

    /// Claim the file by CAS-ing its state to `InProgress` at the job's version.
    /// Returns `false` (don't process) if the file changed since planning, or
    /// another worker won the claim.
    async fn claim(&self, key: &str, job: &Job) -> Result<bool> {
        let entry = Self::entry(job, Status::InProgress, false, None);
        match self.kv.get(key).await? {
            Some((cur, rev)) => {
                if cur.version != job.version {
                    return Ok(false); // file changed since the job was planned
                }
                match self.kv.update(key, &entry, rev).await {
                    Ok(_) => Ok(true),
                    Err(StoreError::Conflict(_)) => Ok(false),
                    Err(e) => Err(e.into()),
                }
            }
            None => match self.kv.create(key, &entry).await {
                Ok(_) => Ok(true),
                Err(StoreError::Conflict(_)) => Ok(false),
                Err(e) => Err(e.into()),
            },
        }
    }

    /// Write the terminal KV state and publish the result event.
    #[allow(clippy::too_many_arguments)]
    async fn finish(
        &self,
        job: &Job,
        key: &str,
        outcome: Outcome,
        used_fallback: bool,
        in_bytes: u64,
        out_bytes: u64,
        duration_secs: f64,
        error: Option<String>,
    ) -> Result<()> {
        let status = match outcome {
            Outcome::Done => Status::Done,
            Outcome::Failed => Status::Failed,
        };

        // Metrics (US4). Counters/histograms are global once the exporter is
        // installed; scraped from the worker's /metrics endpoint.
        let outcome_str = if matches!(outcome, Outcome::Done) {
            "done"
        } else {
            "failed"
        };
        metrics::counter!("apsis_jobs_total", "outcome" => outcome_str).increment(1);
        metrics::histogram!("apsis_transcode_seconds").record(duration_secs);
        if used_fallback {
            metrics::counter!("apsis_used_fallback_total").increment(1);
        }
        if matches!(outcome, Outcome::Done) && in_bytes > out_bytes {
            metrics::counter!("apsis_bytes_saved_total").increment(in_bytes - out_bytes);
        }

        self.kv
            .put(key, &Self::entry(job, status, used_fallback, error.clone()))
            .await?;
        let result = TranscodeResult {
            job_id: job.id,
            path: job.path.clone(),
            version: job.version.clone(),
            outcome,
            used_fallback,
            input_bytes: in_bytes,
            output_bytes: out_bytes,
            duration_secs,
            error,
        };
        publish_result(&self.client, &result).await?;
        Ok(())
    }

    fn entry(
        job: &Job,
        status: Status,
        used_fallback: bool,
        last_error: Option<String>,
    ) -> StateEntry {
        StateEntry {
            status,
            version: job.version.clone(),
            job_id: Some(job.id),
            attempts: 1,
            used_fallback,
            updated_at: OffsetDateTime::now_utc(),
            last_error,
        }
    }
}

/// Emit `AckKind::Progress` every `ack_wait/2` to keep the lease alive during a
/// long transcode. Loops until dropped (process finished) or the connection drops.
async fn heartbeat(msg: &async_nats::jetstream::Message, ack_wait: std::time::Duration) {
    let period = (ack_wait / 2).max(std::time::Duration::from_secs(1));
    loop {
        tokio::time::sleep(period).await;
        if msg.ack_with(AckKind::Progress).await.is_err() {
            break; // connection gone — let the select arm surface a retriable error
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apsis_common::{connect, ensure_topology};
    use apsis_engine::{Probe, Profile, StreamInfo, plan};
    use std::time::Duration;

    fn ffmpeg_available() -> bool {
        std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_ok()
    }

    fn cpu_worker_config() -> WorkerConfig {
        // CPU-only (no VAAPI device in CI/sandbox); identity path_map + defaults.
        serde_json::from_str(r#"{"concurrency":1,"backend":[{"kind":"cpu"}]}"#).unwrap()
    }

    /// End-to-end: a non-compliant clip is transcoded and safely replaced, and KV
    /// records Done (SC-001, worker half). Requires a running nats-server
    /// (process-compose) + ffmpeg; skips otherwise.
    #[tokio::test]
    async fn transcodes_and_replaces_end_to_end() {
        if !ffmpeg_available() {
            eprintln!("skipping: ffmpeg not on PATH");
            return;
        }
        let url = std::env::var("APSIS_TEST_NATS")
            .unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());
        let Ok(Ok((client, ctx))) =
            tokio::time::timeout(Duration::from_secs(2), connect(&url)).await
        else {
            eprintln!("skipping: no nats-server at {url} (run `process-compose up`)");
            return;
        };
        let tuning = ConsumerTuning::for_concurrency(1);
        let kv = ensure_topology(&ctx, &tuning).await.unwrap();

        // Generate a tiny h264 clip and plan it to hevc (encode).
        let dir = std::env::temp_dir().join(format!("apsis-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("clip.mkv");
        std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=d=1:s=128x128",
                "-c:v",
                "libx264",
            ])
            .arg(&src)
            .output()
            .unwrap();
        let profile: Profile = serde_json::from_str(
            r#"{"video":{"codec":"hevc","skip_codecs":[]},"audio":{},"subtitles":{},"output":{"container":"mkv"}}"#,
        )
        .unwrap();
        let probe = Probe {
            video: Some(StreamInfo {
                index: 0,
                codec_type: "video".into(),
                codec: "h264".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let file_plan = plan(src.to_str().unwrap(), &probe, &profile);
        let job = Job {
            id: ulid::Ulid::new(),
            path: src.to_string_lossy().into_owned(),
            version: "1:1".into(),
            profile: "test".into(),
            plan: file_plan,
            profile_config: profile,
            enqueued_at: OffsetDateTime::UNIX_EPOCH,
        };

        let cfg = cpu_worker_config();
        let worker = Worker::new(client, ctx.clone(), KvStateStore::new(kv), &cfg);

        let outcome = worker.process(&job).await.unwrap();
        assert_eq!(outcome, Outcome::Done, "expected a successful transcode");
        assert!(src.exists(), "original replaced in place");
        // KV records Done at the job's version.
        let (entry, _) = worker
            .kv
            .get(&job.path)
            .await
            .unwrap()
            .expect("state written");
        assert_eq!(entry.status, Status::Done);
        assert_eq!(entry.version, "1:1");
        std::fs::remove_dir_all(&dir).ok();
    }
}
