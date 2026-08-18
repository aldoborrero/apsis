//! apsis-common — shared infrastructure for the coordinator and worker.
//!
//! Wire schemas ([`schema`]), config ([`config`], figment + garde), filesystem
//! helpers ([`fsutil`]), path translation ([`pathmap`]), the `JetStream` topology
//! ([`nats`]), and the [`StateStore`]/[`JobPublisher`] abstractions with NATS
//! impls + in-memory fakes ([`store`]) are all implemented (spec 002 foundation).

pub mod config;
pub mod fsutil;
pub mod nats;
pub mod obs;
pub mod pathmap;
pub mod schema;
pub mod store;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use config::{
    BackendConfig, BackendKind, ConfigError, ConsumerConfig, Library, Reconcile, SchedulerConfig,
    VerifyConfig, WorkerConfig, load_scheduler, load_worker,
};
pub use fsutil::{DEFAULT_VIDEO_EXTENSIONS, is_video, version_token};
pub use nats::{ConsumerTuning, bind_job_consumer, connect, ensure_topology};
pub use obs::init_tracing;
pub use pathmap::PathMap;
pub use schema::{Job, Outcome, StateEntry, Status, TranscodeResult};
pub use store::{
    FakeJobPublisher, FakeStateStore, JobPublisher, KvStateStore, NatsPublisher, StateStore,
    StoreError, publish_result,
};
