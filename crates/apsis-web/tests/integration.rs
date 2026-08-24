//! Gated integration tests (spec 006 T009 + T014).
//!
//! Each drives the extractor-free server helpers (`apsis_web::server::*_inner` /
//! `publish_pause` / `request_*`) against a real `nats-server` (`APSIS_TEST_NATS`, or the
//! process-compose default) and skips cleanly when none is reachable — so `cargo test` stays
//! green on a machine without NATS.
//!
//! T009 asserts the file table is a faithful projection of the seeded KV. T014 asserts each
//! control helper puts *exactly* the spec 005 wire frame on the wire that an operator would
//! send by hand with the `nats` CLI — same subject, same JSON — and threads the owner's reply
//! back out.

use std::time::Duration;

use apsis_common::control::{
    CancelOutcome, CancelReply, CancelRequest, Disposition, PauseIntent, PauseMode, PauseScope,
    SUBJECT_CONTROL_CANCEL, SUBJECT_CONTROL_PAUSE, SUBJECT_CONTROL_STATE, StateControlReply,
    StateControlRequest, StateOp, StateOutcome,
};
use apsis_common::{
    ConsumerTuning, KvStateStore, StateStore, Status, connect, ensure_topology,
    schema::{Decision, DecisionKind, StateEntry},
};
use apsis_web::server::{list_files_inner, publish_pause, request_cancel, request_state};
use futures::StreamExt;
use time::OffsetDateTime;
use tokio::time::timeout;

fn nats_url() -> String {
    std::env::var("APSIS_TEST_NATS").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string())
}

fn tuning() -> ConsumerTuning {
    ConsumerTuning {
        ack_wait: Duration::from_secs(5),
        max_deliver: 3,
        max_ack_pending: 1,
        backoff: vec![],
    }
}

/// Connect + provision the KV, or `None` (⇒ skip) when no server answers within 2s.
async fn connect_or_skip() -> Option<(async_nats::Client, KvStateStore)> {
    let url = nats_url();
    let Ok(Ok((client, ctx))) = timeout(Duration::from_secs(2), connect(&url)).await else {
        eprintln!("no nats-server at {url}; skipping");
        return None;
    };
    let store = ensure_topology(&ctx, &tuning()).await.ok()?;
    Some((client, KvStateStore::new(store)))
}

// ---- T009: the file table reflects the KV --------------------------------------------------

#[tokio::test]
async fn list_files_reflects_seeded_kv() {
    let Some((_client, kv)) = connect_or_skip().await else {
        return;
    };
    let path = "/hdd/media/tv/apsis-web-it-t009.mkv";
    let _ = kv.delete(path).await; // clean slate across reruns

    let entry = StateEntry {
        status: Status::Done,
        version: "1723800000:1048576000".into(),
        job_id: None,
        attempts: 2,
        used_fallback: false,
        updated_at: OffsetDateTime::UNIX_EPOCH,
        last_error: None,
        decision: Some(Decision {
            kind: DecisionKind::CompliantCodec,
            detail: "hevc".into(),
        }),
    };
    kv.put(path, &entry).await.expect("seed KV");

    let rows = list_files_inner(&kv).await.expect("list_files_inner");
    let row = rows
        .iter()
        .find(|r| r.path == path)
        .expect("seeded file appears in the table");
    assert_eq!(row.status, "Done");
    assert_eq!(row.version, "1723800000:1048576000");
    // The decision is projected exactly as the detail view / UI renders it.
    assert_eq!(row.decision.as_deref(), Some("CompliantCodec: hevc"));
    assert!(!row.ignored);

    let _ = kv.delete(path).await;
}

// ---- T014: each control helper emits the spec 005 frame the CLI would --------------------

#[tokio::test]
async fn pause_helper_publishes_spec005_frame() {
    let Some((client, _kv)) = connect_or_skip().await else {
        return;
    };
    let mut sub = client
        .subscribe(SUBJECT_CONTROL_PAUSE)
        .await
        .expect("subscribe pause");
    client.flush().await.unwrap();

    publish_pause(&client, None, false, true)
        .await
        .expect("publish_pause");

    let msg = timeout(Duration::from_secs(2), sub.next())
        .await
        .expect("pause frame arrives")
        .expect("stream open");
    let intent: PauseIntent = serde_json::from_slice(&msg.payload).expect("decode PauseIntent");
    assert_eq!(
        intent,
        PauseIntent {
            scope: PauseScope::Global,
            mode: PauseMode::Soft,
            set: true,
        }
    );
}

#[tokio::test]
async fn cancel_helper_roundtrips_and_matches_cli() {
    let Some((client, _kv)) = connect_or_skip().await else {
        return;
    };
    let mut sub = client
        .subscribe(SUBJECT_CONTROL_CANCEL)
        .await
        .expect("subscribe cancel");
    client.flush().await.unwrap();

    // Stand in for the worker: capture the request, reply with an outcome.
    let responder = client.clone();
    let owner = tokio::spawn(async move {
        let msg = sub.next().await.expect("request arrives");
        let req: CancelRequest =
            serde_json::from_slice(&msg.payload).expect("decode CancelRequest");
        let reply = serde_json::to_vec(&CancelReply {
            outcome: CancelOutcome::Cancelled,
        })
        .unwrap();
        responder
            .publish(msg.reply.expect("reply inbox"), reply.into())
            .await
            .unwrap();
        responder.flush().await.unwrap();
        req
    });

    let outcome = request_cancel(&client, "01JABCDEF0123456789ABCDEFG".into(), false)
        .await
        .expect("request_cancel");
    let req = timeout(Duration::from_secs(2), owner)
        .await
        .expect("owner replies")
        .unwrap();

    assert_eq!(req.job_id, "01JABCDEF0123456789ABCDEFG");
    assert_eq!(req.disposition, Disposition::Defer);
    assert_eq!(outcome, "Cancelled");
}

#[tokio::test]
async fn state_helper_roundtrips_requeue() {
    let Some((client, _kv)) = connect_or_skip().await else {
        return;
    };
    let mut sub = client
        .subscribe(SUBJECT_CONTROL_STATE)
        .await
        .expect("subscribe state");
    client.flush().await.unwrap();

    // Stand in for the coordinator.
    let responder = client.clone();
    let owner = tokio::spawn(async move {
        let msg = sub.next().await.expect("request arrives");
        let req: StateControlRequest =
            serde_json::from_slice(&msg.payload).expect("decode StateControlRequest");
        let reply = serde_json::to_vec(&StateControlReply {
            outcome: StateOutcome::Applied,
            detail: None,
        })
        .unwrap();
        responder
            .publish(msg.reply.expect("reply inbox"), reply.into())
            .await
            .unwrap();
        responder.flush().await.unwrap();
        req
    });

    let outcome = request_state(&client, "/hdd/media/tv/x.mkv".into(), "requeue")
        .await
        .expect("request_state");
    let req = timeout(Duration::from_secs(2), owner)
        .await
        .expect("owner replies")
        .unwrap();

    assert_eq!(req.path, "/hdd/media/tv/x.mkv");
    assert_eq!(req.op, StateOp::Requeue);
    assert_eq!(outcome, "Applied");
}
