# Research — Single-node reconcile & safe transcode (Phase 0)

Decisions that resolve the plan's open points. Each: **Decision / Rationale / Alternatives**.

## D1 — Durable substrate: NATS JetStream, single-node

**Decision.** One `nats-server` with JetStream on rhea. A **job stream** (WorkQueue
retention) carries transcode jobs; a **KV bucket** `transcode_state` holds per-file state;
a **durable pull consumer** delivers jobs to the worker with `AckWait` + `MaxDeliver`.

**Rationale.** The spec's hard parts — durable queue (FR-001/010), worker lease +
crash-redelivery (US3/SC-003), retry-to-failed (FR-009), version-gated change cache
(FR-005) — are native JetStream semantics, not things to hand-roll:
- `AckWait` → the lease: a job in flight is redelivered if not `ack`ed in time (worker
  crash → automatic retry; the temp file is discarded, the original never touched).
- `MaxDeliver` (+ optional `backoff`) → retry-with-limit then **dead-letter** → we mark
  the file `failed@version` in KV (FR-009).
- KV `transcode_state` → the dedup/change cache (FR-005) and observability/history without
  a separate DB.
Using it now makes spec 003 (distribution) purely additive.

**Alternatives rejected.** `apalis`+SQLite (a full durable job queue) duplicates the
reconcile loop's idempotency and is pre-1.0; a bespoke `tokio` mpsc + SQLite/redb state
store reimplements AckWait/MaxDeliver/KV and gets thrown away when NATS arrives in 003.
Both cost *more* total code to *defer* one `nats-server` process.

**Recoverability (constitution).** Desired state = git config; actual = filesystem; KV =
re-derivable cache. Total NATS loss → the coordinator rescans and re-enqueues only true
drift; at worst one redundant probe pass, never a re-transcode of compliant media. Stream +
KV + consumer config are declared in git (`apsis-common` setup, idempotent create-if-absent).

## D2 — Deploy the NATS server as its own unit

**Decision.** Run `nats-server` as a standalone service (its own LXC/container on rhea, or a
NixOS service on a NixOS guest), **not** embedded in the coordinator binary. JetStream file
store on a persistent path.

**Rationale.** Keeps apsis binaries stateless and restartable; lets the worker (and later
sirius) connect to the same endpoint; matches how spec 003 will expose it over NetBird. An
embedded server would tie NATS lifetime to the coordinator and complicate the 003 split.

**Alternatives.** Embedded/in-process NATS (Go only; not available to a Rust binary anyway)
— rejected. Config file for the server is git-tracked; the placement (which LXC) is a
deployment detail settled at rollout, not a code dependency.

## D3 — Output verification (the gate before replace)

**Decision.** After ffmpeg exits 0, verify the temp with `ffprobe` before any replace:
1. **Expected streams present** — the video stream and every planned audio/subtitle output
   track exist (count + codec matches the plan).
2. **Duration within tolerance** — `format.duration` within ±1s (configurable) of the
   source, guarding truncation.
3. **Not truncated / playable** — ffprobe exits 0 and the last packet's pts is near the end
   (`-read_intervals`/`-show_packets` tail check), catching a cut-off encode ffmpeg still
   exited 0 on.
4. **Size sane** — output > 0 and not absurdly larger than input (configurable ceiling;
   larger-than-input → fail per the spec edge case).
Any check fails → discard temp, `nak`/dead-letter → the file stays as-is, job failed.

**Rationale.** ffmpeg's exit code alone is not sufficient (it can 0-exit on a truncated
mux). Stream+duration+tail is the cheap, high-signal set Unmanic's success plugins also use.

**Alternatives.** Full decode re-scan (`-f null -`) is stronger but O(duration) expensive on
every job — deferred to an optional `deep_verify` flag; the tail-packet check catches the
common truncation without a full decode.

## D4 — Atomic replace + metadata preservation

**Decision.** Write the temp in the **same directory** as the source (guarantees same
filesystem → atomic `rename`). On verify success: `fsync` the temp, `fsync` the dir,
`rename(temp, original)` (or rename original→`.bak`, temp→original, unlink `.bak` if the
extension changes), then restore `stat(2)` — owner/group, mode, mtime/atime — onto the new
file. On any failure or crash before the rename, unlink the temp; the original is intact.

**Rationale.** `rename(2)` within a filesystem is atomic — no window where the path is
missing or half-written. The design calls out that Unmanic's core does **not** preserve
stats (`shutil.copyfile`) and leaves it to a plugin; apsis bakes it into core so library
indexers (Jellyfin/Sonarr) see unchanged ownership/timestamps. Temp files use a distinctive
prefix (`.apsis-tmp-<ulid>`) so a crashed run's leftovers are recognizable and swept.

**Alternatives.** Copy-to-temp-elsewhere + move across filesystems is non-atomic (a copy
window) — rejected. Extension change (e.g. `.avi`→`.mkv`) means the original path differs
from the output path; handled by writing the new path then unlinking the old original **only
after** the new file is verified and fsynced.

## D5 — Discovery: inotify + periodic walk + debounce

**Decision.** `notify` (inotify) for near-real-time events, plus a periodic full walk as the
backstop (events can be missed on NFS / at startup). Both funnel into the same reconcile
body. **Debounce (FR-002):** a path is skipped this pass while its `mtime:size` changed
within a configurable window (default 60s) — i.e. still being written by Sonarr/Radarr —
and reconsidered once stable. Non-video extensions and sidecars (`.nfo/.srt/.jpg`) filtered
before any probe.

**Rationale.** inotify alone is unreliable over NFS and misses pre-existing files; the walk
guarantees eventual convergence. Debounce prevents transcoding a half-copied import.

**Alternatives.** Pure polling (Unmanic's default) re-probes everything — rejected by FR-005.
Pure inotify — rejected (misses/NFS). fanotify — heavier, no benefit here.

## D6 — Concurrency bound

**Decision.** A `tokio::sync::Semaphore` in the worker caps concurrent ffmpeg jobs
(configurable; **default 1** for AMD VCN HEVC, which is effectively single-session). The
JetStream consumer's `max_ack_pending` mirrors the bound so undelivered jobs stay queued.

**Rationale.** AMD VCN serializes HEVC encode; over-subscribing just thrashes. One knob,
enforced in two places (local semaphore + consumer pending) so neither the worker nor the
broker over-commits.

## D7 — Metrics & logs (no UI)

**Decision.** `metrics` + `metrics-exporter-prometheus` exposing: `apsis_queue_depth`,
`apsis_jobs_in_flight`, `apsis_jobs_total{outcome}`, `apsis_transcode_seconds` (histogram),
`apsis_bytes_saved_total`, `apsis_used_fallback_total`, `apsis_verify_failures_total`,
`apsis_reconcile_seconds`. `tracing` spans per job (job_id, path, backend, outcome). Scraped
by the existing vmagent; dashboards in the hub Grafana.

**Rationale.** Reuses the homelab monitoring stack (constitution: thin by reuse); satisfies
FR-011/SC-005 without a bespoke web UI.

## D8 — Testing strategy

**Decision.** Logic (reconcile decisions, verify rules, replace/stat, debounce, profile
match) unit-tested behind `Queue`/`StateStore` traits with in-memory fakes — no broker
needed. A gated integration suite runs against a real `nats-server` to assert the JetStream
behaviors that can't be faked meaningfully: `AckWait` redelivery on a dropped ack, and
`MaxDeliver` → dead-letter → `failed@version`. Transcode/verify tested with tiny generated
sample clips (ffmpeg `testsrc`/`sine`), asserting the safe-replace and fallback paths.

**Rationale.** Keeps the fast unit layer broker-free (CI-friendly) while still pinning the
few semantics that only a real JetStream exercises. Sample clips make US1/US3 reproducible.

**Alternatives.** Mock the whole NATS API surface — brittle and would let a wrong AckWait
assumption pass; rejected in favor of a real server for the handful of protocol tests.

## Verified facts (2026-08 crate stack)

- `async-nats` 0.50 — JetStream context (`jetstream::new`), `Context::create_stream`,
  KV via `jetstream::kv` (`create_key_value`, `entry`/`put`/`update` with revision CAS),
  durable pull consumers with `ack_wait`, `max_deliver`, `backoff`, `max_ack_pending`.
- `notify` — cross-platform; inotify on Linux; a `RecommendedWatcher` + a manual walk cover
  D5.
- `tokio::process::Command` with `-progress pipe:1` gives line-parsed ffmpeg progress.
- KV `update(key, val, revision)` is the compare-and-set that makes the `pending` claim
  (FR-005) race-free between the reconcile pass and a concurrent inotify event.
