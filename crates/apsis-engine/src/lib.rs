//! apsis-engine — transcode planner & ffmpeg command builder.
//!
//! Port of the pyflows `_engine`: given a [`Probe`] + [`Profile`], decide the
//! plan ([`FilePlan`]) and (in a later phase) materialize the exact ffmpeg
//! command per backend. Pure logic — no orchestration, queue, or file-watch.
//!
//! See `specs/001-apsis-engine/` for the spec, plan, and data model.
//!
//! **Status:** the plan *logic* (`plan`, audio/subtitle selection, the HDR-copy
//! override) is NOT ported yet — only the types exist. This crate is not
//! oracle-faithful until spec 001 phase 3 (US1) lands `plan_from_probe`.

pub mod config;
pub mod error;
pub mod plan;
pub mod probe;

pub use config::{
    AudioConfig, Encoder, Fallback, HdrPolicy, OutputConfig, Profile, StereoConfig, SubtitleConfig,
    VideoCodec, VideoConfig,
};
pub use error::EngineError;
pub use plan::{
    AudioTrackPlan, FilePlan, OutputPlan, PlanReason, PlanScope, PlanStatus, ReasonCode, SubAction,
    SubtitleTrackPlan, TrackAction, VideoAction, VideoPlan,
};
pub use probe::{Probe, StreamInfo, parse_probe};
