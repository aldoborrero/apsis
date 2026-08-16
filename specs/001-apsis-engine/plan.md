# Implementation Plan: apsis-engine — transcode planner & command builder

**Branch**: `001-apsis-engine` | **Date**: 2026-08-16 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-apsis-engine/spec.md`

## Summary

Port the pyflows `_engine` (probe → plan → ffmpeg command) to a standalone Rust
**library crate `apsis-engine`**. It computes a backend-neutral `FilePlan` from a probe
result + a profile, and materializes an **exact ffmpeg command** per `Backend` (VAAPI
with `-sei hdr`, CPU fallback). Behavioral parity with the Python `_engine` is the
acceptance oracle. No orchestration, queue, or file-watch — the crate is pure logic plus
ffprobe-JSON parsing. Design source of record: `docs/design/rust-scheduler.md`
(§4 Data model, §7 reconcile, Engine-scope note, Appendix A).

## Technical Context

**Language/Version**: Rust stable (edition 2021).

**Primary Dependencies**: `serde` + `serde_json` (ffprobe JSON → typed streams), `garde`
(profile validation, replacing pydantic), `thiserror` (error types). **No async runtime,
no queue, no file-watch** — running ffprobe/ffmpeg is the caller's (worker's) job; the
engine parses probe JSON and builds command specs.

**Storage**: N/A. The engine holds no state; it reads a file only to probe (optional
thin helper), and callers may pass a pre-obtained `Probe` in.

**Testing**: `cargo test`. Port the Python `_engine` fixtures as the **oracle**
(`tests/fixtures/`), plus **golden-command** tests asserting the exact ffmpeg arg vector
per backend.

**Target Platform**: Linux with jellyfin-ffmpeg (behavior matched to it); the crate is
OS-agnostic but validated there.

**Project Type**: Library crate (`apsis-engine`), future workspace member.

**Performance Goals**: N/A hot path — planning is one ffprobe per file; the engine's own
CPU cost is negligible.

**Constraints**: zero orchestration/queue/watch dependencies (FR-008); **no panics** on
malformed input — all fallible paths return `Result` (FR-010); **byte-exact** command
output for golden tests (SC-002).

**Scale/Scope**: ~1,800–2,400 LOC (port of ~1,414 Python) + tests.

## Constitution Check

*GATE: must pass before Phase 0. Re-checked after Phase 1.*

| Principle | Assessment |
|---|---|
| I. Declarative & git-recoverable | Engine is stateless; profiles arrive from config. **PASS** |
| II. Thin by reuse | Reuses `serde`/`garde`; the engine *is* the reused brain (a port, not a rewrite). **PASS** |
| III. Single-pass, single source of truth | Produces exactly one `FilePlan` and one `FfmpegCommandSpec` (accumulator). **PASS** |
| IV. Correctness never sacrifices the library | Engine never replaces files; MUST never plan dropping all audio (FR-006) and never panic (FR-010). **PASS** |
| V. Compile-time modularity, not runtime plugins | `Backend` is a compile-time trait. **PASS** |
| Licensing & provenance | Port of our own `_engine`; oracle is our Python; no third-party source. **PASS** |

No violations → Complexity Tracking empty.

## Project Structure

### Documentation (this feature)

```text
specs/001-apsis-engine/
├── plan.md          # this file
├── research.md      # Phase 0 — decisions & resolved unknowns
├── data-model.md    # Phase 1 — engine types
├── contracts/       # Phase 1 — public API surface (the "contract")
│   └── engine-api.md
├── quickstart.md    # Phase 1 — build/test/use
└── tasks.md         # Phase 2 — /speckit-tasks (not created here)
```

### Source Code (repository root)

A Cargo **workspace** holds all four crates from day one (foundation-first); only
`apsis-engine` is implemented in this spec — the other three are compiling **stubs**.

```text
Cargo.toml                      # [workspace] members = the four crates below
crates/
├── apsis-engine/               # ← implemented in this spec
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs              # public API re-exports
│   │   ├── probe.rs           # ffprobe JSON → Probe / StreamInfo
│   │   ├── config.rs          # Profile + sub-configs (serde + garde)
│   │   ├── plan.rs            # plan_from_probe → FilePlan (the decision core)
│   │   ├── audio.rs           # audio track plan (keep/priority/commentary/stereo)
│   │   ├── subtitles.rs       # subtitle filter (language/format/commentary)
│   │   ├── command.rs         # FilePlan + Backend → FfmpegCommandSpec
│   │   ├── backend/
│   │   │   ├── mod.rs         # Backend trait
│   │   │   ├── vaapi.rs       # hevc_vaapi/av1_vaapi, -sei hdr, HW/SW decode split
│   │   │   └── cpu.rs         # libx265 / libsvtav1
│   │   └── error.rs           # EngineError (thiserror)
│   └── tests/
│       ├── plan_oracle.rs     # SC-001: parity vs ported Python fixtures
│       ├── golden_cmd.rs      # SC-002: exact arg vector per backend
│       └── fixtures/          # ffprobe JSON + expected plans/commands (Python suite)
├── apsis-common/              # STUB — NATS wrapper, telemetry, error types (spec 002/003)
├── apsis-coordinator/         # STUB — scan / reconcile / enqueue (spec 002)
└── apsis-worker/              # STUB — run / verify / replace, backends (spec 002/003)
```

**Structure Decision**: **foundation-first with a full workspace.** All four crates
exist as members from the start (the whole shape is visible and adding real code later is
trivial), but this spec implements only `apsis-engine`; the other three are minimal
compiling stubs (`lib.rs`/`main.rs` that build and do nothing). The engine's module
layout **mirrors the Python `_engine`** (probe/config/plan/audio/subtitles/command) so
the port maps 1:1 with a direct fixture counterpart per case. `backend/` is new — it
factors backend-specific command building (VAAPI vs CPU) behind a trait, per
Constitution V and the design doc's plan/command split.

## Complexity Tracking

None — Constitution Check passes with no violations.
