//! ffmpeg command *builder* (port of `_engine/ffmpeg.py`'s `FFmpegCommand.build`).
//!
//! Only the command-building half is ported — running (progress/stall detection)
//! is the worker's job (spec 002), not the engine's.

use std::collections::BTreeMap;

use crate::config::HardwareConfig;

/// Codecs where VAAPI hardware decode is reliable on AMD/Mesa (Pipeline 1).
pub const DEFAULT_VAAPI_HW_DECODE_CODECS: [&str; 3] = ["hevc", "av1", "vp9"];

/// Builds an ffmpeg argument vector with per-stream codecs, metadata, and the
/// AMD VAAPI pipeline selection (HW-decode vs SW-decode + hwupload).
#[derive(Debug, Clone)]
pub struct FfmpegCommand {
    inputs: Vec<(String, Vec<(String, String)>)>,
    maps: Vec<(String, String, usize)>,
    codecs: BTreeMap<usize, (String, Vec<(String, String)>)>,
    metadata: Vec<(usize, String, String)>,
    dispositions: BTreeMap<usize, String>,
    global_opts: Vec<String>,
    vaapi_device: Option<String>,
    vaapi_name: String,
    vaapi_hw_decode_codecs: Vec<String>,
    vaapi_upload_filter: String,
    input_codec: Option<String>,
    ffmpeg_path: String,
    output: String,
}

impl Default for FfmpegCommand {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            maps: Vec::new(),
            codecs: BTreeMap::new(),
            metadata: Vec::new(),
            dispositions: BTreeMap::new(),
            global_opts: vec!["-y".to_string(), "-nostdin".to_string()],
            vaapi_device: None,
            vaapi_name: "va".to_string(),
            vaapi_hw_decode_codecs: DEFAULT_VAAPI_HW_DECODE_CODECS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            vaapi_upload_filter: "format=nv12,hwupload_vaapi".to_string(),
            input_codec: None,
            ffmpeg_path: "ffmpeg".to_string(),
            output: String::new(),
        }
    }
}

impl FfmpegCommand {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_vaapi_device(&mut self, device: &str) {
        self.vaapi_device = Some(device.to_string());
    }

    /// Apply runtime-configurable hardware/VAAPI settings. (`env` is used at
    /// run-time by the worker, not while building the args.)
    pub fn configure_hardware(&mut self, hw: &HardwareConfig) {
        self.vaapi_name.clone_from(&hw.vaapi.device);
        self.vaapi_hw_decode_codecs = hw
            .vaapi
            .hw_decode_codecs
            .iter()
            .map(|c| c.to_lowercase())
            .collect();
        self.vaapi_upload_filter.clone_from(&hw.vaapi.upload_filter);
    }

    pub fn set_input_codec(&mut self, codec: &str) {
        self.input_codec = Some(codec.to_string());
    }

    pub fn set_ffmpeg_path(&mut self, path: &str) {
        self.ffmpeg_path = path.to_string();
    }

    pub fn add_input(&mut self, path: &str, opts: &[(&str, String)]) {
        let opts = opts
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect();
        self.inputs.push((path.to_string(), opts));
    }

    /// Number of inputs added so far — the stream index of the next input.
    #[must_use]
    pub fn num_inputs(&self) -> usize {
        self.inputs.len()
    }

    /// Map a stream (e.g. `"0:v:0"`) and return its global output index.
    pub fn map_stream(&mut self, spec: &str) -> usize {
        let stype = spec.split(':').nth(1).unwrap_or("").to_string();
        let type_idx = self.maps.iter().filter(|(_, t, _)| *t == stype).count();
        self.maps.push((spec.to_string(), stype, type_idx));
        self.maps.len() - 1
    }

    pub fn set_codec(&mut self, idx: usize, codec: &str, opts: &[(&str, String)]) {
        let opts = opts
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect();
        self.codecs.insert(idx, (codec.to_string(), opts));
    }

    pub fn set_metadata(&mut self, idx: usize, key: &str, value: &str) {
        self.metadata
            .push((idx, key.to_string(), value.to_string()));
    }

    pub fn set_disposition(&mut self, idx: usize, value: &str) {
        self.dispositions.insert(idx, value.to_string());
    }

    pub fn set_output(&mut self, path: &str) {
        self.output = path.to_string();
    }

    fn use_hw_decode(&self) -> bool {
        match &self.input_codec {
            Some(c) => self.vaapi_hw_decode_codecs.iter().any(|x| x == c),
            None => false,
        }
    }

    /// Build the complete ffmpeg argument vector (deterministic ordering).
    #[must_use]
    pub fn build(&self) -> Vec<String> {
        let mut args = vec![self.ffmpeg_path.clone()];
        args.extend(self.global_opts.iter().cloned());

        if let Some(device) = &self.vaapi_device {
            args.push("-init_hw_device".to_string());
            args.push(format!("vaapi={}:{}", self.vaapi_name, device));
            if self.use_hw_decode() {
                for a in [
                    "-hwaccel",
                    "vaapi",
                    "-hwaccel_output_format",
                    "vaapi",
                    "-hwaccel_device",
                ] {
                    args.push(a.to_string());
                }
                args.push(self.vaapi_name.clone());
            } else {
                args.push("-filter_hw_device".to_string());
                args.push(self.vaapi_name.clone());
            }
        }

        for (path, opts) in &self.inputs {
            for (k, v) in opts {
                args.push(format!("-{k}"));
                args.push(v.clone());
            }
            args.push("-i".to_string());
            args.push(path.clone());
        }

        for (spec, _, _) in &self.maps {
            args.push("-map".to_string());
            args.push(spec.clone());
        }

        if self.vaapi_device.is_some() && !self.use_hw_decode() {
            args.push("-vf".to_string());
            args.push(self.vaapi_upload_filter.clone());
        }

        for (idx, (codec, opts)) in &self.codecs {
            let (_, stype, type_idx) = &self.maps[*idx];
            args.push(format!("-c:{stype}:{type_idx}"));
            args.push(codec.clone());
            for (k, v) in opts {
                args.push(format!("-{k}:{stype}:{type_idx}"));
                args.push(v.clone());
            }
        }

        for (idx, key, value) in &self.metadata {
            let (_, stype, type_idx) = &self.maps[*idx];
            args.push(format!("-metadata:s:{stype}:{type_idx}"));
            args.push(format!("{key}={value}"));
        }

        for (idx, value) in &self.dispositions {
            let (_, stype, type_idx) = &self.maps[*idx];
            args.push(format!("-disposition:{stype}:{type_idx}"));
            args.push(value.clone());
        }

        args.push(self.output.clone());
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vaapi_pipeline_2_sw_decode() {
        // h264 input → software decode + hwupload (Pipeline 2), hevc_vaapi + sei=hdr.
        let mut c = FfmpegCommand::new();
        c.set_vaapi_device("/dev/dri/renderD128");
        c.set_input_codec("h264");
        c.add_input("in.mkv", &[]);
        let v = c.map_stream("0:v:0");
        c.set_codec(
            v,
            "hevc_vaapi",
            &[
                ("qp", "22".to_string()),
                ("async_depth", "4".to_string()),
                ("sei", "hdr".to_string()),
            ],
        );
        c.set_metadata(v, "title", "");
        let a = c.map_stream("0:a:0");
        c.set_codec(a, "copy", &[]);
        c.set_disposition(a, "default");
        c.set_output("out.mkv");

        let expected = [
            "ffmpeg",
            "-y",
            "-nostdin",
            "-init_hw_device",
            "vaapi=va:/dev/dri/renderD128",
            "-filter_hw_device",
            "va",
            "-i",
            "in.mkv",
            "-map",
            "0:v:0",
            "-map",
            "0:a:0",
            "-vf",
            "format=nv12,hwupload_vaapi",
            "-c:v:0",
            "hevc_vaapi",
            "-qp:v:0",
            "22",
            "-async_depth:v:0",
            "4",
            "-sei:v:0",
            "hdr",
            "-c:a:0",
            "copy",
            "-metadata:s:v:0",
            "title=",
            "-disposition:a:0",
            "default",
            "out.mkv",
        ];
        assert_eq!(c.build(), expected);
    }

    #[test]
    fn vaapi_pipeline_1_hw_decode() {
        // hevc input → hardware decode (Pipeline 1): -hwaccel, no -vf.
        let mut c = FfmpegCommand::new();
        c.set_vaapi_device("/dev/dri/renderD128");
        c.set_input_codec("hevc");
        c.add_input("in.mkv", &[]);
        let v = c.map_stream("0:v:0");
        c.set_codec(v, "hevc_vaapi", &[("qp", "22".to_string())]);
        c.set_output("out.mkv");

        let expected = [
            "ffmpeg",
            "-y",
            "-nostdin",
            "-init_hw_device",
            "vaapi=va:/dev/dri/renderD128",
            "-hwaccel",
            "vaapi",
            "-hwaccel_output_format",
            "vaapi",
            "-hwaccel_device",
            "va",
            "-i",
            "in.mkv",
            "-map",
            "0:v:0",
            "-c:v:0",
            "hevc_vaapi",
            "-qp:v:0",
            "22",
            "out.mkv",
        ];
        assert_eq!(c.build(), expected);
    }

    #[test]
    fn cpu_no_vaapi() {
        let mut c = FfmpegCommand::new();
        c.add_input("in.mkv", &[]);
        let v = c.map_stream("0:v:0");
        c.set_codec(
            v,
            "libx265",
            &[("crf", "22".to_string()), ("preset", "medium".to_string())],
        );
        c.set_output("out.mkv");

        let expected = [
            "ffmpeg",
            "-y",
            "-nostdin",
            "-i",
            "in.mkv",
            "-map",
            "0:v:0",
            "-c:v:0",
            "libx265",
            "-crf:v:0",
            "22",
            "-preset:v:0",
            "medium",
            "out.mkv",
        ];
        assert_eq!(c.build(), expected);
    }
}
