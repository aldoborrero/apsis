//! US3 — skip already-compliant files cheaply, and never drop all audio.
//!
//! Exercises the public API only (integration test). Asserts the skip
//! invariants across a scenario table and the never-drop-all-audio guard
//! (FR-006) at the plan level.

use apsis_engine::{PlanStatus, Probe, Profile, StreamInfo, VideoAction, plan};

fn profile(json: &str) -> Profile {
    serde_json::from_str(json).unwrap()
}

fn video(codec: &str) -> StreamInfo {
    StreamInfo {
        index: 0,
        codec_type: "video".into(),
        codec: codec.into(),
        ..Default::default()
    }
}

fn audio(index: u32, lang: &str, channels: u32, codec: &str) -> StreamInfo {
    StreamInfo {
        index,
        codec_type: "audio".into(),
        codec: codec.into(),
        language: lang.into(),
        channels,
        ..Default::default()
    }
}

/// hevc is in `skip_codecs`, so an all-hevc/mkv file with no audio is compliant.
const SKIP_HEVC: &str = r#"{"video":{"codec":"hevc","skip_codecs":["hevc"]},"audio":{},"subtitles":{},"output":{"container":"mkv"}}"#;

#[test]
fn skip_invariants_hold_across_scenarios() {
    // (label, input_path, probe, profile-json)
    let compliant = Probe {
        video: Some(video("hevc")),
        ..Default::default()
    };
    let needs_encode = Probe {
        video: Some(video("h264")),
        ..Default::default()
    };
    let container_mismatch = Probe {
        video: Some(video("hevc")),
        ..Default::default()
    };
    let no_video = Probe::default();

    let cases: [(&str, &str, &Probe, &str); 4] = [
        ("compliant", "x.mkv", &compliant, SKIP_HEVC),
        ("needs-encode", "x.mkv", &needs_encode, SKIP_HEVC),
        (
            "container-mismatch",
            "x.mp4",
            &container_mismatch,
            SKIP_HEVC,
        ),
        ("no-video", "x.mkv", &no_video, SKIP_HEVC),
    ];

    for (label, path, probe, prof) in cases {
        let p = plan(path, probe, &profile(prof));

        // Universal: `compliant` is exactly "no reasons".
        assert_eq!(
            p.compliant,
            p.reasons.is_empty(),
            "{label}: compliant must equal reasons.is_empty()"
        );
        // Universal: `compliant` iff status is Compliant.
        assert_eq!(
            p.compliant,
            p.status == PlanStatus::Compliant,
            "{label}: compliant must track the Compliant status"
        );
        // Skip iff there is no transcode to run — i.e. anything but ChangesRequired.
        // That covers BOTH compliant files and unsupported ones (no video).
        assert_eq!(
            p.should_skip,
            p.status != PlanStatus::ChangesRequired,
            "{label}: should_skip must hold for compliant AND unsupported, only ChangesRequired builds"
        );
    }
}

#[test]
fn compliant_file_skips_and_needs_no_command() {
    let probe = Probe {
        video: Some(video("hevc")),
        ..Default::default()
    };
    let p = plan("x.mkv", &probe, &profile(SKIP_HEVC));

    assert_eq!(p.status, PlanStatus::Compliant);
    assert!(p.should_skip, "a compliant file must skip");
    assert!(p.reasons.is_empty(), "a compliant file has no reasons");
    assert_eq!(p.video.action, VideoAction::Copy);
    // Contract: the caller skips command building entirely when `should_skip`.
    // No FfmpegCommand is constructed for this file.
}

#[test]
fn unsupported_file_skips_but_is_not_compliant() {
    let p = plan("x.mkv", &Probe::default(), &profile(SKIP_HEVC));
    assert_eq!(p.status, PlanStatus::Unsupported);
    assert!(p.should_skip, "unsupported (no video) still skips");
    assert!(!p.compliant, "unsupported is not compliant");
    assert!(
        !p.reasons.is_empty(),
        "unsupported carries a NoVideoStream reason"
    );
}

#[test]
fn never_drops_all_audio_on_keep_languages_miss() {
    // Two audio tracks, but keep_languages matches neither → the guard (FR-006)
    // falls back to keeping every track rather than emitting a video-only file.
    let probe = Probe {
        video: Some(video("hevc")),
        audio: vec![audio(1, "eng", 2, "aac"), audio(2, "spa", 2, "aac")],
        ..Default::default()
    };
    let prof = profile(
        r#"{"video":{"codec":"hevc","skip_codecs":["hevc"]},"audio":{"keep_languages":["fre"]},"subtitles":{},"output":{"container":"mkv"}}"#,
    );
    let p = plan("x.mkv", &probe, &prof);

    assert_eq!(
        p.audio.len(),
        2,
        "keep_languages miss must not drop all audio (FR-006)"
    );
    assert!(
        !p.audio.is_empty(),
        "the output plan must always retain at least one audio track when the source has audio"
    );
}
