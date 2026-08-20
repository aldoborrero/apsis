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

use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

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
    /// Encoder speed preset. **CPU-encoder only** (libx265/libsvtav1); the VAAPI
    /// backend has no `-preset` equivalent, so it is ignored on the VAAPI path (a
    /// VAAPI `compression_level` mapping is deferred). Default on CPU is `medium`.
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

    /// The value in bits/second (`"2M"` → `2_000_000`), for threshold comparisons.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )] // a non-negative bitrate magnitude truncated to whole bits/sec — intentional
    pub fn bps(&self) -> u64 {
        let s = self.0.as_str();
        let (num, mult) = match s.as_bytes().last() {
            Some(b'k' | b'K') => (&s[..s.len() - 1], 1_000f64),
            Some(b'm' | b'M') => (&s[..s.len() - 1], 1_000_000f64),
            Some(b'g' | b'G') => (&s[..s.len() - 1], 1_000_000_000f64),
            _ => (s, 1f64),
        };
        (num.parse::<f64>().unwrap_or(0.0) * mult).max(0.0) as u64
    }
}

impl Default for Bitrate {
    fn default() -> Self {
        Self("0k".to_string())
    }
}

impl Bitrate {
    /// Validate a bitrate string against a real grammar — `<digits>[.<digits>]`
    /// with an optional single `k|K|m|M|g|G` suffix, nothing else. Rejects signs,
    /// scientific notation, and trailing junk that would otherwise reach ffmpeg
    /// verbatim (e.g. `-5M`, `5Mbps`, `128kM`, `1e3k`, `+128k`).
    fn parse(s: String) -> Result<Self, String> {
        let bad =
            || format!("invalid bitrate {s:?} (expected e.g. `128` (kbps), \"128k\", or \"5M\")");
        let (num, unit_ok) = match s.chars().next_back() {
            Some(c) if c.is_ascii_alphabetic() => (
                &s[..s.len() - c.len_utf8()],
                matches!(c, 'k' | 'K' | 'm' | 'M' | 'g' | 'G'),
            ),
            _ => (s.as_str(), true),
        };
        let numeric_ok = !num.is_empty()
            && !num.starts_with('.')
            && !num.ends_with('.')
            && num.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            && num.bytes().filter(|&b| b == b'.').count() <= 1
            && num.parse::<f64>().is_ok();
        if unit_ok && numeric_ok {
            Ok(Self(s))
        } else {
            Err(bad())
        }
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityMode {
    pub mode: QualityKind,
    pub value: QualityValue,
}

// Manual, mode-directed Serialize: the fields are `pub`, so a `QualityMode` could
// be built with a value that doesn't match its mode (e.g. `Qp` + `Rate`). Deserialize
// enforces the pairing, but a derived Serialize would emit an unparseable shape for
// such a value. Emitting a value CONSISTENT with the mode keeps serialize total and
// its output always re-deserializable (the coordinator→worker `Job` JSON round-trip),
// with zero change for well-formed values.
impl Serialize for QualityMode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("QualityMode", 2)?;
        st.serialize_field("mode", &self.mode)?;
        match (self.mode, &self.value) {
            (QualityKind::Bitrate, QualityValue::Rate(b)) => st.serialize_field("value", b)?,
            (QualityKind::Bitrate, QualityValue::Num(n)) => {
                st.serialize_field("value", &Bitrate(format!("{n}k")))?;
            }
            (_, QualityValue::Num(n)) => st.serialize_field("value", n)?,
            (_, QualityValue::Rate(_)) => st.serialize_field("value", &0u8)?,
        }
        st.end()
    }
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
            // Bare key `b` → `build` appends the stream specifier → `-b:v:0` (a
            // pre-baked `b:v` here would become the malformed `-b:v:v:0`).
            (QualityKind::Bitrate, QualityValue::Rate(b)) => vec![("b", b.as_arg().to_string())],
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
        // The `{ mode, value }` value literal — an int (qp/crf/vmaf) or a bitrate str.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawVal {
            Int(i64),
            Str(String),
        }
        // Range-check a numeric quality for a non-bitrate mode (no lossy cast, no panic).
        fn num_quality<E: de::Error>(mode: QualityKind, n: i128) -> Result<QualityMode, E> {
            let max = if mode == QualityKind::Vmaf { 100 } else { 51 };
            match u8::try_from(n).ok().filter(|&v| i128::from(v) <= max) {
                Some(v) => Ok(QualityMode {
                    mode,
                    value: QualityValue::Num(v),
                }),
                None => Err(E::custom(format!(
                    "video.quality value must be 0..={max}, got {n}"
                ))),
            }
        }
        // A Visitor (not untagged) so a bare int gives a real range error, a table
        // rejects unknown keys, and a missing `value` is reported — restoring the
        // module's fail-fast invariant for the one field that lacked it.
        struct QmVisitor;
        impl<'de> Visitor<'de> for QmVisitor {
            type Value = QualityMode;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an integer quality (0..=51) or a { mode, value } table")
            }
            fn visit_u64<E: de::Error>(self, n: u64) -> Result<QualityMode, E> {
                num_quality(QualityKind::Auto, i128::from(n))
            }
            fn visit_i64<E: de::Error>(self, n: i64) -> Result<QualityMode, E> {
                num_quality(QualityKind::Auto, i128::from(n))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<QualityMode, A::Error> {
                let mut mode: Option<QualityKind> = None;
                let mut value: Option<RawVal> = None;
                while let Some(k) = map.next_key::<String>()? {
                    match k.as_str() {
                        "mode" => mode = Some(map.next_value()?),
                        "value" => value = Some(map.next_value()?),
                        other => return Err(de::Error::unknown_field(other, &["mode", "value"])),
                    }
                }
                let mode = mode.unwrap_or_default();
                let value = value.ok_or_else(|| de::Error::missing_field("value"))?;
                if mode == QualityKind::Bitrate {
                    let b = match value {
                        RawVal::Int(n) if n >= 0 => Bitrate(format!("{n}k")),
                        RawVal::Int(n) => {
                            return Err(de::Error::custom(format!(
                                "quality bitrate must be non-negative, got {n}"
                            )));
                        }
                        RawVal::Str(s) => Bitrate::parse(s).map_err(de::Error::custom)?,
                    };
                    Ok(QualityMode {
                        mode,
                        value: QualityValue::Rate(b),
                    })
                } else {
                    let n = match value {
                        RawVal::Int(n) => i128::from(n),
                        RawVal::Str(s) => s.parse::<i128>().map_err(|_| {
                            de::Error::custom(format!(
                                "quality value must be an integer, got {s:?}"
                            ))
                        })?,
                    };
                    num_quality(mode, n)
                }
            }
        }
        d.deserialize_any(QmVisitor)
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
        // bitrate mode → bare key "b" (so build makes -b:v:0, not -b:v:v:0)
        let q: QualityMode = serde_json::from_str(r#"{"mode":"bitrate","value":"5M"}"#).unwrap();
        assert_eq!(q.rc_opts(false).unwrap(), vec![("b", "5M".to_string())]);
        // vmaf accepted by schema but not materializable yet (R5)
        let q: QualityMode = serde_json::from_str(r#"{"mode":"vmaf","value":95}"#).unwrap();
        assert!(matches!(q.rc_opts(true), Err(EngineError::Unsupported(_))));
        // out of range rejected at load, with a real message (not an untagged miss)
        let err = serde_json::from_str::<QualityMode>(r#"{"mode":"qp","value":99}"#).unwrap_err();
        assert!(err.to_string().contains("0..=51"), "got: {err}");
        // a negative shorthand gets a real range error too
        let err = serde_json::from_str::<QualityMode>("-5").unwrap_err();
        assert!(err.to_string().contains("0..=51"), "got: {err}");
        // an unknown key in the quality table fails loudly (deny-unknown restored)
        assert!(serde_json::from_str::<QualityMode>(r#"{"mode":"crf","vlaue":18}"#).is_err());

        // mode-directed serialize: a well-formed value round-trips unchanged...
        let q = QualityMode {
            mode: QualityKind::Crf,
            value: QualityValue::Num(18),
        };
        assert_eq!(
            serde_json::from_str::<QualityMode>(&serde_json::to_string(&q).unwrap()).unwrap(),
            q
        );
        // ...and a (unreachable-from-config) mismatched value still serializes to a
        // shape that RE-deserializes, instead of an unparseable one.
        let mismatched = QualityMode {
            mode: QualityKind::Qp,
            value: QualityValue::Rate(Bitrate("0k".to_string())),
        };
        let json = serde_json::to_string(&mismatched).unwrap();
        assert_eq!(json, r#"{"mode":"qp","value":0}"#);
        assert!(serde_json::from_str::<QualityMode>(&json).is_ok());
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
        // valid string forms pass through (incl. a decimal)
        for ok in [r#""5M""#, r#""128k""#, r#""1.5M""#, r#""64000""#] {
            assert!(
                serde_json::from_str::<StereoConfig>(&format!(r#"{{"bitrate":{ok}}}"#)).is_ok(),
                "should accept {ok}"
            );
        }
        // junk fails at load (fail-fast) — the grammar rejects sign/exp/unit-junk
        for bad in [
            r#""loud""#,
            r#""-5M""#,
            r#""5Mbps""#,
            r#""128kM""#,
            r#""1e3k""#,
            r#""+128k""#,
            r#"".5k""#,
            r#""5.""#,
        ] {
            assert!(
                serde_json::from_str::<StereoConfig>(&format!(r#"{{"bitrate":{bad}}}"#)).is_err(),
                "should reject {bad}"
            );
        }
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
