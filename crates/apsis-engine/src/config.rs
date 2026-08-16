//! Profile configuration (port of `_engine/config.py`). Deserialized from the
//! coordinator's `scheduler.toml`; the engine consumes a validated `Profile`.
//!
//! Range validation happens at deserialization (see `de_quality`), so an
//! out-of-range `Profile` cannot be constructed — matching pydantic's
//! construct-time guarantee. A richer `garde` derive can add more rules later.

use serde::{Deserialize, Deserializer, Serialize, de};

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
    // Used by the VAAPI backend in US2 (main10 / p010 for 10-bit sources) — keep.
    #[serde(default = "d_bit_depth")]
    pub bit_depth: u8,
    #[serde(default = "d_encoder")]
    pub encoder: Encoder,
    #[serde(default = "d_quality", deserialize_with = "de_quality")]
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

/// Validate `quality` at deserialization so an out-of-range `Profile` cannot be
/// constructed (matching the Python pydantic `field_validator`). Surfaces as a
/// serde error → `EngineError::ParseJson`.
fn de_quality<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
    let v = u8::deserialize(d)?;
    if v > 51 {
        return Err(de::Error::custom(format!(
            "video.quality must be 0..=51, got {v}"
        )));
    }
    Ok(v)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_out_of_range_quality() {
        let json =
            r#"{"video":{"codec":"hevc","quality":99},"audio":{},"subtitles":{},"output":{}}"#;
        let err = serde_json::from_str::<Profile>(json).unwrap_err();
        assert!(err.to_string().contains("0..=51"), "got: {err}");
    }

    #[test]
    fn applies_python_defaults() {
        let json = r#"{"video":{"codec":"av1"},"audio":{},"subtitles":{},"output":{}}"#;
        let p: Profile = serde_json::from_str(json).unwrap();
        assert_eq!(p.video.codec, VideoCodec::Av1);
        assert_eq!(p.video.quality, 22);
        assert_eq!(p.video.encoder, Encoder::Vaapi);
        assert!(p.audio.remove_commentary);
        assert!(p.audio.preserve_surround);
        assert_eq!(p.audio.default_language, "eng");
        assert_eq!(p.output.container, "mkv");
        assert!(p.output.replace_original);
    }
}
