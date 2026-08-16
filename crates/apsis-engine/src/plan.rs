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

use crate::config::VideoCodec;
use crate::probe::Probe;

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
