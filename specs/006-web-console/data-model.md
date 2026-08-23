# Data model: apsis-web view types

The view types that cross the server-function boundary (all `serde`, in `apsis-web::view`).
They are **projections** of spec 005 data for rendering — not a new source of truth.

```rust
/// One row in the file table (from a KV `StateEntry` + the ignore marker).
#[derive(Clone, Serialize, Deserialize)]
pub struct FileRow {
    pub path: String,
    pub status: Status,                 // apsis_common::Status
    pub version: String,                // mtime:size token
    pub decision: Option<Decision>,     // apsis_common::Decision (why skipped)
    pub ignored: bool,                  // on-disk marker present
    pub job_id: Option<String>,         // set when InProgress (for cancel + progress)
}

/// The detail view for one file.
#[derive(Clone, Serialize, Deserialize)]
pub struct FileDetail {
    pub row: FileRow,
    pub last_error: Option<String>,     // from the StateEntry (e.g. a failed transcode)
    pub used_fallback: bool,
    // The plan is best-effort (only available while a Job is on the stream); v1 may omit it.
}
```

Reused verbatim from `apsis_common` (no re-definition): `Status`, `Decision`/`DecisionKind`,
`PauseState`/`PauseScope`/`PauseMode`, `Disposition`, `CancelOutcome`, `StateOp`, `StateOutcome`,
`ProgressEvent`. The console adds only the two projection structs above.

## Client-side reactive state (Leptos signals)

- `files: Resource<Vec<FileRow>>` — refetched on a ~3 s interval via `list_files`.
- `progress: RwSignal<HashMap<String, ProgressEvent>>` — keyed by `job_id`, fed by the
  `/progress` `EventSource`; the running row reads its entry for the progress bar.
- `pause: Resource<PauseState>` — for the global pause indicator.
- No client-side *source of truth*: every signal is a projection of a server read; actions
  re-fetch after applying.

## Ownership

| Data | Owner (unchanged from spec 005) | apsis-web role |
|------|--------------------------------|----------------|
| `transcode_state` entries + `decision` | coordinator / worker | **read** (list/detail) |
| `__control__/pause` | coordinator | **read** (indicator); set via a *pause intent* |
| `apsis.progress.<job>` | worker | **subscribe** (SSE relay) |
| control subjects | worker / coordinator | **publish** intents only |
