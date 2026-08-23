# Data model: control & introspection

The Rust types this feature adds/changes. Wire shapes are in
[contracts/control-subjects.md](./contracts/control-subjects.md).

## New — `apsis-common::control`

The message structs (`PauseIntent`, `CancelRequest`/`CancelReply`, `StateControlRequest`/
`StateControlReply`, `ProgressEvent`) and their enums (`PauseScope`, `PauseMode`,
`Disposition`, `StateOp`, `CancelOutcome`). Plain `serde` `Serialize`/`Deserialize`; the
request/response types need no `deny_unknown_fields` (forward-compatible additions expected).
Subject constants (`SUBJECT_CONTROL_PAUSE`, `SUBJECT_CONTROL_CANCEL`, `SUBJECT_CONTROL_STATE`,
`subject_progress(job_id)`).

## Changed — `apsis-common::schema::StateEntry`

Add `#[serde(default)] pub decision: Option<Decision>`. **Additive & safe**: `StateEntry` is
one of the top-level structs that deliberately omit `deny_unknown_fields`, so an old consumer
reading a new entry ignores the field, and a new consumer reading an old entry defaults it to
`None`. It MUST NOT embed `PlanReason`/`FilePlan` (those are `deny_unknown_fields`).

```rust
/// Why the coordinator marked a file Done/skip — the *positive* reason (unlike the engine's
/// `reasons`, which is empty for compliant files). A new permissive type on purpose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub kind: DecisionKind,      // compliant_codec | resolution_below | bitrate_below | changes_required
    pub detail: String,         // e.g. "hevc", "480p", "1500k"
}
```

`Status` is **unchanged** — no new variant. An operator *ignore* is NOT a status; it is the
on-disk marker below.

## New — the on-disk ignore marker

A sidecar file `<media>.apsisignore` (empty; presence = ignore). Recoverable (git/FS-level,
survives a KV wipe — Principle I). A tiny helper in `apsis-common` (or the coordinator):
`has_ignore_marker(path) -> bool`, `set_ignore_marker(path)`, `clear_ignore_marker(path)`.
The discover/reconcile gate treats a marked source like a handled file (never reconciled).

## New — the pause control KV key

A single coordinator-owned key in the existing state KV bucket (e.g. `__control__/pause`)
holding the effective pause set:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PauseState {
    pub global: Option<PauseMode>,                 // None = not globally paused
    pub workers: BTreeMap<String, PauseMode>,      // per-worker overrides
}
```

Written only by the coordinator (on a `PauseIntent`); watched read-only by workers, which also
re-read it on reconnect. Ephemeral operational state — defaults to "not paused", so losing it
is safe (Principle I: no live-only *desired* state).

## Changed — `apsis-engine::plan`

`PlanOptions { pub force: bool }` (`Default`), threaded into `plan(...)`; `force` suppresses
`should_skip`. The `FilePlan` gains a positive `skip_reason: Option<SkipReason>` populated
where `should_skip` is decided:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SkipReason {
    CompliantCodec(String),     // already in skip_codecs
    ResolutionBelow(String),    // skip_if_resolution_below gate
    BitrateBelow(String),       // skip_if_bitrate_below gate
}
```

The coordinator maps `SkipReason` → `Decision` when persisting.

## Ownership summary

| State | Sole writer | Readers |
|-------|-------------|---------|
| `transcode_state` entries (incl. `decision`) | coordinator (reconcile + state ops, serialized) & worker (terminal writes) | operators (read-only), console |
| `__control__/pause` | coordinator | workers (watch), operators (read) |
| `<media>.apsisignore` | worker (on ignore) / coordinator (state ops) | reconcile gate |
| `apsis.progress.<job>` | worker | anyone |
