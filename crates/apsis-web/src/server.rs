//! Server functions — the NATS bridge (spec 006). All bodies run only on the server; the
//! client gets a fetch stub. Each `#[server]` fn extracts the reverse-proxy user + shared
//! state, then delegates to a plain (extractor-free) `*_inner` helper. The helpers hold the
//! real logic — reading the spec 005 KV, publishing/requesting control intents — so they are
//! unit/integration-testable against a live NATS without an axum request context.

use crate::view::{FileDetail, FileRow};
use leptos::prelude::*;

/// Server-only shared state: the NATS KV + client, injected as an axum `Extension`.
#[cfg(feature = "ssr")]
#[derive(Clone)]
pub struct ServerState {
    pub kv: apsis_common::KvStateStore,
    pub client: async_nats::Client,
}

// ---- read helpers (extractor-free, testable) --------------------------------------------

/// Build the file table from the KV — the logic behind [`list_files`].
#[cfg(feature = "ssr")]
pub async fn list_files_inner(
    kv: &apsis_common::KvStateStore,
) -> Result<Vec<FileRow>, apsis_common::StoreError> {
    use apsis_common::{StateStore, has_ignore_marker};

    let keys = kv.list_keys().await?;
    let mut rows = Vec::with_capacity(keys.len());
    for path in keys {
        let Some((e, _)) = kv.get(&path).await? else {
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

/// Project one file's full state — the logic behind [`file_detail`].
#[cfg(feature = "ssr")]
pub async fn file_detail_inner(
    kv: &apsis_common::KvStateStore,
    path: &str,
) -> Result<Option<FileDetail>, apsis_common::StoreError> {
    use apsis_common::{StateStore, has_ignore_marker};

    let Some((e, _)) = kv.get(path).await? else {
        return Ok(None);
    };
    Ok(Some(FileDetail {
        status: status_str(e.status).to_string(),
        version: e.version,
        job_id: e.job_id.map(|id| id.to_string()),
        attempts: e.attempts,
        updated_at: e
            .updated_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        last_error: e.last_error,
        decision: e.decision.map(|d| format!("{:?}: {}", d.kind, d.detail)),
        ignored: has_ignore_marker(std::path::Path::new(path)),
        path: path.to_string(),
    }))
}

// ---- control helpers (extractor-free, testable) -----------------------------------------

/// Publish a pause intent — the logic behind [`pause`]. Emits *exactly* the spec 005 frame
/// an operator would `nats pub apsis.control.pause` by hand.
#[cfg(feature = "ssr")]
pub async fn publish_pause(
    client: &async_nats::Client,
    worker: Option<String>,
    hard: bool,
    set: bool,
) -> Result<(), ServerFnError> {
    use apsis_common::control::{PauseIntent, PauseMode, PauseScope, SUBJECT_CONTROL_PAUSE};
    let intent = PauseIntent {
        scope: worker.map_or(PauseScope::Global, PauseScope::Worker),
        mode: if hard {
            PauseMode::Hard
        } else {
            PauseMode::Soft
        },
        set,
    };
    let bytes = serde_json::to_vec(&intent).map_err(sfe)?;
    client
        .publish(SUBJECT_CONTROL_PAUSE, bytes.into())
        .await
        .map_err(sfe)?;
    Ok(())
}

/// Request a cancel and return the owner's outcome — the logic behind [`cancel`].
#[cfg(feature = "ssr")]
pub async fn request_cancel(
    client: &async_nats::Client,
    job_id: String,
    ignore: bool,
) -> Result<String, ServerFnError> {
    use apsis_common::control::{CancelReply, CancelRequest, Disposition, SUBJECT_CONTROL_CANCEL};
    let req = CancelRequest {
        job_id,
        disposition: if ignore {
            Disposition::Ignore
        } else {
            Disposition::Defer
        },
    };
    let bytes = serde_json::to_vec(&req).map_err(sfe)?;
    let reply = client
        .request(SUBJECT_CONTROL_CANCEL, bytes.into())
        .await
        .map_err(sfe)?;
    let r: CancelReply = serde_json::from_slice(&reply.payload).map_err(sfe)?;
    Ok(format!("{:?}", r.outcome))
}

/// Request a manual state op and return the coordinator's outcome — the logic behind
/// [`state_op`]. `op` is one of `requeue` | `retry` | `mark_done` | `force`.
#[cfg(feature = "ssr")]
pub async fn request_state(
    client: &async_nats::Client,
    path: String,
    op: &str,
) -> Result<String, ServerFnError> {
    use apsis_common::control::{
        SUBJECT_CONTROL_STATE, StateControlReply, StateControlRequest, StateOp,
    };
    let op = match op {
        "requeue" => StateOp::Requeue,
        "retry" => StateOp::Retry,
        "mark_done" => StateOp::MarkDone,
        "force" => StateOp::Force,
        _ => return Err(ServerFnError::new(format!("unknown op {op:?}"))),
    };
    let req = StateControlRequest { path, op };
    let bytes = serde_json::to_vec(&req).map_err(sfe)?;
    let reply = client
        .request(SUBJECT_CONTROL_STATE, bytes.into())
        .await
        .map_err(sfe)?;
    let r: StateControlReply = serde_json::from_slice(&reply.payload).map_err(sfe)?;
    Ok(format!("{:?}", r.outcome))
}

// ---- server functions (extract auth + state, then delegate) -----------------------------

/// List every tracked file with its status + persisted decision (spec 006 US1).
#[server]
pub async fn list_files() -> Result<Vec<FileRow>, ServerFnError> {
    let (_user, state) = ssr_ctx().await?;
    list_files_inner(&state.kv).await.map_err(err)
}

/// The full state of one file (spec 006 US2).
// Explicit endpoint struct name — the default (`FileDetail`) would collide with the view type.
#[server(FetchFileDetail)]
pub async fn file_detail(path: String) -> Result<Option<FileDetail>, ServerFnError> {
    let (_user, state) = ssr_ctx().await?;
    file_detail_inner(&state.kv, &path).await.map_err(err)
}

/// Publish a pause intent (spec 005 US1). `worker = None` → global; `hard` → abort in-flight.
#[server]
pub async fn pause(worker: Option<String>, hard: bool, set: bool) -> Result<(), ServerFnError> {
    let (_user, state) = ssr_ctx().await?;
    publish_pause(&state.client, worker, hard, set).await
}

/// Cancel the active transcode of `job_id` (spec 005 US2). Returns the owner's outcome.
#[server]
pub async fn cancel(job_id: String, ignore: bool) -> Result<String, ServerFnError> {
    let (_user, state) = ssr_ctx().await?;
    request_cancel(&state.client, job_id, ignore).await
}

/// A manual state op (spec 005 US3): `requeue` | `retry` | `mark_done` | `force`.
#[server]
pub async fn state_op(path: String, op: String) -> Result<String, ServerFnError> {
    let (_user, state) = ssr_ctx().await?;
    request_state(&state.client, path, &op).await
}

// ---- ssr plumbing -----------------------------------------------------------------------

/// Extract the reverse-proxy user (401 if absent) + the shared NATS state, in that order.
/// Every server function goes through here, so an unauthenticated request never reaches NATS.
#[cfg(feature = "ssr")]
async fn ssr_ctx() -> Result<(crate::auth::ProxyUser, ServerState), ServerFnError> {
    use axum::Extension;
    use leptos_axum::extract;
    let user: crate::auth::ProxyUser = extract().await?;
    let Extension(state): Extension<ServerState> = extract().await?;
    Ok((user, state))
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
