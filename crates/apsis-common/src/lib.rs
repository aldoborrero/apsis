//! apsis-common — shared infrastructure for the coordinator and worker.
//!
//! Wire schemas ([`schema`]), filesystem helpers ([`fsutil`]), and path
//! translation ([`pathmap`]) are implemented. Config (figment/garde) and the
//! NATS wiring (`JetStream` topology + the `Queue`/`StateStore` traits) land in
//! the next Foundational increments (spec 002).

pub mod fsutil;
pub mod pathmap;
pub mod schema;

pub use fsutil::{DEFAULT_VIDEO_EXTENSIONS, is_video, version_token};
pub use pathmap::PathMap;
pub use schema::{Job, Outcome, StateEntry, Status, TranscodeResult};
