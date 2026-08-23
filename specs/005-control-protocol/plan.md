# Implementation Plan: Control & introspection protocol over NATS

**Branch**: `005-control-protocol` | **Date**: 2026-08-23 | **Spec**: [spec.md](./spec.md)

## Summary

Add an operator control plane to a running apsis deployment — pause/resume, cancel the
active transcode, and manual state control (re-queue/force/retry/mark-done) — plus the
introspection substrate (persist the per-file skip *decision*, publish live progress). It is
carried **entirely over NATS subjects** (core request/reply for live signals; no HTTP API,
no JetStream control stream); the worker and coordinator are the **sole writers** of KV state
and execute the intents, enforced by NATS subject permissions. Usable from the `nats` CLI;
constitution-clean (no UI, no config edit). The web console (spec 006) is a later skin.

## Technical Context

**Language/Version**: Rust edition 2024, rust-version 1.87 (workspace).
**Primary Dependencies**: `async-nats` (core pub/sub, request/reply, JetStream KV — already a
dep); `tokio`; `serde`; `metrics`. **No new crate.**
**Storage**: NATS JetStream — the existing `transcode_state` KV (extended with a decision
field) + a new coordinator-owned `control` KV key for pause. Media on the shared FS; the
*ignore* marker is a small on-disk file beside the media (recoverable).
**Testing**: `cargo test` — unit (message schemas, engine `force`/skip-decision) + the gated
integration harness (worker pause/cancel, control×reconcile serialization) alongside the
existing `crash_safety.rs` NATS-backed tests.
**Target Platform**: Linux daemons (`apsis-coordinator`, `apsis-worker`); single-node today,
per-`worker_id` subjects extend to multi-host (spec 003).
**Project Type**: workspace of Rust daemons + a shared lib; this feature touches all three
runtime crates + `apsis-engine`.
**Constraints**: never break crash-safety (spec 002 suite must stay green); control is
operator-interactive (retry-on-failure, no durability); the engine stays pure.
**Scale/Scope**: a handful of control subjects; a bounded protocol, not a subsystem.

## Constitution Check

*GATE — PASS (no amendment).* This adds NATS subjects + two small state fields; it ships no
bespoke UI (Principle V), edits no config, and preserves recoverability (Principle I): every
transcode state stays rescan-derivable, the decision is a rebuildable projection, and the one
non-rederivable piece — an operator *ignore* — is a recoverable **on-disk marker**, not
live-only KV state. Re-checked after design: still clean. Spec 006 (the UI) is the artifact
that will require the Principle V amendment; nothing here does.

## Project Structure

### Documentation (this feature)

```text
specs/005-control-protocol/
├── plan.md              # this file
├── spec.md              # approved (2 review rounds)
├── data-model.md        # message + state schemas (Phase 1)
├── contracts/
│   └── control-subjects.md   # the subject namespace + request/reply shapes (Phase 1)
└── tasks.md             # Phase 2 (/tasks)
```

### Source Code

```text
crates/apsis-common/src/
├── control.rs           # NEW: subject constants + message schemas (Pause/Cancel/StateControl/Progress)
├── schema.rs            # StateEntry gains `decision: Option<Decision>` (permissive type)
└── nats.rs              # subject constants + (doc) subject-permission ACL

crates/apsis-engine/src/
├── plan.rs              # `PlanOptions { force }`; emit a positive `SkipReason` on the FilePlan
└── ...

crates/apsis-worker/src/
├── worker.rs            # pause-gate before claim; cancel handler (child-kill); ack/term mapping
├── control.rs           # NEW: subscribe pause/cancel; publish progress
└── ...

crates/apsis-coordinator/src/
├── control.rs           # NEW: request/reply consumer for state ops, serialized with reconcile
├── reconcile.rs         # persist the decision; honor the on-disk ignore marker; `force` path
└── main.rs              # own the pause KV key; wire the control consumer + audit metric
```

## Design

### 1. Subject namespace (contracts/control-subjects.md)

- `apsis.control.pause` — **publish** a `PauseIntent {scope, mode}`; the coordinator persists
  it to the `control` KV key. `scope ∈ {global, worker:<id>}`, `mode ∈ {soft, hard}`.
- `apsis.control.cancel` — **request/reply** `CancelRequest {job_id, disposition}` →
  `CancelReply {outcome: cancelled | not_running | already_done}`. Broadcast; only the worker
  running `job_id` acts.
- `apsis.control.state` — **request/reply** `StateControlRequest {path, op, force?}` →
  `StateControlReply {outcome}`, op ∈ {requeue, retry, mark_done, force}. Consumed by the
  coordinator.
- `apsis.progress.<job_id>` — **publish** `ProgressEvent {job_id, speed, eta, out_time}`
  (ephemeral, core NATS; no percent — `duration` is `0.0` today).

### 2. State & schemas (apsis-common)

- **`Decision`** — a NEW permissive struct (no `deny_unknown_fields`) holding the positive
  skip reason (`{kind: compliant_codec | resolution_below | bitrate_below | changes_required,
  detail: String}`). Added as `StateEntry.decision: Option<Decision>` — additive at the
  top level (StateEntry omits `deny_unknown_fields`), so old consumers ignore it. It does NOT
  embed `PlanReason`/`FilePlan` (both strict).
- **Pause KV key** — a single coordinator-owned `control` key holding the effective pause set;
  workers watch it read-only and re-read on reconnect.
- **Ignore marker** — an on-disk sidecar (`<file>.apsisignore`) written on *ignore*; the
  discover/gate path skips a source that has one. Recoverable (survives a KV wipe).

### 3. Engine (apsis-engine)

- `plan(path, probe, profile)` → `plan(path, probe, profile, &PlanOptions)` with
  `PlanOptions { force: bool }` (`Default` = not forced); existing callers pass default. When
  `force`, `should_skip` is suppressed so a compliant file is planned as an encode to the
  profile's target.
- The plan output gains the positive **`SkipReason`** (which gate / compliant codec) computed
  where `should_skip` is decided — so a skipped file has a *reason*, not an empty `reasons`.

### 4. Worker (apsis-worker)

- **Pause gate**: before each `claim()`, consult the watched pause key (global or this
  `worker_id`); if soft-paused, don't claim; if hard-paused, also cancel the in-flight job.
  Re-read on reconnect.
- **Cancel**: subscribe to `apsis.control.cancel`; if this worker runs `job_id`, **kill the
  ffmpeg child process** (the transcode future stays pinned/non-cancellable — killing the
  child makes ffmpeg exit and the normal error path discards the temp, leaving the source
  byte-identical). Then apply the disposition: *defer* → clear KV (reconcile re-plans);
  *ignore* → write the on-disk marker. **`ack`/`term` the message, never `nak`** (a nak would
  redeliver and re-transcode). Cancel after `atomic_replace` began → reply `already_done`.
- **Progress**: the transcode already parses ffmpeg `-progress`; publish `ProgressEvent`
  periodically to `apsis.progress.<job_id>`.

### 5. Coordinator (apsis-coordinator)

- **Pause ownership**: subscribe to `apsis.control.pause`; write the effective set to the
  `control` KV key (the single writer). 
- **State-control consumer**: a request/reply handler for `apsis.control.state`, executed
  **serialized with the reconcile loop** (a command channel drained at the top of each pass,
  or a shared mutex around KV writes) so it never races reconcile's unconditional `put()`.
  `requeue` clears the entry; `retry` clears `Failed`; `mark_done` writes `Done@version`
  (computing `mtime:size` if unprobed); `force` enqueues via the `PlanOptions{force}` path.
- **Decision persistence**: in `reconcile_file`, when marking `Done`/skip, populate
  `StateEntry.decision` from the plan's `SkipReason`.
- **Ignore gate**: `discover`/reconcile skips a source carrying the on-disk ignore marker
  (extend the existing gate, alongside `is_handled`).
- **Audit**: every control op logs (op, target, timestamp) and increments
  `apsis_control_ops_total{op}`.

### 6. NATS subject permissions (ACL)

Document (and, where the deployment enforces it, configure) operator credentials as: publish
`apsis.control.*`, subscribe `apsis.progress.*`, **read** the KV — **no KV write**, **no**
publish to the job stream. This makes FR-014 ("owners are sole writers") a real guarantee,
not a convention. The daemons use their own fuller-privilege creds.

## Testing strategy

- **Unit**: `control.rs` message round-trips; `PlanOptions{force}` suppresses `should_skip`;
  the positive `SkipReason` is emitted for compliant + each gate; `StateEntry.decision` is
  additive (old JSON without it still deserializes).
- **Integration** (NATS-backed, alongside `crash_safety.rs`): soft pause → no new claim, queue
  intact; cancel → ffmpeg child dies, **source checksum unchanged**, message `ack`'d not
  `nak`'d, disposition applied; a control op interleaved with a reconcile pass → no KV clobber
  (FR-015); an ignore-marked file is skipped and survives a KV wipe.
- **Gate**: the full spec 002 suite stays green (crash-safety unchanged).

## Phasing (maps to spec user stories)

1. **US1 Pause** (P1) — pause KV key + worker gate + coordinator pause consumer. Lowest risk,
   ships the most-wanted control first.
2. **US2 Cancel** (P2) — child-kill cancellation + disposition + ack/term mapping + progress.
3. **US3 State control & introspection** (P3) — the state ops, the engine `force` + positive
   `SkipReason`, decision persistence, the ignore marker + gate, the audit metric.

## Complexity Tracking

No constitution deviations to justify. The one genuinely invasive change — making the
worker's transcode interruptible — is done by **killing the child**, not by cancelling the
pinned future, so the existing "process() must not be dropped mid-run" safety invariant is
preserved rather than weakened.
