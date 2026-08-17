# Implementation Plan: Single-node library reconcile & safe transcode

**Branch**: `002-single-node-transcode` | **Date**: 2026-08-17 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/002-single-node-transcode/spec.md`

## Summary

apsis runs on one host (rhea) and keeps each configured library converged to its profile:
a **coordinator** reconciles files against the profile via `apsis-engine` (spec 001) and
enqueues only drift; a colocated **VAAPI worker** transcodes non-compliant files and
**atomically replaces** the original only after ffprobe verification, falling back to CPU
on VAAPI failure. Durability, the worker lease, crash-redelivery, and dead-lettering come
from **NATS JetStream (single-node)** — a job stream, a `transcode_state` KV, and a pull
consumer — chosen so spec 003 (the sirius NVENC worker) is **additive**, not a substrate
migration. No web UI: Prometheus metrics + structured logs feed the existing Grafana stack.

## Technical Context

**Language/Version**: Rust (edition 2024, rust-version 1.85; workspace-pinned)

**Primary Dependencies**:
- `apsis-engine` (spec 001) — plan + ffmpeg command builder (the only decision point)
- `tokio` — async runtime, `process` (ffmpeg/ffprobe), `sync::{mpsc, Semaphore}`
- `async-nats` 0.50 — JetStream (stream + pull consumer) + KV bucket + core pub/sub
- `notify` — inotify with a periodic-walk backstop (FR-001)
- `serde` + `toml` + `figment` — layered config (file + env + defaults); **not** serde_yaml
- `garde` — config validation (ranges, cross-refs); `schemars` — JSON-Schema for editors
- `tracing` + `tracing-subscriber` — structured spans per job
- `metrics` + `metrics-exporter-prometheus` — scraped by vmagent (FR-011)
- `ulid` — sortable job ids

**Storage**: NATS JetStream — a job **stream** (work queue, WorkQueue retention) and a
**KV bucket** `transcode_state` (`path → {status, version=mtime:size, job_id, attempts,
updated_at}`). No SQL DB. Desired state is git (config); actual state is the filesystem;
KV is a re-derivable cache (FR-013).

**Testing**: `cargo test` (unit + integration). The queue/state layer sits behind a small
`Queue`/`StateStore` trait so the reconcile and worker logic are testable with an in-memory
fake; a subset of integration tests run against a real `nats-server` (JetStream) when
available (gated), asserting AckWait redelivery and MaxDeliver dead-lettering.

**Target Platform**: Linux (rhea). ffmpeg VAAPI on AMD (`/dev/dri/renderD128`) + CPU
fallback. NATS single-node (a `nats-server` process/container on rhea).

**Project Type**: multi-crate Rust workspace — two daemons (`apsis-coordinator`,
`apsis-worker`) over a shared `apsis-common` (schemas, config, NATS wiring). The stubs
already exist from the spec-001 scaffold.

**Performance Goals**: throughput is ffmpeg-bound, not apsis-bound. A reconcile pass over
a library MUST not re-probe unchanged files (FR-005) — the change-token cache short-circuits
before any ffprobe. Concurrency is deliberately bounded (FR-010; single-session on AMD VCN
HEVC → default 1 VAAPI job).

**Constraints**: never mutate the original until the verified atomic replace (FR-007/008);
preserve owner/mtime/perms; temp on the **same filesystem** as the source; fail-fast on bad
config while the last-good deployment keeps running (FR-012).

**Scale/Scope**: one host, a handful of libraries, thousands of files. One VAAPI worker.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | How this plan complies |
|-----------|------------------------|
| **I — Declarative & git-recoverable** | All desired state (libraries, profiles, worker config) is TOML in git. Runtime state (JetStream stream + KV) is a re-derivable cache: a rescan reconstructs it; losing NATS costs one re-probe pass, never a re-transcode. No live-only state. |
| **II — Thin by reuse** | The decision logic is `apsis-engine` (already built). Durability/lease/dead-letter are JetStream features, not hand-rolled. ffmpeg/ffprobe are shelled, not reimplemented. |
| **III — Single-pass, single source of truth** | Exactly one `plan_file` decision per (path, version); the reconcile loop is the only place drift is decided. Single-pass ffmpeg (engine's command). |
| **IV — Correctness never sacrifices the library** | Atomic replace only after verify; partial outputs discarded; original untouched on any failure; VAAPI→CPU fallback; version-gated failed-suppression stops poison retries. This is the spec's core. |
| **V — Compile-time modularity, not runtime plugins** | Backends (VAAPI/CPU) are the engine's compile-time `Backend` impls. No runtime plugin loader. Worker capabilities are static config. |
| **Observability/Ops** | Prometheus metrics + `tracing` spans; no bespoke UI (FR-011). |
| **Licensing/Provenance** | MIT; own code; no GPL/closed code copied. |

**Gate result: PASS.** One deliberate substrate choice (NATS from the start rather than a
throwaway in-process queue) is justified in Complexity Tracking.

## Project Structure

### Documentation (this feature)

```text
specs/002-single-node-transcode/
├── plan.md              # This file
├── research.md          # Phase 0 — decisions (NATS layout, verify, atomic replace, debounce, testing)
├── data-model.md        # Phase 1 — Job/StateEntry/config entities + JetStream layout
├── quickstart.md        # Phase 1 — run it on a test library
├── contracts/
│   └── nats-protocol.md  # subjects, stream, KV, consumer config, message JSON schemas
└── tasks.md             # Phase 2 — /speckit-tasks (NOT created here)
```

### Source Code (repository root)

```text
crates/
├── apsis-engine/         # spec 001 — DONE (plan + ffmpeg command builder). Consumed as-is.
├── apsis-common/         # shared: config (figment/garde), Job/StateEntry/Result schemas,
│                         #   subjects+bucket constants, JetStream setup (stream/KV/consumer),
│                         #   the Queue/StateStore traits + their NATS impls, path_map, ids.
├── apsis-coordinator/    # bin: reconcile loop
│   src/
│   ├── main.rs           #   figment config load → fail-fast (FR-012); wire NATS; run loop
│   ├── discover.rs       #   inotify (notify) ⊎ periodic walk; is_video filter; debounce (FR-001/002)
│   ├── profile_match.rs  #   longest-matching library path → profile (FR-003)
│   └── reconcile.rs      #   version cache gate → engine.plan → KV CAS + publish job (FR-004/005)
└── apsis-worker/         # bin: consumer + execution
    src/
    ├── main.rs           #   config; subscribe pull consumer; Semaphore bound (FR-010)
    ├── run.rs            #   build cmd from plan (engine Backend); tokio::process; -progress parse
    ├── fallback.rs       #   VAAPI non-zero → retry once on CPU; record used_fallback (FR-006)
    ├── verify.rs         #   ffprobe: streams/duration/not-truncated/size-sane (FR-007)
    └── replace.rs        #   temp on same fs → fsync → rename → restore stat(2) (FR-007/008)

tests/                    # workspace integration tests (real nats-server, gated)
```

**Structure Decision**: The three-crate split already exists from spec 001's foundation
scaffold; this feature fills `apsis-common`, `apsis-coordinator`, and `apsis-worker`. The
coordinator/worker boundary is drawn exactly where JetStream sits between them, so spec 003
adds a second worker binary/host with **zero** coordinator changes.

## Complexity Tracking

| Choice | Why needed | Simpler alternative rejected because |
|--------|-----------|--------------------------------------|
| NATS JetStream in single-node phase | The spec needs a durable queue, a worker lease, crash-redelivery, retry-to-dead-letter, and a version-gated state cache — all **native** JetStream features. Using it now makes spec 003 additive. | An in-process `tokio` queue + SQLite/redb state would reimplement AckWait/MaxDeliver/KV by hand, then be **thrown away** when distribution lands — more total code and a substrate migration, for the sake of deferring one `nats-server` process. |
| Two daemons (coordinator + worker) not one | The reconcile decision and the ffmpeg execution have different failure/restart/concurrency profiles, and the split is the seam spec 003 grows along. | A single process would still need the same internal boundary, and would have to be re-split for distribution. |
