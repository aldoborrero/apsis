//! Profile configuration (port of `_engine/config.py`). Deserialized from the
//! coordinator's `scheduler.toml`; the engine consumes a validated `Profile`.
//!
//! Validation is a manual `Profile::validate` for now (quality range); wiring the
//! `garde` derive is a follow-up (see spec 001 tasks, Polish).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    Hevc,
    Av1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Encoder {
    Vaapi,
    Cpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fallback {
    Cpu,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HdrPolicy {
    Copy,
    Tonemap,
    Encode,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct VideoConfig {
    pub codec: VideoCodec,
    #[serde(default = "d_bit_depth")]
    pub bit_depth: u8,
    #[serde(default = "d_encoder")]
    pub encoder: Encoder,
    #[serde(default = "d_quality")]
    pub quality: u8,
    #[serde(default = "d_fallback")]
    pub fallback: Fallback,
    #[serde(default)]
    pub skip_codecs: Vec<String>,
    #[serde(default = "d_hdr_policy")]
    pub hdr_policy: HdrPolicy,
}

fn d_bit_depth() -> u8 {
    10
}
fn d_encoder() -> Encoder {
    Encoder::Vaapi
}
fn d_quality() -> u8 {
    22
}
fn d_fallback() -> Fallback {
    Fallback::Cpu
}
fn d_hdr_policy() -> HdrPolicy {
    HdrPolicy::Copy
}
fn d_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StereoConfig {
    #[serde(default = "d_aac")]
    pub codec: String,
    #[serde(default = "d_bitrate")]
    pub bitrate: u32,
    #[serde(default = "d_channels")]
    pub channels: u32,
    #[serde(default)]
    pub languages: Vec<String>,
}

fn d_aac() -> String {
    "aac".to_string()
}
fn d_bitrate() -> u32 {
    128
}
fn d_channels() -> u32 {
    2
}

impl Default for StereoConfig {
    fn default() -> Self {
        Self {
            codec: d_aac(),
            bitrate: d_bitrate(),
            channels: d_channels(),
            languages: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AudioConfig {
    #[serde(default)]
    pub keep_languages: Vec<String>,
    #[serde(default = "d_eng")]
    pub default_language: String,
    #[serde(default)]
    pub priority: Vec<String>,
    #[serde(default = "d_true")]
    pub remove_commentary: bool,
    #[serde(default)]
    pub add_stereo: StereoConfig,
    #[serde(default = "d_true")]
    pub preserve_surround: bool,
}

fn d_eng() -> String {
    "eng".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SubtitleConfig {
    #[serde(default)]
    pub keep_languages: Vec<String>,
    #[serde(default)]
    pub default_language: String,
    #[serde(default)]
    pub remove_formats: Vec<String>,
    #[serde(default = "d_true")]
    pub remove_commentary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OutputConfig {
    #[serde(default = "d_mkv")]
    pub container: String,
    #[serde(default = "d_true")]
    pub replace_original: bool,
}

fn d_mkv() -> String {
    "mkv".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Profile {
    pub video: VideoConfig,
    pub audio: AudioConfig,
    pub subtitles: SubtitleConfig,
    pub output: OutputConfig,
}

impl Profile {
    /// Validate value ranges. Returns the first violation as a message.
    pub fn validate(&self) -> Result<(), String> {
        if self.video.quality > 51 {
            return Err(format!(
                "video.quality must be 0..=51, got {}",
                self.video.quality
            ));
        }
        Ok(())
    }
}
