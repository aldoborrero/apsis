//! ffprobe parsing (port of `_engine/probe.py`): raw ffprobe JSON → typed streams.

use serde::{Deserialize, Serialize};

use crate::error::EngineError;

/// One media stream, mirroring the Python `StreamInfo`. Strict: this type faces
/// the oracle-parity fixtures, so every field is required and unknown fields are
/// rejected (a shape mismatch must be a hard error, not a silent default).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamInfo {
    pub index: u32,
    pub codec_type: String,
    pub codec: String,
    pub language: String,
    pub title: String,
    pub channels: u32,
    pub width: u32,
    pub height: u32,
    pub is_default: bool,
    pub color_transfer: String,
    pub color_primaries: String,
    pub color_space: String,
    // Added for the CEL profile-rule context (spec 004); default so existing
    // fixtures + `..Default::default()` construction keep working.
    #[serde(default)]
    pub bitrate: u32,
    #[serde(default)]
    pub bit_depth: u32,
    #[serde(default)]
    pub forced: bool,
}

/// Parsed probe result: the first video stream + all audio/subtitle streams.
// No `Eq`: `duration` is an `f64` (`format.duration` seconds). Nothing relies on `Probe: Eq`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    pub video: Option<StreamInfo>,
    pub audio: Vec<StreamInfo>,
    pub subtitles: Vec<StreamInfo>,
    /// Container duration in seconds (ffprobe `format.duration`), `0.0` if absent. Exposed to
    /// CEL profile rules as `duration`.
    #[serde(default)]
    pub duration: f64,
}

impl Probe {
    /// HDR iff the video stream's colour transfer is PQ or HLG.
    #[must_use]
    pub fn is_hdr(&self) -> bool {
        match &self.video {
            Some(v) => matches!(v.color_transfer.as_str(), "smpte2084" | "arib-std-b67"),
            None => false,
        }
    }
}

// --- Raw ffprobe JSON shapes (deserialization only) ---

#[derive(Deserialize)]
struct RawProbe {
    #[serde(default)]
    streams: Vec<RawStream>,
    #[serde(default)]
    format: RawFormat,
}

#[derive(Deserialize, Default)]
struct RawFormat {
    // ffprobe emits `duration` as a string ("1800.024000"); often absent for raw streams.
    #[serde(default)]
    duration: String,
}

#[derive(Deserialize)]
struct RawStream {
    index: u32,
    codec_type: String,
    #[serde(default)]
    codec_name: String,
    #[serde(default)]
    channels: u32,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    color_transfer: String,
    #[serde(default)]
    color_primaries: String,
    #[serde(default)]
    color_space: String,
    // ffprobe emits these as strings (and often omits them).
    #[serde(default)]
    bit_rate: String,
    #[serde(default)]
    bits_per_raw_sample: String,
    // The reliable 10/12-bit signal (`bits_per_raw_sample` is often absent for HEVC/AV1).
    #[serde(default)]
    pix_fmt: String,
    #[serde(default)]
    tags: RawTags,
    #[serde(default)]
    disposition: RawDisposition,
}

#[derive(Deserialize, Default)]
struct RawTags {
    #[serde(default)]
    language: String,
    #[serde(default)]
    title: String,
    // MKV records per-stream bitrate as a `BPS` (or language-suffixed) tag rather
    // than the container-level `bit_rate`; the reliable bitrate signal for MKV.
    #[serde(default, rename = "BPS", alias = "BPS-eng")]
    bps: String,
}

/// Derive bit depth: `pix_fmt` (reliable for 10/12-bit) → `bits_per_raw_sample` →
/// 8 (SDR default). Returns 0 only if a bogus `bits_per_raw_sample` is present.
fn bit_depth_from(pix_fmt: &str, bits_per_raw_sample: &str) -> u32 {
    if ["10le", "10be", "p010", "p210", "p410"]
        .iter()
        .any(|p| pix_fmt.contains(p))
    {
        10
    } else if ["12le", "12be", "p012", "p212"]
        .iter()
        .any(|p| pix_fmt.contains(p))
    {
        12
    } else if !bits_per_raw_sample.is_empty() {
        bits_per_raw_sample.parse().unwrap_or(0)
    } else {
        8
    }
}

#[derive(Deserialize, Default)]
struct RawDisposition {
    #[serde(default)]
    default: i32,
    #[serde(default)]
    forced: i32,
}

/// Parse `ffprobe -print_format json -show_streams -show_format` output into a [`Probe`].
pub fn parse_probe(json: &str) -> Result<Probe, EngineError> {
    let raw: RawProbe = serde_json::from_str(json)?;
    let mut probe = Probe::default();
    for s in raw.streams {
        let info = StreamInfo {
            index: s.index,
            codec_type: s.codec_type,
            codec: s.codec_name,
            language: s.tags.language,
            title: s.tags.title,
            channels: s.channels,
            width: s.width,
            height: s.height,
            is_default: s.disposition.default != 0,
            color_transfer: s.color_transfer,
            color_primaries: s.color_primaries,
            color_space: s.color_space,
            // stream `bit_rate` first, then MKV's `BPS` tag, else 0 (unknown).
            bitrate: {
                let br = s.bit_rate.parse().unwrap_or(0);
                if br != 0 {
                    br
                } else {
                    s.tags.bps.parse().unwrap_or(0)
                }
            },
            bit_depth: bit_depth_from(&s.pix_fmt, &s.bits_per_raw_sample),
            forced: s.disposition.forced != 0,
        };
        match info.codec_type.as_str() {
            "video" if probe.video.is_none() => probe.video = Some(info),
            "audio" => probe.audio.push(info),
            "subtitle" => probe.subtitles.push(info),
            _ => {}
        }
    }
    probe.duration = raw.format.duration.parse().unwrap_or(0.0);
    Ok(probe)
}

/// Optional helper: run ffprobe on a path and parse the result.
#[cfg(feature = "probe-exec")]
pub fn probe_file(path: &std::path::Path, ffprobe: &std::path::Path) -> Result<Probe, EngineError> {
    let out = std::process::Command::new(ffprobe)
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
        ])
        .arg(path)
        .output()?;
    if !out.status.success() {
        return Err(EngineError::FfprobeStatus {
            code: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    parse_probe(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_ffprobe_json() {
        let json = r#"{"streams":[
            {"index":0,"codec_type":"video","codec_name":"h264","width":1920,"height":1080,
             "color_transfer":"bt709","disposition":{"default":1}},
            {"index":1,"codec_type":"audio","codec_name":"eac3","channels":6,
             "tags":{"language":"eng","title":"Surround"},"disposition":{"default":1}},
            {"index":2,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle",
             "tags":{"language":"eng"}}
        ]}"#;
        let p = parse_probe(json).unwrap();
        let v = p.video.as_ref().unwrap();
        assert_eq!(v.codec, "h264");
        assert_eq!((v.width, v.height), (1920, 1080));
        assert!(v.is_default);
        assert!(!p.is_hdr());
        assert_eq!(p.audio.len(), 1);
        assert_eq!(p.audio[0].language, "eng");
        assert_eq!(p.audio[0].channels, 6);
        assert_eq!(p.subtitles.len(), 1);
        assert_eq!(p.subtitles[0].codec, "hdmv_pgs_subtitle");
    }

    #[test]
    fn parses_bitrate_bitdepth_forced() {
        // ffprobe emits bit_rate / bits_per_raw_sample as strings; disposition.forced as int.
        let json = r#"{"streams":[
            {"index":0,"codec_type":"video","codec_name":"hevc","bit_rate":"8000000",
             "bits_per_raw_sample":"10","color_transfer":"smpte2084"},
            {"index":1,"codec_type":"subtitle","codec_name":"subrip",
             "tags":{"language":"eng"},"disposition":{"forced":1}}
        ]}"#;
        let p = parse_probe(json).unwrap();
        let v = p.video.as_ref().unwrap();
        assert_eq!(v.bitrate, 8_000_000);
        assert_eq!(v.bit_depth, 10);
        assert!(p.subtitles[0].forced);
        // absent fields default cleanly (no panic on missing bit_rate)
        assert_eq!(p.subtitles[0].bitrate, 0);
    }

    #[test]
    fn bit_depth_from_pix_fmt_and_bitrate_from_bps_tag() {
        // Real 10-bit HEVC in MKV: no bits_per_raw_sample, no stream bit_rate — the
        // signals are pix_fmt (yuv420p10le) and the MKV BPS tag.
        let json = r#"{"streams":[
            {"index":0,"codec_type":"video","codec_name":"hevc","pix_fmt":"yuv420p10le",
             "tags":{"BPS":"9500000"}}
        ]}"#;
        let v = parse_probe(json).unwrap().video.unwrap();
        assert_eq!(v.bit_depth, 10, "pix_fmt yuv420p10le → 10-bit");
        assert_eq!(v.bitrate, 9_500_000, "MKV BPS tag → bitrate");
        // 8-bit source with neither signal → default 8, not 0.
        let json = r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264","pix_fmt":"yuv420p"}]}"#;
        assert_eq!(parse_probe(json).unwrap().video.unwrap().bit_depth, 8);
    }

    #[test]
    fn detects_hdr_from_transfer() {
        let json = r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"hevc",
            "color_transfer":"smpte2084"}]}"#;
        assert!(parse_probe(json).unwrap().is_hdr());
    }

    #[test]
    fn parses_duration_from_show_format() {
        // `-show_format` puts the container duration (a string) under `format`; it feeds the
        // CEL `duration` variable. Absent `format` → 0.0, never an error.
        let json = r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"hevc"}],
            "format":{"filename":"x.mkv","format_name":"matroska","duration":"1830.024000"}}"#;
        assert!((parse_probe(json).unwrap().duration - 1830.024).abs() < 1e-6);

        let no_format = r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"hevc"}]}"#;
        assert!(parse_probe(no_format).unwrap().duration.abs() < f64::EPSILON);
    }

    #[cfg(feature = "probe-exec")]
    #[test]
    fn probe_file_errors_on_missing_ffprobe() {
        // Spawning a nonexistent binary yields an I/O error, not a panic (FR-010).
        let result = probe_file(
            std::path::Path::new("/nonexistent/input.mkv"),
            std::path::Path::new("/nonexistent/ffprobe-binary"),
        );
        assert!(matches!(result, Err(EngineError::Ffprobe(_))));
    }
}
