# Tasks: Control & introspection protocol over NATS

**Spec**: [spec.md](./spec.md) · **Plan**: [plan.md](./plan.md) · **Contracts**:
[control-subjects.md](./contracts/control-subjects.md) · **Data model**: [data-model.md](./data-model.md)

Format: `[ID] [P?] [Story] Description`. `[P]` = parallelizable (different files, no dep).

## Phase 1: Foundational (blocks all stories)

- [x] T001 [P] `apsis-common/src/control.rs`: subject constants + message schemas
  (`PauseIntent`/`PauseState`, `CancelRequest`/`Reply`, `StateControlRequest`/`Reply`,
  `ProgressEvent`). Serde round-trip + pause wire-shape tests.
- [x] T002 [P] `apsis-common/src/schema.rs`: `StateEntry.decision: Option<Decision>`
  (`#[serde(default)]`) + permissive `Decision`/`DecisionKind` (not embedding
  `PlanReason`/`FilePlan`).
- [x] T003 [P] `apsis-common`: on-disk ignore-marker helpers (`fsutil`) + `PauseState`
  (`apply`/`effective`) + `KV_CONTROL_PAUSE`; `KvStateStore` gains `get_pause`/`put_pause`.

## Phase 2: User Story 1 — Pause & resume (Priority: P1) 🎯 MVP

**Goal**: pause (soft/hard, global/per-worker) stops new claims and survives restart; resume
is immediate. **Independent test**: publish pause intent, confirm no new claim (read KV),
clear, confirm resume.

- [x] T004 [US1] `apsis-coordinator/src/main.rs`: `consume_pause_control` subscribes to
  `apsis.control.pause`, folds each intent into the persisted `PauseState` (sole writer),
  emits `apsis_control_ops_total{op=pause}`.
- [x] T005 [US1] `apsis-worker`: pause gate before each claim (`is_paused` reads the key each
  cycle — reconnect re-read, fails open on error); soft-paused → don't pull. `APSIS_WORKER_ID`.
- [x] T006 [US1] Integration test `paused_worker_claims_nothing_then_resumes` (NATS-backed,
  gated): soft pause set before the worker starts → the job is never claimed (KV stays
  empty) for ≥3 gate cycles; clearing the pause → the worker resumes and drives it to `Done`.
  **Verified green against a live `nats-server -js`** (9.2s). Hard-pause abort is US2.

## Phase 3: User Story 2 — Cancel the active transcode (Priority: P2)

**Goal**: abort the running transcode, source byte-identical, disposition defer|ignore.
**Independent test**: `nats req` cancel → child killed, checksum unchanged, message ack'd,
file in the chosen state.

- [x] T007 [US2] `run.rs`/`fallback.rs`: `run`/`wait_with_stall` take a `&Notify`; the watch
  loop selects on it and **kills the ffmpeg child** (not dropping the pinned future) →
  `RunOutcome.cancelled`; `transcode` does not fall back on a cancel. Test:
  `cancel_kills_a_running_process`.
- [x] T008 [US2] `worker.rs`: `serve_cancel` subscribes to `apsis.control.cancel` (spawned in
  `main`), targets the `running` registry, replies `Cancelled`/`NotRunning`; `process` applies
  the disposition (defer → `kv.delete`; ignore → on-disk marker) and drains (ack, never nak),
  no `Failed`/result. `KvStateStore::delete`. `apsis_control_ops_total{op=cancel}`.
- [x] T009 [US2] `worker.rs`: the heartbeat loop checks `effective_pause()` each tick and on
  `Hard` fires `cancel_running()` (defer) — a hard pause aborts the in-flight transcode (FR-002).
- [x] T010 [US2] `run.rs`/`worker.rs`: `wait_with_stall` parses the ffmpeg `-progress` blocks
  (`speed`/`out_time_us`) and emits `ProgressTick`s over a channel; `process` publishes each as a
  `ProgressEvent` to `apsis.progress.<job_id>` (ephemeral, no percent). Test:
  `progress_lines_emit_a_tick_at_the_boundary`.
- [x] T011 [US2] Integration test `cancel_active_transcode_leaves_source_intact` (gated,
  NATS-backed): cancel via request/reply → reply `Cancelled`, **source byte-identical**, defer
  clears the KV, no redelivery. **Verified green against a live `nats-server -js`.**
  (`already_done`/`not_running` reply paths covered by the serve_cancel logic; the
  ignore-marker gate is honored in T013.)

## Phase 4: User Story 3 — State control & introspection (Priority: P3)

**Goal**: requeue/retry/mark-done/force + persist the positive skip decision + honor the
ignore marker. **Independent test**: each op transitions state correctly; a skipped file's
decision is readable from `nats kv get`.

- [x] T012 [US3] `plan.rs`: `PlanOptions { force }` (plan delegates); force suppresses
  `should_skip` + turns a compliant Copy into an Encode. Positive `SkipReason` on the FilePlan.
  Tests: `force_transcodes_a_compliant_file`, `skip_reason_reports_why`.
- [x] T013 [US3] `reconcile.rs`: persist `SkipReason → StateEntry.decision` on Done/skip; honor
  the on-disk ignore marker first (survives a KV wipe); `reconcile_file_opts(force)` bypasses the
  change-gate + plans with `PlanOptions{force}`.
- [x] T014 [US3] `main.rs`/`reconcile.rs`: `serve_state_control` (request/reply) + `apply_state_op`
  (requeue/retry/mark_done/force), run under a shared `recon_lock` serialized with the reconcile
  pass (FR-015). `mark_done` computes `mtime:size`. `StateStore::delete` added.
- [x] T015 [US3] Audit: `apsis_control_ops_total{op}` for pause/cancel/every state op; each logged.
- [x] T016 [US3] Unit tests `state_op_mark_done_then_requeue`, `state_op_force_enqueues_a_compliant_file`
  (apply_state_op logic vs FakeStore/Publisher — the NATS request/reply wiring is thin, of the
  serve_cancel shape already covered by an integration test). Serialization is FR-015 by the
  recon_lock; ignore-marker gate is in reconcile_file_opts.

## Phase 5: Polish & cross-cutting

- [x] T017 [P] `contracts/control-subjects.md` documents the NATS subject-permission ACL
  (operators: publish `apsis.control.*`, subscribe `apsis.progress.*`, read KV; no KV write) — FR-014.
- [x] T018 [P] `docs/apsis-operations.md` gained a "Control plane" section — `nats` CLI recipes
  for pause/cancel/requeue/force/retry/mark-done, the progress subject, the ignore marker, and
  the ACL (SC-003).
- [x] T019 Gate green: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings &&
  cargo test --workspace`; the spec 002 crash-safety suite stays green (pause/cancel added
  alongside it, all pass against a live nats-server).

## Dependencies

- Phase 1 blocks all. US1 (P1) is independently shippable (the MVP). US2 depends on the worker
  child-kill (T007) and enables hard-pause (T009). US3 depends on the engine change (T012).
- `force` (T012/T013) is the only engine/plan change; the rest is daemon + common wiring.

## Notes

- **Constitution-clean**: no amendment. No new dependency (`async-nats` covers pub/req/KV).
- Recoverability: pause + progress are ephemeral; the decision is a rebuildable projection;
  *ignore* is the recoverable on-disk marker — never a live-only KV status (FR-016).
