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
  **DEFERRED to the start of US2 (Phase 4)** — CEL isn't used until then; adding it now would
  be an unused dependency. US1 needs no CEL.

---

## Phase 2: Foundational (blocks ALL user stories)

**Purpose**: the shared types every story reads — probe fields the CEL context + skip-gates need,
and the unified `Bitrate` type. **⚠️ No US work starts until this is done.**

- [X] T002 [P] Extend `crates/apsis-engine/src/probe.rs`: added `StreamInfo.bitrate`,
  `bit_depth`, `forced` + parse from ffprobe JSON (`bit_rate`/`bits_per_raw_sample` strings,
  `disposition.forced`). `title`/`width`/`height`/`color_transfer`/`is_default` already existed.
  Test `parses_bitrate_bitdepth_forced`. (research R4)
- [X] T003 [P] Added `Bitrate` type (`config.rs`): custom `Deserialize` accepting a bare int
  (kbps → `"128k"`) **or** a unit string (`"128k"`/`"5M"`), junk rejected at load; migrated
  `StereoConfig.bitrate: u32 → Bitrate` (+ `AudioAction.bitrate`, `command.rs`); back-compat test
  `bitrate_accepts_int_and_string_and_rejects_junk`. (data-model §Bitrate)

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

- [X] T007 [US1] `config.rs`: `QualityMode {mode: auto|qp|crf|bitrate|vmaf, value: Num|Rate}` +
  bare-int shorthand → `{auto, N}` (range-validated at load; vmaf reserved, errors at
  command-build via `EngineError::Unsupported`); `VideoConfig` + `preset`, `max_resolution`,
  `crop` (`Crop` enum), `custom_args`, `skip_if_resolution_below`, `skip_if_bitrate_below`.
  `command.rs` materializes via `QualityMode::rc_opts` (auto→qp/crf per backend) + `preset`.
- [X] T008 [US1] `config.rs`: `AudioConfig` + `transcode` (`AudioTranscode`), `add_mono`
  (`MonoConfig`), `max_channels`, `normalize`; `SubtitleConfig` + `order`, `forced_only`,
  `extract`; `OutputConfig` + `conform`, `strip_metadata`, `keep_chapters`. Tests:
  `deserializes_spec004_coverage_fields`, `quality_mode_forms_and_rc_opts`.
- [x] T009 [US1] `crates/apsis-engine/src/audio.rs` + `command.rs`: `transcode` (re-encode kept),
  `add_mono` (from best kept source), `max_channels` done; `normalize` = single-pass `loudnorm`
  on encoded tracks only (a copy can't be filtered). Keep-≥1-audio failsafe already per-filter.
- [~] T010 [US1] `crates/apsis-engine/src/subtitles.rs`: `forced_only` + `order` done.
  **`extract` DEFERRED** — a sidecar `.srt` is a *second* ffmpeg output whose file lifecycle
  must interact with the worker's temp-write + atomic-replace, and image subs (PGS/VobSub) need
  OCR, not ffmpeg. Own increment/spec, not a command-layer tweak.
- [x] T011 [US1] `crates/apsis-engine/src/plan.rs` + `command.rs`: skip-gates (`skip_if_*`),
  `conform` (subtitles), `max_resolution` (per-backend `scale`/`scale_vaapi`, downscale-only,
  encode-only). **`crop` DEFERRED** — autocrop needs `cropdetect` (an analysis pass), which
  conflicts with single-pass (Principle III). audio-conform DEFERRED (codec ambiguity).
- [x] T012 [US1] `crates/apsis-engine/src/command.rs`: preset, `scale` filters, audio encode,
  metadata/chapters, `custom_args` last, `quality.mode` rc-opts, `Bitrate` audio args — all in
  the single command. (crop filter deferred with T011.)

**Checkpoint**: US1 fully functional — a complete policy expressible + tested, back-compat intact.

---

## Phase 4: User Story 2 — CEL conditional overrides (Priority: P2)

**Goal**: `[[profiles.X.rule]] when = "<CEL>" set = {…}` layered into an effective profile before
the single planner runs.

**Independent Test**: base profile + one rule; plan a matching and a non-matching file → override
applied only to the match; effective = base + overrides (ordered, last-write-wins).

**Depends on**: US1 (rules `set` US1 fields) + Foundational.

### Tests for US2

- [x] T013 [P] [US2] Tests written alongside impl (not TDD-order): `overrides.rs` (6) — matching
  layers/last-write-wins/literal-not-CEL/unknown-path-rejected/bad-predicate; `config.rs` rule
  parse + `cel` smoke; coordinator integration (`rule_override_reaches_enqueued_job`).

### Implementation for US2

- [x] T014 [US2] `config.rs`: `ProfileRule { when, set: BTreeMap<String, SetValue> }` +
  `Profile.rules` (TOML `rule`, default empty → back-compat). `SetValue(serde_json::Value)` keeps
  the raw value; literal-vs-CEL is decided by the resolver. Added `cel` 0.14 (renamed from the
  spec's `cel-interpreter`). Profile/Job drop `Eq`.
- [x] T015 [US2] NEW `crates/apsis-engine/src/overrides.rs`: `build_context` (cel_context_version 1)
  + `resolve_effective_profile` via JSON round-trip (dotted-path `set`, last-write-wins, re-validate
  on deserialize). Type-directed literal-vs-CEL (string on string field = literal, else CEL);
  computing a string field via CEL is NOT expressible (documented). cel→JSON via the `json` feature.
- [x] T016 [US2] `crates/apsis-coordinator/src/reconcile.rs`: resolve the effective profile between
  probe and `plan()`; effective profile feeds skip-gates AND `Job.profile_config` (worker never
  sees rules). No rules → fast path. **`duration` context field is 0.0** until the probe carries it
  (contract: 0.0 = unknown) — small follow-up in probe.rs.

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

- [x] T017 [P] [US3] Tests: config-load rejects syntax / unknown context field / static
  out-of-range / unknown path (5 in `config.rs`); engine `validate_rules` accepts sane + rejects
  static; the FR-013/FR-015 split (`height/50` loads at canary, fails a 3000-line file).

### Implementation for US3

- [x] T018 [US3] `apsis_engine::validate_rules(base, rules)`: compile + canary-evaluate every
  `when`/`${…}` `set` against a synthetic full context (normal 1080p file), then apply each rule to
  the base and re-deserialize (unknown path / type / static range). Wired into `scheduler_from`
  (fail-fast) as `ConfigError::InvalidRule`. (research R2)
- [x] T019 [US3] Per-file validation — **already delivered by the US2 reconcile wiring**: a
  CEL-computed value out of range for a real file fails `resolve_effective_profile` → the file is
  marked `Failed@ver` (+ `apsis_override_failed_total`), daemon and siblings unaffected (FR-015).
  Now covered by a test.

**Checkpoint**: all three stories independently functional.

---

## Phase 6: Polish & cross-cutting

- [x] T020 [P] Docs: `docs/apsis-operations.md` gained a full Profile field reference (all
  spec-004 additions, with deferred fields marked) + a Conditional overrides (CEL) section
  documenting the `${…}` marker, the `cel_context_version: 1` context (linked to the contract),
  and the fail-fast-at-load / per-file-Failed semantics. README rewritten for apsis.
- [x] T021 `apsis-coordinator --check-config [path]`: loads + fully validates the scheduler
  config incl. every CEL rule (via the load-time canary), exits 0/1, no NATS/ffprobe. Verified
  against a good config and a bad-rule config.
- [x] T022 Gate green throughout: `cargo fmt && cargo clippy --workspace --all-targets -D warnings
  && cargo test --workspace` (9 suites).
- [~] T023 [P] Fail-fast half of quickstart exercised end-to-end (`--check-config` on the CEL-rule
  example + a broken rule). The live transcode e2e (profiles → NATS → worker → atomic-replace)
  needs a real media library + running NATS, out of scope for a unit-test environment.

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
