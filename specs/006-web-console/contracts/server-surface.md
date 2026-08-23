# Contract: apsis-web server surface

The only ways the browser talks to the server: Leptos **server functions** (typed RPC,
POST under `/api/`) and one **SSE route** for live progress. Every one is a thin wrapper over
the spec 005 NATS surface — the browser holds no NATS credentials, and nothing here does more
than the `nats` CLI can (constitution v3.0.0 gate).

## Server functions (reads)

```rust
// KV keys under `transcode_state` (excluding __control__/pause), each get → a row.
#[server] async fn list_files() -> Result<Vec<FileRow>, ServerFnError>;

// One file's full detail: StateEntry + decision (+ in-flight plan best-effort).
#[server] async fn file_detail(path: String) -> Result<FileDetail, ServerFnError>;

// The effective pause set (read of __control__/pause).
#[server] async fn pause_state() -> Result<PauseState, ServerFnError>;
```

## Server functions (controls — publish spec 005 intents)

Each returns the same outcome the equivalent `nats` command produces (SC-003).

```rust
// publish apsis.control.pause
#[server] async fn pause(scope: PauseScope, mode: PauseMode, set: bool)
    -> Result<(), ServerFnError>;

// request/reply apsis.control.cancel
#[server] async fn cancel(job_id: String, disposition: Disposition)
    -> Result<CancelOutcome, ServerFnError>;

// request/reply apsis.control.state (requeue | retry | mark_done | force)
#[server] async fn state_op(path: String, op: StateOp)
    -> Result<StateOutcome, ServerFnError>;
```

- `pause`/`cancel`/`state_op` reuse the `apsis_common::control` message types verbatim — the
  server function serializes exactly the spec 005 payload and awaits the same reply.
- A **down owner** → the request/reply times out → `ServerFnError` → the UI shows a retryable
  error (US3-3). Never a silent success.
- **Authorization**: a control server function requires the reverse-proxy user header (auth.rs);
  absent → 401 before any publish. Reads may be looser but are gated the same way for simplicity.

## SSE route — live progress

```
GET /progress            (text/event-stream)
```

Subscribes to core-NATS `apsis.progress.>` and forwards each `ProgressEvent` as an SSE `data:`
line. The client opens one `EventSource`, routes each tick into a per-`job_id` signal → the
running row's progress bar. Ephemeral/fire-and-forget: a dropped connection just reconnects and
resumes; no state is lost (progress is only meaningful live).

## What is deliberately NOT here

- **No KV writes**: the server never `put`/`delete`s `transcode_state` — it only publishes
  control intents; the coordinator/worker remain the sole KV writers (spec 005 FR-014).
- **No config surface**: no server function reads or writes `scheduler.toml`/`worker.toml`.
- **No aggregate/history endpoints**: savings-over-time, failure rates, latencies stay in
  Grafana; this surface is per-file live state + control only.
