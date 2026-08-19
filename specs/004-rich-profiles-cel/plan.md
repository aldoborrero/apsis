# Implementation Plan: Rich transcode profiles with CEL conditional overrides

**Branch**: `004-rich-profiles-cel` | **Date**: 2026-08-18 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/004-rich-profiles-cel/spec.md`

## Summary

Widen what a `scheduler.toml` transcode profile can express — two things: (1) fill the
declarative coverage gaps the reference engine lacks (quality-as-mode, preset, resolution,
general audio transcode, `add_mono`, subtitle order/forced/extract, skip gates, container
conform, metadata) plus an engine-guaranteed "keep ≥1 audio" failsafe; (2) add per-file
**conditional overrides** authored in **CEL** — `[[profiles.X.rule]] when = "<CEL>" set = {…}` —
resolved into a single *effective profile* **before** the planner runs, so the "one `FilePlan`
per file" invariant holds. CEL is used in both the `when` predicate and (computed) `set` values,
evaluated against a versioned probe/file context contract, pure + type-checked at load. Scope is
the Profile only; hooks, the planner-replacement seam, and the `apsis-engine`→`apsis-planner`
rename are separate specs. Enabled by constitution **v2.0.0** (Principle V: bounded declarative
extensibility).

## Technical Context

**Language/Version**: Rust, edition 2024, rust-version 1.87 (workspace pin)

**Primary Dependencies**: existing — `serde`, `garde` (validation), `figment`+`toml` (config
loading). New — `cel-interpreter` (pure CEL evaluation; MIT). No other additions.

**Storage**: config files (`scheduler.toml`); no database. Runtime state unchanged (NATS KV).

**Testing**: `cargo test` — unit + golden-fixture tests in `apsis-engine` (probe→plan
assertions), plus config-load fail-fast tests. Follows the existing engine test style.

**Target Platform**: Linux daemons (`apsis-coordinator`, `apsis-worker`).

**Project Type**: single Rust workspace (crates under `crates/`).

**Performance Goals**: override resolution runs **only in the coordinator's reconcile**, only
for changed/new files (version-gate skips the rest) — off the transcode hot path. CEL is
compiled+type-checked once at config load; per-file evaluation is an AST walk over a small
activation. No measurable impact on reconcile latency.

**Constraints**: CEL MUST be pure + guaranteed-terminating (version-gate validity); config MUST
fail-fast at load (garde-consistent); existing `scheduler.toml` MUST load unchanged (back-compat
`quality = N` and `add_stereo.bitrate` int); every profile MUST stay hardware-agnostic — same
effective `FilePlan` on the two implemented backends **VAAPI and CPU** (NVENC is spec 003;
`encoder` stays in the profile for now).

**Scale/Scope**: a handful of libraries × a handful of profiles × a handful of rules each. Not
a hot loop.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Evaluated against constitution **v2.0.0**:

| Principle | Verdict | Note |
|-----------|---------|------|
| I. Declarative & git-recoverable | ✅ PASS | everything is `scheduler.toml`; overrides + CEL are config; no live-only state; a rescan re-derives everything. |
| II. Thin by reuse | ✅ PASS (justified) | exactly one new crate (`cel-interpreter`), no bespoke expression engine, no host/UI/transfer layer. See Complexity Tracking. |
| III. Single-pass, single source of truth | ✅ PASS | overrides resolve to **one** effective profile **before** the single planner; still exactly one `FilePlan` per file; workers don't re-plan. |
| IV. Correctness never sacrifices the library | ✅ PASS | no change to transcode/verify/replace; the "keep ≥1 audio" failsafe *strengthens* it. |
| V. Bounded declarative extensibility (v2.0.0) | ✅ PASS | CEL is the permitted pure config-expression language; no visual graph/editor, no bespoke UI, no dynamic-ABI/WASM host. Stays inside the bounded surface. |
| Observability & fail-fast | ✅ PASS | CEL type-checked at config load → invalid expression stops startup, last-good keeps running. |
| Licensing & Provenance | ✅ PASS | `cel-interpreter` is MIT (verify exact license in research); CEL the language is an open Google spec. |

**Result**: PASS. No unjustified violations. (The one dependency is tracked below.)

**Post-design re-check (after Phase 1)**: still PASS. The design keeps resolution pure and in the
coordinator (III intact — one `FilePlan`); adds no host/UI/graph (V intact); the CEL context is a
versioned contract, not raw ffprobe exposure (I intact); the only honest caveat surfaced is that
`cel-rust` lacks a full static type-checker, handled by compile + canary-eval at load (research R2)
— still fail-fast, no principle weakened.

## Project Structure

### Documentation (this feature)

```text
specs/004-rich-profiles-cel/
├── plan.md              # This file
├── research.md          # Phase 0 — decisions (CEL crate, quality-mode semantics, probe fields)
├── data-model.md        # Phase 1 — Profile/ProfileRule/QualityMode/effective-profile entities
├── contracts/
│   ├── cel-context.md   # the versioned CEL evaluation context (the "expression API")
│   └── profile-schema.md# the extended Profile config schema (the config API)
├── quickstart.md        # Phase 1 — worked example profiles (coverage + CEL rules) + validation
└── tasks.md             # Phase 2 — /speckit-tasks (NOT created here)
```

### Source Code (repository root)

```text
crates/
├── apsis-engine/                      # the reference planner (→ apsis-planner later; out of scope)
│   └── src/
│       ├── config.rs                  # EXTEND: Profile fields (quality-as-mode, preset, max_resolution,
│       │                              #         crop, custom_args, audio.transcode/add_mono/max_channels/
│       │                              #         normalize, subtitles.order/forced_only/extract,
│       │                              #         skip gates, output.conform/strip_metadata/keep_chapters);
│       │                              #         + ProfileRule { when, set }; QualityMode
│       ├── probe.rs                   # EXTEND: add the MISSING CEL fields (video bitrate + bit_depth,
│       │                              #         audio title, subtitle forced; width/height/color_transfer
│       │                              #         already exist) + Probe->CEL context builder
│       ├── overrides.rs               # NEW: resolve_effective_profile(base, rules, probe) -> Profile
│       │                              #      (CEL compile-at-load + per-file evaluate + layer + validate)
│       ├── plan.rs / audio.rs /       # EXTEND: construction for the new fields (add_mono, subtitle
│       │   subtitles.rs               #         order, conform, resolution, audio transcode) + failsafe
│       └── command.rs                 # EXTEND: materialize new plan fields into ffmpeg (preset/scale/
│                                      #         crop/audio-encode/metadata) per backend
└── apsis-coordinator/
    └── src/reconcile.rs               # WIRE: call resolve_effective_profile(base, rules, probe) before plan()

tests/  (in-crate #[cfg(test)] + golden fixtures under apsis-engine)
├── unit: Profile deserialization (new fields + back-compat), QualityMode, rule layering
├── golden: probe+profile(+rules) -> FilePlan assertions for each new capability
└── fail-fast: bad CEL / unknown field / out-of-range set -> config load error
```

**Structure Decision**: Single workspace, no new crate. The Profile schema + rule types + the
pure `resolve_effective_profile` all live in `apsis-engine` (planning-adjacent, pure, reusable as
a library — matching the crate's stated "usable as a standalone library" role). The coordinator's
`reconcile.rs` gains one call to resolve the effective profile before `plan()`. Override
resolution is deliberately **not** in the worker (workers never re-plan — Constitution III).

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|--------------------------------------|
| New dependency `cel-interpreter` (vs "thin") | The feature is an expression language for conditions/computed values across profile rules (and, later, hook filters). A pure, standard, off-the-shelf language beats a bespoke parser. | **Bespoke mini-DSL**: reinvents parsing/typing/eval — more code, non-standard, the "half-DSL" trap. **Fixed structured matcher**: no computed `set` values, less expressive; kept only as a contingency (research.md) if the crate proves unfit. Constitution v2.0.0 explicitly permits a pure config-expression language. |
