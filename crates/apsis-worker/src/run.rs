//! Run ffmpeg for a plan → a temp beside the source (FR-007 same-fs), so the
//! later `rename` is atomic. The engine builds the transcode argv; the worker
//! overrides the output to the temp and adds `-progress` for liveness.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use apsis_engine::{Backend, EngineError, FilePlan, Profile};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use ulid::Ulid;

#[derive(Debug, Error)]
pub(crate) enum RunError {
    #[error("build ffmpeg command: {0}")]
    Build(#[from] EngineError),
    #[error("ffmpeg io: {0}")]
    Io(#[from] io::Error),
}

/// Result of one ffmpeg invocation. The temp is left on disk for the caller to
/// verify + install, or to clean up on failure.
pub(crate) struct RunOutcome {
    pub temp: PathBuf,
    pub success: bool,
    /// The child was killed by an operator cancel (spec 005 US2) — distinct from a
    /// plain failure so the caller does NOT fall back to CPU or record `Failed`.
    pub cancelled: bool,
    /// Last few KiB of stderr, for diagnostics on failure.
    pub stderr_tail: String,
}

/// One parsed `-progress` block from ffmpeg (spec 005 T010). `eta` is not computed —
/// the source duration isn't carried by the probe yet.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProgressTick {
    pub speed: f64,
    pub out_time_s: f64,
}

/// A temp path `.apsis-tmp-<ulid>.<ext>` in the same directory as `output`.
pub(crate) fn temp_path(output: &Path) -> PathBuf {
    let dir = output.parent().unwrap_or_else(|| Path::new("."));
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("mkv");
    dir.join(format!(".apsis-tmp-{}.{ext}", Ulid::new()))
}

/// Build the ffmpeg argv for `plan` via `backend`, writing to `temp` and emitting
/// machine-readable progress on stdout. `argv[0]` is the backend's ffmpeg path.
///
/// # Errors
/// If the engine cannot build the command for this plan.
pub(crate) fn build_argv(
    backend: &dyn Backend,
    plan: &FilePlan,
    profile: &Profile,
    temp: &Path,
) -> Result<Vec<String>, RunError> {
    let mut cmd = backend.build(plan, profile)?;
    cmd.set_output(temp.to_string_lossy().as_ref());
    let mut argv = cmd.build();
    // `-progress pipe:1` is an execution concern (liveness/stall detection),
    // injected by the worker right after the ffmpeg binary.
    argv.splice(1..1, ["-progress".to_string(), "pipe:1".to_string()]);
    Ok(argv)
}

/// Run `backend`'s command for `plan`, writing to a fresh temp beside the output.
///
/// stderr is drained concurrently (bounded tail); stdout (`-progress`) is watched
/// for a stall — no line within `stall_timeout` means a hung ffmpeg, which is
/// killed and reported as a non-success so the caller can fall back.
///
/// # Errors
/// Command-build or process-spawn/wait failures. A non-zero exit or a stall is
/// *not* an error — both surface via [`RunOutcome::success`] = `false`.
pub(crate) async fn run(
    backend: &dyn Backend,
    plan: &FilePlan,
    profile: &Profile,
    stall_timeout: Duration,
    cancel: &tokio::sync::Notify,
    progress: Option<&tokio::sync::mpsc::UnboundedSender<ProgressTick>>,
) -> Result<RunOutcome, RunError> {
    let output = PathBuf::from(&plan.output.output_path);
    let temp = temp_path(&output);
    let argv = build_argv(backend, plan, profile, &temp)?;

    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true) // if this future is ever dropped, don't orphan ffmpeg
        .spawn()?;

    // Drain stderr concurrently so its pipe can't deadlock while we watch stdout.
    let stderr = child.stderr.take().expect("stderr piped");
    let stderr_task = tokio::spawn(read_tail(stderr, 4096));

    let end = match wait_with_stall(&mut child, stall_timeout, cancel, progress).await {
        Ok(end) => end,
        Err(e) => {
            stderr_task.abort(); // don't leak the drain task on the error path
            let _ = std::fs::remove_file(&temp); // don't leak the partial temp
            return Err(e.into());
        }
    };
    let stderr_tail = stderr_task.await.unwrap_or_default();

    let (success, cancelled, stderr_tail) = match end {
        RunEnd::Exited(status) => (status.success(), false, stderr_tail),
        RunEnd::Stalled => (
            false,
            false,
            format!("ffmpeg stalled: no progress for {stall_timeout:?}, killed\n{stderr_tail}"),
        ),
        // An operator cancel: discard the partial temp here so a cancelled run never
        // leaves one for verify/install (the source is only ever touched at replace).
        RunEnd::Cancelled => {
            let _ = std::fs::remove_file(&temp);
            (false, true, "cancelled by operator".to_string())
        }
    };
    Ok(RunOutcome {
        temp,
        success,
        cancelled,
        stderr_tail,
    })
}

enum RunEnd {
    Exited(std::process::ExitStatus),
    Stalled,
    Cancelled,
}

/// Wait for the child while reading its `-progress` stdout. stdout EOF signals the
/// process is finishing (then we reap the exit status); if no progress line
/// arrives within `stall_timeout` the process is hung (e.g. a VAAPI/VCN driver
/// deadlock) — kill it and report a stall. `stall_timeout == 0` disables the watch.
async fn wait_with_stall(
    child: &mut tokio::process::Child,
    stall_timeout: Duration,
    cancel: &tokio::sync::Notify,
    progress: Option<&tokio::sync::mpsc::UnboundedSender<ProgressTick>>,
) -> io::Result<RunEnd> {
    let stdout = child.stdout.take().expect("stdout piped");
    let mut lines = BufReader::new(stdout).lines();
    // Accumulate `key=value` progress lines until a `progress=` boundary, then emit a tick.
    let (mut speed, mut out_time_s) = (0.0f64, 0.0f64);
    loop {
        // Read the next progress line, but also wake on a cancel: killing the child
        // (not dropping this future) preserves the "process must not be dropped mid-run"
        // invariant — the normal cleanup below still runs (spec 005 FR-006/008).
        let read = async {
            if stall_timeout.is_zero() {
                Ok(lines.next_line().await)
            } else {
                tokio::time::timeout(stall_timeout, lines.next_line()).await
            }
        };
        let line = tokio::select! {
            res = read => res,
            () = cancel.notified() => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Ok(RunEnd::Cancelled);
            }
        };
        let line = match line {
            Ok(res) => res,
            Err(_elapsed) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Ok(RunEnd::Stalled);
            }
        };
        match line {
            Ok(Some(l)) => {
                if let Some(tx) = progress {
                    parse_progress_line(&l, &mut speed, &mut out_time_s, tx);
                }
            }
            Ok(None) | Err(_) => break, // stdout closed → process finishing
        }
    }
    Ok(RunEnd::Exited(child.wait().await?))
}

/// Fold one ffmpeg `-progress` `key=value` line into the running `speed`/`out_time`, and on a
/// `progress=` boundary emit a [`ProgressTick`] (spec 005 T010). Send errors (no receiver)
/// are ignored — progress is fire-and-forget.
fn parse_progress_line(
    line: &str,
    speed: &mut f64,
    out_time_s: &mut f64,
    tx: &tokio::sync::mpsc::UnboundedSender<ProgressTick>,
) {
    let Some((key, val)) = line.split_once('=') else {
        return;
    };
    match key {
        "speed" => *speed = val.trim().trim_end_matches('x').parse().unwrap_or(*speed),
        "out_time_us" | "out_time_ms" => {
            // ffmpeg's `out_time_ms` is actually microseconds (historical misnomer).
            if let Ok(us) = val.trim().parse::<f64>() {
                *out_time_s = us / 1_000_000.0;
            }
        }
        "progress" => {
            let _ = tx.send(ProgressTick {
                speed: *speed,
                out_time_s: *out_time_s,
            });
        }
        _ => {}
    }
}

/// Read a stream, keeping only the last `cap` bytes (a UTF-8-lossy tail).
async fn read_tail<R: tokio::io::AsyncRead + Unpin>(mut reader: R, cap: usize) -> String {
    let mut buf = Vec::new();
    let _ = reader.read_to_end(&mut buf).await;
    let start = buf.len().saturating_sub(cap);
    String::from_utf8_lossy(&buf[start..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use apsis_engine::{CpuBackend, Probe, StreamInfo, plan};

    fn hevc_encode_plan(input: &str) -> (FilePlan, Profile) {
        let profile: Profile = serde_json::from_str(
            r#"{"video":{"codec":"hevc","skip_codecs":[]},"audio":{},"subtitles":{},"output":{"container":"mkv"}}"#,
        )
        .unwrap();
        let src = Probe {
            video: Some(StreamInfo {
                index: 0,
                codec_type: "video".into(),
                codec: "h264".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let p = plan(input, &src, &profile);
        (p, profile)
    }

    #[test]
    fn progress_lines_emit_a_tick_at_the_boundary() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (mut speed, mut out) = (0.0, 0.0);
        // Mid-block updates accumulate; only `progress=` emits.
        parse_progress_line("speed=1.5x", &mut speed, &mut out, &tx);
        parse_progress_line("out_time_us=5000000", &mut speed, &mut out, &tx);
        assert!(rx.try_recv().is_err(), "no tick before the boundary");
        parse_progress_line("progress=continue", &mut speed, &mut out, &tx);
        let t = rx.try_recv().expect("a tick at the boundary");
        assert!((t.speed - 1.5).abs() < f64::EPSILON);
        assert!((t.out_time_s - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn temp_path_sits_beside_output_with_dot_prefix() {
        let t = temp_path(Path::new("/hdd/tv/clip.mkv"));
        assert_eq!(t.parent().unwrap(), Path::new("/hdd/tv"));
        let name = t.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with(".apsis-tmp-"), "{name}");
        assert_eq!(t.extension().and_then(|e| e.to_str()), Some("mkv"));
    }

    #[test]
    fn build_argv_targets_temp_and_injects_progress() {
        let (p, profile) = hevc_encode_plan("/hdd/tv/clip.mkv");
        let cpu = CpuBackend {
            ffmpeg_path: "ffmpeg".into(),
        };
        let temp = Path::new("/hdd/tv/.apsis-tmp-z.mkv");
        let argv = build_argv(&cpu, &p, &profile, temp).unwrap();

        assert_eq!(argv[0], "ffmpeg");
        assert_eq!(&argv[1..3], &["-progress", "pipe:1"]);
        assert_eq!(
            argv.last().unwrap(),
            "/hdd/tv/.apsis-tmp-z.mkv",
            "output is the temp"
        );
        // CPU backend encodes to libx265 (hevc target).
        assert!(argv.iter().any(|a| a == "libx265"));
    }

    /// Real transcode of a generated clip. Requires ffmpeg (present in the
    /// devshell); skips gracefully otherwise.
    #[tokio::test]
    async fn runs_a_real_cpu_transcode() {
        if !apsis_common::testkit::ffmpeg_available() {
            eprintln!("skipping: ffmpeg not on PATH");
            return;
        }
        let dir = std::env::temp_dir().join(format!("apsis-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = apsis_common::testkit::sample_h264(&dir, "src.mkv", 1, "128x128");

        let (p, profile) = hevc_encode_plan(src.to_str().unwrap());
        let cpu = CpuBackend {
            ffmpeg_path: "ffmpeg".into(),
        };
        let out = run(
            &cpu,
            &p,
            &profile,
            Duration::from_secs(30),
            &tokio::sync::Notify::new(),
            None,
        )
        .await
        .unwrap();
        assert!(out.success, "transcode failed: {}", out.stderr_tail);
        assert!(!out.cancelled);
        assert!(out.temp.exists(), "temp output written");
        assert!(std::fs::metadata(&out.temp).unwrap().len() > 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn stall_kills_a_hung_process() {
        // `sleep` emits no stdout → no "progress" → the watch must kill it fast.
        let mut child = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let start = std::time::Instant::now();
        let end = wait_with_stall(
            &mut child,
            Duration::from_millis(300),
            &tokio::sync::Notify::new(),
            None,
        )
        .await
        .unwrap();
        assert!(matches!(end, RunEnd::Stalled));
        assert!(start.elapsed() < Duration::from_secs(5), "killed promptly");
    }

    #[tokio::test]
    async fn cancel_kills_a_running_process() {
        // A long-sleeping child (emits no progress, but stall is disabled) is killed
        // promptly when the cancel is fired.
        let mut child = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let cancel = std::sync::Arc::new(tokio::sync::Notify::new());
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            c2.notify_one();
        });
        let start = std::time::Instant::now();
        // stall disabled (0) → only the cancel can end it.
        let end = wait_with_stall(&mut child, Duration::ZERO, &cancel, None)
            .await
            .unwrap();
        assert!(matches!(end, RunEnd::Cancelled));
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "killed promptly on cancel"
        );
    }

    #[tokio::test]
    async fn progress_output_prevents_stall() {
        // Ticks every 100ms keep the 500ms watch alive; the process exits cleanly.
        let mut child = Command::new("sh")
            .args(["-c", "for i in 1 2 3 4 5; do echo tick; sleep 0.1; done"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let end = wait_with_stall(
            &mut child,
            Duration::from_millis(500),
            &tokio::sync::Notify::new(),
            None,
        )
        .await
        .unwrap();
        assert!(matches!(end, RunEnd::Exited(s) if s.success()));
    }
}
