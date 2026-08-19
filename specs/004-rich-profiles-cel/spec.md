# Feature Specification: Rich transcode profiles with CEL conditional overrides

**Feature Branch**: `004-rich-profiles-cel` (spec dir; no branch created — no `before_specify` hook)

**Created**: 2026-08-18

**Status**: Draft

**Input**: User description: "Use spec-kit to improve the profiles with CEL before anything else — broaden what a transcode Profile can express declaratively (coverage) and add per-file conditional overrides authored in CEL. Profile scope only; hooks, the planner-replacement seam, and the `apsis-engine`→`apsis-planner` rename are separate."

## Constitution alignment *(read first — governance gate)* — RESOLVED

Constitution **Principle V was amended (v2.0.0, 2026-08-18)** from "compile-time modularity,
no runtime plugins" to **"bounded declarative extensibility; no runtime plugin host."** That
amendment — a *bounded* widening for policy + integration flexibility — explicitly permits
this feature: a **pure, non-Turing-complete, side-effect-free config-expression language**
(CEL) that **conditions and computes declarative config values only** and CANNOT construct a
plan, perform I/O, loop, or replace the engine (validation-adjacent, like `garde`; not a
scripting host). The prohibitions that remain (visual node graph/editor, bespoke UI,
dynamic-ABI/WASM host) are untouched, and Principles I–IV hold: **I** (declarative, in
`scheduler.toml`, git-recoverable, no live-only state), **II** (one existing crate, no
bespoke host/UI grown), **III** (still one `FilePlan` per file; overrides resolve *before*
the single planner runs), **IV** (unchanged safety). This spec is therefore **unblocked**;
the amendment is recorded in the constitution and `docs/design/rust-scheduler.md` (D4).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Complete transcode policy, declaratively (Priority: P1)

A homelab operator (or someone adopting apsis with *different* preferences/hardware) writes
a single `scheduler.toml` profile that fully expresses their library-cleanup policy —
target codec and quality *strategy*, resolution cap, audio track handling (keep/transcode/
generate stereo **and mono**), subtitle handling (keep/order/forced/extract), skip gates,
and container conforming — **without hitting a field they can't express** and without
writing code. The same profile runs unchanged across backends — VAAPI and CPU today (NVENC is
added in spec 003) — because the profile names no hardware.

**Why this priority**: This is the "usable by other people" payoff. Today's profile covers
audio/subtitle language rules and HDR well but lacks the table-stakes video/quality/skip/
output knobs that Unmanic, Tdarr and FileFlows all expose, so a new adopter hits a wall.
Closing that gap is the foundation; the conditional layer (US2) builds on it.

**Independent Test**: Author a profile exercising each new field against a fixture probe and
assert the resulting `FilePlan` (streams, actions, target codec/quality, container). No CEL
required. Delivers a fully expressible policy on its own.

**Acceptance Scenarios**:

1. **Given** a profile with `quality = { mode = "crf", value = 20 }`, **When** a file is
   planned for a CPU encode, **Then** the plan carries CRF-20 semantics; **and** a profile
   with the shorthand `quality = 22` resolves to the encoder-appropriate mode with value 22.
2. **Given** `video.max_resolution = "1080p"` and a 2160p source, **When** planned, **Then**
   the plan includes a downscale to 1080p; a 720p source is left unscaled.
3. **Given** `audio.transcode = { codec = "opus", bitrate = "128k" }`, **When** planned,
   **Then** kept audio tracks are re-encoded to Opus (not only the generated stereo clone).
4. **Given** `audio.add_mono = { languages = ["eng"] }`, **When** planned, **Then** a mono
   track is generated from the best English source.
5. **Given** `subtitles.order = ["eng","spa","jpn"]`, **When** planned, **Then** kept
   subtitle tracks appear in that order with the configured default.
6. **Given** `output.conform = true` and a source stream the target container cannot hold,
   **When** planned, **Then** that stream is dropped rather than causing a mux failure.
7. **Given** an aggressive `audio.keep_languages` that matches no track, **When** planned,
   **Then** the engine's failsafe keeps at least one audio stream (never a silent file).

### User Story 2 - Per-file conditional overrides in CEL (Priority: P2)

Within one profile, the operator expresses **content-conditional** policy without code:
"if the source is 4K, target AV1 at a lower quality"; "if there is a TrueHD 7.1 track,
transcode audio to E-AC3"; "if the bitrate is already low, skip". Rules layer over the
base profile and are authored as CEL predicates plus (optionally CEL-computed) settings.

**Why this priority**: It replaces the imperative branching that pushed Unmanic/Tdarr/
FileFlows toward plugins/graphs, but stays declarative, portable and single-pass. It is P2
because US1 must exist first (there is nothing to override without the richer base), and
because a profile with zero rules (the common case, including the reference operator's own)
is fully functional without it.

**Independent Test**: Given a base profile plus one `[[profiles.X.rule]]`, plan a file that
matches the `when` and one that doesn't; assert the override is applied only to the match,
and that the effective profile equals base-plus-overrides.

**Acceptance Scenarios**:

1. **Given** `when = "video.height >= 2160"` / `set = { "video.codec" = "av1" }`, **When** a
   2160p file is planned, **Then** the effective codec is AV1; a 1080p file stays at the base
   codec.
2. **Given** `set = { "video.quality.value" = "video.height >= 2160 ? 24 : 22" }`, **When**
   planned, **Then** the quality value is computed per file from the probe.
3. **Given** two rules whose `when` both match, **When** planned, **Then** they layer in a
   defined, documented order (first-match / ordered-layering) with a deterministic result.
4. **Given** `when = "audio.exists(a, a.codec == 'truehd')"`, **When** a file with a TrueHD
   track is planned, **Then** the audio-transcode override applies.

### User Story 3 - Fail-fast, safe evaluation (Priority: P3)

Invalid profile logic is caught at **config load**, not per file at runtime, and rule
evaluation can never hang, mutate state, or perform I/O.

**Why this priority**: Correctness/operability guardrail. It protects Principle I (config is
the source of truth) and the reconcile version-gate (which assumes planning is a pure
function of probe+profile). Lower priority only because US1/US2 deliver the visible value.

**Independent Test**: Load a config whose `when`/`set` references an unknown field or wrong
type → startup fails with a clear error and the daemon does not start. Evaluate a rule twice
on the same probe → identical result.

**Acceptance Scenarios**:

1. **Given** `when = "video.codc == 'h264'"` (typo), **When** the config loads, **Then**
   startup fails naming the bad expression; the last-good deployment keeps running.
2. **Given** any rule, **When** evaluated against a probe, **Then** it terminates and yields
   the same result every time (pure, deterministic).

### Edge Cases

- A `when` or `set` expression that is syntactically valid but type-mismatched (comparing a
  string to a number) → rejected at load.
- A rule `set`s a field to an out-of-range **static** value (e.g. `quality.value = 99`) →
  rejected at load. A **CEL-computed** value whose range depends on the file (e.g.
  `"... ? 24 : 99"`) → the load-time canary catches it only if the canary probe hits the bad
  branch; otherwise it fails per-file at reconcile (FR-015), never silently.
- Multiple overlapping rules (see US2 #3) → deterministic ordered layering, documented.
- `bit_depth`/`encoder` today live in the profile but are hardware-specific — they MUST NOT
  make a profile non-portable (see FR on hardware-agnostic profiles).
- A source with no audio at all → the "keep ≥1 audio" failsafe is vacuous, not an error.
- VMAF mode requested but unsupported in this release → see Assumptions.

## Requirements *(mandatory)*

### Functional Requirements — Profile coverage (US1)

- **FR-001**: The profile MUST express video `quality` as a **mode discriminator**
  (`qp | crf | bitrate | vmaf`) with a value, and MUST accept the bare-int shorthand
  `quality = N` for backward compatibility with existing `scheduler.toml` files.
- **FR-002**: The profile MUST support video `preset` (encoder speed/efficiency),
  `max_resolution` (downscale only when the source is larger), and `crop`.
- **FR-003**: The profile MUST provide a `custom_args` raw-ffmpeg escape hatch for filters
  the declarative schema does not model (denoise/deinterlace/fps/etc.), so those are NOT
  first-class fields in this feature. `custom_args` MUST be injected into the **single**
  ffmpeg command (the accumulator), never a chained/second invocation — preserving
  Constitution III (single-pass). Two-pass-only flags (`-pass 2`) are unsupported.
- **FR-004**: The profile MUST support general audio `transcode` (target codec + bitrate)
  of the **kept** tracks — not only the generated stereo clone — plus `add_mono`,
  `max_channels`, and `normalize`.
- **FR-005**: The profile MUST support subtitle `order` (fixed positioning), `forced_only`,
  and `extract` (to sidecar), in addition to the existing keep/remove rules.
- **FR-006**: The profile MUST support skip gates `skip_if_resolution_below` and
  `skip_if_bitrate_below`. (Skip-if-already-processed is already provided by the
  coordinator's `mtime:size` version-gate and is NOT re-implemented here.)
- **FR-007**: The profile MUST support output `conform` (drop streams the target container
  cannot hold), `strip_metadata`, and `keep_chapters`.
- **FR-008**: The engine MUST guarantee that stream/language filtering never yields a file
  with **zero audio streams** (a "keep at least one" failsafe); this is an engine invariant,
  not a configurable field.
- **FR-009**: Every profile MUST remain **hardware-agnostic**: the same profile MUST produce
  an equivalent `FilePlan` regardless of which backend materializes it — verifiable on the two
  implemented backends (VAAPI, CPU); NVENC is added in spec 003. Hardware-specific concerns
  (device, `sei` workaround) live in `worker.toml`; `encoder` selection stays in the profile for
  now (its move to the worker is spec 003).

### Functional Requirements — CEL conditional overrides (US2)

- **FR-010**: A profile MUST accept an ordered list of conditional override rules, each with
  a `when` predicate and a `set` map of profile-field overrides
  (`[[profiles.<name>.rule]] when = "<CEL>" set = { "<dotted.field>" = <value|CEL> }`).
- **FR-011**: Rule resolution MUST layer overrides over the base profile into a single
  **effective profile** BEFORE the planner runs, preserving "one `FilePlan` per file"
  (Constitution III). The layering order MUST be deterministic and documented
  (first-match / ordered).
- **FR-012**: CEL MUST be usable in BOTH the `when` predicate AND `set` values (computed
  settings), evaluated against a **documented, versioned context contract**: `video`
  (codec, width, height, bitrate, hdr, color_transfer, bit_depth), `audio[]`
  (index, codec, language, channels, title, default), `subtitles[]`
  (index, codec, language, forced), and file-level `path`, `container`, `duration`, `size`.
  This context contract is a public, stable API surface (see `contracts/cel-context.md`).

### Functional Requirements — Safety & validation (US3)

- **FR-013**: All CEL expressions MUST be **statically validated at config load** — compiled
  (syntax) and canary-evaluated against a synthetic full-context probe (unknown fields, obvious
  type mismatches) — and a failure MUST stop startup (fail-fast, consistent with `garde`).
  NOTE (honest limit): `cel-rust` has no full static type-checker, so this catches
  statically-detectable errors, **not** every possible runtime type/range outcome (see FR-015).
- **FR-014**: CEL evaluation MUST be **pure and guaranteed-terminating** — no I/O, no
  unbounded loops, no mutation — so planning stays a pure function of `(probe, profile)` and
  the reconcile version-gate remains valid.
- **FR-015**: A rule `set` value MUST be validated against the target field's rules (e.g.
  `quality.value` range). A **static** value is validated at load. A **CEL-computed** value can
  only be range-checked **per file** after resolution; a violation MUST fail *that file's*
  reconcile with a logged, structured error (treated as an invalid plan → the file is skipped,
  not transcoded, and never silently mis-encoded) — it MUST NOT crash the daemon or affect
  other files.

### Governance requirement

- **FR-016**: **RESOLVED.** Constitution Principle V was amended to **v2.0.0** ("bounded
  declarative extensibility; no runtime plugin host"), a *bounded* widening that permits a
  pure config-expression language (CEL) and the two versioned seams while keeping thin,
  single-pass, no-UI. This spec's CEL use is compliant. The implementation MUST NOT exceed
  that bounded surface (no visual graph/editor, no bespoke UI, no dynamic-ABI/WASM host).

### Key Entities

- **Profile**: the declarative transcode policy for a library — video/audio/subtitles/output
  sections. Extended here with the FR-001..007 fields. Hardware-agnostic (FR-009).
- **ProfileRule**: an ordered `{ when, set }` conditional override attached to a profile.
- **QualityMode**: a discriminator `{ mode, value }` replacing the bare quality int.
- **CEL context contract**: the stable set of probe/file variables CEL sees (FR-012); the
  same contract a future planner seam would consume.
- **Effective profile**: base profile with matching rule overrides layered in (FR-011) — the
  single input the planner actually plans against.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A transcode policy that today requires editing engine code or is inexpressible
  can be expressed entirely in `scheduler.toml` for **100%** of this table-stakes checklist:
  `preset`, `max_resolution`, `crop`, `custom_args`, `quality` mode (qp/crf/bitrate),
  `audio.transcode`, `add_mono`, `max_channels`, `normalize`, subtitle `order`/`forced_only`/
  `extract`, `skip_if_resolution_below`, `skip_if_bitrate_below`, `output.conform`/
  `strip_metadata`/`keep_chapters`.
- **SC-002**: The **same** profile, unchanged, produces an equivalent `FilePlan` regardless of
  which of the **currently implemented** backends (VAAPI, CPU) materializes it — verified by
  planning the same fixture under both. (NVENC portability is validated in spec `003`, which
  adds that backend; this feature does not add it.)
- **SC-003**: Per-file content branching (≥4 distinct cases, e.g. 4K→AV1, TrueHD→E-AC3,
  low-bitrate→skip, short→fast-preset) is achievable with **zero lines of code** — config
  only.
- **SC-004**: **Statically-detectable** malformations (unknown field, syntax error, type
  mismatch, out-of-range *static* `set`) are caught at config load in **100%** of cases. A
  range violation from a **CEL-computed** `set` value (only knowable per file) fails that file
  loudly at reconcile (FR-015) — never a silent mis-encode.
- **SC-005**: Rule evaluation is deterministic — identical `(probe, profile)` yields an
  identical effective profile every run (no non-determinism, no I/O).
- **SC-006**: Existing `scheduler.toml` files continue to load unchanged (`quality = N`
  shorthand honored) — zero-migration upgrade.

## Assumptions

- **Reference planner unchanged**: `apsis-engine` remains the default planner and owns all
  plan *construction*; this feature only widens what a profile *declares* + adds a pre-planner
  override-resolution step. No new planner, no runtime code plugin.
- **Out of scope (separate specs)**: post-transcode hooks, the pluggable planner seam, and
  the `apsis-engine`→`apsis-planner` rename. Filters beyond crop (denoise/deinterlace/fps/
  burn-in/strip-DoVI) are out of scope — covered by `custom_args`.
- **VMAF mode**: `quality.mode = "vmaf"` (AutoCRF, trial-encodes) is accepted in the schema
  but MAY be deferred to a later release; `qp`/`crf`/`bitrate` are in scope now.
- **CEL now (decided)**: the design commits to CEL directly — not a fixed-matcher-first
  fallback — enabling the unified `when`-predicate + computed-`set`-value API (FR-012). The
  Rust `cel-interpreter` crate is less mature than CEL-in-Go; this is an **accepted risk**. A
  fixed structured matcher (`when = { video_codec = "h264", resolution_gt = "1080p" }`, keys
  ANDed, no computed `set` values) remains a documented **contingency** if the crate proves
  unfit during implementation — same FR-012 context contract — but it is not the default path.
- **Probe surface**: FR-012's context assumes the probe carries width/height/bitrate/
  color_transfer/forced — some may need to be added to the probe parser as part of this work.
- The reference operator's own four libraries need only the US1 fields `add_mono` + subtitle
  `order`; they define **no** rules — validating that US2 is optional and US1 stands alone.
