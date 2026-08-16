//! T022 — edge cases: corrupt input never panics, no-video is unsupported,
//! HDR is preserved under `hdr_policy=copy`, and `und`-language audio survives.

use apsis_engine::{PlanStatus, Probe, Profile, StreamInfo, VideoAction, parse_probe, plan};

fn profile(json: &str) -> Profile {
    serde_json::from_str(json).unwrap()
}

fn video(codec: &str, transfer: &str) -> StreamInfo {
    StreamInfo {
        index: 0,
        codec_type: "video".into(),
        codec: codec.into(),
        color_transfer: transfer.into(),
        ..Default::default()
    }
}

const SKIP_HEVC: &str = r#"{"video":{"codec":"hevc","skip_codecs":["hevc"]},"audio":{},"subtitles":{},"output":{"container":"mkv"}}"#;

#[test]
fn corrupt_probe_json_errors_without_panicking() {
    // Garbage in → a typed error, not a panic (FR-010).
    assert!(parse_probe("this is not json").is_err());
    assert!(parse_probe("").is_err());
    // Structurally valid JSON with the wrong shape (streams isn't an array).
    assert!(parse_probe(r#"{"streams":42}"#).is_err());
}

#[test]
fn empty_stream_list_parses_to_no_video_and_is_unsupported() {
    let probe = parse_probe(r#"{"streams":[]}"#).unwrap();
    assert!(probe.video.is_none());
    let p = plan("x.mkv", &probe, &profile(SKIP_HEVC));
    assert_eq!(p.status, PlanStatus::Unsupported);
    assert!(p.should_skip);
    assert_eq!(p.video.action, VideoAction::Unsupported);
}

#[test]
fn hdr_is_preserved_under_copy_policy() {
    // An h264 HDR source would normally encode, but hdr_policy=copy wins.
    let prof = profile(
        r#"{"video":{"codec":"hevc","skip_codecs":[],"hdr_policy":"copy"},"audio":{},"subtitles":{},"output":{"container":"mkv"}}"#,
    );
    let probe = Probe {
        video: Some(video("h264", "smpte2084")),
        ..Default::default()
    };
    let p = plan("x.mkv", &probe, &prof);
    assert!(p.source_probe.is_hdr());
    assert_eq!(p.video.action, VideoAction::Copy);
    assert!(
        p.reasons
            .iter()
            .any(|r| r.message.contains("HDR content preserved"))
    );
}

#[test]
fn und_language_audio_is_kept_and_titled() {
    // Undetermined-language audio must not crash the namer or be dropped.
    let probe = parse_probe(
        r#"{"streams":[
            {"index":0,"codec_type":"video","codec_name":"hevc"},
            {"index":1,"codec_type":"audio","codec_name":"aac","channels":2,
             "tags":{"language":"und"}}
        ]}"#,
    )
    .unwrap();
    let p = plan("x.mkv", &probe, &profile(SKIP_HEVC));
    assert_eq!(p.audio.len(), 1);
    assert_eq!(p.audio[0].language, "und");
    // language_name falls back to the uppercased code.
    assert_eq!(p.audio[0].title_after, "UND / AAC / Stereo");
}
