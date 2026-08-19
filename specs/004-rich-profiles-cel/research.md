# Phase 0 Research: Rich profiles + CEL

Decisions that resolve the plan's unknowns. Format: **Decision / Rationale / Alternatives**.

## R1 — CEL implementation crate

**Decision**: `cel-interpreter` (crate `cel-interpreter`, the `clarkmcc/cel-rust` project). MIT
licensed → provenance-clean (Constitution: permissive/MIT). Compile an expression to a `Program`
at config load; evaluate against a `Context` populated per-file from the probe.

**Rationale**: it is the maintained Rust CEL implementation, supports custom variables/functions
and the standard comprehension macros (`exists`, `all`, `filter`, `map`, `exists_one`), and is a
pure library (no I/O), matching the purity requirement.

**Alternatives**: `cel-go` via FFI (rejected — Go runtime, cgo, provenance/thin cost); a bespoke
expression parser (rejected — the half-DSL trap, non-standard); a fixed structured matcher
(kept as the **R6 contingency**, not the default).

## R2 — Fail-fast validation of CEL (honest limitation)

**Decision**: validate every profile CEL expression **at config load** in two passes: (1)
**compile** the expression (catches syntax errors); (2) **canary-evaluate** it against a
synthetic probe context where every documented variable is present with a typed sample value —
surfacing unknown-field and obvious type errors before startup completes. A load failure stops
the daemon (garde-consistent), last-good keeps running.

**Rationale**: `cel-rust` does **not** ship a full static type-checker like `cel-go`'s
`cel.Check`. Parse-at-load + canary-eval is the pragmatic equivalent that satisfies FR-013 for
the realistic error classes (typos, wrong-typed comparisons, unknown fields).

**Alternatives**: full static type-check (not available in cel-rust — would require porting
cel-go's checker); validate lazily per-file (rejected — violates fail-fast; an error would only
surface mid-reconcile). **FR-013 refinement**: "type-checked at load" is delivered as
compile + canary-eval, not full static typing — recorded as a known limitation.

## R3 — `quality` as a mode + hardware-agnostic resolution

**Decision**: `quality = { mode, value }` with `mode ∈ {qp, crf, bitrate, vmaf}`; the bare
`quality = N` shorthand deserializes to `{ mode = "auto", value = N }`. The plan carries the
mode+value abstractly; the **backend** maps it at command-build time: VAAPI `auto`/`qp` → CQP
`-qp`; CPU `auto`/`crf` → `-crf`; `bitrate` → `-b:v`; `vmaf` → AutoCRF (deferred, R5).

**Rationale**: keeps the profile **hardware-agnostic** (FR-009) — the same `quality = 22` yields
QP-22 on VAAPI and CRF-22 on CPU because the encoder-specific mapping lives in the per-worker
backend, not the profile. `auto` preserves today's behaviour exactly (back-compat, SC-006).

**Alternatives**: a single int (today — rejected, per-encoder semantics differ); mode fixed in
the profile (rejected — would make `qp` profiles non-portable to NVENC).

## R4 — Probe fields the CEL context needs

**Decision**: `probe.rs` already carries video `width`, `height`, `color_transfer` (and
`Probe::is_hdr()`). The genuinely **missing** fields to add to the parser/`StreamInfo` are:
video `bitrate` and `bit_depth`, audio `title`, subtitle `forced`. All are present in the
ffprobe JSON. `hdr` in the CEL context is derived by reusing `Probe::is_hdr()`, which tests the
**raw** ffprobe transfer values `{"smpte2084","arib-std-b67"}` (NOT the display names PQ/HLG) —
the CEL context contract exposes the raw `color_transfer` string, so `hdr` and any user
`color_transfer ==` comparison agree on the same raw values.

**Rationale**: the CEL context contract (contracts/cel-context.md) is only as rich as the probe.
These are the minimum fields to express the spec's example rules (resolution, HDR, bitrate,
TrueHD-by-codec, forced subs).

**Alternatives**: expose raw ffprobe JSON to CEL (rejected — unstable, unversioned contract);
add fields lazily (rejected — the context contract must be stable/complete up front).

## R5 — VMAF / AutoCRF

**Decision**: accept `mode = "vmaf"` in the schema (forward-compat) but **defer implementation**
to a later spec; loading a profile with `mode = "vmaf"` is valid config, and the backend returns
a clear "not yet supported" error at command-build until implemented.

**Rationale**: AutoCRF needs iterative trial-encodes — a materially larger, worker-side feature
out of proportion to this Profile-scoped spec. Reserving the enum value avoids a later
config-breaking change.

**Alternatives**: implement now (rejected — scope); omit the enum value (rejected — a later add
would be a config-breaking change to the `mode` set).

## R6 — Override layering semantics

**Decision**: rules are an **ordered list**; resolution walks them **top-to-bottom** and, for
each rule whose `when` is true, merges its `set` map over the accumulating effective profile —
**last-write-wins per dotted field** (`"video.codec"`, `"video.quality.value"`). The base profile
is the initial accumulator. The final effective profile is validated (R2 field rules) before
planning.

**Rationale**: ordered layering (not first-match-only) lets one file match several rules that set
*different* fields (e.g. one rule sets codec, another sets audio) — the common real pattern — and
keeps a deterministic, documented result (SC-005). Mirrors the familiar kustomize/patch model.

**Alternatives**: first-match-wins only (rejected — can't compose orthogonal overrides); a
priority/weight system (rejected — over-engineered; order in the file is sufficient and legible).

## R7 — Where resolution runs

**Decision**: `resolve_effective_profile(base, rules, probe) -> Profile` is a **pure function in
`apsis-engine`**, called by the **coordinator's reconcile** immediately before `plan()`. Workers
never see rules and never re-plan (Constitution III). CEL `Program`s are compiled once at config
load and reused across files.

**Rationale**: the coordinator is the single decision point; it already has the probe; the result
is one effective profile feeding the one `plan()` call → one `FilePlan`. Keeping the resolver in
`apsis-engine` keeps it pure and unit-testable without NATS.

**Alternatives**: resolve in the worker (rejected — workers re-planning violates III); resolve in
`apsis-common` (rejected — it's planning logic, belongs with the planner).

## R8 — "keep ≥1 audio" failsafe

**Decision**: after audio filtering in the engine, if the kept-audio set is empty **and** the
source had ≥1 audio stream, retain the highest-priority source track (by the profile's `priority`
/ `default_language`, else the first). An engine invariant, not configurable (FR-008).

**Rationale**: prevents the silent-muted-file footgun the prior-art survey flagged (no tool has
this guard). Cheap, always-correct.

**Alternatives**: make it configurable (rejected — there is no legitimate "leave it muted"
policy); warn only (rejected — the damage is already done at replace time).
