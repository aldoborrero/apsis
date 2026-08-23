# Feature Specification: apsis-web operator console

**Feature Branch**: `006-web-console`
**Created**: 2026-08-23
**Status**: Draft — **BLOCKED on a constitution amendment (see governance gate)**
**Input**: A thin web console that lets an operator *see* per-file state and live progress,
*understand* why a file was skipped/transcoded, and *act* (pause, cancel the active
transcode, re-queue/force/retry/mark) — as a browser skin over the spec 005 NATS control &
introspection protocol.

## Constitution alignment *(read first — governance gate)* — REQUIRES AMENDMENT

This is the artifact the constitution deliberately excludes. **Principle V** states apsis
ships *"no bespoke UI"* and pushes observability to *"Prometheus … viewed in Grafana"*;
**design decision D5** is *"No web UI — observability via Grafana + VictoriaMetrics + logs"*.
A web console **cannot be built until those are amended.**

**Required governance action (a prerequisite task, not optional):** amend the constitution
(bump the version, update the amendment log) and reflect it in `docs/design/rust-scheduler.md`
D5, from *"no bespoke UI"* to the carve-out below. This is a **larger** widening than a thin
skin: the console is a **Leptos full-stack reactive app** (SSR + WASM hydration + a
`cargo-leptos` build pipeline), so the amendment deliberately relaxes "thin" for the operator
console specifically — accepting a compiled WASM frontend and a build step in exchange for a
real interactive UI.

> A **reactive operator console** is permitted, built with Leptos (full-stack Rust: SSR +
> client-side hydration). It reads only the spec 005 NATS surface (KV state/decision + the
> progress subjects) and mutates state ONLY by publishing to the **defined control-intent
> surface** (the spec 005 control subjects) — no privileged backdoor; its server functions may
> do nothing the `nats` CLI cannot. It MUST NOT edit config (config stays git-only TOML), MUST
> NOT contain a visual pipeline graph/editor, and does NOT replace Grafana for aggregate/
> historical observability. The console's own compiled WASM frontend (Leptos hydration) is
> **not** the prohibited runtime **plugin host** — a dynamic-ABI/WASM host that loads untrusted
> plugins stays prohibited. Still prohibited: config editing in the UI, a visual graph/editor,
> a dynamic-ABI/WASM plugin host.

If the amendment is rejected, this spec is closed and the control protocol (spec 005)
remains fully usable via the `nats` CLI + Grafana. **Do not implement 006 before the
amendment lands.**

## User Scenarios & Testing *(mandatory)*

The console is one page (or a few) served by a new `apsis-web` daemon that is *only* a NATS
client + web server. All reads come from the KV/subjects of spec 005; all writes are spec
005 intents.

### User Story 1 - See what's happening, per file (Priority: P1)

**Why this priority**: the "live per-file state" gap Grafana can't fill; pure read, lowest
risk, and validates the whole read path before any control button exists.

**Independent Test**: with jobs flowing, the console lists every tracked file with its
current state and streams live progress for the running one, all sourced from the KV +
progress subject — no new backend beyond spec 005.

**Acceptance Scenarios**:

1. **Given** files in various states, **When** the operator opens the console, **Then** it
   lists each with its status (Done / Pending / InProgress / Failed) read from the KV, plus
   an "ignored" indicator for any file carrying the on-disk ignore marker (spec 005 FR-016).
2. **Given** a running transcode, **When** the operator watches, **Then** its progress
   updates live (a Leptos signal fed by a server function streaming the spec 005 progress
   subject) without a refresh.

### User Story 2 - Understand a decision (Priority: P2)

**Why this priority**: the strongest reason a UI beats Grafana — "why did apsis skip/encode
this?" — surfacing the decision the coordinator already persisted (spec 005 FR-011).

**Independent Test**: click a skipped file; the console shows its decision (the positive
skip reason — which gate/compliant codec applied) read from the KV entry; no re-probe.

**Acceptance Scenarios**:

1. **Given** a skipped file, **When** the operator opens it, **Then** the console shows
   *why* (compliant codec / skip-gate / rule) from the persisted decision.
2. **Given** a file that was transcoded, **When** opened, **Then** it shows the plan that
   ran (from the `Job`).

### User Story 3 - Act on it (Priority: P3)

**Why this priority**: the interactive control the operator wanted; each button is a spec
005 intent, so the console adds *no* new authority — it is exactly as powerful as the CLI.

**Independent Test**: each button (pause/resume, cancel-active with defer|ignore, re-queue,
force, retry, mark-done) publishes the corresponding spec 005 intent and reflects the
resulting state change; behaviour is identical to issuing it from `nats`.

**Acceptance Scenarios**:

1. **Given** the console, **When** the operator clicks Pause, **Then** it publishes the spec
   005 pause intent (a control subject) and the UI shows the paused state (surviving restart).
2. **Given** a running transcode, **When** the operator clicks Cancel and picks a
   disposition, **Then** it sends the spec 005 cancel request and shows the outcome.
3. **Given** any control action, **When** the owner is down, **Then** the console surfaces
   the failed request (timeout) and the operator can retry — no silent success.

### Edge Cases

- The console **loses its NATS connection** → it shows stale-with-warning, never fabricates
  state; reconnect repopulates from the KV.
- **Config is never editable** here — there is no form that writes `scheduler.toml`.
- An action the operator lacks permission for is refused at the **auth** layer before any
  intent is published.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: `apsis-web` MUST be a **separate daemon** that is only a NATS client + web
  server. It MUST hold **no privileged access** beyond publishing spec 005 intents and
  reading spec 005 KV/subjects — it can do nothing the `nats` CLI cannot.
- **FR-002**: All reads (state, decision, progress) MUST come from **spec 005** (KV watch +
  progress subject); the console maintains **no independent source of truth** and adds **no
  new backend** — a missing capability is added to spec 005, not smuggled here.
- **FR-003**: All writes MUST be **spec 005 intents**; the console MUST NOT touch the KV
  terminal state or media directly.
- **FR-004**: The frontend is **Leptos** (full-stack Rust: SSR + client-side hydration).
  Reactive per-file state and live progress use Leptos **signals**; server-side reads/writes
  use **server functions** (transparently called from the client, running only on the server —
  so all NATS access is server-side). Built with **`cargo-leptos`** (a wasm frontend target +
  the native server binary). This is served by/integrated with **axum** (`leptos_axum`), the
  same tokio runtime as `async-nats`.
- **FR-005**: The console MUST sit behind the homelab **SSO (Zitadel OIDC)** via the reverse
  proxy and be **mesh/LAN-only, never public** — control actions are mutations.
- **FR-006**: The console MUST NOT provide **config editing** or a **visual pipeline
  graph/editor** (constitution).
- **FR-007**: Aggregate/historical/observability (savings over time, failure rates,
  latencies) MUST remain in **Grafana**; the console is for per-file live state, decisions,
  and control — it does not duplicate Grafana.

### Key Entities

- **apsis-web daemon** — a Leptos SSR server (on axum) + NATS client; a stateless projection of
  spec 005 (its server functions read the KV/subjects and publish control intents).
- **File row** — a rendered view of a KV `StateEntry` + its decision + (if running) live
  progress.
- **Action** — a button mapped 1:1 to a spec 005 intent.

## Success Criteria *(mandatory)*

- **SC-001**: The console renders every tracked file's current state and streams live
  progress for the running one, sourced entirely from spec 005 — no new backend.
- **SC-002**: Opening a skipped file shows *why* (its persisted decision) with no re-probe.
- **SC-003**: Every control button produces **exactly** the state change the equivalent
  `nats` command produces (verified by driving the same op both ways).
- **SC-004**: The console can be **removed** and the deployment keeps working — control via
  CLI, observability via Grafana are unaffected (proves it is an optional layer, not core).
- **SC-005**: The constitution amendment is landed and reflected in D5 **before** any
  console code is written.

## Assumptions

- **Depends on spec 005** (control & introspection protocol) being implemented first.
- **Blocked on the constitution amendment** above — this is a hard prerequisite task.
- The homelab Zitadel + reverse proxy are the auth/ingress substrate (as for other homelab
  services).
- **Stack (decided): Leptos** (full-stack Rust, SSR + hydration) on **axum** via `leptos_axum`,
  built with **`cargo-leptos`** (adds a wasm frontend target — the dev shell/flake gains
  `cargo-leptos` + the `wasm32-unknown-unknown` target and the build-graph plumbing). Leptos
  is the current leading Rust web framework (signals, server functions, SSR; outperforms
  React). This deliberately accepts a WASM SPA + build pipeline for the console — a larger
  relaxation of "thin" than an htmx skin, chosen for a real reactive UI.
- Single page or a small set; no multi-user collaboration, no historical dashboards (those
  are Grafana).
