//! Planning types + logic (port of `_engine/plan.py`). The abstract,
//! backend-neutral decision the coordinator computes and ships to the worker.
//!
//! Both the types and [`plan`] (`plan_from_probe`: audio/subtitle selection, the
//! HDR-copy override, container/track reason emission, default-track selection)
//! are ported and covered by unit tests (US1).
//!
//! The oracle-parity types below use `deny_unknown_fields` and no silent
//! defaults, so a fixture with a missing/renamed field is a hard error, not a
//! false-green equality. Path handling deliberately mirrors Python `pathlib`
//! (`py_suffix`/`py_with_suffix`), not Rust's `Path::extension`.

use serde::{Deserialize, Serialize};

use crate::audio::{AudioAction, AudioActionKind, build_audio_plan};
use crate::config::{Bitrate, HdrPolicy, Profile, VideoCodec};
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
    /// Encode bitrate for `Encode` tracks (default/unused for `Copy`); the value the
    /// command materializes, so `add_stereo`/`add_mono`/`transcode` each keep their own.
    #[serde(default)]
    pub bitrate: Bitrate,
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

/// Final path component (after the last `/`) — Python `PurePath.name`.
fn file_name(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// Python `PurePath.suffix`: the last dot-extension of the final component,
/// **including** the dot, or `""` when there is none. Matches `CPython` exactly —
/// a leading-dot-only name (`.mkv`, `.hidden`) or a trailing dot (`movie.`) has
/// no suffix, so it diverges from Rust's `Path::extension()` on those.
fn py_suffix(path: &str) -> &str {
    let name = file_name(path);
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => &name[i..],
        _ => "",
    }
}

/// Python `PurePath.with_suffix`: replace the final component's suffix, appending
/// when it has none (so `movie.` → `movie..mkv`, `.mkv` → `.mkv.mkv`).
fn py_with_suffix(path: &str, new_suffix: &str) -> String {
    let name = file_name(path);
    let dir = &path[..path.len() - name.len()]; // keeps the trailing '/' if any
    let stem = &name[..name.len() - py_suffix(path).len()];
    format!("{dir}{stem}{new_suffix}")
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

/// Whether `container` can hold a subtitle of `codec`. MP4 only holds text subs
/// (`mov_text`); MKV/others hold text + image subs. Used by `output.conform` to drop
/// subs a target container can't mux (avoids a launch-time failure). Audio conform
/// (which would need re-encoding an incompatible codec) is deferred.
fn container_holds_subtitle(container: &str, codec: &str) -> bool {
    match container
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
        .as_str()
    {
        "mp4" | "m4v" | "mov" => matches!(codec, "mov_text" | "tx3g"),
        _ => true,
    }
}

/// Parse a resolution string to its target height: `"1080p"`/`"720i"` → 1080/720,
/// `"1920x1080"` → 1080, `"4k"`/`"8k"` → 2160/4320. `None` if unrecognized.
pub(crate) fn parse_resolution_height(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(rest) = s.strip_suffix(['p', 'i']) {
        return rest.parse().ok();
    }
    if let Some((_, h)) = s.split_once(['x', 'X']) {
        return h.parse().ok();
    }
    match s.to_ascii_lowercase().as_str() {
        "4k" => Some(2160),
        "8k" => Some(4320),
        _ => None,
    }
}

/// Compute the full [`FilePlan`] for a file (port of `plan_from_probe`).
#[must_use]
#[allow(clippy::too_many_lines)] // faithful 1:1 port of the single Python `plan_from_probe`
pub fn plan(input_path: &str, probe: &Probe, profile: &Profile) -> FilePlan {
    let output_ext = container_suffix(profile);
    // Path handling mirrors Python `pathlib` (suffix/with_suffix), NOT Rust's
    // `Path::extension`/`with_extension` — they diverge on dotted names (see
    // `py_suffix`/`py_with_suffix`), which flips container decisions and paths.
    let output_path = if profile.output.replace_original {
        py_with_suffix(input_path, &output_ext)
    } else {
        format!("<temp>{output_ext}")
    };
    let src_suffix = py_suffix(input_path); // includes the dot, or "" (Python `.suffix`)
    let source_container = {
        let ext = src_suffix.trim_start_matches('.');
        (!ext.is_empty()).then(|| ext.to_string()) // Python `.suffix.lstrip('.') or None`
    };
    let output = OutputPlan {
        input_path: input_path.to_string(),
        output_path,
        replace_original: profile.output.replace_original,
        source_container,
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

    if src_suffix.to_lowercase() != output_ext.to_lowercase() {
        let shown = if src_suffix.is_empty() {
            "<none>"
        } else {
            src_suffix
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
        // Python builds `{s.index: s for s in probe.audio}` and `.get()`s it, so a
        // duplicate index resolves to the LAST such stream — `rev().find()` matches.
        let current = probe
            .audio
            .iter()
            .rev()
            .find(|s| s.index == action.stream.index);
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
            bitrate: action.bitrate.clone(),
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
        // Last-wins on duplicate index, matching Python's `{s.index: s}` dict.
        let current = probe.subtitles.iter().rev().find(|s| s.index == sub.index);
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

    // Conform (spec 004): drop subtitle tracks the target container can't hold.
    if profile.output.conform {
        subtitle_items.retain(|s| {
            s.target_codec
                .as_deref()
                .is_none_or(|c| container_holds_subtitle(&profile.output.container, c))
        });
    }

    let compliant = reasons.is_empty();
    // Skip gates (spec 004): a source already *below* a resolution/bitrate threshold
    // is left alone even if its codec would otherwise be transcoded. Only fires on a
    // KNOWN value (>0) — an unknown (0) probe field never triggers a skip.
    let gate_skip = {
        let res = profile
            .video
            .skip_if_resolution_below
            .as_deref()
            .and_then(parse_resolution_height)
            .is_some_and(|t| video.height > 0 && video.height < t);
        let br = profile
            .video
            .skip_if_bitrate_below
            .as_ref()
            .is_some_and(|b| video.bitrate > 0 && u64::from(video.bitrate) < b.bps());
        res || br
    };
    let should_skip = compliant || gate_skip;
    FilePlan {
        status: if should_skip {
            PlanStatus::Compliant
        } else {
            PlanStatus::ChangesRequired
        },
        compliant,
        should_skip,
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
    fn skip_gates_leave_small_sources_alone() {
        let small = |h: u32, br: u32| {
            let mut v = video("h264", ""); // h264 ∉ skip_codecs → would transcode
            v.height = h;
            v.bitrate = br;
            Probe {
                video: Some(v),
                ..Default::default()
            }
        };
        let with = |gate: &str| {
            profile(&format!(
                r#"{{"video":{{"codec":"hevc","skip_codecs":["hevc"],{gate}}},"audio":{{}},"subtitles":{{}},"output":{{}}}}"#
            ))
        };
        // below the resolution / bitrate threshold → skipped despite the codec mismatch
        assert!(
            plan(
                "x.mkv",
                &small(400, 1_000_000),
                &with(r#""skip_if_resolution_below":"480p""#)
            )
            .should_skip
        );
        assert!(
            plan(
                "x.mkv",
                &small(400, 1_000_000),
                &with(r#""skip_if_bitrate_below":"2M""#)
            )
            .should_skip
        );
        // a 1080p / 10M source is above the gate → still transcodes
        assert!(
            !plan(
                "x.mkv",
                &small(1080, 10_000_000),
                &with(r#""skip_if_resolution_below":"480p""#)
            )
            .should_skip
        );
        // unknown probe value (0) never triggers a skip
        assert!(
            !plan(
                "x.mkv",
                &small(0, 0),
                &with(r#""skip_if_resolution_below":"480p""#)
            )
            .should_skip
        );
    }

    #[test]
    fn conform_drops_container_incompatible_subtitles() {
        let pgs = StreamInfo {
            index: 1,
            codec_type: "subtitle".into(),
            codec: "hdmv_pgs_subtitle".into(),
            language: "eng".into(),
            ..Default::default()
        };
        let probe = Probe {
            video: Some(video("hevc", "")),
            subtitles: vec![pgs],
            ..Default::default()
        };
        let out = |c: &str| {
            profile(&format!(
                r#"{{"video":{{"codec":"hevc","skip_codecs":["hevc"]}},"audio":{{}},"subtitles":{{}},"output":{{"container":"{c}","conform":true}}}}"#
            ))
        };
        // mp4 can't mux PGS → conform drops it; mkv holds it → kept.
        assert!(plan("x.mkv", &probe, &out("mp4")).subtitles.is_empty());
        assert_eq!(plan("x.mkv", &probe, &out("mkv")).subtitles.len(), 1);
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

    #[test]
    fn py_path_semantics_match_cpython() {
        // Verified against python3 pathlib (see review). Rust `Path::extension`
        // would disagree on the dotted cases.
        assert_eq!(py_suffix("movie.mkv"), ".mkv");
        assert_eq!(py_suffix("a.b.mkv"), ".mkv");
        assert_eq!(py_suffix("..mkv"), ".mkv");
        assert_eq!(py_suffix(".mkv"), "");
        assert_eq!(py_suffix("movie."), "");
        assert_eq!(py_suffix("movie"), "");
        assert_eq!(py_suffix("folder.d/movie"), ""); // dot only in the directory

        assert_eq!(py_with_suffix("movie.mkv", ".mkv"), "movie.mkv");
        assert_eq!(py_with_suffix("..mkv", ".mkv"), "..mkv"); // NOT ".."
        assert_eq!(py_with_suffix("movie.", ".mkv"), "movie..mkv");
        assert_eq!(py_with_suffix(".mkv", ".mkv"), ".mkv.mkv");
        assert_eq!(py_with_suffix("movie", ".mkv"), "movie.mkv");
        assert_eq!(py_with_suffix("folder/clip.mp4", ".mkv"), "folder/clip.mkv");
    }

    #[test]
    fn output_path_and_container_follow_pathlib_not_extension() {
        let probe = Probe {
            video: Some(video("hevc", "")),
            ..Default::default()
        };
        // Trailing-dot: with_suffix appends → "movie..mkv"; suffix "" → no container.
        let p = plan("movie.", &probe, &profile(HEVC));
        assert_eq!(p.output.output_path, "movie..mkv");
        assert_eq!(p.output.source_container, None);
        assert!(
            p.reasons
                .iter()
                .any(|r| r.code == ReasonCode::ContainerMismatch)
        );

        // Double-leading-dot: with_suffix → "..mkv" (Rust with_extension would give ".."),
        // suffix ".mkv" matches target → compliant, not a container change.
        let p = plan("..mkv", &probe, &profile(HEVC));
        assert_eq!(p.output.output_path, "..mkv");
        assert_eq!(p.output.source_container.as_deref(), Some("mkv"));
        assert!(p.compliant && p.should_skip);
    }

    #[test]
    fn duplicate_audio_index_resolves_to_last_like_python_dict() {
        // Two audio streams share index 1; Python's `{s.index: s}` keeps the last.
        let probe = Probe {
            video: Some(video("hevc", "")),
            audio: vec![
                audio(1, "eng", 6, "eac3", "First"),
                audio(1, "eng", 6, "eac3", "Second"),
            ],
            ..Default::default()
        };
        let p = plan("x.mkv", &probe, &profile(HEVC));
        // `title_before` comes from the `current` lookup — must be the LAST stream.
        assert!(
            p.audio
                .iter()
                .all(|a| a.title_before.as_deref() == Some("Second")),
            "duplicate-index lookup must resolve to the last stream (Python dict semantics)"
        );
    }
}
