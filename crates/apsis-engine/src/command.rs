//! Materialize a [`FilePlan`] into an ffmpeg command (port of
//! `_engine/command.py`'s `build_encode_command_from_plan`).

use crate::config::{Encoder, HardwareConfig, Profile, VideoCodec};
use crate::error::EngineError;
use crate::ffmpeg::FfmpegCommand;
use crate::plan::{FilePlan, TrackAction, VideoAction, parse_resolution_height};
use crate::probe::StreamInfo;

/// Backend selection + paths for building the command.
pub struct BuildOptions<'a> {
    pub vaapi_device: &'a str,
    pub use_cpu: bool,
    pub ffmpeg_path: &'a str,
    pub hardware: Option<&'a HardwareConfig>,
}

fn vaapi_encoder(codec: VideoCodec) -> &'static str {
    match codec {
        VideoCodec::Hevc => "hevc_vaapi",
        VideoCodec::Av1 => "av1_vaapi",
    }
}

fn cpu_encoder(codec: VideoCodec) -> &'static str {
    match codec {
        VideoCodec::Hevc => "libx265",
        VideoCodec::Av1 => "libsvtav1",
    }
}

/// Relative `0:a:N` / `0:s:N` index for a source stream, by absolute index.
///
/// Mirrors the Python `indices.index(source_index)` (first match), and mirrors
/// its `ValueError` when the plan references a stream absent from the probe —
/// we fail loudly rather than mint a command that muxes the wrong stream.
fn relative_index(
    streams: &[StreamInfo],
    source_index: u32,
    kind: &str,
) -> Result<usize, EngineError> {
    streams
        .iter()
        .position(|s| s.index == source_index)
        .ok_or_else(|| {
            let indices: Vec<u32> = streams.iter().map(|s| s.index).collect();
            EngineError::PlanProbeMismatch(format!(
                "{kind} stream index {source_index} not found in available {kind} streams: {indices:?}"
            ))
        })
}

/// Build the ffmpeg command for `plan` under `profile` and the given options.
///
/// # Errors
/// Returns [`EngineError::PlanProbeMismatch`] if a track plan lacks a
/// `source_index`, or references a stream absent from `plan.source_probe`.
pub fn build_command(
    plan: &FilePlan,
    profile: &Profile,
    opts: &BuildOptions<'_>,
) -> Result<FfmpegCommand, EngineError> {
    let mut cmd = FfmpegCommand::new();
    cmd.set_ffmpeg_path(opts.ffmpeg_path);

    let use_vaapi = profile.video.encoder == Encoder::Vaapi && !opts.use_cpu;
    if let Some(hw) = opts.hardware {
        cmd.configure_hardware(hw);
    }
    if use_vaapi {
        cmd.set_vaapi_device(opts.vaapi_device);
        if let Some(v) = &plan.source_probe.video {
            cmd.set_input_codec(&v.codec);
        }
    }
    cmd.add_input(&plan.output.input_path, &[]);

    // --- Video ---
    let v_idx = cmd.map_stream("0:v:0");
    if plan.video.action == VideoAction::Copy {
        cmd.set_codec(v_idx, "copy", &[]);
    } else if use_vaapi {
        let codec = profile.video.codec;
        let async_depth = opts.hardware.map_or(4, |h| h.vaapi.async_depth);
        let mut vopts = profile.video.quality.rc_opts(true)?;
        vopts.push(("async_depth", async_depth.to_string()));
        // AMD a53_cc workaround: emit HDR SEI only, dropping a53_cc, on hevc/h264.
        // The engine only targets hevc/av1; av1 has no SEI concept, so scope to hevc.
        if codec == VideoCodec::Hevc {
            vopts.push(("sei", "hdr".to_string()));
        }
        cmd.set_codec(v_idx, vaapi_encoder(codec), &vopts);
    } else {
        let mut copts = profile.video.quality.rc_opts(false)?;
        copts.push((
            "preset",
            profile
                .video
                .preset
                .clone()
                .unwrap_or_else(|| "medium".to_string()),
        ));
        cmd.set_codec(v_idx, cpu_encoder(profile.video.codec), &copts);
    }
    cmd.set_metadata(v_idx, "title", "");

    // max_resolution: downscale only, and only when the video is being encoded
    // (a `-vf` with `-c:v copy` is rejected by ffmpeg). Backend-specific filter —
    // `scale_vaapi` runs on the uploaded VAAPI surface, `scale` on CPU frames.
    // `-1`/`-2` keeps the source aspect (CPU rounds width to even for the encoder).
    if plan.video.action != VideoAction::Copy
        && let Some(max) = profile.video.max_resolution.as_deref()
        && let Some(target_h) = parse_resolution_height(max)
        && let Some(src) = &plan.source_probe.video
        && src.height > target_h
    {
        let filter = if use_vaapi {
            format!("scale_vaapi=w=-1:h={target_h}")
        } else {
            format!("scale=-2:{target_h}")
        };
        cmd.add_video_filter(&filter);
    }

    // --- Audio ---
    for item in &plan.audio {
        // Python asserts source_index is present, then raises if it is not found
        // in the probe (command.py). We mirror both as PlanProbeMismatch rather
        // than silently dropping or mis-mapping a stream.
        let src = item.source_index.ok_or_else(|| {
            EngineError::PlanProbeMismatch("audio track plan is missing source_index".to_string())
        })?;
        let rel = relative_index(&plan.source_probe.audio, src, "audio")?;
        let a_idx = cmd.map_stream(&format!("0:a:{rel}"));
        if item.action == TrackAction::Copy {
            cmd.set_codec(a_idx, "copy", &[]);
        } else {
            // The encode bitrate rides on the plan item (add_stereo/add_mono/transcode
            // each set their own) — one source of truth, no divergence.
            cmd.set_codec(
                a_idx,
                &item.target_codec,
                &[
                    ("ac", item.target_channels.to_string()),
                    ("b", item.bitrate.as_arg().to_string()),
                ],
            );
        }
        cmd.set_metadata(a_idx, "title", &item.title_after);
        cmd.set_disposition(a_idx, if item.default { "default" } else { "0" });
    }

    // --- Subtitles ---
    for item in &plan.subtitles {
        let src = item.source_index.ok_or_else(|| {
            EngineError::PlanProbeMismatch(
                "subtitle track plan is missing source_index".to_string(),
            )
        })?;
        let rel = relative_index(&plan.source_probe.subtitles, src, "subtitle")?;
        let s_idx = cmd.map_stream(&format!("0:s:{rel}"));
        cmd.set_codec(s_idx, "copy", &[]);
        cmd.set_disposition(s_idx, if item.default { "default" } else { "0" });
    }
    // CC recovery (cc_subtitle_path) is deferred — Phase 2, `_RECOVER_CC` parked.

    // Output-level knobs (spec 004): metadata/chapters stripping, then the raw
    // custom_args escape hatch — all in the single command, before the output file.
    let mut extra: Vec<String> = Vec::new();
    if profile.output.strip_metadata {
        extra.extend(["-map_metadata".to_string(), "-1".to_string()]);
    }
    if !profile.output.keep_chapters {
        extra.extend(["-map_chapters".to_string(), "-1".to_string()]);
    }
    extra.extend(profile.video.custom_args.iter().cloned());
    cmd.add_output_args(&extra);

    cmd.set_output(&plan.output.output_path);
    Ok(cmd)
}

/// A transcode backend: turns a plan into a concrete ffmpeg command.
pub trait Backend {
    /// # Errors
    /// Propagates [`EngineError::PlanProbeMismatch`] from [`build_command`].
    fn build(&self, plan: &FilePlan, profile: &Profile) -> Result<FfmpegCommand, EngineError>;

    /// Short identifier for logs/metrics (`"vaapi"`, `"cpu"`).
    fn name(&self) -> &'static str;
}

/// VAAPI (AMD) backend: `hevc_vaapi`/`av1_vaapi` with the `sei=hdr` workaround.
pub struct VaapiBackend {
    pub hardware: HardwareConfig,
    pub vaapi_device: String,
    pub ffmpeg_path: String,
}

impl Backend for VaapiBackend {
    fn build(&self, plan: &FilePlan, profile: &Profile) -> Result<FfmpegCommand, EngineError> {
        build_command(
            plan,
            profile,
            &BuildOptions {
                vaapi_device: &self.vaapi_device,
                use_cpu: false,
                ffmpeg_path: &self.ffmpeg_path,
                hardware: Some(&self.hardware),
            },
        )
    }

    fn name(&self) -> &'static str {
        "vaapi"
    }
}

/// CPU fallback backend: `libx265`/`libsvtav1`.
pub struct CpuBackend {
    pub ffmpeg_path: String,
}

impl Backend for CpuBackend {
    fn build(&self, plan: &FilePlan, profile: &Profile) -> Result<FfmpegCommand, EngineError> {
        build_command(
            plan,
            profile,
            &BuildOptions {
                vaapi_device: "",
                use_cpu: true,
                ffmpeg_path: &self.ffmpeg_path,
                hardware: None,
            },
        )
    }

    fn name(&self) -> &'static str {
        "cpu"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HardwareConfig;
    use crate::plan::plan;
    use crate::probe::{Probe, StreamInfo};

    fn profile() -> Profile {
        serde_json::from_str(
            r#"{"video":{"codec":"hevc","skip_codecs":[]},"audio":{},"subtitles":{},"output":{"container":"mkv"}}"#,
        )
        .unwrap()
    }

    fn video(codec: &str) -> StreamInfo {
        StreamInfo {
            index: 0,
            codec_type: "video".into(),
            codec: codec.into(),
            ..Default::default()
        }
    }

    fn audio(index: u32, lang: &str, channels: u32, codec: &str) -> StreamInfo {
        StreamInfo {
            index,
            codec_type: "audio".into(),
            codec: codec.into(),
            language: lang.into(),
            channels,
            ..Default::default()
        }
    }

    fn vaapi() -> VaapiBackend {
        VaapiBackend {
            hardware: HardwareConfig::default(),
            vaapi_device: "/dev/dri/renderD128".into(),
            ffmpeg_path: "ffmpeg".into(),
        }
    }

    #[test]
    fn vaapi_video_only() {
        let probe = Probe {
            video: Some(video("h264")),
            ..Default::default()
        };
        let plan = plan("in.mkv", &probe, &profile());
        let args = vaapi().build(&plan, &profile()).unwrap().build();
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
            "-metadata:s:v:0",
            "title=",
            "in.mkv",
        ];
        assert_eq!(args, expected);
    }

    #[test]
    fn vaapi_with_audio_copy_and_retitle() {
        let probe = Probe {
            video: Some(video("h264")),
            audio: vec![audio(1, "eng", 6, "eac3")],
            ..Default::default()
        };
        let plan = plan("in.mkv", &probe, &profile());
        let args = vaapi().build(&plan, &profile()).unwrap().build();
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
            "-metadata:s:a:0",
            "title=English / EAC3 / 5.1",
            "-disposition:a:0",
            "default",
            "in.mkv",
        ];
        assert_eq!(args, expected);
    }

    #[test]
    fn cpu_video_only() {
        let probe = Probe {
            video: Some(video("h264")),
            ..Default::default()
        };
        let plan = plan("in.mkv", &probe, &profile());
        let cpu = CpuBackend {
            ffmpeg_path: "ffmpeg".into(),
        };
        let args = cpu.build(&plan, &profile()).unwrap().build();
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
            "-metadata:s:v:0",
            "title=",
            "in.mkv",
        ];
        assert_eq!(args, expected);
    }

    #[test]
    fn errors_when_audio_source_index_absent_from_probe() {
        // A plan whose audio item points at a stream the probe doesn't have must
        // fail loudly (Python raises ValueError) — never silently map 0:a:0.
        let probe = Probe {
            video: Some(video("h264")),
            audio: vec![audio(1, "eng", 2, "aac")],
            ..Default::default()
        };
        let mut plan = plan("in.mkv", &probe, &profile());
        plan.audio[0].source_index = Some(99); // not in probe.audio
        let err = vaapi().build(&plan, &profile()).unwrap_err();
        assert!(
            matches!(err, EngineError::PlanProbeMismatch(ref m) if m.contains("99")),
            "expected PlanProbeMismatch mentioning 99, got: {err}"
        );
    }

    #[test]
    fn errors_when_audio_source_index_missing() {
        let probe = Probe {
            video: Some(video("h264")),
            audio: vec![audio(1, "eng", 2, "aac")],
            ..Default::default()
        };
        let mut plan = plan("in.mkv", &probe, &profile());
        plan.audio[0].source_index = None; // Python asserts source_index is not None
        let err = vaapi().build(&plan, &profile()).unwrap_err();
        assert!(matches!(err, EngineError::PlanProbeMismatch(_)));
    }

    #[test]
    fn output_knobs_and_custom_args_precede_output() {
        let prof: Profile = serde_json::from_str(
            r#"{"video":{"codec":"hevc","skip_codecs":[],"custom_args":["-x265-params","log-level=none"]},
                "audio":{},"subtitles":{},
                "output":{"container":"mkv","strip_metadata":true,"keep_chapters":false}}"#,
        )
        .unwrap();
        let probe = Probe {
            video: Some(video("h264")),
            ..Default::default()
        };
        let p = plan("in.mkv", &probe, &prof);
        let args = vaapi().build(&p, &prof).unwrap().build();
        let joined = args.join(" ");
        assert!(joined.contains("-map_metadata -1"), "{joined}");
        assert!(joined.contains("-map_chapters -1"), "{joined}");
        assert!(joined.contains("-x265-params log-level=none"), "{joined}");
        // the output file is the last arg; custom_args come before it (single pass)
        assert_eq!(args.last().map(String::as_str), Some("in.mkv"));
        let cx = args.iter().position(|a| a == "-x265-params").unwrap();
        assert!(cx < args.len() - 1);
    }

    #[test]
    fn bitrate_mode_emits_valid_video_stream_specifier() {
        // quality.mode = bitrate must materialize as `-b:v:0 5M`, NOT the malformed
        // `-b:v:v:0` a pre-baked `b:v` key would produce (the golden tests otherwise
        // only cover default auto→qp/crf, so this path was untested).
        let prof: Profile = serde_json::from_str(
            r#"{"video":{"codec":"hevc","skip_codecs":[],"quality":{"mode":"bitrate","value":"5M"}},
                "audio":{},"subtitles":{},"output":{"container":"mkv"}}"#,
        )
        .unwrap();
        let probe = Probe {
            video: Some(video("h264")),
            ..Default::default()
        };
        let p = plan("in.mkv", &probe, &prof);
        let joined = vaapi().build(&p, &prof).unwrap().build().join(" ");
        assert!(joined.contains("-b:v:0 5M"), "{joined}");
        assert!(
            !joined.contains("-b:v:v:0"),
            "malformed specifier: {joined}"
        );
    }

    #[test]
    fn generated_stereo_audio_encodes_with_ac_and_bitrate() {
        // A 6ch source + add_stereo(eng) → the plan emits copy(5.1) AND a generated
        // aac stereo track; this exercises the ENCODE audio branch of build_command
        // (target codec / -ac / -b), which the copy-only golden tests never reach.
        let prof: Profile = serde_json::from_str(
            r#"{"video":{"codec":"hevc","skip_codecs":["hevc"]},
                "audio":{"add_stereo":{"languages":["eng"]}},
                "subtitles":{},"output":{"container":"mkv"}}"#,
        )
        .unwrap();
        let probe = Probe {
            video: Some(video("hevc")),
            audio: vec![audio(1, "eng", 6, "eac3")],
            ..Default::default()
        };
        let plan = plan("in.mkv", &probe, &prof);
        assert_eq!(plan.audio.len(), 2, "copy 5.1 + generated stereo");
        assert_eq!(plan.audio[1].action, TrackAction::Encode);

        let args = vaapi().build(&plan, &prof).unwrap().build();
        let joined = args.join(" ");
        // Encoded stereo track a:1: aac, downmixed to 2ch, at add_stereo.bitrate.
        assert!(joined.contains("-c:a:1 aac"), "{joined}");
        assert!(joined.contains("-ac:a:1 2"), "{joined}");
        assert!(joined.contains("-b:a:1 128k"), "{joined}");
        // The original surround track a:0 stays a copy.
        assert!(joined.contains("-c:a:0 copy"), "{joined}");
    }

    #[test]
    fn max_resolution_downscales_only_when_larger() {
        let prof: Profile = serde_json::from_str(
            r#"{"video":{"codec":"hevc","skip_codecs":[],"max_resolution":"1080p"},
                "audio":{},"subtitles":{},"output":{"container":"mkv"}}"#,
        )
        .unwrap();
        let uhd = StreamInfo {
            index: 0,
            codec_type: "video".into(),
            codec: "h264".into(),
            height: 2160,
            ..Default::default()
        };

        // VAAPI SW-decode: the scale runs on the uploaded surface, so it must come
        // AFTER the hwupload filter inside the single `-vf`.
        let probe = Probe {
            video: Some(uhd.clone()),
            ..Default::default()
        };
        let plan = plan("in.mkv", &probe, &prof);
        let joined = vaapi().build(&plan, &prof).unwrap().build().join(" ");
        assert!(
            joined.contains("-vf format=nv12,hwupload_vaapi,scale_vaapi=w=-1:h=1080"),
            "{joined}"
        );

        // CPU path: plain `scale`, even width.
        let cpu = CpuBackend {
            ffmpeg_path: "ffmpeg".into(),
        };
        let joined = cpu.build(&plan, &prof).unwrap().build().join(" ");
        assert!(joined.contains("-vf scale=-2:1080"), "{joined}");

        // Source already ≤ target height → no scale filter at all.
        let sd = StreamInfo { height: 720, ..uhd };
        let probe_sd = Probe {
            video: Some(sd),
            ..Default::default()
        };
        let plan_sd = crate::plan::plan("in.mkv", &probe_sd, &prof);
        let joined = vaapi().build(&plan_sd, &prof).unwrap().build().join(" ");
        assert!(!joined.contains("scale_vaapi"), "no downscale: {joined}");
    }
}
