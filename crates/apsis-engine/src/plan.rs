//! Planning types (port of `_engine/plan.py`). The abstract, backend-neutral
//! decision the coordinator computes and ships to the worker. (The `plan`
//! function itself is implemented in the US1 phase.)

use serde::{Deserialize, Serialize};

use crate::config::VideoCodec;
use crate::probe::Probe;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanReason {
    pub code: String,
    pub message: String,
    pub scope: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoAction {
    Copy,
    Encode,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct FilePlan {
    pub status: PlanStatus,
    pub compliant: bool,
    pub should_skip: bool,
    pub source_probe: Probe,
    pub output: OutputPlan,
    pub video: VideoPlan,
    #[serde(default)]
    pub audio: Vec<AudioTrackPlan>,
    #[serde(default)]
    pub subtitles: Vec<SubtitleTrackPlan>,
    #[serde(default)]
    pub reasons: Vec<PlanReason>,
}
