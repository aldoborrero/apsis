//! Profile configuration (port of `_engine/config.py`). Deserialized from the
//! coordinator's `scheduler.toml`; the engine consumes a validated `Profile`.
//!
//! Range validation happens at deserialization (see [`QualityMode`]/[`Bitrate`]), so
//! an out-of-range `Profile` cannot be constructed — matching pydantic's
//! construct-time guarantee. A richer `garde` derive can add more rules later.
//!
//! **Deliberate divergence from the Python oracle:** these config structs use
//! `#[serde(deny_unknown_fields)]`, whereas the Python engine (pydantic) ignores
//! unknown keys. apsis loads these from operator-authored `scheduler.toml` with no
//! Python in the loop, so a typo like `qualiy = 20` should fail loudly rather than
//! be silently dropped to a default. This strictness applies only to the config
//! structs — the ffprobe-facing probe types stay permissive (trust boundary).

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::error::EngineError;

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
    #[serde(default = "d_quality")]
    pub quality: QualityMode,
    #[serde(default = "d_fallback")]
    pub fallback: Fallback,
    #[serde(default)]
    pub skip_codecs: Vec<String>,
    #[serde(default = "d_hdr_policy")]
    pub hdr_policy: HdrPolicy,
    // spec 004 coverage additions (all optional/additive).
    #[serde(default)]
    pub preset: Option<String>,
    /// Downscale-if-larger target, e.g. `"1080p"` or `"1920x1080"`.
    #[serde(default)]
    pub max_resolution: Option<String>,
    #[serde(default)]
    pub crop: Crop,
    /// Raw-ffmpeg escape hatch, injected into the single command (never a 2nd pass).
    #[serde(default)]
    pub custom_args: Vec<String>,
    /// Compliance gates: skip a source strictly below these (already small/efficient).
    #[serde(default)]
    pub skip_if_resolution_below: Option<String>,
    #[serde(default)]
    pub skip_if_bitrate_below: Option<Bitrate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Crop {
    #[default]
    None,
    Auto,
}

fn d_bit_depth() -> u8 {
    10
}
fn d_encoder() -> Encoder {
    Encoder::Vaapi
}
fn d_quality() -> QualityMode {
    QualityMode {
        mode: QualityKind::Auto,
        value: QualityValue::Num(22),
    }
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

impl Bitrate {
    /// Validate a canonical string (`128k`/`5M`), rejecting junk (fail-fast).
    fn parse(s: String) -> Result<Self, String> {
        let num = s.trim_end_matches(char::is_alphabetic);
        if num.is_empty() || num.parse::<f64>().is_err() {
            return Err(format!(
                "invalid bitrate {s:?} (expected an int in kbps or a string like \"128k\"/\"5M\")"
            ));
        }
        Ok(Self(s))
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
        match Raw::deserialize(d)? {
            Raw::Int(kbps) => Ok(Self(format!("{kbps}k"))),
            Raw::Str(s) => Self::parse(s).map_err(de::Error::custom),
        }
    }
}

/// Video quality control mode (spec 004): the semantics differ per encoder, so the
/// value is interpreted against the mode. `auto` = the backend's native RC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum QualityKind {
    #[default]
    Auto,
    Qp,
    Crf,
    Bitrate,
    Vmaf,
}

/// The value carried by a [`QualityMode`]: an integer (qp/crf/vmaf/auto) or a
/// bitrate (bitrate mode). Which one is enforced by [`QualityMode`]'s deserialize.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum QualityValue {
    Num(u8),
    Rate(Bitrate),
}

/// Video quality: `{ mode, value }`, or the back-compat shorthand `quality = N`
/// (→ `{ auto, N }`). Keeps the profile hardware-agnostic — `auto` resolves to the
/// backend's native RC (VAAPI qp / CPU crf) at command-build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QualityMode {
    pub mode: QualityKind,
    pub value: QualityValue,
}

impl QualityMode {
    /// The rate-control ffmpeg options, given whether the backend is VAAPI (else CPU).
    ///
    /// # Errors
    /// `quality.mode = "vmaf"` (`AutoCRF`) is accepted by the schema but not yet
    /// materializable (spec 004 R5).
    pub fn rc_opts(&self, vaapi: bool) -> Result<Vec<(&'static str, String)>, EngineError> {
        Ok(match (self.mode, &self.value) {
            (QualityKind::Auto, QualityValue::Num(n)) => {
                vec![(if vaapi { "qp" } else { "crf" }, n.to_string())]
            }
            (QualityKind::Qp, QualityValue::Num(n)) => vec![("qp", n.to_string())],
            (QualityKind::Crf, QualityValue::Num(n)) => vec![("crf", n.to_string())],
            (QualityKind::Bitrate, QualityValue::Rate(b)) => vec![("b:v", b.as_arg().to_string())],
            (QualityKind::Vmaf, _) => {
                return Err(EngineError::Unsupported(
                    "quality.mode = \"vmaf\" (AutoCRF) — deferred (spec 004 R5)".to_string(),
                ));
            }
            _ => {
                return Err(EngineError::PlanProbeMismatch(
                    "quality mode/value type mismatch".to_string(),
                ));
            }
        })
    }
}

impl<'de> Deserialize<'de> for QualityMode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawVal {
            Int(u64),
            Str(String),
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Short(u64),
            Full {
                #[serde(default)]
                mode: QualityKind,
                value: RawVal,
            },
        }
        let (mode, raw) = match Raw::deserialize(d)? {
            Raw::Short(n) => (QualityKind::Auto, RawVal::Int(n)),
            Raw::Full { mode, value } => (mode, value),
        };
        let value = if mode == QualityKind::Bitrate {
            let b = match raw {
                RawVal::Int(kbps) => Bitrate(format!("{kbps}k")),
                RawVal::Str(s) => Bitrate::parse(s).map_err(de::Error::custom)?,
            };
            QualityValue::Rate(b)
        } else {
            let n: u8 = match raw {
                RawVal::Int(n) => u8::try_from(n)
                    .map_err(|_| de::Error::custom(format!("quality value {n} out of range")))?,
                RawVal::Str(s) => s.parse().map_err(|_| {
                    de::Error::custom(format!("quality value must be an integer, got {s:?}"))
                })?,
            };
            let max = if mode == QualityKind::Vmaf { 100 } else { 51 };
            if n > max {
                return Err(de::Error::custom(format!(
                    "video.quality value must be 0..={max}, got {n}"
                )));
            }
            QualityValue::Num(n)
        };
        Ok(Self { mode, value })
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
    // spec 004 additions.
    /// Re-encode the KEPT tracks to this codec/bitrate (not just the stereo clone).
    #[serde(default)]
    pub transcode: Option<AudioTranscode>,
    /// Generate a mono track from the best kept source per language.
    #[serde(default)]
    pub add_mono: Option<MonoConfig>,
    #[serde(default)]
    pub max_channels: Option<u32>,
    #[serde(default)]
    pub normalize: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioTranscode {
    pub codec: String,
    pub bitrate: Bitrate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonoConfig {
    #[serde(default = "d_aac")]
    pub codec: String,
    #[serde(default = "d_mono_bitrate")]
    pub bitrate: Bitrate,
    #[serde(default)]
    pub languages: Vec<String>,
}

fn d_mono_bitrate() -> Bitrate {
    Bitrate("64k".to_string())
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
    // spec 004 additions.
    /// Fixed positional order (language codes); kept subs are emitted in this order.
    #[serde(default)]
    pub order: Vec<String>,
    #[serde(default)]
    pub forced_only: bool,
    /// Extract kept subs to sidecar files, e.g. `["srt"]` (in-container track still kept).
    #[serde(default)]
    pub extract: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)] // config flags, not a state machine
pub struct OutputConfig {
    #[serde(default = "d_mkv")]
    pub container: String,
    #[serde(default = "d_true")]
    pub replace_original: bool,
    // spec 004 additions.
    /// Drop streams the target container cannot hold (avoids mux failures).
    #[serde(default)]
    pub conform: bool,
    #[serde(default)]
    pub strip_metadata: bool,
    #[serde(default = "d_true")]
    pub keep_chapters: bool,
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
    fn quality_mode_forms_and_rc_opts() {
        // shorthand → auto; auto resolves to qp (VAAPI) / crf (CPU)
        let q: QualityMode = serde_json::from_str("22").unwrap();
        assert_eq!(q.mode, QualityKind::Auto);
        assert_eq!(q.rc_opts(true).unwrap(), vec![("qp", "22".to_string())]);
        assert_eq!(q.rc_opts(false).unwrap(), vec![("crf", "22".to_string())]);
        // explicit modes
        let q: QualityMode = serde_json::from_str(r#"{"mode":"crf","value":18}"#).unwrap();
        assert_eq!(q.rc_opts(true).unwrap(), vec![("crf", "18".to_string())]);
        let q: QualityMode = serde_json::from_str(r#"{"mode":"bitrate","value":"5M"}"#).unwrap();
        assert_eq!(q.rc_opts(false).unwrap(), vec![("b:v", "5M".to_string())]);
        // vmaf accepted by schema but not materializable yet (R5)
        let q: QualityMode = serde_json::from_str(r#"{"mode":"vmaf","value":95}"#).unwrap();
        assert!(matches!(q.rc_opts(true), Err(EngineError::Unsupported(_))));
        // out of range still rejected at load
        assert!(serde_json::from_str::<QualityMode>(r#"{"mode":"qp","value":99}"#).is_err());
    }

    #[test]
    fn deserializes_spec004_coverage_fields() {
        let json = r#"{
            "video": {"codec":"hevc","preset":"medium","max_resolution":"1080p","crop":"auto",
                      "custom_args":["-x"],"skip_if_resolution_below":"480p","skip_if_bitrate_below":"2M"},
            "audio": {"transcode":{"codec":"opus","bitrate":"160k"},
                      "add_mono":{"languages":["eng"]},"max_channels":6,"normalize":true},
            "subtitles": {"order":["eng","spa"],"forced_only":true,"extract":["srt"]},
            "output": {"conform":true,"strip_metadata":true,"keep_chapters":false}
        }"#;
        let p: Profile = serde_json::from_str(json).unwrap();
        assert_eq!(p.video.preset.as_deref(), Some("medium"));
        assert_eq!(p.video.crop, Crop::Auto);
        assert_eq!(p.video.skip_if_bitrate_below.unwrap().as_arg(), "2M");
        assert_eq!(p.audio.transcode.unwrap().codec, "opus");
        assert_eq!(p.audio.add_mono.unwrap().bitrate.as_arg(), "64k"); // default
        assert_eq!(p.audio.max_channels, Some(6));
        assert_eq!(p.subtitles.order, vec!["eng", "spa"]);
        assert!(p.subtitles.forced_only);
        assert!(p.output.conform && !p.output.keep_chapters);
        // an unknown field still fails (deny_unknown_fields intact)
        assert!(
            serde_json::from_str::<Profile>(
                r#"{"video":{"codec":"hevc","presett":"x"},"audio":{},"subtitles":{},"output":{}}"#
            )
            .is_err()
        );
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
        assert_eq!(p.video.quality.mode, QualityKind::Auto);
        assert_eq!(p.video.quality.value, QualityValue::Num(22));
        assert_eq!(p.video.encoder, Encoder::Vaapi);
        assert!(p.audio.remove_commentary);
        assert!(p.audio.preserve_surround);
        assert_eq!(p.audio.default_language, "eng");
        assert_eq!(p.output.container, "mkv");
        assert!(p.output.replace_original);
    }
}
