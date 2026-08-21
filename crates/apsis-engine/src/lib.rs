//! apsis-engine — transcode planner & ffmpeg command builder.
//!
//! Port of the pyflows `_engine`: given a [`Probe`] + [`Profile`], decide the
//! plan ([`FilePlan`]) and (in a later phase) materialize the exact ffmpeg
//! command per backend. Pure logic — no orchestration, queue, or file-watch.
//!
//! See `specs/001-apsis-engine/` for the spec, plan, and data model.
//!
//! **Status:** planning ([`plan`]), probe parsing, audio/subtitle selection, and
//! command building (the [`Backend`] trait + [`FfmpegCommand`]) are ported and
//! covered by unit + golden tests (US1 + US2). A JSON-fixture oracle harness
//! (exporting from the Python engine) is the remaining parity task (T009).
//!
//! # Example
//!
//! ```
//! use apsis_engine::{Backend, HardwareConfig, Profile, VaapiBackend, parse_probe, plan};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let ffprobe_json = r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264"}]}"#;
//! let profile: Profile = serde_json::from_str(
//!     r#"{"video":{"codec":"hevc","skip_codecs":["hevc"]},
//!         "audio":{},"subtitles":{},"output":{"container":"mkv"}}"#,
//! )?;
//!
//! let probe = parse_probe(ffprobe_json)?; // or probe_file(path, ffprobe) with `probe-exec`
//! let file_plan = plan("/media/in.mkv", &probe, &profile);
//!
//! if file_plan.should_skip {
//!     // already compliant (or unsupported) → no transcode job
//! } else {
//!     let backend = VaapiBackend {
//!         hardware: HardwareConfig::default(),
//!         vaapi_device: "/dev/dri/renderD128".into(),
//!         ffmpeg_path: "ffmpeg".into(),
//!     };
//!     let args: Vec<String> = backend.build(&file_plan, &profile)?.build(); // exact ffmpeg argv
//!     assert_eq!(args[0], "ffmpeg");
//! }
//! # Ok(()) }
//! ```

pub mod audio;
pub mod command;
pub mod config;
mod constants;
pub mod error;
pub mod ffmpeg;
pub mod overrides;
pub mod plan;
pub mod probe;
pub mod subtitles;

pub use audio::{AudioAction, AudioActionKind, build_audio_plan};
pub use command::{Backend, BuildOptions, CpuBackend, VaapiBackend, build_command};
pub use config::{
    AudioConfig, AudioTranscode, Bitrate, Crop, Encoder, Fallback, HardwareConfig, HdrPolicy,
    MonoConfig, OutputConfig, Profile, ProfileRule, QualityKind, QualityMode, QualityValue,
    SetValue, StereoConfig, SubtitleConfig, VaapiConfig, VideoCodec, VideoConfig,
};
pub use error::EngineError;
pub use ffmpeg::FfmpegCommand;
pub use overrides::{
    CEL_CONTEXT_VERSION, FileFacts, build_context, resolve_effective_profile, validate_rules,
};
pub use plan::{
    AudioTrackPlan, FilePlan, OutputPlan, PlanReason, PlanScope, PlanStatus, ReasonCode, SubAction,
    SubtitleTrackPlan, TrackAction, VideoAction, VideoPlan, plan,
};
pub use probe::{Probe, StreamInfo, parse_probe};
pub use subtitles::filter_subtitles;
