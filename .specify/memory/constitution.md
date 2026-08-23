# apsis Constitution

apsis is a thin, distributed media-transcode scheduler in Rust. This constitution
governs its **design and implementation decisions**. The full design rationale lives
in `docs/design/rust-scheduler.md`; this file is the normative summary.

## Core Principles

### I. Declarative & git-recoverable

The whole **desired state** lives in versioned config (`scheduler.toml`,
`worker.toml`) deployed via nix. All **runtime state** (NATS streams, KV, queues) is a
*cache* of a computation over git-config + the media library, and MUST be fully
re-derivable by a rescan. There MUST be **no live-only state**: a from-scratch rebuild
with empty runtime state MUST converge to the same result on the next reconcile.

### II. Thin by reuse

apsis writes **orchestration glue and nothing else**. Every hard part — durable queue,
pub/sub, config parsing/validation, file-watch, async runtime, metrics — MUST be a
crate, not bespoke code. The transcode brain is the ported engine (`apsis-engine`);
**aggregate observability is Grafana, not a bespoke dashboard**. New complexity MUST be
justified against "thin": if a feature re-grows apsis toward a general product (an
**unbounded** product UI, a file-transfer layer, a runtime plugin host), it MUST be
rejected or explicitly re-scoped. The **bounded operator console** (Principle V, spec 006)
is such an explicit re-scope — a read-mostly control/introspection UI over the spec 005
NATS surface, not a general product.

### III. Single-pass, single source of truth

Each file is transcoded in **one coherent ffmpeg pass** — ordered stages contribute to
one `FfmpegCommandSpec` (the accumulator), never a chain of per-operation passes. The
**coordinator computes exactly one abstract `FilePlan`** per file (the only decision
point); workers materialize it per backend and MUST NOT re-plan. Decision logic MUST
NOT be distributed across independent voting steps.

### IV. Correctness never sacrifices the library

The original file MUST NOT be mutated until a new output is written to a temp on the
same filesystem, **verified** (ffprobe: streams/duration/not-truncated), and atomically
renamed. Delivery is **at-least-once + idempotent**: a duplicate or replayed job on an
already-compliant file MUST be a no-op (`should_skip`). Work MUST hold a **lease**
(JetStream `AckWait` + heartbeats) so a crashed worker's job is redelivered, never
stranded. Poison jobs MUST dead-letter and alert, never loop forever.

### V. Bounded declarative extensibility; no runtime plugin host

apsis is extensible along a **declarative, bounded** surface so others can adapt it
without forking — a deliberate, *bounded* widening from the original "tool" (flexibility
of **policy and integration**, NOT a general product surface). Permitted:

- **Declarative config.** The `Profile` (transcode policy) plus per-file **conditional
  overrides** authored in a **pure, non-Turing-complete config-expression language** (CEL):
  side-effect-free, guaranteed-terminating, type-checked at load. It **conditions and
  computes config values only** — it MUST NOT construct plans, perform I/O, loop, or
  replace the engine. It is validation-adjacent (like `garde`), not a scripting host.
- **Two typed seams**, each shipping a batteries-included default and replaceable via a
  **versioned JSON contract**: the **planner** (`plan(probe,profile) → FilePlan`; default
  `apsis-engine`) and post-event **hooks** (default = a first-class *arr* helper +
  `exec`/`webhook`). A replacement planner still emits the abstract `FilePlan` (preserving
  portability + single-pass); hooks run **post-commit** and never mutate the transcode.
- **Backends** remain a `Backend` trait extended in code.
- **A bounded operator console** (spec 006): a **read-mostly** web UI (Leptos — full-stack
  Rust, SSR + WASM hydration) that observes per-file state + persisted decisions, streams live
  progress, and triggers the **spec 005 control intents** (pause / cancel / requeue / force /
  retry / mark-done). It reads **only** the spec 005 NATS surface (KV + progress subjects) and
  mutates state **only** by publishing to the spec 005 control-intent subjects — it holds **no
  privileged backdoor** (its server functions do nothing the `nats` CLI cannot), **edits no
  config**, and does **not** replace Grafana for aggregate/historical observability. Its own
  compiled WASM frontend (Leptos hydration) is the *app*, NOT a plugin host.

Still prohibited (would re-grow apsis into a heavy product): a **user-authored visual node
graph or editor**, an **unbounded product UI** (config editing in the UI, or anything beyond
the bounded read-mostly console above), a general **dynamic-ABI / WASM plugin host**, or any
extension that breaks **single-pass, portability, or the git-recoverable config**.
Principles I, III, IV are **unchanged**; Principle II and Observability are updated in step
with the bounded console. This widens *what config can express, where implementations plug in,
and a single bounded control surface* — nothing else.

## Observability & Operations

**Aggregate and historical observability is Grafana** — metrics MUST be Prometheus, scraped
into the VictoriaMetrics hub and viewed in Grafana; logs are structured (`tracing`) to
journald/VictoriaLogs; alerting is VMAlert. apsis MAY additionally ship the **bounded operator
console** (Principle V, spec 006) for *per-file live state, decisions, and control* — it does
**not** duplicate Grafana's aggregate/history role. Config MUST **fail fast**: invalid
`scheduler.toml`/`worker.toml` stops startup (`garde` validation), and the last-good
deployment keeps running until fixed. Every silent cap or dropped work MUST be logged.

## Licensing & Provenance

apsis is **MIT-licensed** and its provenance MUST stay clean:
- `apsis-engine` is a port of our **own** `_engine` (Python → Rust) — unencumbered.
- Unmanic (GPLv3) was studied for **patterns/edge-cases only** (ideas, not code);
  apsis code is original and copies no GPL source → no copyleft.
- FileFlows and Tdarr (closed cores) were studied **black-box from public docs and
  their open, permissively/GPL-licensed plugin repos only** — **no decompilation, no
  reverse engineering**. Nothing proprietary enters apsis.
- Dependencies MUST be permissive (MIT/Apache-2.0) and MIT-compatible.

## Governance

This constitution supersedes ad-hoc preferences for apsis. Any change to a Core
Principle MUST be reflected in `docs/design/rust-scheduler.md` (the Decisions table)
and bump the version below. Every design or code review MUST verify compliance;
deviations MUST be justified in writing against the relevant principle. The design doc
is the runtime guidance for implementation detail; this file is the constitution it
must not contradict.

**Amendments**:
- **2.0.0** (2026-08-18): Redefined Principle V from "compile-time modularity, no runtime
  plugins" to "bounded declarative extensibility; no runtime plugin host" — permits a pure
  config-expression language (CEL) for profile conditions and two versioned, replaceable
  seams (planner, hooks) with batteries-included defaults, while still prohibiting a visual
  graph/editor, bespoke UI, or dynamic-ABI/WASM plugin host. A *bounded* widening (policy +
  integration flexibility); Principles I–IV and no-UI unchanged. Reflected in
  `docs/design/rust-scheduler.md` (D4). Drives spec `004-rich-profiles-cel`.
- **3.0.0** (2026-08-24): Reversed the flagship "no bespoke UI" stance, **bounded**: permits a
  single **read-mostly operator console** (Leptos full-stack; SSR + WASM hydration) that
  observes per-file state/decisions + live progress and triggers the spec 005 control intents.
  It reads only the spec 005 NATS surface and mutates only via the spec 005 control-intent
  subjects (no privileged backdoor, no config editing, no visual graph/editor); Grafana keeps
  the aggregate/historical role. Deliberately relaxes "thin" for the console (accepting a
  compiled WASM frontend + `cargo-leptos` build) in exchange for a real reactive UI. Updated
  Principles II & V and Observability & Operations; Principles I, III, IV unchanged. Reflected
  in `docs/design/rust-scheduler.md` (D5). Drives spec `006-web-console`.

**Version**: 3.0.0 | **Ratified**: 2026-08-16 | **Last Amended**: 2026-08-24
