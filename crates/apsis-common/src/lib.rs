//! apsis-common — shared infrastructure for the coordinator and worker.
//!
//! Wire schemas ([`schema`]), filesystem helpers ([`fsutil`]), and path
//! translation ([`pathmap`]) are implemented. Config (figment/garde) and the
//! NATS wiring (`JetStream` topology + the `Queue`/`StateStore` traits) land in
//! the next Foundational increments (spec 002).

pub mod config;
pub mod fsutil;
pub mod nats;
pub mod pathmap;
pub mod schema;
pub mod store;

pub use config::{
    BackendConfig, BackendKind, ConfigError, Library, Reconcile, SchedulerConfig, VerifyConfig,
    WorkerConfig, load_scheduler, load_worker,
};
pub use fsutil::{DEFAULT_VIDEO_EXTENSIONS, is_video, version_token};
pub use nats::{ConsumerTuning, bind_job_consumer, connect, ensure_topology};
pub use pathmap::PathMap;
pub use schema::{Job, Outcome, StateEntry, Status, TranscodeResult};
pub use store::{
    FakeQueue, FakeStateStore, JobPublisher, KvStateStore, NatsPublisher, StateStore, StoreError,
};
