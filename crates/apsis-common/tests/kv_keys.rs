//! Gated integration test: a media path with characters NATS KV rejects as raw keys (spaces,
//! parentheses) must round-trip through `KvStateStore` against a real `nats-server`. Guards the
//! base64url key encoding — before it, `put` of such a path failed with `invalid key`.
//!
//! Skips cleanly when no server is reachable (`APSIS_TEST_NATS`, or the process-compose default).

use std::time::Duration;

use apsis_common::{
    ConsumerTuning, KvStateStore, StateStore, Status, connect, ensure_topology, schema::StateEntry,
};
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

fn entry() -> StateEntry {
    StateEntry {
        status: Status::Done,
        version: "1723800000:2100000000".into(),
        job_id: None,
        attempts: 0,
        used_fallback: false,
        updated_at: OffsetDateTime::UNIX_EPOCH,
        last_error: None,
        decision: None,
    }
}

#[tokio::test]
async fn path_with_spaces_and_parens_round_trips() {
    let url = nats_url();
    let Ok(Ok((_client, ctx))) = timeout(Duration::from_secs(2), connect(&url)).await else {
        eprintln!("no nats-server at {url}; skipping");
        return;
    };
    let kv = KvStateStore::new(ensure_topology(&ctx, &tuning()).await.expect("kv bucket"));

    // The exact key the `nats` CLI / raw KV rejects with "invalid key".
    let path = "/hdd/media/movies/Dune (2021)/Dune.mkv";
    let _ = kv.delete(path).await; // clean slate across reruns

    // put + get round-trip (this is the write that used to fail).
    kv.put(path, &entry()).await.expect("put spaced path");
    let got = kv.get(path).await.expect("get").expect("present");
    assert_eq!(got.0.status, Status::Done);

    // list_keys decodes the base64url key back to the real path.
    let keys = kv.list_keys().await.expect("list");
    assert!(
        keys.contains(&path.to_string()),
        "list_keys yields the real path: {keys:?}"
    );

    // delete removes it (and does not surface after).
    kv.delete(path).await.expect("delete");
    assert!(kv.get(path).await.expect("get after delete").is_none());
}
