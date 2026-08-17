//! `JetStream` topology: names + idempotent provisioning (create-if-absent), so a
//! cold NATS + git config self-provisions (contract §invariant 4).
//!
//! `ensure_topology` is safe to call on every startup. See
//! `specs/002-single-node-transcode/contracts/nats-protocol.md`.

use std::time::Duration;

use async_nats::jetstream::consumer::{AckPolicy, Consumer, pull};
use async_nats::jetstream::stream::{
    Config as StreamConfig, DiscardPolicy, RetentionPolicy, StorageType,
};
use async_nats::jetstream::{self, Context, kv};

pub const STREAM_NAME: &str = "APSIS_JOBS";
pub const SUBJECT_WILDCARD: &str = "jobs.transcode.>";
pub const SUBJECT_LOCAL: &str = "jobs.transcode.local";
pub const SUBJECT_RESULT: &str = "jobs.result";
pub const KV_BUCKET: &str = "transcode_state";
pub const CONSUMER_NAME: &str = "worker-local";

/// Consumer lease/retry tuning (contract §consumer). Usually derived from the
/// worker's `concurrency` + defaults.
#[derive(Debug, Clone)]
pub struct ConsumerTuning {
    pub ack_wait: Duration,
    pub max_deliver: i64,
    pub max_ack_pending: i64,
    pub backoff: Vec<Duration>,
}

impl Default for ConsumerTuning {
    fn default() -> Self {
        Self::for_concurrency(1)
    }
}

impl ConsumerTuning {
    /// Default tuning with `max_ack_pending` mirroring the worker's concurrency
    /// (contract §consumer — the broker-side bound of the local semaphore). Use
    /// this so the two can never silently drift.
    #[must_use]
    pub fn for_concurrency(concurrency: u32) -> Self {
        Self {
            ack_wait: Duration::from_mins(30),
            max_deliver: 4,
            max_ack_pending: i64::from(concurrency.max(1)),
            backoff: vec![
                Duration::from_mins(1),
                Duration::from_mins(5),
                Duration::from_mins(15),
            ],
        }
    }
}

/// Connect to NATS and return a `JetStream` context.
///
/// # Errors
/// Connection failure.
pub async fn connect(url: &str) -> Result<Context, async_nats::Error> {
    let client = async_nats::connect(url).await?;
    Ok(jetstream::new(client))
}

fn pull_config(tuning: &ConsumerTuning) -> pull::Config {
    pull::Config {
        durable_name: Some(CONSUMER_NAME.to_string()),
        name: Some(CONSUMER_NAME.to_string()),
        filter_subject: SUBJECT_LOCAL.to_string(),
        ack_policy: AckPolicy::Explicit,
        ack_wait: tuning.ack_wait,
        max_deliver: tuning.max_deliver,
        max_ack_pending: tuning.max_ack_pending,
        backoff: tuning.backoff.clone(),
        ..Default::default()
    }
}

/// Create the stream, pull consumer, and KV bucket if absent; return the KV store.
///
/// **Cold-start provisioning only.** On a *warm* NATS whose stream/consumer/bucket
/// already exist with a *different* config, `get_or_create_*` returns the existing
/// object unchanged — this does **not** reconcile drift back to the git config.
/// Changing tuning after first provisioning currently requires deleting the object
/// (or a future `update_stream`/`update_consumer` reconcile pass).
///
/// # Errors
/// Any `JetStream` provisioning failure.
pub async fn ensure_topology(
    ctx: &Context,
    tuning: &ConsumerTuning,
) -> Result<kv::Store, async_nats::Error> {
    let stream = ctx
        .get_or_create_stream(StreamConfig {
            name: STREAM_NAME.to_string(),
            subjects: vec![SUBJECT_WILDCARD.to_string()],
            retention: RetentionPolicy::WorkQueue,
            storage: StorageType::File,
            discard: DiscardPolicy::Old,
            max_age: Duration::ZERO, // jobs persist until acked (crash-safe)
            ..Default::default()
        })
        .await?;
    stream
        .get_or_create_consumer(CONSUMER_NAME, pull_config(tuning))
        .await?;
    ensure_kv(ctx).await
}

/// Idempotently obtain the KV bucket. `get`-first so a transient error is never
/// mistaken for "absent" (which would spuriously try to create); if `create`
/// races an existing bucket, `get` again rather than surfacing "already in use".
async fn ensure_kv(ctx: &Context) -> Result<kv::Store, async_nats::Error> {
    if let Ok(store) = ctx.get_key_value(KV_BUCKET).await {
        return Ok(store);
    }
    match ctx
        .create_key_value(kv::Config {
            bucket: KV_BUCKET.to_string(),
            history: 1,
            storage: StorageType::File,
            ..Default::default()
        })
        .await
    {
        Ok(store) => Ok(store),
        Err(_) => Ok(ctx.get_key_value(KV_BUCKET).await?),
    }
}

/// Bind the worker's pull consumer (idempotent; provisions if missing).
///
/// # Errors
/// If the stream or consumer cannot be fetched/created.
pub async fn bind_job_consumer(
    ctx: &Context,
    tuning: &ConsumerTuning,
) -> Result<Consumer<pull::Config>, async_nats::Error> {
    let stream = ctx.get_stream(STREAM_NAME).await?;
    let consumer = stream
        .get_or_create_consumer(CONSUMER_NAME, pull_config(tuning))
        .await?;
    Ok(consumer)
}
