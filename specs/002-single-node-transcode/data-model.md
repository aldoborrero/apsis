# Data model — Single-node reconcile & safe transcode (Phase 1)

Types live in `apsis-common` (shared by coordinator + worker). JSON on the wire (design
choice: debuggable; MessagePack later if size matters). All timestamps are RFC3339 UTC.

## Config (desired state — TOML in git)

### `scheduler.toml` (coordinator)

```
Library      { name: String, path: String, profile: String,
               extensions: Vec<String> = video defaults }
Profile      = apsis_engine::Profile        # consumed as-is by the engine (spec 001)
Reconcile    { scan_interval: Duration = 5m, debounce: Duration = 60s,
               inotify: bool = true }
```
- Profile selection: **longest-matching** `Library.path` prefix wins (FR-003).
- `Profile` is exactly the engine's validated profile — no duplicate schema.

### `worker.toml` (worker)

```
Worker       { concurrency: u32 = 1,                     # FR-010 (AMD VCN → 1)
               path_map: Map<String,String> = {},         # coordinator path → local mount
               ffmpeg: PathBuf = "ffmpeg", ffprobe: PathBuf = "ffprobe",
               verify: VerifyConfig, backends: Vec<BackendConfig> }
VerifyConfig { duration_tolerance: Duration = 1s, max_size_ratio: f64 = 1.5,
               deep_verify: bool = false }
BackendConfig{ kind: "vaapi" | "cpu", device: Option<String>,   # renderD128 / cpu
               hardware: apsis_engine::HardwareConfig }          # reuse engine config
```
- `path_map` is identity on rhea; real on sirius (spec 003). Applied to `Job.path` →
  local path before probing/transcoding.
- Config load is **fail-fast** (`garde` validation); invalid config → refuse to start,
  last-good deployment keeps running (FR-012).

## Runtime entities

### `Job` — one file's transcode unit (stream message)

```
Job {
  id: Ulid,                       # sortable
  path: String,                   # coordinator-space absolute path
  version: String,                # "mtime:size" — the change token
  profile: String,                # library's profile name (for logging/routing)
  plan: apsis_engine::FilePlan,   # the abstract decision (engine output) — carries source_probe
  enqueued_at: DateTime,
}
```
The job carries the **abstract plan**, not an ffmpeg command — the worker materializes the
command for its own backend (VAAPI/CPU) via `apsis_engine::Backend`. (Design: jobs carry
metadata; media stays on the shared filesystem.)

### `StateEntry` — KV `transcode_state` value (key = `path`)

```
StateEntry {
  status: Unknown | Pending | InProgress | Done | Failed,
  version: String,                # "mtime:size" this state refers to
  job_id: Option<Ulid>,
  attempts: u32,                  # for FR-009 retry limit
  used_fallback: bool,            # last run used CPU fallback
  updated_at: DateTime,
  last_error: Option<String>,     # on Failed
}
```
- **Version-gated:** a state only blocks re-work while `version` matches the file's current
  `mtime:size`. A changed file supersedes `Done`/`Failed` → re-evaluated (state machine).
- **CAS:** the `Pending` claim is a KV `update(key, val, revision)` so a periodic-walk pass
  and a concurrent inotify event can't double-enqueue (FR-005).

### `TranscodeResult` — worker → result subject (and folded into KV)

```
TranscodeResult {
  job_id: Ulid, path: String, version: String,
  outcome: Done | Failed,
  used_fallback: bool,
  input_bytes: u64, output_bytes: u64,     # → apsis_bytes_saved_total
  duration_secs: f64,                      # wall-clock transcode time
  error: Option<String>,
}
```

## File state machine (mirrors design §7)

```
unknown --> done        : compliant (plan.should_skip)
unknown --> pending     : drift (plan says process)
pending --> in_progress : worker pulls (KV CAS)
in_progress --> done    : verified + atomically replaced
in_progress --> failed  : MaxDeliver hit or verify fail (dead-letter)
in_progress --> pending : AckWait lapse (worker crash → redeliver)
failed  --> pending     : file changes (new mtime:size)
done    --> pending     : file changes (new mtime:size)
```
`version = mtime:size` gates every exit from a terminal state.

## JetStream layout (constants in `apsis-common`)

| Object | Name | Config |
|--------|------|--------|
| Stream | `APSIS_JOBS` | subjects `jobs.transcode.>`; retention **WorkQueue**; file storage |
| Subject (single-node) | `jobs.transcode.local` | one worker consumes it (003 adds `.vaapi`/`.nvenc`) |
| Result subject | `jobs.result` | fire-and-forget event → metrics + KV fold |
| KV bucket | `transcode_state` | history=1, file storage; key = file path |
| Consumer | `worker-local` | durable **pull**; `ack_wait`, `max_deliver`, `backoff`, `max_ack_pending = concurrency` |

Full contract (message JSON, consumer settings, create-if-absent semantics) in
[contracts/nats-protocol.md](./contracts/nats-protocol.md).
