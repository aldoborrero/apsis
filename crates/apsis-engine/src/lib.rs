//! apsis-engine — transcode planner & ffmpeg command builder.
//!
//! Port of the pyflows `_engine`: given a [`Probe`] + [`Profile`], decide the
//! plan ([`FilePlan`]) and (in a later phase) materialize the exact ffmpeg
//! command per backend. Pure logic — no orchestration, queue, or file-watch.
//!
//! See `specs/001-apsis-engine/` for the spec, plan, and data model.
//!
//! **Status:** planning ([`plan`]), probe parsing, and audio/subtitle selection
//! are ported and covered by unit tests. Command building (the `Backend` trait)
//! is the next phase (US2). A JSON-fixture oracle harness (exporting from the
//! Python engine) is the remaining parity task (T009).

pub mod audio;
pub mod config;
mod constants;
pub mod error;
pub mod plan;
pub mod probe;
pub mod subtitles;

pub use audio::{AudioAction, AudioActionKind, build_audio_plan};
pub use config::{
    AudioConfig, Encoder, Fallback, HdrPolicy, OutputConfig, Profile, StereoConfig, SubtitleConfig,
    VideoCodec, VideoConfig,
};
pub use error::EngineError;
pub use plan::{
    AudioTrackPlan, FilePlan, OutputPlan, PlanReason, PlanScope, PlanStatus, ReasonCode, SubAction,
    SubtitleTrackPlan, TrackAction, VideoAction, VideoPlan, plan,
};
pub use probe::{Probe, StreamInfo, parse_probe};
pub use subtitles::filter_subtitles;
