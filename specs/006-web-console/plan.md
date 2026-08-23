# Implementation Plan: apsis-web operator console

**Branch**: `006-web-console` | **Date**: 2026-08-24 | **Spec**: [spec.md](./spec.md)

## Summary

A **Leptos** full-stack operator console (`apsis-web`) — a read-mostly reactive UI over the
spec 005 NATS protocol. It **observes** per-file state + persisted decisions and **live
progress**, and **acts** by publishing the spec 005 control intents. All NATS access is
server-side (Leptos **server functions** + one SSE route); the browser holds no credentials
and can do nothing the `nats` CLI cannot. No new backend, no new source of truth — a stateless
projection of spec 005. Permitted by constitution **v3.0.0** (bounded operator console).

## Technical Context

**Language/Version**: Rust edition 2024, rust-version 1.87.
**Primary Dependencies**: `leptos` + `leptos_axum` + `leptos_meta`/`leptos_router` (full-stack:
SSR + hydration), `axum`, `tokio`, `async-nats` (reused), `serde`. Reuses `apsis-common`
(control message types, `KvStateStore`, `connect`, `StateEntry`/`Decision`). **No new backend
crate.**
**Storage**: none new — reads the spec 005 KV (`transcode_state`, incl. `decision`) + progress
subjects; the `__control__/pause` key. Writes nothing to the KV directly (only publishes
intents).
**Testing**: server functions unit/integration-tested against a **live `nats-server`** (gated,
like `crash_safety.rs`) — the NATS bridge is the risk surface. UI components get light smoke
coverage; the acceptance surface is "a button produces the same state change as the `nats`
command" (SC-003).
**Target Platform**: a Linux daemon behind the homelab reverse proxy (Zitadel OIDC),
mesh/LAN-only. `cargo-leptos` builds a wasm frontend (`wasm32-unknown-unknown`) + the native
SSR server binary.
**Project Type**: full-stack web (SSR server + wasm client) — the first non-daemon crate.
**Constraints**: reads ONLY the spec 005 NATS surface; mutates ONLY via the spec 005
control-intent subjects; no config editing; no visual graph/editor; Grafana keeps
aggregate/history. Removable without affecting control (CLI) or observability (Grafana).
**Scale/Scope**: one operator (few concurrent), a library of thousands of files; a table + a
detail view + action buttons. No multi-user collaboration, no historical dashboards.

## Constitution Check

*GATE — PASS under v3.0.0.* Principle V now permits a **bounded read-mostly operator console**;
this spec is that console. The gates that MUST hold (and are re-checked at review):
- **Reads only spec 005**: KV state/decision + progress subjects; no independent store (Obs.).
- **Writes only spec 005 intents**: server functions publish to the control subjects; the UI
  never touches KV terminal state or media (Principle V, Principle IV).
- **No privileged backdoor**: server functions do nothing the `nats` CLI cannot; NATS creds
  are server-side only.
- **No config editing** (Principle I: config stays git-only TOML); **no visual graph/editor**.
- **Grafana keeps aggregate/history** (Observability); the console is per-file live + control.
- The Leptos **hydration WASM is the app**, not the prohibited dynamic-ABI/WASM plugin host.

## Project Structure

### Documentation (this feature)

```text
specs/006-web-console/
├── plan.md              # this file
├── spec.md              # approved; amendment landed (v3.0.0)
├── data-model.md        # the view/server-fn types (Phase 1)
├── contracts/
│   └── server-surface.md   # server functions + the progress SSE route (Phase 1)
└── tasks.md             # Phase 2 (/tasks)
```

### Source Code

```text
crates/apsis-web/
├── Cargo.toml           # leptos features (ssr, hydrate) + [package.metadata.leptos]
├── src/
│   ├── main.rs          # [ssr] the axum + leptos_axum server; NATS client in AppState; auth mw
│   ├── lib.rs           # [hydrate] wasm entry (hydrate the app)
│   ├── app.rs           # the Leptos app: routes, shell, the FileTable + Detail + controls
│   ├── server/          # server functions (run only on the server; hold the NATS client)
│   │   ├── read.rs      # list_files / file_detail / pause_state  (KV reads)
│   │   └── control.rs   # pause / cancel / state_op  (publish spec 005 intents)
│   ├── progress.rs      # [ssr] an axum SSE route subscribing to apsis.progress.>
│   ├── auth.rs          # [ssr] extractor: trust the reverse-proxy user header
│   └── components/      # FileRow, StatusBadge, ProgressBar, DecisionCard, ActionButtons
│   └── view.rs          # shared view types (serde) crossing the server-fn boundary
└── style/               # pico.css (or minimal), embedded by cargo-leptos

nix/ (flake)             # dev shell gains cargo-leptos + wasm32-unknown-unknown; a package
                         # builds apsis-web via cargo-leptos (server bin + site assets)
```

## Design

### 1. The crate & full-stack model

`apsis-web` is a Leptos SSR app: the same component tree renders to HTML on the server and
hydrates to WASM in the browser. `cargo-leptos` compiles two artifacts — the `hydrate`-feature
wasm bundle and the `ssr`-feature server binary. `main.rs` (ssr) builds an axum router:
`leptos_axum` routes for the app + server functions, the progress SSE route, and the static
site assets; it connects to NATS once and stores the client + JetStream KV handle in the axum
`AppState` (and as a Leptos context for server functions).

### 2. Server functions — the NATS bridge (all server-side)

Reads (`server/read.rs`):
- `list_files() -> Vec<FileRow>` — KV **keys** under `transcode_state` (excluding the
  `__control__/pause` key), each `get` → `{path, status, decision, version, ignored}`. `ignored`
  from the on-disk marker check (server-side FS access).
- `file_detail(path) -> FileDetail` — the `StateEntry` + `decision` (+ the plan from the `Job`
  if in-flight, best-effort).
- `pause_state() -> PauseState`.

Controls (`server/control.rs`) — each publishes the exact spec 005 intent and returns the
outcome, so behaviour is identical to the `nats` CLI (SC-003):
- `pause(scope, mode, set)` → publish `apsis.control.pause`.
- `cancel(job_id, disposition)` → request/reply `apsis.control.cancel` → `CancelOutcome`.
- `state_op(path, op)` → request/reply `apsis.control.state` → `StateOutcome`.

### 3. Live data

- **Progress**: an axum **SSE route** (`/progress`) subscribes to `apsis.progress.>` and
  forwards each `ProgressEvent` as an SSE message. The client opens one `EventSource` and routes
  ticks into a per-job signal → the running row's progress bar updates live. (Core NATS, no
  new state.)
- **State**: `list_files` is a Leptos **resource** the UI refetches on an interval (~3 s) — a
  KV-watch → SSE stream of state changes is a later refinement, not needed for v1.

### 4. UI

One route (`/`): a `FileTable` of rows (path, `StatusBadge`, a decision tooltip/`DecisionCard`
on click, a `ProgressBar` for the running one) with context-dependent `ActionButtons` (cancel
for `InProgress`; requeue/force for `Done`; retry for `Failed`; mark-done; ignore-toggle) and a
global **pause/resume** control showing the effective `PauseState`. Actions call the control
server functions and reflect the returned outcome (incl. a failed/timeout request → a visible
error, per US3-3). pico.css (classless) for styling.

### 5. Auth

`auth.rs` is an axum extractor/middleware that reads the **reverse-proxy-forwarded user header**
(e.g. `X-Forwarded-User` / `X-Auth-Request-User` from the Zitadel oauth2-proxy) and attaches it
to the request; a control server function requires it (defense-in-depth; absent → 401). Primary
auth is the proxy; apsis-web is **mesh/LAN-only, never public**. No OIDC code in the binary.

### 6. Nix / build

The flake dev shell gains **`cargo-leptos`** + the **`wasm32-unknown-unknown`** target. A nix
package builds `apsis-web` via `cargo-leptos build --release` (server bin + hashed site assets);
the NixOS module runs the server bin behind the proxy with `NATS_URL` + `APSIS_WEB_ADDR`. This
is the main new-infra risk (cargo-leptos under crane/naersk is fiddly) — spike it early.

## Testing strategy

- **Server functions (the bridge)**: gated integration tests against a live `nats-server` —
  `list_files` reflects KV entries incl. the decision; `pause`/`cancel`/`state_op` publish the
  correct spec 005 intent and return the outcome the `nats` CLI would (SC-003); a control call
  to a down owner surfaces the timeout.
- **Auth**: unit-test the header extractor (present → user; absent → 401).
- **UI**: light smoke (the app renders SSR without panicking; components compile). Deep UI
  testing is out of scope — the value is in the server-fn correctness.
- **Gate**: `cargo fmt` + `cargo clippy` + `cargo test`; `cargo-leptos build` succeeds.

## Phasing (maps to spec user stories)

1. **Foundational** — the `apsis-web` crate skeleton (Leptos+axum SSR, NATS client in state,
   the auth extractor) + the flake `cargo-leptos`/wasm plumbing + a hello-world hydrated page.
   De-risks the build first.
2. **US1 Observe** (P1) — `list_files` + the `FileTable` + the progress SSE. Read-only, lowest
   risk, validates the whole read path.
3. **US2 Explain** (P2) — `file_detail` + the `DecisionCard` ("why skipped").
4. **US3 Act** (P3) — the control server functions + `ActionButtons` + pause control.
5. **Polish** — auth hardening, the nix package + NixOS module, `docs/apsis-operations.md`
   "Console" section.

## Complexity Tracking

The genuinely new complexity is the **Leptos/`cargo-leptos` build under nix** (a wasm target +
a two-artifact build) — the first non-daemon crate and the first frontend toolchain in the
repo. Everything else reuses `apsis-common` (NATS, control types, KV) and adds no new backend.
The constitutional relaxation (a WASM SPA + build pipeline) is the accepted cost recorded in the
v3.0.0 amendment; the invariants (reads/writes only via spec 005, no config editing) keep the
console bounded.
