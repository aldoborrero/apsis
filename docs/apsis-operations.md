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
`NaN` ratio, unknown key inside a profile, or a broken CEL rule) makes the daemon refuse to
start, so the last-good deployment keeps running.

## Profiles

A profile has four blocks — `video`, `audio`, `subtitles`, `output`. Every field is optional
with a sane default; unknown keys are rejected. Full example with the common fields:

```toml
[profiles.tv.video]
codec       = "hevc"            # hevc | av1  (target codec)
encoder     = "vaapi"           # vaapi | cpu  (which backend materializes it)
quality     = 22                # shorthand for { mode = "auto", value = 22 }
# quality   = { mode = "crf", value = 20 }   # or qp | crf | bitrate (value "5M" for bitrate)
fallback    = "cpu"             # cpu | none  (fallback if the primary backend fails)
skip_codecs = ["hevc", "av1"]   # a file already in one of these is compliant (copy-through)
hdr_policy  = "copy"            # copy | tonemap | encode
preset      = "medium"          # CPU-encoder only (libx265/libsvtav1); ignored on VAAPI
max_resolution = "1080p"        # downscale-if-larger ("1080p" | "1920x1080" | "4k")
custom_args    = []             # raw ffmpeg args appended to the single command (escape hatch)
skip_if_resolution_below = "480p"   # compliance gate: leave already-small sources alone
skip_if_bitrate_below    = "1500k"  # compliance gate: leave already-efficient sources alone

[profiles.tv.audio]
keep_languages    = ["eng", "spa", "jpn"]   # empty = keep all; never drops ALL audio
priority          = ["eng", "spa"]          # output ordering
remove_commentary = true
preserve_surround = true
add_stereo   = { codec = "aac", bitrate = "128k", channels = 2, languages = ["eng"] }
add_mono     = { codec = "aac", bitrate = "64k", languages = ["eng"] }   # generate a mono clone
transcode    = { codec = "opus", bitrate = "160k" }   # re-encode KEPT tracks (not just clones)
max_channels = 6                            # cap channels on transcoded/generated tracks
normalize    = false                        # single-pass EBU R128 loudnorm on ENCODED tracks only

[profiles.tv.subtitles]
keep_languages = ["eng", "spa", "jpn"]
remove_formats = ["hdmv_pgs_subtitle", "dvd_subtitle"]
order          = ["eng", "spa"]             # positional output order
forced_only    = false

[profiles.tv.output]
container        = "mkv"
replace_original = true
conform          = true         # drop SUBTITLE streams the container can't hold (avoids mux failures)
strip_metadata   = false        # -map_metadata -1
keep_chapters    = true
```

**`quality` modes** — `auto` resolves to the backend's native rate control (VAAPI `qp` / CPU
`crf`); `qp` and `crf` force those; `bitrate` takes a rate value (`"5M"`). `vmaf` is accepted by
the schema but not yet materializable.

**Not yet materialized** (accepted by the schema, applied in a later spec): `video.crop`
(auto-crop needs a `cropdetect` analysis pass — conflicts with single-pass encoding),
`subtitles.extract` (sidecar `.srt` output), and `quality.mode = "vmaf"`. Setting them loads
fine but has no effect yet.

## Conditional overrides (CEL rules)

A profile can carry rules that override the base per file. Each rule has a `when` predicate and
a `set` map, evaluated against the probed file. Rules apply in order, last-write-wins, before
the single plan is computed — the worker only ever sees the resolved *effective* profile.

```toml
[[profiles.tv.rule]]
when = "video.height >= 2160"
set  = { "video.codec" = "av1", "video.quality.value" = "${video.height >= 2160 ? 24 : 22}" }

[[profiles.tv.rule]]
when = "audio.exists(a, a.codec == 'truehd' && a.channels > 6)"
set  = { "audio.transcode" = { codec = "eac3", bitrate = "640k" } }
```

- **`when`** is a [CEL](https://cel.dev) predicate (boolean). **`set`** keys are dotted profile
  paths; values are **literals by default**, or CEL expressions when wrapped in `${…}`. So
  `"video.codec" = "av1"` is a literal but `"video.quality.value" = "${… ? 24 : 22}"` is
  computed. `${…}` is reserved — a literal value that must contain `${x}` is not expressible.
- **Context** (`cel_context_version: 1`): `video?` (`codec, width, height, bitrate, hdr,
  color_transfer, bit_depth`), `audio[]`/`subtitles[]` (`codec, language, channels, …`), and
  `path, container, duration, size`. The full field table + macro surface is in
  [`specs/004-rich-profiles-cel/contracts/cel-context.md`](../specs/004-rich-profiles-cel/contracts/cel-context.md).
  Prefer the comprehension macros (`audio.exists(a, …)`, `size(audio)`) over positional
  indexing; guard any index with a size check.
- **Validation is fail-fast at load**: every `when`/`${…}` is compiled and canary-evaluated
  against a representative synthetic file, so a syntax error, unknown context field, unknown
  `set` path, or a *statically* out-of-range value refuses to start. A value that only goes out
  of range for a specific real file fails *that file* at reconcile (recorded `Failed@version`,
  counter `apsis_override_failed_total`), never the daemon — and, like any `Failed` file, is not
  retried until its `mtime:size` changes.

> `duration` is currently always `0.0` (the probe reads streams, not container format) — do not
> gate rules on it yet.

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

## Control plane (spec 005)

Operator control travels over NATS subjects — no web UI, no HTTP API. Everything below works
from the `nats` CLI (in the dev shell); the daemons are the sole writers of state, so a
control client only ever *publishes intents*. Each op increments
`apsis_control_ops_total{op}` and is logged.

**Pause / resume** — publish a pause intent; the coordinator persists it, workers watch it.

```bash
# soft pause (finish in-flight, stop taking new work), globally
nats pub apsis.control.pause '{"scope":"global","mode":"soft","set":true}'
# hard pause (also abort the running transcode)
nats pub apsis.control.pause '{"scope":"global","mode":"hard","set":true}'
# drain one host
nats pub apsis.control.pause '{"scope":{"worker":"rhea"},"mode":"soft","set":true}'
# resume
nats pub apsis.control.pause '{"scope":"global","mode":"soft","set":false}'
```

**Cancel the active transcode** — request/reply; the source is left byte-identical.

```bash
# defer: re-queued next reconcile; ignore: recoverable on-disk marker, never retried
nats req apsis.control.cancel '{"job_id":"<ulid>","disposition":"defer"}'
# → {"outcome":"cancelled"}  (or "not_running" / "already_done")
```

**State control** — request/reply to the coordinator (serialized with reconcile):

```bash
nats req apsis.control.state '{"path":"/hdd/media/tv/x.mkv","op":"requeue"}'    # re-evaluate
nats req apsis.control.state '{"path":"/hdd/media/tv/x.mkv","op":"force"}'      # transcode despite skip
nats req apsis.control.state '{"path":"/hdd/media/tv/x.mkv","op":"retry"}'      # clear a Failed
nats req apsis.control.state '{"path":"/hdd/media/tv/x.mkv","op":"mark_done"}'  # never transcode until it changes
# → {"outcome":"applied"}  (or "not_found" / "noop")
```

**Introspection** — the per-file state (incl. the *decision*: why it was skipped) is in the
KV; live progress is on a per-job subject.

```bash
nats kv get transcode_state /hdd/media/tv/x.mkv      # status + decision (why skipped)
nats sub 'apsis.progress.>'                            # live {speed, eta_s, out_time_s}
```

**Ignore marker** — an *ignore* disposition writes `<file>.apsisignore` beside the media; the
reconcile gate honors it (an ignored file is never probed) and it survives a KV wipe. Delete
the marker to un-ignore.

**Authorization**: operator NATS credentials should be scoped to **publish `apsis.control.*`,
subscribe `apsis.progress.*`, read the KV** — no KV write, no job-stream publish (the daemons
hold those). This makes "owners are the sole writers" enforced, not conventional. See
[`specs/005-control-protocol/contracts/control-subjects.md`](../specs/005-control-protocol/contracts/control-subjects.md).

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
