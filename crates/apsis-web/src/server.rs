//! Server functions — the NATS bridge (spec 006). All bodies run only on the server; the
//! client gets a fetch stub. Reads the spec 005 KV; (later) publishes control intents.

use crate::view::FileRow;
use leptos::prelude::*;

/// Server-only shared state: the NATS KV + client, injected as an axum `Extension`.
#[cfg(feature = "ssr")]
#[derive(Clone)]
pub struct ServerState {
    pub kv: apsis_common::KvStateStore,
    pub client: async_nats::Client,
}

/// List every tracked file with its status + persisted decision (spec 006 US1). Reads only
/// the spec 005 KV — no new backend.
#[server]
pub async fn list_files() -> Result<Vec<FileRow>, ServerFnError> {
    use apsis_common::{StateStore, has_ignore_marker};
    use axum::Extension;
    use leptos_axum::extract;

    let Extension(state): Extension<ServerState> = extract().await?;
    let keys = state.kv.list_keys().await.map_err(err)?;
    let mut rows = Vec::with_capacity(keys.len());
    for path in keys {
        let Some((e, _)) = state.kv.get(&path).await.map_err(err)? else {
            continue;
        };
        rows.push(FileRow {
            status: status_str(e.status).to_string(),
            version: e.version,
            decision: e.decision.map(|d| format!("{:?}: {}", d.kind, d.detail)),
            ignored: has_ignore_marker(std::path::Path::new(&path)),
            job_id: e.job_id.map(|id| id.to_string()),
            path,
        });
    }
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(rows)
}

#[cfg(feature = "ssr")]
fn status_str(s: apsis_common::Status) -> &'static str {
    use apsis_common::Status;
    match s {
        Status::Unknown => "Unknown",
        Status::Pending => "Pending",
        Status::InProgress => "InProgress",
        Status::Done => "Done",
        Status::Failed => "Failed",
    }
}

#[cfg(feature = "ssr")]
fn err(e: apsis_common::StoreError) -> ServerFnError {
    ServerFnError::new(e.to_string())
}

/// Map any Display error to a `ServerFnError`.
#[cfg(feature = "ssr")]
fn sfe(e: impl std::fmt::Display) -> ServerFnError {
    ServerFnError::new(e.to_string())
}

/// Publish a pause intent (spec 005 US1). `worker = None` → global; `hard` → abort in-flight.
#[server]
pub async fn pause(worker: Option<String>, hard: bool, set: bool) -> Result<(), ServerFnError> {
    use apsis_common::control::{PauseIntent, PauseMode, PauseScope, SUBJECT_CONTROL_PAUSE};
    use axum::Extension;
    use leptos_axum::extract;
    let Extension(state): Extension<ServerState> = extract().await?;
    let intent = PauseIntent {
        scope: worker.map_or(PauseScope::Global, PauseScope::Worker),
        mode: if hard { PauseMode::Hard } else { PauseMode::Soft },
        set,
    };
    let bytes = serde_json::to_vec(&intent).map_err(sfe)?;
    state
        .client
        .publish(SUBJECT_CONTROL_PAUSE, bytes.into())
        .await
        .map_err(sfe)?;
    Ok(())
}

/// Cancel the active transcode of `job_id` (spec 005 US2). Returns the owner's outcome.
#[server]
pub async fn cancel(job_id: String, ignore: bool) -> Result<String, ServerFnError> {
    use apsis_common::control::{
        CancelReply, CancelRequest, Disposition, SUBJECT_CONTROL_CANCEL,
    };
    use axum::Extension;
    use leptos_axum::extract;
    let Extension(state): Extension<ServerState> = extract().await?;
    let req = CancelRequest {
        job_id,
        disposition: if ignore {
            Disposition::Ignore
        } else {
            Disposition::Defer
        },
    };
    let bytes = serde_json::to_vec(&req).map_err(sfe)?;
    let reply = state
        .client
        .request(SUBJECT_CONTROL_CANCEL, bytes.into())
        .await
        .map_err(sfe)?;
    let r: CancelReply = serde_json::from_slice(&reply.payload).map_err(sfe)?;
    Ok(format!("{:?}", r.outcome))
}

/// A manual state op (spec 005 US3): `requeue` | `retry` | `mark_done` | `force`.
#[server]
pub async fn state_op(path: String, op: String) -> Result<String, ServerFnError> {
    use apsis_common::control::{
        StateControlReply, StateControlRequest, StateOp, SUBJECT_CONTROL_STATE,
    };
    use axum::Extension;
    use leptos_axum::extract;
    let op = match op.as_str() {
        "requeue" => StateOp::Requeue,
        "retry" => StateOp::Retry,
        "mark_done" => StateOp::MarkDone,
        "force" => StateOp::Force,
        _ => return Err(ServerFnError::new(format!("unknown op {op:?}"))),
    };
    let Extension(state): Extension<ServerState> = extract().await?;
    let req = StateControlRequest { path, op };
    let bytes = serde_json::to_vec(&req).map_err(sfe)?;
    let reply = state
        .client
        .request(SUBJECT_CONTROL_STATE, bytes.into())
        .await
        .map_err(sfe)?;
    let r: StateControlReply = serde_json::from_slice(&reply.payload).map_err(sfe)?;
    Ok(format!("{:?}", r.outcome))
}
