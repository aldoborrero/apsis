# Contract: control & introspection subjects

The NATS wire contract for the control plane. Operators publish/request these; the worker and
coordinator consume and execute. All payloads are JSON (serde). Subject constants live in
`apsis-common`.

## Subjects

| Subject | Kind | Payload → Reply | Consumer |
|---------|------|-----------------|----------|
| `apsis.control.pause` | publish | `PauseIntent` | coordinator (persists), workers (watch KV) |
| `apsis.control.cancel` | request/reply | `CancelRequest` → `CancelReply` | worker running the job |
| `apsis.control.state` | request/reply | `StateControlRequest` → `StateControlReply` | coordinator |
| `apsis.progress.<job_id>` | publish | `ProgressEvent` | anyone (CLI, console) |

Request/reply uses NATS' built-in reply-subject. A request to a **down** owner times out
(the client retries) — nothing is queued or replayed (interactive, non-durable).

## Messages

```jsonc
// apsis.control.pause  (publish; coordinator persists to the `control` KV key)
PauseIntent {
  "scope": "global" | { "worker": "<worker_id>" },
  "mode":  "soft" | "hard",   // soft: finish in-flight, withhold new claims; hard: also cancel in-flight
  "set":   true               // false = resume (clear)
}

// apsis.control.cancel  (request/reply; broadcast, only the worker running job_id acts)
CancelRequest  { "job_id": "<ulid>", "disposition": "defer" | "ignore" }
CancelReply    { "outcome": "cancelled" | "not_running" | "already_done" }

// apsis.control.state  (request/reply; coordinator, serialized with reconcile)
StateControlRequest {
  "path": "/coordinator/space/file.mkv",
  "op":   "requeue" | "retry" | "mark_done" | "force"
}
StateControlReply { "outcome": "applied" | "not_found" | "noop", "detail": "<optional>" }

// apsis.progress.<job_id>  (publish; ephemeral, core NATS)
ProgressEvent { "job_id": "<ulid>", "speed": 3.2, "eta_s": 812, "out_time_s": 415.0 }
```

## Semantics

- **pause**: `set:true` writes the effective pause into the coordinator-owned `control` KV
  key; workers watch it and re-read on reconnect (FR-001). `hard` additionally triggers a
  cancel of the in-flight job (as `apsis.control.cancel` with `disposition:defer`).
- **cancel**: the worker kills its ffmpeg **child** (source left byte-identical), applies the
  disposition (defer → clear KV; ignore → write the on-disk marker), and **`ack`/`term`s** the
  JetStream message — **never `nak`** (a nak redelivers and re-transcodes). A cancel arriving
  after `atomic_replace` began → `already_done`.
- **state**: executed serialized with the reconcile loop (single writer). `force` uses the
  `PlanOptions{force}` planning path to suppress `should_skip`.
- **progress**: fire-and-forget; only meaningful while the job runs. `speed`/`eta`/`out_time`
  from ffmpeg `-progress` — **no `percent`** (`duration` is `0.0` until the probe reads
  container format).

## Authorization (subject permissions)

Operator credentials: **publish** `apsis.control.*`, **subscribe** `apsis.progress.*`,
**read** the KV. **No** KV write, **no** publish to the job stream. The daemons hold fuller
credentials. This makes the "owners are the sole writers" invariant (FR-014) enforced, not
conventional.
