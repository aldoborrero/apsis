//! Gated crash-safety integration tests (spec 002 T028 + T029).
//!
//! Both drive the *built* `apsis-worker` binary against a real `nats-server`
//! (`APSIS_TEST_NATS`, or the process-compose default) so they exercise the true
//! lease/redelivery machinery, not an in-process shortcut. They skip cleanly when
//! no server (or, for T028, no ffmpeg) is available.
//!
//! The worker binary uses the ONE hardcoded stream/consumer, so the two tests
//! would stomp each other's topology if run in parallel — a module-level async
//! lock serialises them (same test binary ⇒ same process ⇒ the static is shared).

use std::path::Path;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use apsis_common::nats::{CONSUMER_NAME, STREAM_NAME};
use apsis_common::{
    ConsumerTuning, Job, JobPublisher, KvStateStore, NatsPublisher, StateEntry, StateStore, Status,
    connect, ensure_topology,
};
use apsis_engine::{Probe, Profile, StreamInfo, plan};
use async_nats::jetstream::Context;
use async_nats::jetstream::kv::Store;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};

const WORKER_BIN: &str = env!("CARGO_BIN_EXE_apsis-worker");

/// Serialise the two tests — they share the hardcoded `APSIS_JOBS`/`worker-local`
/// topology, which each resets at start.
fn topology_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn nats_url() -> String {
    std::env::var("APSIS_TEST_NATS").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string())
}

fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok()
}

/// Fast tuning for the initial provision — matches the `[consumer]` block the
/// worker binary reapplies (so the shared consumer ends up with a 5s lease /
/// immediate redelivery / 3 deliveries, not the production 30-min defaults).
fn test_tuning() -> ConsumerTuning {
    ConsumerTuning {
        ack_wait: Duration::from_secs(5),
        max_deliver: 3,
        max_ack_pending: 1,
        backoff: vec![],
    }
}

/// Drop any pre-existing consumer + queued jobs (the names are shared across runs)
/// and re-provision with `tuning`. `get_or_create_consumer` never reconciles drift,
/// so deleting first is the only way our short lease/backoff actually takes effect
/// over whatever a prior run/smoke left behind.
async fn reset_topology(ctx: &Context, tuning: &ConsumerTuning) -> Store {
    if let Ok(stream) = ctx.get_stream(STREAM_NAME).await {
        let _ = stream.delete_consumer(CONSUMER_NAME).await;
        let _ = stream.purge().await;
    }
    ensure_topology(ctx, tuning)
        .await
        .expect("provision test topology")
}

/// A job that plans an h264 source to hevc (an encode — never a skip).
fn hevc_job(input: &str) -> Job {
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
    let file_plan = plan(input, &probe, &profile);
    // The version MUST be the source's real change token: atomic_replace verifies
    // the file it swaps in for still matches it (the changed-source guard). A fake
    // token would read as "superseded" and never install. (Missing file — T029's
    // poison job never reaches replace — falls back to a placeholder.)
    let version =
        apsis_common::version_token(Path::new(input)).unwrap_or_else(|_| "0:0".to_string());
    Job {
        id: ulid::Ulid::new(),
        path: input.to_string(),
        version,
        profile: "test".into(),
        plan: file_plan,
        profile_config: profile,
        enqueued_at: time::OffsetDateTime::UNIX_EPOCH,
    }
}

fn spawn_worker(cfg: &Path, url: &str, metrics_port: u16, pipe_stderr: bool) -> Child {
    Command::new(WORKER_BIN)
        .env("APSIS_WORKER_CONFIG", cfg)
        .env("NATS_URL", url)
        .env("APSIS_METRICS_ADDR", format!("127.0.0.1:{metrics_port}"))
        .stdout(Stdio::null())
        .stderr(if pipe_stderr {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .kill_on_drop(true)
        .spawn()
        .expect("spawn apsis-worker binary")
}

/// Poll KV until the file's status is `want`; `None` on timeout.
async fn wait_status(
    kv: &KvStateStore,
    path: &str,
    want: Status,
    within: Duration,
) -> Option<StateEntry> {
    timeout(within, async {
        loop {
            if let Ok(Some((entry, _))) = kv.get(path).await
                && entry.status == want
            {
                return entry;
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .ok()
}

fn ffprobe_video_codec(path: &Path) -> Option<String> {
    let out = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-select_streams",
            "v",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .ok()?;
    let codec = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!codec.is_empty()).then_some(codec)
}

/// T028 / SC-003: `kill -9` the worker mid-transcode → the lease expires, the job
/// is redelivered, a fresh worker re-claims the abandoned `InProgress` and
/// completes — and the original is byte-identical across the crash (no torn write).
#[tokio::test]
async fn crash_mid_transcode_redelivers_and_completes() {
    let _guard = topology_lock().lock().await;
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg not on PATH");
        return;
    }
    let url = nats_url();
    let Ok(Ok((_client, ctx))) = timeout(Duration::from_secs(2), connect(&url)).await else {
        eprintln!("skipping: no nats-server at {url} (run `process-compose up`)");
        return;
    };
    let tuning = test_tuning();
    let kv = KvStateStore::new(reset_topology(&ctx, &tuning).await);

    // A real h264 clip long enough that the encode is still running when we detect
    // the claim and kill (hevc is slow — 6s @ 480p never finishes sub-second).
    let dir = std::env::temp_dir().join(format!("apsis-crash-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("clip.mkv");
    let made = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=d=6:s=640x480",
            "-c:v",
            "libx264",
        ])
        .arg(&src)
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "gen: {}",
        String::from_utf8_lossy(&made.stderr)
    );
    let original = std::fs::read(&src).unwrap();

    // Fast lease so the killed worker's job redelivers in ~5s (the worker binary
    // applies this tuning, updating the shared consumer — see reset_topology).
    let cfg = dir.join("worker.toml");
    std::fs::write(
        &cfg,
        "concurrency = 1\n[[backend]]\nkind = \"cpu\"\n\
         [consumer]\nack_wait = \"5s\"\nmax_deliver = 3\nbackoff = []\n",
    )
    .unwrap();

    let job = hevc_job(src.to_str().unwrap());
    NatsPublisher::new(ctx.clone())
        .publish(&job)
        .await
        .expect("publish job");

    // Worker 1: claim + start encoding, then SIGKILL mid-run.
    let mut w1 = spawn_worker(&cfg, &url, 19_311, false);
    let claimed = wait_status(&kv, &job.path, Status::InProgress, Duration::from_secs(20)).await;
    assert!(
        claimed.is_some(),
        "worker never claimed the job (InProgress)"
    );
    sleep(Duration::from_millis(300)).await; // let ffmpeg write a partial temp
    w1.start_kill().expect("SIGKILL worker 1");
    let _ = w1.wait().await;

    // Nothing was installed mid-encode → the original is byte-identical.
    assert_eq!(
        std::fs::read(&src).unwrap(),
        original,
        "source corrupted or replaced by the crash"
    );

    // Worker 2: after the lease expires the job redelivers; re-claim + complete.
    let mut w2 = spawn_worker(&cfg, &url, 19_312, false);
    let done = wait_status(&kv, &job.path, Status::Done, Duration::from_secs(90)).await;
    let _ = w2.start_kill();
    let _ = w2.wait().await;

    let done = done.expect("job never completed after redelivery");
    assert_eq!(
        done.version, job.version,
        "Done recorded at the job's version"
    );
    assert_eq!(
        ffprobe_video_codec(&src).as_deref(),
        Some("hevc"),
        "final file is not a valid hevc transcode"
    );
    assert_ne!(
        std::fs::read(&src).unwrap(),
        original,
        "source unchanged — it was never actually transcoded"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// T029 / SC-004: an always-failing (retriable) job is redelivered exactly
/// `max_deliver` times, then lands `Failed@version`. A bogus ffmpeg path makes the
/// process spawn fail on every delivery (an Io error → nak → redeliver), so no real
/// ffmpeg or clip is needed.
#[tokio::test]
async fn poison_job_dead_letters_after_max_deliver() {
    let _guard = topology_lock().lock().await;
    let url = nats_url();
    let Ok(Ok((_client, ctx))) = timeout(Duration::from_secs(2), connect(&url)).await else {
        eprintln!("skipping: no nats-server at {url} (run `process-compose up`)");
        return;
    };
    let tuning = test_tuning();
    let kv = KvStateStore::new(reset_topology(&ctx, &tuning).await);

    let dir = std::env::temp_dir().join(format!("apsis-poison-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Bogus ffmpeg → every delivery's spawn fails (Io) → nak → redeliver. Empty
    // backoff makes redelivery immediate so 3 deliveries take milliseconds.
    let cfg = dir.join("worker.toml");
    std::fs::write(
        &cfg,
        "concurrency = 1\nffmpeg = \"/nonexistent/apsis-ffmpeg\"\n[[backend]]\nkind = \"cpu\"\n\
         [consumer]\nack_wait = \"5s\"\nmax_deliver = 3\nbackoff = []\n",
    )
    .unwrap();

    let job = hevc_job(dir.join("ghost.mkv").to_str().unwrap());
    NatsPublisher::new(ctx.clone())
        .publish(&job)
        .await
        .expect("publish job");

    let mut w = spawn_worker(&cfg, &url, 19_313, true);
    let failed = wait_status(&kv, &job.path, Status::Failed, Duration::from_secs(30)).await;

    // Stop the worker and read its stderr to assert the redelivery sequence.
    let _ = w.start_kill();
    let out = w.wait_with_output().await.expect("collect worker output");
    let logs = String::from_utf8_lossy(&out.stderr);

    let failed = failed.unwrap_or_else(|| panic!("job never reached Failed. worker logs:\n{logs}"));
    assert_eq!(
        failed.version, job.version,
        "Failed recorded at the job's version (suppresses re-queue until mtime:size changes)"
    );
    // Redelivered exactly max_deliver (=3): deliveries 1 & 2 retriable, the 3rd
    // dead-lettered. The worker prints one line per delivery.
    assert!(
        logs.contains("retriable (1/3)"),
        "missing 1st redelivery. logs:\n{logs}"
    );
    assert!(
        logs.contains("retriable (2/3)"),
        "missing 2nd redelivery. logs:\n{logs}"
    );
    assert!(
        logs.contains("dead-lettered after 3"),
        "not dead-lettered at max_deliver. logs:\n{logs}"
    );

    std::fs::remove_dir_all(&dir).ok();
}
