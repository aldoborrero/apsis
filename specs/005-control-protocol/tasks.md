# Tasks: Control & introspection protocol over NATS

**Spec**: [spec.md](./spec.md) · **Plan**: [plan.md](./plan.md) · **Contracts**:
[control-subjects.md](./contracts/control-subjects.md) · **Data model**: [data-model.md](./data-model.md)

Format: `[ID] [P?] [Story] Description`. `[P]` = parallelizable (different files, no dep).

## Phase 1: Foundational (blocks all stories)

- [ ] T001 [P] `apsis-common/src/control.rs` (NEW): subject constants + message schemas
  (`PauseIntent`, `CancelRequest`/`Reply`, `StateControlRequest`/`Reply`, `ProgressEvent`) and
  their enums per contracts/control-subjects.md. Unit-test serde round-trips.
- [ ] T002 [P] `apsis-common/src/schema.rs`: add `StateEntry.decision: Option<Decision>`
  (`#[serde(default)]`) + the permissive `Decision`/`DecisionKind` types (NOT embedding
  `PlanReason`/`FilePlan`). Test: old JSON without `decision` still deserializes.
- [ ] T003 [P] `apsis-common`: the on-disk ignore-marker helpers (`has_/set_/clear_ignore_marker`)
  + the `PauseState` type and the `__control__/pause` KV key constant.

## Phase 2: User Story 1 — Pause & resume (Priority: P1) 🎯 MVP

**Goal**: pause (soft/hard, global/per-worker) stops new claims and survives restart; resume
is immediate. **Independent test**: publish pause intent, confirm no new claim (read KV),
clear, confirm resume.

- [ ] T004 [US1] `apsis-coordinator/src/control.rs` (NEW): subscribe `apsis.control.pause`;
  fold each `PauseIntent` into `PauseState` and write the `__control__/pause` KV key (sole
  writer). Wire into `main.rs`.
- [ ] T005 [US1] `apsis-worker`: watch `__control__/pause`; **before each `claim()`** consult
  the effective state for `global`/this `worker_id`; soft-paused → don't claim. Re-read on
  reconnect (not solely via the watch stream) — FR-001.
- [ ] T006 [US1] Integration test (NATS-backed, alongside `crash_safety.rs`): soft pause →
  worker claims nothing, queue intact; clear → claiming resumes; pause survives a worker
  restart. Hard-pause path deferred to US2 (needs cancel).

## Phase 3: User Story 2 — Cancel the active transcode (Priority: P2)

**Goal**: abort the running transcode, source byte-identical, disposition defer|ignore.
**Independent test**: `nats req` cancel → child killed, checksum unchanged, message ack'd,
file in the chosen state.

- [ ] T007 [US2] `apsis-worker`: expose a **child handle / kill signal** for the in-flight
  ffmpeg so it can be killed without dropping the pinned `process()` future (preserve the
  existing "must not drop mid-run" invariant).
- [ ] T008 [US2] `apsis-worker/src/control.rs` (NEW): subscribe `apsis.control.cancel`; if this
  worker runs `job_id`, kill the child, let the normal error path discard the temp, apply the
  disposition (defer → clear KV; ignore → write marker), and **`ack`/`term` the message (never
  `nak`)**. Reply `cancelled`/`not_running`/`already_done` (already-done if past
  `atomic_replace`).
- [ ] T009 [US2] `apsis-worker`: wire **hard pause** (FR-002) to trigger a defer-cancel of the
  in-flight job.
- [ ] T010 [US2] `apsis-worker`: publish `ProgressEvent {speed,eta,out_time}` to
  `apsis.progress.<job_id>` from the ffmpeg `-progress` parser (no percent).
- [ ] T011 [US2] Integration test: cancel a running transcode → **source checksum unchanged**,
  temp gone, message `ack`'d not `nak`'d; defer → re-planned next reconcile; ignore → marker
  written; cancel of a not-running job → `not_running`.

## Phase 4: User Story 3 — State control & introspection (Priority: P3)

**Goal**: requeue/retry/mark-done/force + persist the positive skip decision + honor the
ignore marker. **Independent test**: each op transitions state correctly; a skipped file's
decision is readable from `nats kv get`.

- [ ] T012 [US3] `apsis-engine/src/plan.rs`: add `PlanOptions { force }` threaded into
  `plan(...)` (existing callers pass `Default`); `force` suppresses `should_skip`. Emit a
  positive `SkipReason` on the `FilePlan` where `should_skip` is decided. Unit tests:
  force plans an encode for a compliant file; each gate yields its `SkipReason`.
- [ ] T013 [US3] `apsis-coordinator/src/reconcile.rs`: persist `SkipReason → StateEntry.decision`
  when marking `Done`/skip; honor the on-disk ignore marker in the gate (alongside
  `is_handled`); thread `force` from a control request into the `PlanOptions` path.
- [ ] T014 [US3] `apsis-coordinator/src/control.rs`: request/reply consumer for
  `apsis.control.state` (requeue/retry/mark_done/force), **executed serialized with the
  reconcile loop** (command channel drained per pass, or a KV-write mutex) — FR-015. `mark_done`
  computes `mtime:size` if unprobed.
- [ ] T015 [US3] Audit: every control op logs (op, target, timestamp) + increments
  `apsis_control_ops_total{op}` (+ `describe_counter!`). FR-017.
- [ ] T016 [US3] Integration test: control op interleaved with a reconcile pass → **no KV
  clobber** (FR-015); an ignore-marked file is skipped and survives a KV wipe; a skipped file's
  `decision` is readable via `nats kv`.

## Phase 5: Polish & cross-cutting

- [ ] T017 [P] `contracts/control-subjects.md` → document the **NATS subject-permission ACL**
  for operator vs daemon credentials (FR-014); note where the deployment enforces it.
- [ ] T018 [P] `docs/apsis-operations.md`: a "Control plane" section — the `nats` CLI recipes
  for each op (pause/cancel/requeue/force/retry/mark-done), the progress subject, and the
  ignore marker. This is the acceptance surface (SC-003).
- [ ] T019 Gate: `nix develop --command bash -c 'cargo fmt && cargo clippy --workspace
  --all-targets -- -D warnings && cargo test --workspace'`; the full spec 002 crash-safety
  suite stays green (SC-005).

## Dependencies

- Phase 1 blocks all. US1 (P1) is independently shippable (the MVP). US2 depends on the worker
  child-kill (T007) and enables hard-pause (T009). US3 depends on the engine change (T012).
- `force` (T012/T013) is the only engine/plan change; the rest is daemon + common wiring.

## Notes

- **Constitution-clean**: no amendment. No new dependency (`async-nats` covers pub/req/KV).
- Recoverability: pause + progress are ephemeral; the decision is a rebuildable projection;
  *ignore* is the recoverable on-disk marker — never a live-only KV status (FR-016).
