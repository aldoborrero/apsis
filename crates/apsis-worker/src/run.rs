//! Run ffmpeg for a plan → a temp beside the source (FR-007 same-fs), so the
//! later `rename` is atomic. The engine builds the transcode argv; the worker
//! overrides the output to the temp and adds `-progress` for liveness.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

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
    /// Last few KiB of stderr, for diagnostics on failure.
    pub stderr_tail: String,
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
/// Both pipes are drained concurrently so ffmpeg can't deadlock on a full buffer.
///
/// # Errors
/// Command-build or process-spawn/wait failures. A non-zero ffmpeg exit is *not*
/// an error — it is reported via [`RunOutcome::success`] so the caller can fall
/// back to CPU.
pub(crate) async fn run(
    backend: &dyn Backend,
    plan: &FilePlan,
    profile: &Profile,
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

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let progress = tokio::spawn(drain_progress(stdout));
    let stderr_tail = tokio::spawn(read_tail(stderr, 4096));

    let status = child.wait().await.map_err(|e| {
        let _ = std::fs::remove_file(&temp); // don't leak the partial temp
        RunError::from(e)
    })?;
    let _ = progress.await;
    let stderr_tail = stderr_tail.await.unwrap_or_default();

    Ok(RunOutcome {
        temp,
        success: status.success(),
        stderr_tail,
    })
}

/// Consume ffmpeg's `-progress` stream so the pipe never blocks. (Stall detection
/// on `out_time`/`speed` is a later refinement; for now we just drain.)
async fn drain_progress<R: tokio::io::AsyncRead + Unpin>(reader: R) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(_line)) = lines.next_line().await {}
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
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            eprintln!("skipping: ffmpeg not on PATH");
            return;
        }
        let dir = std::env::temp_dir().join(format!("apsis-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.mkv");
        // Generate a tiny h264 source.
        let made = std::process::Command::new("ffmpeg")
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
        assert!(
            made.status.success(),
            "gen: {}",
            String::from_utf8_lossy(&made.stderr)
        );

        let (p, profile) = hevc_encode_plan(src.to_str().unwrap());
        let cpu = CpuBackend {
            ffmpeg_path: "ffmpeg".into(),
        };
        let out = run(&cpu, &p, &profile).await.unwrap();
        assert!(out.success, "transcode failed: {}", out.stderr_tail);
        assert!(out.temp.exists(), "temp output written");
        assert!(std::fs::metadata(&out.temp).unwrap().len() > 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
