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
