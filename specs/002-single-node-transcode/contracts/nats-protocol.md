# Contract — NATS JetStream protocol (single-node)

The coordinator↔worker contract. All names are constants in `apsis-common`; all resources
are created **idempotently** on startup (create-if-absent), so the config is reconstructable
from git and a cold NATS is self-provisioning.

## Stream `APSIS_JOBS`

```
name:      "APSIS_JOBS"
subjects:  ["jobs.transcode.>"]
retention: WorkQueue          # a job is removed once acked → the queue drains, no rebuild
storage:   File
discard:   Old
max_age:   0                  # jobs persist until acked (crash-safe)
```
WorkQueue retention means an acked job leaves the stream — the stream *is* the backlog of
outstanding work, not a log. Exactly one consumer per subject may bind (enforced by NATS).

## Subjects

| Subject | Direction | Payload | Notes |
|---------|-----------|---------|-------|
| `jobs.transcode.local` | coordinator → worker | `Job` (JSON) | single-node; 003 adds `jobs.transcode.{vaapi,nvenc}` for routing |
| `jobs.result` | worker → coordinator/metrics | `TranscodeResult` (JSON) | core publish (not in the work stream); coordinator folds it into metrics + a completion log (worker owns the terminal KV write) |

## KV bucket `transcode_state`

```
bucket:   "transcode_state"
history:  1
storage:  File
key:      the file's coordinator-space path
value:    StateEntry (JSON)
```
- **Claim (CAS):** coordinator writes `Pending` with `update(key, value, revision)`; a stale
  revision (another pass/event already claimed) fails the CAS → skip. This is the FR-005
  race guard.
- **Change gate:** before probing, the coordinator reads the entry; if `version` matches the
  file's current `mtime:size` and `status ∈ {done, pending, in_progress, failed}`, it
  short-circuits (no probe, no enqueue).
- **Terminal write (worker-owned).** The implementation converged on the **worker** writing
  the terminal `Done`/`Failed@version` in `finish()` — including `Failed@version` on
  dead-letter — so terminal state never depends on the coordinator being alive (crash-safe).
  It is the single terminal writer, version-guarded against a newer claim.
- **Fold result (observability).** The worker also publishes `TranscodeResult` to `jobs.result`;
  the coordinator subscribes and folds it into metrics (`apsis_results_total{outcome}`) + a
  structured completion log — history without a separate DB. This is **not** a second KV
  writer; it never touches `transcode_state`.

## Consumer `worker-local`

```
name:            "worker-local"
type:            durable pull
filter_subject:  "jobs.transcode.local"
ack_policy:      Explicit
ack_wait:        30m               # > longest expected transcode; lease before redelivery
max_deliver:     4                 # attempts, then terminate → dead-letter (→ Failed@version)
backoff:         [1m, 5m, 15m]     # between redeliveries
max_ack_pending: <worker.concurrency>   # broker-side mirror of the local Semaphore
```

### Delivery semantics (the safety-critical part)

- **In-flight lease.** The worker `ack`s **only after** verify + atomic replace succeed.
  While transcoding, it periodically `in_progress`-heartbeats (extends `AckWait` via
  `ack_wait`/`working`) so a legitimately long encode isn't redelivered.
- **Crash → redeliver.** Worker dies mid-transcode → no `ack` → after `AckWait` the job is
  redelivered. The temp file (`.apsis-tmp-<ulid>`) is discarded on the retry; the original
  was never touched. (US3 / SC-003.)
- **Poison → dead-letter.** On the `max_deliver`-th delivery the **worker** itself writes
  `Failed@version` with `last_error` and acks (drains the message) — it doesn't wait for a
  broker `term`/advisory. It is not retried until the file's `mtime:size` changes (FR-009).
- **retriable vs terminal.** A *retriable* failure (transient I/O; VAAPI glitch not already
  handled by the in-job CPU fallback) → `nak` (redeliver, subject to `backoff`/`max_deliver`).
  A *terminal* failure (verify says the source is unencodable, output larger than input) →
  write `Failed@version` to KV, then **`ack`** to remove it from the WorkQueue (no wasted
  retries). Note: `async-nats` 0.42's `AckKind` has no `Term`; ack-after-recording is the
  terminal path, and `max_deliver` still dead-letters a job that keeps `nak`-ing.

## Message schemas (JSON)

### `Job` → `jobs.transcode.local`
```json
{
  "id": "01J...ULID",
  "path": "/hdd/media/tv/Show/S01E01.mkv",
  "version": "1723800000:1048576000",
  "profile": "tv",
  "plan": { "...": "apsis_engine::FilePlan" },
  "enqueued_at": "2026-08-17T10:00:00Z"
}
```

### `TranscodeResult` → `jobs.result`
```json
{
  "job_id": "01J...ULID",
  "path": "/hdd/media/tv/Show/S01E01.mkv",
  "version": "1723800000:1048576000",
  "outcome": "done",
  "used_fallback": false,
  "input_bytes": 1048576000,
  "output_bytes": 524288000,
  "duration_secs": 412.3,
  "error": null
}
```

### `StateEntry` (KV value)
```json
{
  "status": "in_progress",
  "version": "1723800000:1048576000",
  "job_id": "01J...ULID",
  "attempts": 1,
  "used_fallback": false,
  "updated_at": "2026-08-17T10:00:05Z",
  "last_error": null
}
```

## Invariants

1. A `Job` is published **only** after a successful KV `Pending` CAS — no unclaimed jobs.
2. The worker mutates the original **only** between a passing verify and its `ack`.
3. Every terminal KV state carries the `version` it applies to; a new `mtime:size`
   supersedes it. No terminal state is permanent across a real file change.
4. All stream/KV/consumer objects are create-if-absent → a cold NATS + git config
   self-provisions; nothing to restore by hand.
