---
description: "Task list for 004-rich-profiles-cel"
---

# Tasks: Rich transcode profiles with CEL conditional overrides

**Input**: Design documents in `specs/004-rich-profiles-cel/` (spec.md, plan.md, research.md,
data-model.md, contracts/, quickstart.md)

**Tests**: INCLUDED — the spec has acceptance scenarios (US1–US3) and the plan mandates unit +
golden + fail-fast tests. Rust in-crate `#[cfg(test)]` + golden fixtures (no separate `tests/`
tree needed).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: parallelizable (different file, no dependency on an incomplete task)
- **[Story]**: US1 / US2 / US3
- Paths are real workspace paths (single Rust workspace under `crates/`).

## Path conventions (this feature)

- Profile schema + rule types + resolver: `crates/apsis-engine/src/`
- Coordinator wiring: `crates/apsis-coordinator/src/reconcile.rs`
- Tests: in-crate `#[cfg(test)]` in the same files + golden fixtures in `apsis-engine`.

---

## Phase 1: Setup (shared)

**Purpose**: dependency + build.

- [ ] T001 Add `cel-interpreter` (MIT) to `crates/apsis-engine/Cargo.toml` `[dependencies]`;
  confirm `cargo build -p apsis-engine` is green and `nix flake check` still passes. (research R1)

---

## Phase 2: Foundational (blocks ALL user stories)

**Purpose**: the shared types every story reads — probe fields the CEL context + skip-gates need,
and the unified `Bitrate` type. **⚠️ No US work starts until this is done.**

- [ ] T002 [P] Extend `crates/apsis-engine/src/probe.rs`: add the MISSING `StreamInfo` fields —
  video `bitrate` + `bit_depth`, audio `title`, subtitle `forced` — and parse them from the
  ffprobe JSON. (`width`/`height`/`color_transfer` already exist.) Unit-test the parse. (research R4)
- [ ] T003 [P] Add a `Bitrate` type in `crates/apsis-engine/src/config.rs` with a custom
  `Deserialize` accepting a bare int (kbps) **or** a unit string (`"128k"`/`"5M"`); migrate
  `StereoConfig.bitrate: u32` → `Bitrate`; unit-test that `bitrate = 128` still loads (SC-006).
  (data-model §Bitrate)

**Checkpoint**: probe carries the CEL/skip fields; one bitrate convention exists.

---

## Phase 3: User Story 1 — Complete declarative coverage (Priority: P1) 🎯 MVP

**Goal**: express a full library-cleanup policy in `scheduler.toml` (quality-mode, preset,
resolution, audio transcode/mono, subtitle order/forced/extract, skip gates, conform, metadata)
+ the keep-≥1-audio failsafe. No CEL.

**Independent Test**: golden `probe + profile → FilePlan` assertions for each new field; a profile
with new fields plans correctly; existing `scheduler.toml` loads unchanged.

### Tests for US1 (write first, expect FAIL)

- [ ] T004 [P] [US1] `config.rs` tests: `QualityMode` (qp/crf/bitrate) + shorthand `quality = 22`
  back-compat; range validation (`value` 0–51 for qp/crf).
- [ ] T005 [P] [US1] Golden tests (`plan.rs`/`audio.rs`/`subtitles.rs`): `max_resolution` downscale
  (2160→1080, 720 untouched), `add_mono`, subtitle `order`, `audio.transcode`, `output.conform`
  (drops container-incompatible stream), skip gates (`skip_if_resolution_below`/`_bitrate_below`,
  strictly-below boundary).
- [ ] T006 [P] [US1] Test the keep-≥1-audio failsafe: an aggressive `keep_languages` matching no
  track → exactly one audio stream retained (subsumes the existing `audio.rs` guards).

### Implementation for US1

- [ ] T007 [US1] `config.rs`: `QualityMode { mode: qp|crf|bitrate|vmaf, value }` + bare-int
  shorthand → `{auto, N}`; `VideoConfig` adds `preset`, `max_resolution`, `crop`, `custom_args`,
  `skip_if_resolution_below`, `skip_if_bitrate_below` (all `deny_unknown_fields`).
- [ ] T008 [US1] `config.rs`: `AudioConfig` adds `transcode`, `add_mono`, `max_channels`,
  `normalize`; `SubtitleConfig` adds `order`, `forced_only`, `extract`; `OutputConfig` adds
  `conform`, `strip_metadata`, `keep_chapters`.
- [ ] T009 [US1] `crates/apsis-engine/src/audio.rs`: construct `transcode` (re-encode kept),
  `add_mono` (from best kept source), `max_channels`, `normalize`; consolidate the keep-≥1-audio
  failsafe (one guard covering all filters, per data-model §invariant).
- [ ] T010 [US1] `crates/apsis-engine/src/subtitles.rs`: `forced_only` → `order` → `extract`
  (sidecar copy; in-container track still kept unless filtered).
- [ ] T011 [US1] `crates/apsis-engine/src/plan.rs`: skip-gate evaluation (`skip_codecs` →
  `skip_if_*`); `max_resolution`+`crop` into `VideoPlan` (crop-then-scale); `conform` after all
  selection (never drops the last audio). Implement the fixed pipeline in
  data-model §"Resolution & construction order".
- [ ] T012 [US1] `crates/apsis-engine/src/command.rs`: materialize new plan fields into the
  **single** ffmpeg command — preset, scale/crop filters, audio encode, metadata/chapters,
  `custom_args` appended last (never a 2nd pass); `quality.mode = auto` → `-qp` (VAAPI) / `-crf`
  (CPU). Adapt to the `Bitrate` type for audio bitrate args.

**Checkpoint**: US1 fully functional — a complete policy expressible + tested, back-compat intact.

---

## Phase 4: User Story 2 — CEL conditional overrides (Priority: P2)

**Goal**: `[[profiles.X.rule]] when = "<CEL>" set = {…}` layered into an effective profile before
the single planner runs.

**Independent Test**: base profile + one rule; plan a matching and a non-matching file → override
applied only to the match; effective = base + overrides (ordered, last-write-wins).

**Depends on**: US1 (rules `set` US1 fields) + Foundational.

### Tests for US2

- [ ] T013 [P] [US2] `overrides.rs` tests: ordered layering + last-write-wins; `when` match/no-match
  (height/hdr/`audio.exists(truehd)`); computed `set` value (`"... ? 24 : 22"`); coordinator
  integration (effective profile reaches `plan()`).

### Implementation for US2

- [ ] T014 [US2] `config.rs`: `ProfileRule { when: String, set: BTreeMap<String, SetValue> }` and
  `Profile.rules: Vec<ProfileRule>`; `SetValue` = literal-or-CEL-string.
- [ ] T015 [US2] NEW `crates/apsis-engine/src/overrides.rs`: build the CEL context from `Probe`
  (per contracts/cel-context.md, `cel_context_version: 1`); compile each `when`/`set` `Program`;
  `resolve_effective_profile(base, rules, ctx) -> Profile` — ordered layering, dotted-path `set`
  application, computed-value evaluation. Pure, unit-testable without NATS. (research R6/R7)
- [ ] T016 [US2] `crates/apsis-coordinator/src/reconcile.rs`: call `resolve_effective_profile`
  before `plan()`; run skip-evaluation on the **effective** profile (a rule may set a value a gate
  reads). Workers unchanged (they never see rules — Constitution III).

**Checkpoint**: US1 + US2 both work independently.

---

## Phase 5: User Story 3 — Fail-fast + safe evaluation (Priority: P3)

**Goal**: invalid CEL caught at config load; computed-value range violations fail per-file loudly,
never the daemon; evaluation pure/terminating.

**Independent Test**: bad config (`when = "video.heigth >= 2160"`) → startup fails; a computed
`set` that yields an out-of-range value only for real inputs → that file fails at reconcile
(logged, skipped), daemon and other files unaffected.

**Depends on**: US2 (validates its CEL).

### Tests for US3

- [ ] T017 [P] [US3] Tests: syntax error / unknown field / type mismatch / static out-of-range
  `set` → **config load** error (fail-fast). Computed out-of-range `set` → per-file error, file
  skipped, daemon survives, other files still processed.

### Implementation for US3

- [ ] T018 [US3] Load-time validation: on config load, **compile** + **canary-evaluate** every
  `when`/`set` CEL against a synthetic full-context probe (all contract fields populated); a
  failure aborts startup, wired into the existing `garde`/figment fail-fast path. (research R2)
- [ ] T019 [US3] Per-file validation of the resolved effective profile (FR-015): a computed `set`
  value violating a field's rules → structured `tracing` error, the file is skipped (invalid plan,
  not transcoded), the daemon and sibling files are unaffected.

**Checkpoint**: all three stories independently functional.

---

## Phase 6: Polish & cross-cutting

- [ ] T020 [P] Docs: extend the `scheduler.toml` reference / `docs/apsis-operations.md` with the
  new profile fields + CEL rule syntax + the `cel_context_version: 1` contract; note the constitution
  v2.0.0 rationale.
- [ ] T021 Ensure a `--check-config <scheduler.toml>` path exists (used by quickstart.md) that runs
  full load-time validation without connecting to NATS; or document the existing check command.
- [ ] T022 Gate: `nix develop --command bash -c 'cargo fmt && cargo clippy --workspace
  --all-targets -- -D warnings && cargo test -p apsis-engine -p apsis-coordinator'`.
- [ ] T023 [P] Run `quickstart.md` end-to-end against the built binaries (the two example profiles
  + the CEL rules + the fail-fast checks).

---

## Dependencies & Execution Order

- **Setup (P1)** → **Foundational (P2)** → **US1 (P3)** → **US2 (P4)** → **US3 (P5)** → **Polish**.
- **US1** is the MVP and can ship alone (coverage + failsafe, no CEL).
- **US2 depends on US1** (its `set` targets US1's fields) and Foundational.
- **US3 depends on US2** (it validates US2's CEL).
- Foundational **T002/T003 are [P]** (different files). US1 tests **T004/T005/T006 [P]**. US1 impl:
  T007/T008 [P] (config), then T009/T010 [P], then T011 (plan, depends on config), then T012
  (command, depends on plan).

## Parallel example (US1 config)

```
# after Foundational:
T004  QualityMode + shorthand tests
T005  golden field tests
T006  failsafe test
# then implement config in parallel:
T007  VideoConfig fields
T008  Audio/Subtitle/Output fields
```

## Implementation strategy

1. **MVP** = Setup + Foundational + **US1**. Stop, validate (golden tests + a real `scheduler.toml`
   load), ship — this alone delivers `add_mono` + subtitle `order` + AV1 readiness + coverage.
2. **US2** adds CEL overrides (opt-in; the reference operator uses none).
3. **US3** hardens validation.
4. Each story is independently testable; commit per task or logical group; run T022 before pushing.

## Notes

- Tests-first per story (write, see FAIL, implement).
- `[P]` = different file, no incomplete dependency.
- No worker changes (Constitution III: workers never re-plan).
- Out of scope (separate specs): hooks, planner seam, `apsis-engine`→`apsis-planner` rename, NVENC
  backend, VMAF/AutoCRF implementation.
