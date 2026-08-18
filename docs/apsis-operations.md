# apsis — operations runbook (single-node, spec 002)

apsis is a thin distributed media-transcode scheduler. In the single-node phase it runs two
daemons on one host over a local NATS JetStream:

- **`apsis-coordinator`** — walks the configured libraries, decides drift via the engine, and
  publishes transcode jobs (the reconcile loop).
- **`apsis-worker`** — pulls jobs, transcodes (VAAPI → CPU fallback), verifies, and atomically
  replaces the original.

State (the job stream + the `transcode_state` KV) lives in NATS JetStream; the desired state
(libraries + profiles) lives in git-tracked TOML. Nothing is live-only: a rescan re-derives
everything, so losing NATS costs at most one re-probe pass, never a re-transcode.

## Local dev / test

```bash
nix develop            # brings process-compose, nats-server, ffmpeg, the Rust toolchain
process-compose up     # starts a JetStream NATS on :4222 (monitoring :8222)
```

`process-compose.yaml` stages `coordinator` and `worker` (disabled by default — they need a
real library path). Run them by hand against the local NATS once you have config:

```bash
export NATS_URL=nats://127.0.0.1:4222
APSIS_SCHEDULER_CONFIG=./scheduler.toml cargo run -p apsis-coordinator
APSIS_WORKER_CONFIG=./worker.toml       cargo run -p apsis-worker
```

## Configuration

`scheduler.toml` (coordinator) — libraries + profiles + reconcile cadence:

```toml
[reconcile]
scan_interval = "5m"   # full walk cadence (inotify is a future add)
debounce      = "60s"  # skip files whose mtime is younger than this (still importing)

[[library]]
name    = "tv"
path    = "/hdd/media/tv"   # MUST be absolute
profile = "tv"

[profiles.tv.video]
codec       = "hevc"
skip_codecs = ["hevc", "av1"]   # a file already in one of these is compliant
# … audio / subtitles / output — see the engine's Profile
[profiles.tv.audio]
[profiles.tv.subtitles]
[profiles.tv.output]
container        = "mkv"
replace_original = true
```

`worker.toml` (worker) — backends + verify + path map:

```toml
concurrency  = 1          # AMD VCN HEVC is single-session
stall_timeout = "2m"      # kill ffmpeg if it emits no progress for this long

[verify]
duration_tolerance = "1s"
max_size_ratio     = 1.5   # reject a bloated / larger-than-input output

[consumer]              # JetStream lease/retry (optional; prod defaults shown)
ack_wait    = "30m"     # per-delivery lease; a long encode heartbeats within it
max_deliver = 4         # deliveries before dead-lettering Failed@version
backoff     = ["1m", "5m", "15m"]   # redelivery schedule; [] = immediate

[[backend]]              # first = primary
kind   = "vaapi"
device = "/dev/dri/renderD128"
[[backend]]              # second = fallback
kind = "cpu"

[path_map]              # coordinator path → this host's local mount (identity on rhea)
# "/hdd" = "/mnt/rhea-hdd"   # e.g. on sirius/WSL
```

Config is **fail-fast**: an invalid or typo'd config (unknown profile, relative library path,
`NaN` ratio, unknown key inside a profile) makes the daemon refuse to start, so the last-good
deployment keeps running.

## Environment variables

| Var | Default | Used by |
|-----|---------|---------|
| `NATS_URL` | `nats://127.0.0.1:4222` | both |
| `APSIS_SCHEDULER_CONFIG` | `scheduler.toml` | coordinator |
| `APSIS_WORKER_CONFIG` | `worker.toml` | worker |
| `APSIS_FFPROBE` | `ffprobe` | coordinator (probe) |
| `APSIS_METRICS_ADDR` | `0.0.0.0:9100` (coord) / `:9101` (worker) | both |

## Observability

Both daemons expose a Prometheus scrape endpoint at `APSIS_METRICS_ADDR`:

- worker `:9101/metrics` — `apsis_jobs_total{outcome}`, `apsis_transcode_seconds`,
  `apsis_used_fallback_total`, `apsis_bytes_saved_total`, `apsis_verify_failures_total`
- coordinator `:9100/metrics` — `apsis_reconcile_seconds`, `apsis_reconcile_enqueued_total`,
  `apsis_results_total{outcome}` (its view of worker completions, from the `jobs.result` feed)

Point vmagent at both; dashboards go in the hub Grafana.

**Structured logs** go to stderr via `tracing` (no ANSI). Level is `RUST_LOG` (default `info`);
each transcode runs inside a `job` span (`job_id`, `path`, `version`, `backend`, `outcome`) and
each reconcile inside a `reconcile` span (`path`, `version`, `outcome`), so a single job's events
share those fields. Example: `RUST_LOG=apsis_worker=debug` for verbose worker tracing.

## Safety model (why it won't corrupt the library)

- The worker writes the transcode to `.apsis-tmp-<ulid>` **beside** the source (same
  filesystem), verifies it (streams present, duration within tolerance, size sane), and only
  then installs it atomically. On any failure the original is byte-identical.
- **Source-changed-during-transcode is not a revert.** A new import can overwrite the source
  while a long encode runs; installing the now-stale output would silently drop the newer
  content. The same-container replace closes this atomically with `renameat2(RENAME_EXCHANGE)`
  — swap temp ↔ source, then check the content swapped *out* is the version we transcoded
  from; if not, swap back (original restored) and discard. Filesystems without the flag (NFS,
  older ZFS) fall back to a re-stat + rename with an unavoidable sub-ms window.
- Ownership, mode, and mtime are re-applied so Jellyfin/Sonarr see the file unchanged.
- A crash mid-transcode leaves the temp untouched-and-orphaned; the coordinator sweeps
  `.apsis-tmp-*` older than 6h at startup and never reconciles a temp as a source.
- The worker heartbeats the NATS lease during a long encode (no mid-run redelivery), and a
  job that keeps failing is dead-lettered after `max_deliver` and recorded `Failed@version`
  — suppressed until the file's `mtime:size` changes.

## Recoverability

- **KV wiped / NATS lost:** the coordinator rescans and re-enqueues only true drift. At worst
  one redundant probe pass. Nothing to restore by hand.
- **Cold NATS:** `ensure_topology` create-if-absent provisions the stream, consumer, and KV
  from the code — a fresh `nats-server` self-provisions on first connect.

## Rollout

Run against a **test** library in parallel with Unmanic (which keeps prod) and compare
outputs before pointing prod libraries at apsis. See `docs/design/rust-scheduler.md` §13.
