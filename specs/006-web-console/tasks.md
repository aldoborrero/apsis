# Tasks: apsis-web operator console

**Spec**: [spec.md](./spec.md) · **Plan**: [plan.md](./plan.md) · **Contracts**:
[server-surface.md](./contracts/server-surface.md) · **Data model**: [data-model.md](./data-model.md)

Format: `[ID] [P?] [Story] Description`. `[P]` = parallelizable (different files, no dep).
**Gate every task against the v3.0.0 invariants**: reads only the spec 005 NATS surface;
writes only spec 005 control intents; no config editing; no visual graph/editor.

## Phase 1: Foundational (blocks all stories) — de-risk the build first

- [x] T001 `crates/apsis-web/` skeleton: `Cargo.toml` with `leptos` (`ssr`/`hydrate` features)
  + `leptos_axum` + `axum` + `[package.metadata.leptos]`; add to the workspace members. A
  hello-world Leptos app (`app.rs`) that SSR-renders and hydrates.
- [x] T002 Nix/build spike: flake dev shell gains `cargo-leptos` + `wasm32-unknown-unknown`;
  `cargo-leptos build` succeeds in the shell. (Highest-risk new infra — do it before UI work.)
- [x] T003 `main.rs` (ssr): axum + `leptos_axum` router; connect to NATS once and hold the
  client + JetStream KV handle in `AppState` + a Leptos context. Env: `NATS_URL`,
  `APSIS_WEB_ADDR`.
- [x] T004 [P] `auth.rs`: an axum extractor that reads the reverse-proxy user header
  (`X-Forwarded-User` / `X-Auth-Request-User`); present → user, absent → 401. Unit test both.
- [x] T005 [P] `view.rs`: the `FileRow` / `FileDetail` projection types (serde). Reuse
  `apsis_common` types for everything else.

## Phase 2: User Story 1 — See what's happening (Priority: P1) 🎯 MVP

**Goal**: the console lists every tracked file's state + streams live progress. Read-only.

- [x] T006 [US1] `server/read.rs`: `list_files()` — enumerate KV keys under `transcode_state`
  (exclude `__control__/pause`), `get` each → `FileRow` (+ `ignored` from the on-disk marker);
  `pause_state()`.
- [x] T007 [US1] `progress.rs`: axum SSE route `/progress` subscribing to `apsis.progress.>`,
  forwarding each `ProgressEvent` as SSE.
- [x] T008 [US1] `app.rs` + `components/`: `FileTable` (a `Resource` over `list_files`, ~3 s
  refetch), `StatusBadge`, `ProgressBar` fed by a `progress` signal off the `/progress`
  `EventSource`; the global pause indicator.
- [x] T009 [US1] Integration test (gated, live `nats-server`): `list_files` reflects seeded KV
  entries (status + decision + ignored); the SSE route forwards a published `ProgressEvent`.

## Phase 3: User Story 2 — Understand a decision (Priority: P2)

- [x] T010 [US2] `server/read.rs`: `file_detail(path)` — `StateEntry` + `decision` (+ last_error
  / used_fallback).
- [x] T011 [US2] `components/DecisionCard`: click a row → show *why* it was skipped (the
  persisted `Decision`), no re-probe. Test: a skipped file's decision renders.

## Phase 4: User Story 3 — Act on it (Priority: P3)

- [x] T012 [US3] `server/control.rs`: `pause` (publish), `cancel` (req/reply), `state_op`
  (req/reply) — each serializing the exact spec 005 payload and returning the owner's outcome.
  Require the auth header.
- [x] T013 [US3] `components/ActionButtons` + the pause control: context-dependent buttons
  (cancel for InProgress; requeue/force for Done; retry for Failed; mark-done; ignore-toggle);
  reflect the returned outcome; a down owner → visible retryable error (US3-3).
- [x] T014 [US3] Integration test (gated): each control button's server fn publishes the SAME
  spec 005 intent the `nats` CLI does and yields the same state change (SC-003); a control call
  to a down owner surfaces the timeout.

## Phase 5: Polish & cross-cutting

- [x] T015 [P] `nix/`: a package building `apsis-web` via `cargo-leptos build --release` (server
  bin + hashed site assets) + a NixOS module running it behind the proxy, mesh/LAN-only.
- [x] T016 [P] `docs/apsis-operations.md`: a "Console" section (URL, what it shows, that it is
  removable, the auth/ingress).
- [x] T017 Gate: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings &&
  cargo test --workspace`; `cargo-leptos build` succeeds; the console can be removed and control
  (CLI) + observability (Grafana) still work (SC-004).

## Dependencies

- Phase 1 blocks all; **T002 (the nix/cargo-leptos build) is the critical de-risk** — do it
  first. US1 (read path) is the independently-shippable MVP; US2/US3 build on it.
- Depends on **spec 005** (implemented). No new backend crate; reuses `apsis-common`.

## Notes

- **Constitution v3.0.0 gates** (review every change): no privileged backdoor (server fns do
  nothing the `nats` CLI can't), no config editing, no visual graph/editor, Grafana keeps
  aggregate/history. The console is removable (SC-004) — it is a projection, not core.
- Leptos version: pin to the current stable; expect `leptos` + `leptos_axum` + `leptos_meta` +
  `leptos_router` at matching versions.
