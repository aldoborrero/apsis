//! Primary-then-fallback transcode (FR-006): try the hardware backend; on a
//! non-zero exit, retry once on the fallback (CPU) and record that it was used.
//! Generic over `&dyn Backend` so it serves VAAPI (rhea) or NVENC (sirius, 003).

use apsis_engine::{Backend, FilePlan, Profile};

use crate::run::{RunError, RunOutcome, run};

pub(crate) struct Transcoded {
    pub outcome: RunOutcome,
    pub used_fallback: bool,
}

/// Run `primary`; if it exits non-zero and a `fallback` is configured, discard
/// its temp and retry on the fallback. Returns the surviving outcome plus whether
/// the fallback ran.
///
/// # Errors
/// A spawn/wait failure (a non-zero ffmpeg exit is not an error — it drives the
/// fallback).
pub(crate) async fn transcode(
    primary: &dyn Backend,
    fallback: Option<&dyn Backend>,
    plan: &FilePlan,
    profile: &Profile,
) -> Result<Transcoded, RunError> {
    let first = run(primary, plan, profile).await?;
    let Some(fallback) = fallback else {
        return Ok(Transcoded {
            outcome: first,
            used_fallback: false,
        });
    };
    if first.success {
        return Ok(Transcoded {
            outcome: first,
            used_fallback: false,
        });
    }
    // Primary failed → drop its partial temp, retry on the fallback.
    let _ = std::fs::remove_file(&first.temp);
    let second = run(fallback, plan, profile).await?;
    Ok(Transcoded {
        outcome: second,
        used_fallback: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use apsis_engine::{CpuBackend, HardwareConfig, Probe, StreamInfo, VaapiBackend, plan};

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
        (plan(input, &src, &profile), profile)
    }

    /// A VAAPI backend pointed at a non-existent device fails at init; the CPU
    /// fallback must still produce a valid output (spec AS-3). Requires ffmpeg.
    #[tokio::test]
    async fn vaapi_failure_falls_back_to_cpu() {
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            eprintln!("skipping: ffmpeg not on PATH");
            return;
        }
        let dir = std::env::temp_dir().join(format!("apsis-fallback-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.mkv");
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
        assert!(made.status.success());

        let (p, profile) = hevc_encode_plan(src.to_str().unwrap());
        let vaapi = VaapiBackend {
            hardware: HardwareConfig::default(),
            vaapi_device: "/dev/dri/renderD999".into(), // does not exist → init fails
            ffmpeg_path: "ffmpeg".into(),
        };
        let cpu = CpuBackend {
            ffmpeg_path: "ffmpeg".into(),
        };

        let t = transcode(&vaapi, Some(&cpu as &dyn Backend), &p, &profile)
            .await
            .unwrap();
        assert!(t.used_fallback, "VAAPI should have failed and fallen back");
        assert!(
            t.outcome.success,
            "CPU fallback failed: {}",
            t.outcome.stderr_tail
        );
        assert!(t.outcome.temp.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn no_fallback_returns_primary_outcome() {
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            eprintln!("skipping: ffmpeg not on PATH");
            return;
        }
        let dir = std::env::temp_dir().join(format!("apsis-nofb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.mkv");
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

        let (p, profile) = hevc_encode_plan(src.to_str().unwrap());
        let cpu = CpuBackend {
            ffmpeg_path: "ffmpeg".into(),
        };
        // primary = CPU, no fallback configured.
        let t = transcode(&cpu, None, &p, &profile).await.unwrap();
        assert!(!t.used_fallback);
        assert!(t.outcome.success);
        std::fs::remove_dir_all(&dir).ok();
    }
}
