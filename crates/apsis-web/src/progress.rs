//! Live progress SSE (spec 006 T007 / US1). A plain axum route — not a Leptos server function
//! — because it is a long-lived stream. It subscribes to core-NATS `apsis.progress.>` and
//! relays each `ProgressEvent` JSON verbatim as an SSE `data:` frame. Stateless: the browser
//! filters by `job_id`. Gated by the same reverse-proxy user header as everything else.

use crate::auth::ProxyUser;
use crate::server::ServerState;
use apsis_common::control::SUBJECT_PROGRESS_WILDCARD;
use axum::Extension;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::StreamExt;
use futures::stream::BoxStream;
use std::convert::Infallible;

/// `GET /progress` — an SSE stream of every worker's live transcode progress.
pub async fn progress_sse(
    _user: ProxyUser,
    Extension(state): Extension<ServerState>,
) -> impl IntoResponse {
    let stream: BoxStream<'static, Result<Event, Infallible>> =
        match state.client.subscribe(SUBJECT_PROGRESS_WILDCARD).await {
            Ok(sub) => sub
                .map(|msg| {
                    // The payload is already a `ProgressEvent` JSON — forward it untouched.
                    let data = String::from_utf8_lossy(&msg.payload).into_owned();
                    Ok(Event::default().data(data))
                })
                .boxed(),
            // If the subscribe fails the client simply sees an immediately-closed stream and
            // reconnects; the progress channel is best-effort, never a source of truth.
            Err(_) => futures::stream::empty().boxed(),
        };
    Sse::new(stream).keep_alive(KeepAlive::default())
}
