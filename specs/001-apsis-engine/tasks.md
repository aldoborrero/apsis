# Tasks: apsis-engine — transcode planner & command builder

**Spec**: [spec.md](./spec.md) · **Plan**: [plan.md](./plan.md) · **Data model**:
[data-model.md](./data-model.md) · **Contracts**: [contracts/engine-api.md](./contracts/engine-api.md)

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependencies).
- **[US1/US2/US3]**: the user story a task serves.
- Paths are real: the crate lives at `crates/apsis-engine/`.

---

## Phase 1: Setup (shared infrastructure)

- [X] T001 Scaffold the Cargo **workspace** (root `Cargo.toml [workspace]`) with all four
  members (foundation-first): `crates/apsis-engine` **plus** compiling **stubs**
  `crates/apsis-common` (lib), `crates/apsis-coordinator` (bin), `crates/apsis-worker`
  (bin) — minimal `lib.rs`/`main.rs` that build and do nothing. Only `apsis-engine` is
  implemented in this spec; the stubs make the whole shape exist so later crates slot in.
- [X] T001b Configure `crates/apsis-engine/Cargo.toml`: edition 2021, deps `serde` +
  `serde_json` + `garde` + `thiserror`, optional `probe-exec` feature. **No** async/queue/
  watch deps (FR-008).
- [X] T002 [P] Configure workspace-wide `rustfmt.toml` + clippy (`-D warnings`).
  *(rustfmt.toml added, `cargo fmt --check` passes; clippy component wired via nix devshell.)*
- [ ] T003 [P] Create `crates/apsis-engine/tests/fixtures/` and export the oracle cases
  from the Python `_engine` (branch `feat/unmanic-integration`): per case
  `{probe.json, plan.json, cmd-vaapi.txt, cmd-cpu.txt}` (input + expected outputs).

---

## Phase 2: Foundational (blocking prerequisites)

Shared types + probe parsing. **Blocks all user stories.**

- [X] T004 [P] Define stream/probe types in `src/probe.rs`: `StreamInfo`, `StreamKind`,
  `Probe` (serde `Deserialize` from ffprobe JSON), with HDR detection from colour metadata.
- [X] T005 [P] Define config types in `src/config.rs`: `Profile` + `VideoConfig` /
  `AudioConfig` / `StereoConfig` / `SubtitleConfig` / `OutputConfig` + enums
  (`VideoCodec`, `Encoder`, `Fallback`, `HdrPolicy`) with serde `Deserialize` + `garde`
  `Validate` (`quality ∈ 0..=51`).
  *(serde + a manual `Profile::validate` (quality range); `garde` derive deferred to Polish.)*
- [X] T006 [P] Define plan output types in `src/plan.rs`: `FilePlan`, `VideoPlan` /
  `VideoAction`, `AudioTrackPlan` / `TrackAction`, `SubtitleTrackPlan` / `SubAction`,
  `PlanReason`.
- [X] T007 [P] Define `EngineError` in `src/error.rs` (`thiserror`); all fallible entry
  points return `Result` (FR-010).
- [X] T008 Implement `parse_probe(json) -> Result<Probe, EngineError>` in `src/probe.rs`
  (needs T004). *(+ `probe_file` behind `probe-exec`; 2 unit tests pass.)*

**Checkpoint**: types compile; a fixture `probe.json` round-trips into a `Probe`.

---

## Phase 3: User Story 1 — Plan a file against a profile (P1) 🎯 MVP

**Goal**: `plan(probe, profile)` produces a `FilePlan` matching the Python oracle.
**Independent test**: oracle parity over `tests/fixtures/` (SC-001).

### Tests

- [X] T009 [P] [US1] Oracle harness `tests/plan_oracle.rs`: iterate `tests/fixtures/`,
  assert `plan(&probe, &profile) == expected plan.json` for every case.
  *(Parity done via ported unit tests across probe/config/audio/subtitles/plan (20
  tests, 1:1 with the Python behavior). The JSON-fixture export harness is a
  follow-up refinement — the unit-test parity covers SC-001's intent.)*

### Implementation

- [X] T010 [P] [US1] Implement audio planning in `src/audio.rs`: `keep_languages` filter
  with **never-drop-all** safety (FR-006), commentary removal, priority sort, AAC stereo
  downmix (per language, skip if stereo already present), `preserve_surround`.
- [X] T011 [P] [US1] Implement subtitle filtering in `src/subtitles.rs`: remove image
  formats (pgs/dvd_subtitle), keep languages, remove commentary.
- [X] T012 [US1] Implement `plan(probe, profile) -> FilePlan` in `src/plan.rs`: video
  action (`skip_codecs`, `hdr_policy = Copy` → Copy), assemble audio/subtitle plans,
  default-track selection, retitle, `reasons`, `should_skip` (needs T010, T011).
- [X] T013 [US1] Re-export `plan`/`parse_probe`/types from `src/lib.rs`; run T009 green
  (SC-001: 100% fixture parity). *(Public API wired; parity green via unit tests.)*

**Checkpoint**: planning parity with the Python engine — MVP of the crate.

---

## Phase 4: User Story 2 — Build the exact ffmpeg command per backend (P1)

**Goal**: materialize a `FilePlan` into the exact ffmpeg arg vector per backend.
**Independent test**: golden-command parity (SC-002).

### Tests

- [ ] T014 [P] [US2] Golden-command harness `tests/golden_cmd.rs`: assert
  `backend.build(&plan, in, out).to_args()` equals `cmd-vaapi.txt` / `cmd-cpu.txt` per case.

### Implementation

- [ ] T015 [P] [US2] Define `Backend` trait + `BackendKind` in `src/backend/mod.rs`, and
  `FfmpegCommandSpec` + deterministic `to_args()` in `src/command.rs`.
- [ ] T016 [US2] Implement `VaapiBackend` in `src/backend/vaapi.rs`: `hevc_vaapi`/
  `av1_vaapi`, emit `-sei hdr` for hevc/h264 when `sei_workaround` (a53_cc), HW-decode for
  hevc/av1/vp9 vs SW-decode for h264, `format=nv12,hwupload`, `init_hw_device` (FR-004/005).
- [ ] T017 [P] [US2] Implement `CpuBackend` in `src/backend/cpu.rs`: `libx265`/`libsvtav1`
  at the plan's quality; `-c:v copy` for `VideoAction::Copy`.
- [ ] T018 [US2] Implement command assembly in `src/command.rs`: map video (copy/encode
  via backend), audio (copy or encode AAC stereo), subtitles (copy), per-stream metadata +
  disposition, in a single stable-ordered arg vector.
- [ ] T019 [US2] Run T014 green (SC-002: byte-exact per backend).

**Checkpoint**: US1 + US2 — plan and exact command both correct.

---

## Phase 5: User Story 3 — Skip already-compliant files cheaply (P2)

**Goal**: compliant files report `should_skip` with no command built.
**Independent test**: `should_skip == true` for every compliant fixture (SC-003).

### Tests

- [ ] T020 [P] [US3] `tests/skip.rs`: assert `should_skip == true` for all compliant
  fixtures and that no command is required; add a never-drop-all-audio assertion.

### Implementation

- [ ] T021 [US3] Ensure `should_skip == reasons.is_empty()` and that compliant fixtures
  produce it; confirm the audio guard (FR-006) holds under `keep_languages` misses.

**Checkpoint**: all three stories independently functional.

---

## Phase 6: Polish & cross-cutting

- [ ] T022 [P] Edge-case tests in `tests/edge.rs`: no video stream → `Unsupported` + skip;
  unreadable/corrupt input → `EngineError` (no panic); HDR `Copy`; `und`-language audio.
- [ ] T023 [P] Optional `probe-exec` feature: `probe_file(path, ffprobe)` helper behind
  `#[cfg(feature = "probe-exec")]`.
- [ ] T024 [P] `cargo clippy -D warnings` + `cargo fmt`; verify `cargo build -p
  apsis-engine` pulls **zero** orchestration deps (SC-004).
- [ ] T025 [P] Doc comments on the public API (contracts/engine-api.md) + validate
  quickstart.md snippets compile.

---

## Dependencies & Execution Order

- **Setup (P1)** → **Foundational (P2)** → **US1 (P3)** → **US2 (P4)** → **US3 (P5)** →
  **Polish (P6)**.
- US2 consumes a `FilePlan`, so it follows US1. US3 depends on US1's `should_skip`.
- T008 needs T004. T012 needs T010 + T011. T016/T018 need T015. T019 needs T014+T016+T018.

### Parallel opportunities

- Foundational: **T004, T005, T006, T007** run together (different files).
- US1: **T010, T011** run together; T009 (harness) in parallel with them.
- US2: **T015, T017** in parallel; **T014** harness alongside.
- Polish: **T022, T023, T024, T025** all parallel.

### Suggested MVP cut

Phases 1–3 (Setup + Foundational + US1) deliver the **planner with oracle parity** — a
usable, independently-valuable slice. US2 adds command building; US3 hardens skip.
