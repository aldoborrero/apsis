//! Profile configuration (port of `_engine/config.py`). Deserialized from the
//! coordinator's `scheduler.toml`; the engine consumes a validated `Profile`.
//!
//! Range validation happens at deserialization (see `de_quality`), so an
//! out-of-range `Profile` cannot be constructed — matching pydantic's
//! construct-time guarantee. A richer `garde` derive can add more rules later.
//!
//! **Deliberate divergence from the Python oracle:** these config structs use
//! `#[serde(deny_unknown_fields)]`, whereas the Python engine (pydantic) ignores
//! unknown keys. apsis loads these from operator-authored `scheduler.toml` with no
//! Python in the loop, so a typo like `qualiy = 20` should fail loudly rather than
//! be silently dropped to a default. This strictness applies only to the config
//! structs — the ffprobe-facing probe types stay permissive (trust boundary).

use serde::{Deserialize, Deserializer, Serialize, de};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    Hevc,
    Av1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Encoder {
    Vaapi,
    Cpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fallback {
    Cpu,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HdrPolicy {
    Copy,
    Tonemap,
    Encode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

/// An audio bitrate. Deserializes from **either** a bare integer (kbps: `128` →
/// `128k`) **or** a unit string (`"128k"`, `"5M"`), and serializes to the canonical
/// ffmpeg argument string. One bitrate convention across `add_stereo`/`add_mono`/
/// `transcode`/`quality` (spec 004); the bare int keeps existing configs working.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Bitrate(String);

impl Bitrate {
    /// The canonical ffmpeg argument form (e.g. `"128k"`).
    #[must_use]
    pub fn as_arg(&self) -> &str {
        &self.0
    }
}

impl Default for Bitrate {
    fn default() -> Self {
        Self("0k".to_string())
    }
}

impl<'de> Deserialize<'de> for Bitrate {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Int(u64),
            Str(String),
        }
        let s = match Raw::deserialize(d)? {
            Raw::Int(kbps) => format!("{kbps}k"),
            Raw::Str(s) => s,
        };
        // Fail-fast: <number>[k|M|…]; reject junk at config load.
        let num = s.trim_end_matches(char::is_alphabetic);
        if num.is_empty() || num.parse::<f64>().is_err() {
            return Err(de::Error::custom(format!(
                "invalid bitrate {s:?} (expected an int in kbps or a string like \"128k\"/\"5M\")"
            )));
        }
        Ok(Self(s))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StereoConfig {
    #[serde(default = "d_aac")]
    pub codec: String,
    #[serde(default = "d_bitrate")]
    pub bitrate: Bitrate,
    #[serde(default = "d_channels")]
    pub channels: u32,
    #[serde(default)]
    pub languages: Vec<String>,
}

fn d_aac() -> String {
    "aac".to_string()
}
fn d_bitrate() -> Bitrate {
    Bitrate("128k".to_string())
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    #[serde(default = "d_mkv")]
    pub container: String,
    #[serde(default = "d_true")]
    pub replace_original: bool,
}

fn d_mkv() -> String {
    "mkv".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub video: VideoConfig,
    pub audio: AudioConfig,
    pub subtitles: SubtitleConfig,
    pub output: OutputConfig,
}

/// VAAPI hardware settings (port of `_engine/config.py`'s `VaapiConfig`). This is
/// worker/host-level config the engine's command builder consumes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaapiConfig {
    #[serde(default = "d_va_name")]
    pub device: String,
    #[serde(default = "d_hw_decode")]
    pub hw_decode_codecs: Vec<String>,
    #[serde(default = "d_sw_decode")]
    pub sw_decode_codecs: Vec<String>,
    #[serde(default = "d_upload_filter")]
    pub upload_filter: String,
    #[serde(default = "d_async_depth")]
    pub async_depth: u32,
}

fn d_va_name() -> String {
    "va".to_string()
}
fn d_hw_decode() -> Vec<String> {
    vec!["hevc".to_string(), "av1".to_string(), "vp9".to_string()]
}
fn d_sw_decode() -> Vec<String> {
    vec!["h264".to_string()]
}
fn d_upload_filter() -> String {
    "format=nv12,hwupload_vaapi".to_string()
}
fn d_async_depth() -> u32 {
    4
}

impl Default for VaapiConfig {
    fn default() -> Self {
        Self {
            device: d_va_name(),
            hw_decode_codecs: d_hw_decode(),
            sw_decode_codecs: d_sw_decode(),
            upload_filter: d_upload_filter(),
            async_depth: d_async_depth(),
        }
    }
}

/// Hardware/environment config (port of `_engine/config.py`'s `HardwareConfig`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardwareConfig {
    #[serde(default = "d_hw_env")]
    pub env: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub vaapi: VaapiConfig,
}

fn d_hw_env() -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    m.insert("AMD_DEBUG".to_string(), "noefc".to_string());
    m
}

impl Default for HardwareConfig {
    fn default() -> Self {
        Self {
            env: d_hw_env(),
            vaapi: VaapiConfig::default(),
        }
    }
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
    fn bitrate_accepts_int_and_string_and_rejects_junk() {
        // back-compat: bare int (kbps) → canonical "128k"
        let s: StereoConfig = serde_json::from_str(r#"{"bitrate":128}"#).unwrap();
        assert_eq!(s.bitrate.as_arg(), "128k");
        // string forms pass through
        let s: StereoConfig = serde_json::from_str(r#"{"bitrate":"5M"}"#).unwrap();
        assert_eq!(s.bitrate.as_arg(), "5M");
        // junk fails at load (fail-fast)
        assert!(serde_json::from_str::<StereoConfig>(r#"{"bitrate":"loud"}"#).is_err());
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
