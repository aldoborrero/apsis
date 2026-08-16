//! apsis-engine — transcode planner & ffmpeg command builder.
//!
//! Port of the pyflows `_engine`: given a [`Probe`] + [`Profile`], decide the
//! plan ([`FilePlan`]) and (in a later phase) materialize the exact ffmpeg
//! command per backend. Pure logic — no orchestration, queue, or file-watch.
//!
//! See `specs/001-apsis-engine/` for the spec, plan, and data model.

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
    AudioTrackPlan, FilePlan, OutputPlan, PlanReason, PlanStatus, SubAction, SubtitleTrackPlan,
    TrackAction, VideoAction, VideoPlan,
};
pub use probe::{parse_probe, Probe, StreamInfo};
