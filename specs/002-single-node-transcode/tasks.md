# Tasks: Single-node library reconcile & safe transcode

**Spec**: [spec.md](./spec.md) · **Plan**: [plan.md](./plan.md) · **Data model**:
[data-model.md](./data-model.md) · **Contract**: [contracts/nats-protocol.md](./contracts/nats-protocol.md)
· **Research**: [research.md](./research.md)

## Format: `[ID] [P?] [Story] Description`

- **[P]**: parallelizable (different files, no dependency on an incomplete task).
- **[US1/US2/US3/US4]**: the user story a task serves.
- Crates already exist as stubs from spec 001: `apsis-common` (lib),
  `apsis-coordinator` (bin), `apsis-worker` (bin). `apsis-engine` is done and consumed.
- Tests are included only where they pin a **safety** property (never-corrupt, verify,
  crash-redeliver) — matching the spec's core; not blanket TDD.

---

## Phase 1: Setup (shared infrastructure)

- [X] T001 Add dependencies to the three crates' `Cargo.toml`: `apsis-common` (`serde`,
  `serde_json`, `toml`, `figment`, `garde`, `schemars`, `async-nats` 0.50, `tokio`, `ulid`,
  `thiserror`, `time`), `apsis-coordinator` + `apsis-worker` (`tokio` full, `tracing`,
  `tracing-subscriber`, `metrics`, `metrics-exporter-prometheus`, `notify` for coordinator,
  `apsis-common`, `apsis-engine`). Keep workspace lints (`clippy::all` deny, pedantic warn).
- [X] T002 [P] Add `nats-server` (JetStream) to `nix/devshell.nix` so integration tests and
  local runs have a broker; document the gate env var (`APSIS_TEST_NATS`) in the shell.
- [X] T003 [P] Wire `tracing-subscriber` init helper in `crates/apsis-common/src/obs.rs`
  (`init_tracing`, `RUST_LOG`-driven, no ANSI), called by both binaries. The
  `metrics-exporter-prometheus` init already lives per-binary (`install_metrics`).

---

## Phase 2: Foundational (blocking prerequisites) — `apsis-common`

Everything below blocks all user stories.

- [X] T004 [P] Define wire schemas in `crates/apsis-common/src/schema.rs`: `Job`,
  `StateEntry` (+ `Status` enum), `TranscodeResult` (+ `Outcome`), with serde + `schemars`;
  `Job.plan` is `apsis_engine::FilePlan`. Match [data-model.md](./data-model.md) exactly.
- [X] T005 [P] Define config in `crates/apsis-common/src/config.rs`: `SchedulerConfig`
  (libraries + `[profiles]` = `apsis_engine::Profile` + `[reconcile]`), `WorkerConfig`
  (concurrency, `path_map`, ffmpeg/ffprobe paths, `[verify]`, `[[backend]]`), loaded via
  `figment` (file+env) with `garde` validation; **fail-fast** on invalid (FR-012).
- [X] T006 [P] Define JetStream constants + idempotent provisioning in
  `crates/apsis-common/src/nats.rs`: stream `APSIS_JOBS` (WorkQueue), subjects, KV bucket
  `transcode_state`, consumer `worker-local` config (`ack_wait`, `max_deliver`, `backoff`,
  `max_ack_pending`); `ensure_topology(&Context)` create-if-absent (contract §invariant 4).
- [X] T007 Define the `Queue` + `StateStore` traits in
  `crates/apsis-common/src/store.rs` (publish job / pull+ack / nak / term; KV get / CAS-put),
  with a NATS-backed impl **and** an in-memory fake for unit tests (needs T004, T006).
- [X] T008 [P] `path_map` translation (coordinator path → local mount, longest-prefix) in
  `crates/apsis-common/src/pathmap.rs` — identity on rhea; real on sirius (spec 003).
- [X] T009 [P] `version_token(path) -> "mtime:size"` + `is_video(path)` helpers in
  `crates/apsis-common/src/fsutil.rs`; unit-tested.
- [X] T010 Re-export the public surface from `crates/apsis-common/src/lib.rs`; `cargo build`
  the three crates green (needs T004–T009).

**Checkpoint**: shared types compile; JetStream topology provisions against a dev server.

---

## Phase 3: User Story 1 — Safe transcode & replace (P1) 🎯 MVP

**Goal**: given a job for a non-compliant file, transcode it and replace the original **only
after** verification; VAAPI with CPU fallback. **Independent test**: publish a `Job` for a
sample clip → the file is compliant + intact and the original is atomically replaced.

### Implementation — `apsis-worker`

- [X] T011 [US1] Worker skeleton in `crates/apsis-worker/src/main.rs`: load `WorkerConfig`,
  connect NATS, `ensure_topology`, bind the `worker-local` pull consumer, `Semaphore` bound
  to `concurrency` (FR-010).
- [X] T012 [US1] `run.rs`: build the ffmpeg command from `Job.plan` via
  `apsis_engine::VaapiBackend`, run through `tokio::process` to a temp
  `.apsis-tmp-<ulid>` in the **source directory**, parse `-progress pipe:1` into progress
  (FR-007 temp-on-same-fs).
- [X] T013 [US1] `fallback.rs`: on VAAPI non-zero exit, retry once with
  `apsis_engine::CpuBackend`; record `used_fallback` (FR-006).
- [X] T014 [US1] `verify.rs`: ffprobe the temp — expected streams present, duration within
  tolerance, tail-packet not-truncated, size sane (research D3); returns a typed verdict.
- [X] T015 [US1] `replace.rs`: on verify pass, `fsync` temp + dir, atomic `rename`
  (handle extension change: new path then unlink old original), restore `stat(2)`
  (owner/mode/mtime); on any failure/crash, unlink temp — original untouched (FR-007/008).
- [X] T016 [US1] Wire the pull loop: pull → `InProgress` KV CAS → run → fallback → verify →
  replace → `ack` + publish `TranscodeResult`; verify/terminal fail → discard temp +
  `nak`/`term` (contract §delivery).

### Tests (safety-critical)

- [X] T017 [P] [US1] `crates/apsis-worker/tests/replace.rs`: verify-pass → original replaced,
  `stat` preserved; verify-**fail** → original byte-identical, temp gone (SC-001 safety half).
- [X] T018 [P] [US1] `crates/apsis-worker/tests/fallback.rs`: a forced VAAPI failure falls
  back to CPU and still produces a valid, verified output (spec AS-3). Uses a `testsrc` clip.

**Checkpoint**: publishing a job transcodes + safely replaces one file; MVP of the worker.

---

## Phase 4: User Story 2 — Idempotent reconcile (P1) — `apsis-coordinator`

**Goal**: enqueue only drift; unchanged files are neither re-probed nor re-queued.
**Independent test**: reconcile a compliant library twice → 0 jobs; a mixed library →
only non-compliant files queued.

- [X] T019 [US2] Coordinator skeleton in `crates/apsis-coordinator/src/main.rs`: load
  `SchedulerConfig`, connect NATS, `ensure_topology`, run the reconcile loop on
  `scan_interval`.
- [X] T020 [P] [US2] `profile_match.rs`: longest-matching `Library.path` → profile (FR-003);
  unit-tested with nested/overlapping libraries.
- [X] T021 [P] [US2] `discover.rs`: `notify` inotify watch ⊎ periodic walk, `is_video`
  filter, **debounce** on unstable `mtime:size` (FR-001/002); both feed one reconcile body.
- [X] T022 [US2] `reconcile.rs`: the one-pass algorithm (research/design §7) — version-cache
  gate (skip unchanged, no probe) → `apsis_engine::plan` → `should_skip` ? KV `Done` : KV
  `Pending` CAS + publish `Job` (FR-004/005). Needs T020, T021, and common T006/T007.
- [X] T023 [US2] Result-consumer task in the coordinator: subscribes to the core `jobs.result`
  subject and folds each `TranscodeResult` into `apsis_results_total{outcome}` + a structured
  completion log — history without a separate DB. NB the design converged on the **worker**
  owning the terminal KV write (crash-safe: it records `Failed@version` itself on dead-letter),
  so this consumer is the observability half, not a second KV writer (contract updated).

### Tests

- [X] T024 [P] [US2] `crates/apsis-coordinator/tests/idempotent.rs` (in-memory fakes): a
  compliant library → 0 jobs; running twice with no change → 0 both times; an unchanged file
  is **not** re-probed (assert the engine/probe is not called) (SC-002, FR-005).

**Checkpoint**: US1 + US2 — the full reconcile→transcode→replace loop works on one host.

---

## Phase 5: User Story 3 — Crash-safe (P2)

**Goal**: an interrupted transcode never corrupts the original; the file is retried; a poison
file is capped then suppressed. **Independent test**: kill the worker mid-transcode → original
intact + redelivered; an unencodable file → `Failed` after `max_deliver`.

- [X] T025 [US3] Long-transcode lease: heartbeat `AckWait` (`working`/in-progress) so a
  legitimately long encode isn't redelivered mid-run (contract §delivery).
- [X] T026 [US3] Dead-letter handling: on `MaxDeliver`/`term`, write `Failed@version` +
  `last_error` to KV and suppress re-queue until `mtime:size` changes (FR-009).
- [X] T027 [US3] Startup sweep: discard orphan `.apsis-tmp-*` files from a prior crash before
  reconciling (FR-008 partial-output cleanup).

### Tests (gated on a real `nats-server`, `APSIS_TEST_NATS`)

- [X] T028 [P] [US3] `tests/crash_safety.rs::crash_mid_transcode_redelivers_and_completes`:
  SIGKILL the built worker mid-transcode → after the (config-shortened) lease the job is
  redelivered, a fresh worker re-claims the abandoned `InProgress` and completes; the original
  is byte-identical across the crash (SC-003). Gated on `APSIS_TEST_NATS` + ffmpeg.
- [X] T029 [P] [US3] `tests/crash_safety.rs::poison_job_dead_letters_after_max_deliver`: a
  retriable-forever job (bogus ffmpeg → spawn error) is redelivered exactly `max_deliver`
  times then lands `Failed@version` (SC-004); asserted from the worker's redelivery log. The
  `mtime:size`-change re-queue is the coordinator's version gate (covered by the reconcile
  unit tests). Gated on `APSIS_TEST_NATS`.

**Checkpoint**: all three P1/P2 stories independently functional and safe.

---

## Phase 6: User Story 4 — Observable without a UI (P3)

**Goal**: Prometheus metrics + structured logs; no web UI.
**Independent test**: scrape the endpoint → documented series present and current.

- [X] T030 [US4] Emit the metric set (research D7) from coordinator + worker:
  `apsis_queue_depth`, `apsis_jobs_in_flight`, `apsis_jobs_total{outcome}`,
  `apsis_transcode_seconds`, `apsis_bytes_saved_total`, `apsis_used_fallback_total`,
  `apsis_verify_failures_total`, `apsis_reconcile_seconds`.
- [X] T031 [P] [US4] `tracing` spans per job/file: the worker wraps `process` in a `job` span
  (job_id, path, version, backend, outcome — the last two recorded as they're decided); the
  coordinator wraps `reconcile_file` in a `reconcile` span (path, version, outcome). Operational
  `eprintln!`s became structured `info!`/`warn!`/`error!` events (`Backend::name()` supplies the
  backend field).
- [X] T032 [P] [US4] Expose the Prometheus endpoint (both binaries) and add a Grafana
  dashboard JSON under `docs/` (or the homelab monitoring path) for the series (SC-005).

---

## Phase 7: Polish & cross-cutting

- [X] T033 [P] Config fail-fast end-to-end: an invalid `scheduler.toml`/`worker.toml` makes
  the binary refuse to start with a clear error; add a rejects-bad-config test (FR-012).
- [X] T034 [P] `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt` clean
  in the nix devshell; no orchestration deps leak into `apsis-engine`.
- [X] T035 [P] Shared sample-clip helpers (`ffmpeg_available`, `sample_h264`) in
  `apsis-common`'s `testkit` feature (off in release builds), used by the worker's unit
  (`run`/`fallback`/`worker`) and integration (`crash_safety`) tests instead of a hand-rolled
  `ffmpeg -f lavfi` in each.
- [X] T036 [P] Update `docs/` (CHANGELOG + a short `docs/services/apsis.md` runbook) and
  validate the `quickstart.md` commands against the built binaries.

---

## Dependencies & Execution Order

- **Setup (P1)** → **Foundational (P2)** → **US1 / US2 (P1 stories)** → **US3 (P2)** →
  **US4 (P3)** → **Polish**.
- US1 and US2 are independent once Foundational lands (US1 = worker, tested by publishing a
  job; US2 = coordinator, tested with in-memory fakes). Do US1 first for the MVP.
- US3 depends on both (needs a running consume/replace loop + reconcile). US4 cross-cuts.
- T010 gates all stories. T016 needs T012–T015. T022 needs T020–T021 + T006/T007.

### Parallel opportunities

- Foundational: **T004, T005, T006, T008, T009** (different files).
- US1: **T017, T018** tests alongside; T012/T013/T014/T015 are sequential in the pipeline but
  land in separate files.
- US2: **T020, T021** parallel; **T024** alongside.
- US3 tests **T028, T029** parallel. Polish **T033–T036** all parallel.

### Suggested MVP scope

Phases 1–4 (Setup + Foundational + **US1 + US2**) deliver the **safe single-host reconcile→
transcode→replace loop** — the independently-valuable core. US3 hardens crash-safety; US4
adds observability.
