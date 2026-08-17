//! Output verification (FR-007, research D3): before *any* replace, prove the
//! transcode is intact. ffmpeg's exit code alone isn't enough (it can 0-exit on a
//! truncated mux), so we re-probe the output and check expected streams, a
//! plausible size, and — when durations are known — that it isn't truncated.
//!
//! [`check`] is the pure decision over already-probed data; the ffprobe execution
//! that feeds it lands with the run loop.

use apsis_common::config::VerifyConfig;
use apsis_engine::{FilePlan, Probe};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub(crate) enum VerifyFailure {
    #[error("output has no video stream")]
    NoVideo,
    #[error("audio track count: expected {expected}, got {got}")]
    AudioCount { expected: usize, got: usize },
    #[error("subtitle track count: expected {expected}, got {got}")]
    SubtitleCount { expected: usize, got: usize },
    #[error("output is empty")]
    Empty,
    #[error("output {got} bytes exceeds {ratio}x input {input} bytes")]
    TooLarge { got: u64, input: u64, ratio: f64 },
    #[error("duration {got:.3}s differs from source {expected:.3}s by more than {tol:.3}s")]
    Duration { got: f64, expected: f64, tol: f64 },
}

/// Verify a probed output against the plan and config. `in_duration`/`out_duration`
/// are seconds when known (from ffprobe `-show_format`); the duration check is
/// skipped when either is absent.
///
/// # Errors
/// The first failing check, as a [`VerifyFailure`].
#[allow(clippy::cast_precision_loss)] // byte counts are < 2^53; f64 is exact there
pub(crate) fn check(
    output: &Probe,
    plan: &FilePlan,
    in_bytes: u64,
    out_bytes: u64,
    in_duration: Option<f64>,
    out_duration: Option<f64>,
    cfg: &VerifyConfig,
) -> Result<(), VerifyFailure> {
    if output.video.is_none() {
        return Err(VerifyFailure::NoVideo);
    }
    if output.audio.len() != plan.audio.len() {
        return Err(VerifyFailure::AudioCount {
            expected: plan.audio.len(),
            got: output.audio.len(),
        });
    }
    if output.subtitles.len() != plan.subtitles.len() {
        return Err(VerifyFailure::SubtitleCount {
            expected: plan.subtitles.len(),
            got: output.subtitles.len(),
        });
    }
    if out_bytes == 0 {
        return Err(VerifyFailure::Empty);
    }
    // Size ceiling: a bloated output (or larger-than-input) is a failed transcode.
    if out_bytes as f64 > in_bytes as f64 * cfg.max_size_ratio {
        return Err(VerifyFailure::TooLarge {
            got: out_bytes,
            input: in_bytes,
            ratio: cfg.max_size_ratio,
        });
    }
    // Truncation guard: the output must run about as long as the source.
    if let (Some(i), Some(o)) = (in_duration, out_duration) {
        let tol = cfg.duration_tolerance.as_secs_f64();
        if (i - o).abs() > tol {
            return Err(VerifyFailure::Duration {
                got: o,
                expected: i,
                tol,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use apsis_engine::{StreamInfo, plan};

    fn video() -> StreamInfo {
        StreamInfo {
            index: 0,
            codec_type: "video".into(),
            codec: "hevc".into(),
            ..Default::default()
        }
    }
    fn audio(index: u32) -> StreamInfo {
        StreamInfo {
            index,
            codec_type: "audio".into(),
            codec: "aac".into(),
            language: "eng".into(),
            channels: 2,
            ..Default::default()
        }
    }

    // A plan for a source with 1 video + 1 audio, hevc-in-skip so it's a copy.
    fn plan_1v1a() -> FilePlan {
        let profile = serde_json::from_str(
            r#"{"video":{"codec":"hevc","skip_codecs":["hevc"]},"audio":{},"subtitles":{},"output":{"container":"mkv"}}"#,
        )
        .unwrap();
        let src = Probe {
            video: Some(video()),
            audio: vec![audio(1)],
            ..Default::default()
        };
        plan("in.mkv", &src, &profile)
    }

    fn cfg() -> VerifyConfig {
        VerifyConfig::default() // duration_tolerance 1s, max_size_ratio 1.5
    }

    fn good_output() -> Probe {
        Probe {
            video: Some(video()),
            audio: vec![audio(1)],
            ..Default::default()
        }
    }

    #[test]
    fn passes_a_well_formed_output() {
        let p = plan_1v1a();
        assert!(
            check(
                &good_output(),
                &p,
                1000,
                400,
                Some(60.0),
                Some(60.3),
                &cfg()
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_missing_video() {
        let p = plan_1v1a();
        let out = Probe {
            video: None,
            audio: vec![audio(1)],
            ..Default::default()
        };
        assert_eq!(
            check(&out, &p, 1000, 400, None, None, &cfg()),
            Err(VerifyFailure::NoVideo)
        );
    }

    #[test]
    fn rejects_dropped_audio_track() {
        let p = plan_1v1a(); // expects 1 audio
        let out = Probe {
            video: Some(video()),
            audio: vec![], // ffmpeg dropped it
            ..Default::default()
        };
        assert!(matches!(
            check(&out, &p, 1000, 400, None, None, &cfg()),
            Err(VerifyFailure::AudioCount {
                expected: 1,
                got: 0
            })
        ));
    }

    #[test]
    fn rejects_empty_and_oversized() {
        let p = plan_1v1a();
        assert_eq!(
            check(&good_output(), &p, 1000, 0, None, None, &cfg()),
            Err(VerifyFailure::Empty)
        );
        // 1600 > 1000 * 1.5
        assert!(matches!(
            check(&good_output(), &p, 1000, 1600, None, None, &cfg()),
            Err(VerifyFailure::TooLarge { .. })
        ));
    }

    #[test]
    fn rejects_truncated_and_skips_when_unknown() {
        let p = plan_1v1a();
        // 60s source, 30s output → truncated (tol 1s).
        assert!(matches!(
            check(
                &good_output(),
                &p,
                1000,
                400,
                Some(60.0),
                Some(30.0),
                &cfg()
            ),
            Err(VerifyFailure::Duration { .. })
        ));
        // unknown durations → check skipped, passes.
        assert!(check(&good_output(), &p, 1000, 400, None, Some(30.0), &cfg()).is_ok());
    }
}
