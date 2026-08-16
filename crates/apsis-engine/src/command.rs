//! Materialize a [`FilePlan`] into an ffmpeg command (port of
//! `_engine/command.py`'s `build_encode_command_from_plan`).

use crate::config::{Encoder, HardwareConfig, Profile, VideoCodec};
use crate::ffmpeg::FfmpegCommand;
use crate::plan::{FilePlan, TrackAction, VideoAction};

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

/// Build the ffmpeg command for `plan` under `profile` and the given options.
#[must_use]
pub fn build_command(plan: &FilePlan, profile: &Profile, opts: &BuildOptions<'_>) -> FfmpegCommand {
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
        let mut vopts: Vec<(&str, String)> = vec![
            ("qp", profile.video.quality.to_string()),
            ("async_depth", async_depth.to_string()),
        ];
        // AMD a53_cc workaround: emit HDR SEI only, dropping a53_cc, on hevc/h264.
        // The engine only targets hevc/av1; av1 has no SEI concept, so scope to hevc.
        if codec == VideoCodec::Hevc {
            vopts.push(("sei", "hdr".to_string()));
        }
        cmd.set_codec(v_idx, vaapi_encoder(codec), &vopts);
    } else {
        cmd.set_codec(
            v_idx,
            cpu_encoder(profile.video.codec),
            &[
                ("crf", profile.video.quality.to_string()),
                ("preset", "medium".to_string()),
            ],
        );
    }
    cmd.set_metadata(v_idx, "title", "");

    // --- Audio ---
    for item in &plan.audio {
        // source_index always Some for real tracks; the invariant holds because the
        // plan was derived from this probe. Fall back rather than panic (FR-010).
        let Some(src) = item.source_index else {
            continue;
        };
        let rel = plan
            .source_probe
            .audio
            .iter()
            .position(|s| s.index == src)
            .unwrap_or(0);
        let a_idx = cmd.map_stream(&format!("0:a:{rel}"));
        if item.action == TrackAction::Copy {
            cmd.set_codec(a_idx, "copy", &[]);
        } else {
            let bitrate = profile.audio.add_stereo.bitrate;
            cmd.set_codec(
                a_idx,
                &item.target_codec,
                &[
                    ("ac", item.target_channels.to_string()),
                    ("b", format!("{bitrate}k")),
                ],
            );
        }
        cmd.set_metadata(a_idx, "title", &item.title_after);
        cmd.set_disposition(a_idx, if item.default { "default" } else { "0" });
    }

    // --- Subtitles ---
    for item in &plan.subtitles {
        let Some(src) = item.source_index else {
            continue;
        };
        let rel = plan
            .source_probe
            .subtitles
            .iter()
            .position(|s| s.index == src)
            .unwrap_or(0);
        let s_idx = cmd.map_stream(&format!("0:s:{rel}"));
        cmd.set_codec(s_idx, "copy", &[]);
        cmd.set_disposition(s_idx, if item.default { "default" } else { "0" });
    }
    // CC recovery (cc_subtitle_path) is deferred — Phase 2, `_RECOVER_CC` parked.

    cmd.set_output(&plan.output.output_path);
    cmd
}

/// A transcode backend: turns a plan into a concrete ffmpeg command.
pub trait Backend {
    fn build(&self, plan: &FilePlan, profile: &Profile) -> FfmpegCommand;
}

/// VAAPI (AMD) backend: `hevc_vaapi`/`av1_vaapi` with the `sei=hdr` workaround.
pub struct VaapiBackend {
    pub hardware: HardwareConfig,
    pub vaapi_device: String,
    pub ffmpeg_path: String,
}

impl Backend for VaapiBackend {
    fn build(&self, plan: &FilePlan, profile: &Profile) -> FfmpegCommand {
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
}

/// CPU fallback backend: `libx265`/`libsvtav1`.
pub struct CpuBackend {
    pub ffmpeg_path: String,
}

impl Backend for CpuBackend {
    fn build(&self, plan: &FilePlan, profile: &Profile) -> FfmpegCommand {
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
        let args = vaapi().build(&plan, &profile()).build();
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
        let args = vaapi().build(&plan, &profile()).build();
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
        let args = cpu.build(&plan, &profile()).build();
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
}
