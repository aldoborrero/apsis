//! Planning types (port of `_engine/plan.py`). The abstract, backend-neutral
//! decision the coordinator computes and ships to the worker.
//!
//! **TODO(US1):** the plan *logic* (`plan_from_probe`, audio/subtitle selection,
//! the HDR-copy override, reason emission) is NOT ported yet — only the types.
//! This crate is not oracle-faithful until that lands (spec 001, phase 3).
//!
//! The oracle-parity types below use `deny_unknown_fields` and no silent
//! defaults, so a fixture with a missing/renamed field is a hard error, not a
//! false-green equality.

use serde::{Deserialize, Serialize};

use crate::audio::{AudioAction, AudioActionKind, build_audio_plan};
use crate::config::{HdrPolicy, Profile, VideoCodec};
use crate::probe::{Probe, StreamInfo};
use crate::subtitles::filter_subtitles;

/// Why a change is required (closed set, mirrors the Python `ReasonCode` literal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    VideoCodecMismatch,
    ContainerMismatch,
    AudioTrackCountChanged,
    AudioTrackChanged,
    SubtitleTrackCountChanged,
    SubtitleTrackChanged,
    NoVideoStream,
}

/// The scope a reason applies to (closed set, mirrors the Python `PlanScope`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanScope {
    Video,
    Audio,
    Subtitles,
    Container,
    General,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanReason {
    pub code: ReasonCode,
    pub message: String,
    pub scope: PlanScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoAction {
    Copy,
    Encode,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoPlan {
    pub source_index: Option<u32>,
    pub source_codec: Option<String>,
    pub target_codec: VideoCodec,
    pub action: VideoAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackAction {
    Copy,
    Encode,
    Drop,
    Generate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioTrackPlan {
    pub source_index: Option<u32>,
    pub language: String,
    pub action: TrackAction,
    pub source_codec: Option<String>,
    pub target_codec: String,
    pub source_channels: Option<u32>,
    pub target_channels: u32,
    pub title_before: Option<String>,
    pub title_after: String,
    pub default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubAction {
    Copy,
    Drop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubtitleTrackPlan {
    pub source_index: Option<u32>,
    pub language: String,
    pub action: SubAction,
    pub source_codec: Option<String>,
    pub target_codec: Option<String>,
    pub title_before: Option<String>,
    pub title_after: Option<String>,
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputPlan {
    pub input_path: String,
    pub output_path: String,
    pub replace_original: bool,
    pub source_container: Option<String>,
    pub target_container: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Compliant,
    ChangesRequired,
    Unsupported,
}

/// The full plan for a file. `source_probe` is retained because command building
/// needs the ordered source streams to map absolute indices to `0:a:N`/`0:s:N`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilePlan {
    pub status: PlanStatus,
    pub compliant: bool,
    pub should_skip: bool,
    pub source_probe: Probe,
    pub output: OutputPlan,
    pub video: VideoPlan,
    pub audio: Vec<AudioTrackPlan>,
    pub subtitles: Vec<SubtitleTrackPlan>,
    pub reasons: Vec<PlanReason>,
}

// --- Planning logic (port of `_engine/plan.py`) ---

fn language_name(code: &str) -> String {
    match code {
        "eng" => "English",
        "spa" => "Spanish",
        "jpn" => "Japanese",
        "fre" => "French",
        "ger" => "German",
        "ita" => "Italian",
        "por" => "Portuguese",
        "chi" => "Chinese",
        "kor" => "Korean",
        "ara" => "Arabic",
        "rus" => "Russian",
        "dut" => "Dutch",
        other => return other.to_uppercase(),
    }
    .to_string()
}

fn channel_name(channels: u32) -> String {
    match channels {
        1 => "Mono".to_string(),
        2 => "Stereo".to_string(),
        6 => "5.1".to_string(),
        8 => "7.1".to_string(),
        n => format!("{n}ch"),
    }
}

fn track_title(language: &str, codec: &str, channels: u32) -> String {
    format!(
        "{} / {} / {}",
        language_name(language),
        codec.to_uppercase(),
        channel_name(channels)
    )
}

/// Target container extension, incl. the leading dot (e.g. `.mkv`).
fn container_suffix(profile: &Profile) -> String {
    let c = profile.output.container.trim().trim_start_matches('.');
    if c.is_empty() {
        ".mkv".to_string()
    } else {
        format!(".{c}")
    }
}

/// Python-style list repr (`['a', 'b']`) so reason messages match the oracle.
fn py_list(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| format!("'{s}'")).collect();
    format!("[{}]", inner.join(", "))
}

fn select_default_audio_pos(audio_plan: &[AudioAction], default_language: &str) -> Option<usize> {
    if audio_plan.is_empty() {
        return None;
    }
    Some(
        audio_plan
            .iter()
            .position(|a| a.stream.language == default_language)
            .unwrap_or(0),
    )
}

fn select_default_subtitle_pos(subs: &[StreamInfo], default_language: &str) -> Option<usize> {
    if subs.is_empty() {
        return None;
    }
    if !default_language.is_empty()
        && let Some(pos) = subs.iter().position(|s| s.language == default_language)
    {
        return Some(pos);
    }
    Some(0)
}

/// Compute the full [`FilePlan`] for a file (port of `plan_from_probe`).
#[must_use]
#[allow(clippy::too_many_lines)] // faithful 1:1 port of the single Python `plan_from_probe`
pub fn plan(input_path: &str, probe: &Probe, profile: &Profile) -> FilePlan {
    let output_ext = container_suffix(profile);
    let input = std::path::Path::new(input_path);
    let output_path = if profile.output.replace_original {
        input
            .with_extension(output_ext.trim_start_matches('.'))
            .to_string_lossy()
            .into_owned()
    } else {
        format!("<temp>{output_ext}")
    };
    let output = OutputPlan {
        input_path: input_path.to_string(),
        output_path,
        replace_original: profile.output.replace_original,
        source_container: input.extension().map(|e| e.to_string_lossy().into_owned()),
        target_container: profile.output.container.clone(),
    };

    let Some(video) = probe.video.as_ref() else {
        return FilePlan {
            status: PlanStatus::Unsupported,
            compliant: false,
            should_skip: true,
            source_probe: probe.clone(),
            output,
            video: VideoPlan {
                source_index: None,
                source_codec: None,
                target_codec: profile.video.codec,
                action: VideoAction::Unsupported,
            },
            audio: Vec::new(),
            subtitles: Vec::new(),
            reasons: vec![PlanReason {
                code: ReasonCode::NoVideoStream,
                message: "no video stream present".to_string(),
                scope: PlanScope::Video,
            }],
        };
    };

    let audio_plan = build_audio_plan(&probe.audio, &profile.audio);
    let kept_subs = filter_subtitles(&probe.subtitles, &profile.subtitles);
    let default_audio_pos = select_default_audio_pos(&audio_plan, &profile.audio.default_language);
    let default_sub_pos =
        select_default_subtitle_pos(&kept_subs, &profile.subtitles.default_language);

    let mut reasons: Vec<PlanReason> = Vec::new();

    let mut video_action = VideoAction::Copy;
    if !profile.video.skip_codecs.contains(&video.codec) {
        video_action = VideoAction::Encode;
        reasons.push(PlanReason {
            code: ReasonCode::VideoCodecMismatch,
            message: format!(
                "video codec {} not in skip_codecs {}",
                video.codec,
                py_list(&profile.video.skip_codecs)
            ),
            scope: PlanScope::Video,
        });
    }

    if probe.is_hdr()
        && profile.video.hdr_policy == HdrPolicy::Copy
        && video_action == VideoAction::Encode
    {
        video_action = VideoAction::Copy;
        reasons.push(PlanReason {
            code: ReasonCode::VideoCodecMismatch,
            message: "HDR content preserved (hdr_policy=copy)".to_string(),
            scope: PlanScope::Video,
        });
    }

    let src_suffix = input
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    if src_suffix.to_lowercase() != output_ext.to_lowercase() {
        let shown = if src_suffix.is_empty() {
            "<none>"
        } else {
            src_suffix.as_str()
        };
        reasons.push(PlanReason {
            code: ReasonCode::ContainerMismatch,
            message: format!("extension {shown} differs from target {output_ext}"),
            scope: PlanScope::Container,
        });
    }

    let mut audio_items: Vec<AudioTrackPlan> = Vec::new();
    if probe.audio.len() != audio_plan.len() {
        reasons.push(PlanReason {
            code: ReasonCode::AudioTrackCountChanged,
            message: format!(
                "audio track count would change from {} to {}",
                probe.audio.len(),
                audio_plan.len()
            ),
            scope: PlanScope::Audio,
        });
    }
    for (i, action) in audio_plan.iter().enumerate() {
        let is_copy = action.action == AudioActionKind::Copy;
        let expected_codec = if is_copy {
            action.stream.codec.clone()
        } else {
            action.codec.clone()
        };
        let expected_channels = if is_copy {
            action.stream.channels
        } else {
            action.channels
        };
        let expected_title =
            track_title(&action.stream.language, &expected_codec, expected_channels);
        let current = probe.audio.iter().find(|s| s.index == action.stream.index);
        let default = default_audio_pos == Some(i);
        audio_items.push(AudioTrackPlan {
            source_index: Some(action.stream.index),
            language: action.stream.language.clone(),
            action: if is_copy {
                TrackAction::Copy
            } else {
                TrackAction::Encode
            },
            source_codec: current.map(|c| c.codec.clone()),
            target_codec: expected_codec.clone(),
            source_channels: current.map(|c| c.channels),
            target_channels: expected_channels,
            title_before: current.map(|c| c.title.clone()),
            title_after: expected_title.clone(),
            default,
        });
        if let Some(c) = current
            && (c.language != action.stream.language
                || c.codec != expected_codec
                || c.channels != expected_channels
                || c.title != expected_title
                || c.is_default != default)
        {
            reasons.push(PlanReason {
                code: ReasonCode::AudioTrackChanged,
                message: format!(
                    "audio track {i} would be updated to {expected_title}{}",
                    if default { " [default]" } else { "" }
                ),
                scope: PlanScope::Audio,
            });
        }
    }

    let mut subtitle_items: Vec<SubtitleTrackPlan> = Vec::new();
    if probe.subtitles.len() != kept_subs.len() {
        reasons.push(PlanReason {
            code: ReasonCode::SubtitleTrackCountChanged,
            message: format!(
                "subtitle track count would change from {} to {}",
                probe.subtitles.len(),
                kept_subs.len()
            ),
            scope: PlanScope::Subtitles,
        });
    }
    for (i, sub) in kept_subs.iter().enumerate() {
        let current = probe.subtitles.iter().find(|s| s.index == sub.index);
        let default = default_sub_pos == Some(i);
        subtitle_items.push(SubtitleTrackPlan {
            source_index: Some(sub.index),
            language: sub.language.clone(),
            action: SubAction::Copy,
            source_codec: current.map(|c| c.codec.clone()),
            target_codec: Some(sub.codec.clone()),
            title_before: current.map(|c| c.title.clone()),
            title_after: Some(sub.title.clone()),
            default,
        });
        if let Some(c) = current
            && (c.language != sub.language
                || c.codec != sub.codec
                || c.title != sub.title
                || c.is_default != default)
        {
            let lang = if sub.language.is_empty() {
                "unknown"
            } else {
                sub.language.as_str()
            };
            reasons.push(PlanReason {
                code: ReasonCode::SubtitleTrackChanged,
                message: format!(
                    "subtitle track {i} would be updated to {lang} / {}{}",
                    sub.codec.to_uppercase(),
                    if default { " [default]" } else { "" }
                ),
                scope: PlanScope::Subtitles,
            });
        }
    }

    let compliant = reasons.is_empty();
    FilePlan {
        status: if compliant {
            PlanStatus::Compliant
        } else {
            PlanStatus::ChangesRequired
        },
        compliant,
        should_skip: compliant,
        source_probe: probe.clone(),
        output,
        video: VideoPlan {
            source_index: Some(video.index),
            source_codec: Some(video.codec.clone()),
            target_codec: profile.video.codec,
            action: video_action,
        },
        audio: audio_items,
        subtitles: subtitle_items,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn audio(index: u32, lang: &str, channels: u32, codec: &str, title: &str) -> StreamInfo {
        StreamInfo {
            index,
            codec_type: "audio".into(),
            codec: codec.into(),
            language: lang.into(),
            channels,
            title: title.into(),
            ..Default::default()
        }
    }

    const HEVC: &str = r#"{"video":{"codec":"hevc","skip_codecs":["hevc"]},"audio":{},"subtitles":{},"output":{"container":"mkv"}}"#;

    #[test]
    fn no_video_is_unsupported() {
        let p = plan("x.mkv", &Probe::default(), &profile(HEVC));
        assert_eq!(p.status, PlanStatus::Unsupported);
        assert!(p.should_skip);
        assert_eq!(p.reasons[0].code, ReasonCode::NoVideoStream);
        assert_eq!(p.video.action, VideoAction::Unsupported);
    }

    #[test]
    fn compliant_when_nothing_to_change() {
        let probe = Probe {
            video: Some(video("hevc", "")),
            ..Default::default()
        };
        let p = plan("x.mkv", &probe, &profile(HEVC));
        assert!(p.compliant && p.should_skip);
        assert!(p.reasons.is_empty());
        assert_eq!(p.video.action, VideoAction::Copy);
    }

    #[test]
    fn h264_needs_encode() {
        let probe = Probe {
            video: Some(video("h264", "")),
            ..Default::default()
        };
        let p = plan("x.mkv", &probe, &profile(HEVC));
        assert_eq!(p.video.action, VideoAction::Encode);
        assert!(!p.should_skip);
        assert!(
            p.reasons
                .iter()
                .any(|r| r.code == ReasonCode::VideoCodecMismatch)
        );
    }

    #[test]
    fn hdr_override_forces_copy() {
        let prof = profile(
            r#"{"video":{"codec":"hevc","skip_codecs":[],"hdr_policy":"copy"},"audio":{},"subtitles":{},"output":{"container":"mkv"}}"#,
        );
        let probe = Probe {
            video: Some(video("h264", "smpte2084")),
            ..Default::default()
        };
        let p = plan("x.mkv", &probe, &prof);
        assert_eq!(p.video.action, VideoAction::Copy);
        assert!(
            p.reasons
                .iter()
                .any(|r| r.message.contains("HDR content preserved"))
        );
    }

    #[test]
    fn container_mismatch_flagged() {
        let probe = Probe {
            video: Some(video("hevc", "")),
            ..Default::default()
        };
        let p = plan("x.mp4", &probe, &profile(HEVC));
        assert!(
            p.reasons
                .iter()
                .any(|r| r.code == ReasonCode::ContainerMismatch)
        );
        assert!(!p.should_skip);
    }

    #[test]
    fn audio_retitle_is_a_change() {
        let probe = Probe {
            video: Some(video("hevc", "")),
            audio: vec![audio(1, "eng", 6, "eac3", "Original")],
            ..Default::default()
        };
        let p = plan("x.mkv", &probe, &profile(HEVC));
        assert_eq!(p.audio.len(), 1);
        assert_eq!(p.audio[0].title_after, "English / EAC3 / 5.1");
        assert!(p.audio[0].default);
        assert!(
            p.reasons
                .iter()
                .any(|r| r.code == ReasonCode::AudioTrackChanged)
        );
    }
}
