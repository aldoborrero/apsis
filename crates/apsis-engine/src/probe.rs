//! ffprobe parsing (port of `_engine/probe.py`): raw ffprobe JSON → typed streams.

use serde::{Deserialize, Serialize};

use crate::error::EngineError;

/// One media stream, mirroring the Python `StreamInfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamInfo {
    pub index: u32,
    pub codec_type: String,
    pub codec: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub channels: u32,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub color_transfer: String,
    #[serde(default)]
    pub color_primaries: String,
    #[serde(default)]
    pub color_space: String,
}

/// Parsed probe result: the first video stream + all audio/subtitle streams.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Probe {
    pub video: Option<StreamInfo>,
    #[serde(default)]
    pub audio: Vec<StreamInfo>,
    #[serde(default)]
    pub subtitles: Vec<StreamInfo>,
}

impl Probe {
    /// HDR iff the video stream's colour transfer is PQ or HLG.
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
}

#[derive(Deserialize, Default)]
struct RawDisposition {
    #[serde(default)]
    default: i32,
}

/// Parse `ffprobe -print_format json -show_streams` output into a [`Probe`].
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
        };
        match info.codec_type.as_str() {
            "video" if probe.video.is_none() => probe.video = Some(info),
            "audio" => probe.audio.push(info),
            "subtitle" => probe.subtitles.push(info),
            _ => {}
        }
    }
    Ok(probe)
}

/// Optional helper: run ffprobe on a path and parse the result.
#[cfg(feature = "probe-exec")]
pub fn probe_file(path: &std::path::Path, ffprobe: &std::path::Path) -> Result<Probe, EngineError> {
    let out = std::process::Command::new(ffprobe)
        .args(["-v", "quiet", "-print_format", "json", "-show_streams"])
        .arg(path)
        .output()?;
    if !out.status.success() {
        return Err(EngineError::FfprobeStatus);
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
    fn detects_hdr_from_transfer() {
        let json = r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"hevc",
            "color_transfer":"smpte2084"}]}"#;
        assert!(parse_probe(json).unwrap().is_hdr());
    }
}
