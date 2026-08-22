# apsis

**A thin, distributed media-transcode scheduler in Rust.** A central coordinator
walks your media libraries and decides *whether* and *how* to transcode; a pool of
workers pull those decisions and do the work — verify, then atomically replace the
original. Jobs carry only metadata; the media stays on shared storage.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](#license)
![Rust](https://img.shields.io/badge/rust-edition%202024-orange.svg)
![status](https://img.shields.io/badge/status-single--node%20working-yellow.svg)

apsis is a FOSS successor to [Unmanic](https://github.com/Unmanic/unmanic) for the
homelab. The transcode *brain* (probe → plan → ffmpeg command, tuned for AMD VAAPI)
was already ours; apsis is the orchestration around it — the part Unmanic provided —
rebuilt to be **git-recoverable**, **observable through metrics instead of a web UI**,
and **distributable across GPU hosts**. The name is orbital: an *apsis* is a key point
of an orbit, mirroring the central-coordinator / orbiting-workers topology.

> **Why build it?** The "modern Unmanic" alternatives are not open: Tdarr is
> proprietary, and FileFlows' application source is unpublished. Unmanic (GPLv3) is
> the only truly-FOSS orchestrator in this space — so the only MIT-licensed upgrade
> path is to own it.

---

## How it works

```
   libraries + profiles        ┌──────────────────────────────────┐
   (scheduler.toml, in git) ──▶ │          apsis-coordinator        │
                                │   scan → probe → plan → enqueue   │
                                │          (reconcile loop)         │
                                └─────────────────┬────────────────┘
                                                  │  jobs (metadata only)
                                        ┌─────────▼─────────┐
                                        │    NATS JetStream  │  job stream + KV state
                                        └─────────┬─────────┘
                                                  │  pull · claim · ack
                                ┌─────────────────▼────────────────┐
   shared filesystem  ◀────────▶│           apsis-worker            │
   (media over NFS)             │  claim → transcode → verify →     │
                                │         atomic-replace            │
                                │    VAAPI (AMD)  →  CPU fallback    │
                                └───────────────────────────────────┘
```

- **The coordinator reconciles.** It walks each library, gates on a cheap
  `mtime:size` change token (unchanged files are never re-probed), probes what
  changed, and asks the engine for a plan. A file that already complies is marked
  `Done`; drift is claimed and published as a job. Desired state is git-tracked TOML —
  losing NATS costs at most one re-probe pass, never a re-transcode.
- **The worker executes, safely.** It claims a job under a JetStream lease, builds the
  ffmpeg command for *its* backend, transcodes to a temp file, **verifies** the output
  (duration within tolerance, not bloated), and only then **atomically replaces** the
  original. A crash mid-encode redelivers the job; a persistently failing job
  dead-letters as `Failed@version` and alerts — it never loops forever.
- **Media never moves.** Jobs are metadata; workers read and write the shared
  filesystem directly (a per-worker `path_map` maps coordinator paths to local mounts).

## Features

- **Hardware + CPU encoding** — AMD **VAAPI** HEVC/AV1 with automatic **libx265 /
  libsvtav1** fallback, including the AMD 780M `sei=hdr` workaround for the VCN
  `a53_cc` bug.
- **Declarative profiles** — per-library video/audio/subtitle/output rules: codec
  targets, quality modes (`qp` / `crf` / `bitrate`), audio language keep-lists,
  commentary removal, stereo/mono downmix generation, subtitle filtering, container
  conform. Never drops *all* audio.
- **Conditional overrides with CEL** — bring your own policy without forking Rust.
  A profile can carry rules whose [CEL](https://cel.dev) predicate matches on the
  probed file and whose `set` layers overrides onto the base profile — computed values
  wrapped in `${…}`:

  ```toml
  [[profiles.tv.rule]]
  when = "video.height >= 2160"
  set  = { "video.codec" = "av1", "video.quality.value" = "${video.height >= 2160 ? 24 : 22}" }

  [[profiles.tv.rule]]
  when = "audio.exists(a, a.codec == 'truehd' && a.channels > 6)"
  set  = { "audio.transcode" = { codec = "eac3", bitrate = "640k" } }
  ```

  Every rule is **compiled and canary-evaluated at config load** — a syntax error,
  unknown field, or static out-of-range value refuses to start. A value that only goes
  out of range for a specific real file fails *that file*, never the daemon.
- **Fail-fast, git-recoverable config** — libraries and profiles are TOML in git; an
  invalid config refuses to start so the last-good deployment keeps running. No
  live-only state to lose.
- **Observability, not a web UI** — both daemons export Prometheus metrics and
  structured `tracing` logs; dashboards and alerting live in Grafana + VictoriaMetrics.

## Quickstart (local, single node)

Everything is in the Nix dev shell — Rust toolchain, `ffmpeg`, `nats-server`,
`process-compose`.

```bash
git clone https://github.com/aldoborrero/apsis && cd apsis
direnv allow            # or: nix develop
process-compose up      # local JetStream NATS on :4222 (monitoring :8222)
```

Point the two daemons at a library and run them against the local NATS:

```bash
export NATS_URL=nats://127.0.0.1:4222
APSIS_SCHEDULER_CONFIG=./scheduler.toml cargo run -p apsis-coordinator
APSIS_WORKER_CONFIG=./worker.toml       cargo run -p apsis-worker
```

### Configuration

`scheduler.toml` (coordinator) — libraries, profiles, reconcile cadence:

```toml
[reconcile]
scan_interval = "5m"
debounce      = "60s"        # ignore files still being written

[[library]]
name    = "tv"
path    = "/hdd/media/tv"    # must be absolute
profile = "tv"

[profiles.tv.video]
codec       = "hevc"
skip_codecs = ["hevc", "av1"]   # already-compliant codecs
quality     = 22                # shorthand for { mode = "auto", value = 22 }
[profiles.tv.audio]
keep_languages = ["eng", "spa", "jpn"]
add_stereo     = { codec = "aac", bitrate = "128k", channels = 2, languages = ["eng"] }
[profiles.tv.subtitles]
keep_languages = ["eng", "spa", "jpn"]
[profiles.tv.output]
container        = "mkv"
replace_original = true
```

`worker.toml` (worker) — backends (first is primary), verify guards, path map:

```toml
concurrency   = 1            # AMD VCN HEVC is single-session
stall_timeout = "2m"

[verify]
max_size_ratio = 1.5         # reject a bloated output

[[backend]]
kind   = "vaapi"
device = "/dev/dri/renderD128"
[[backend]]
kind = "cpu"                 # fallback

[path_map]
# "/hdd" = "/mnt/rhea-hdd"   # coordinator path → this host's mount
```

The full runbook — every field, env vars, metrics, and multi-host notes — is in
[`docs/apsis-operations.md`](docs/apsis-operations.md).

## Project layout

```
crates/
├── apsis-engine        the transcode brain: probe → plan → ffmpeg command,
│                       declarative Profile + CEL overrides (pure, no I/O)
├── apsis-common        shared: config loaders, NATS/JetStream, job + state schema
├── apsis-coordinator   the reconcile loop: scan → probe → plan → enqueue
└── apsis-worker        the executor: claim → transcode → verify → atomic-replace

docs/
├── design/rust-scheduler.md    architecture & settled decisions
└── apsis-operations.md         operations runbook (config, metrics, env)
specs/                          spec-kit feature specs (001-engine … 004-cel-profiles)
.specify/memory/constitution.md the project's governing principles
```

## Status

apsis runs **single-node** today: the engine (spec 001), the coordinator + worker
reconcile-and-transcode loop with crash-safety (spec 002), and rich declarative
profiles + CEL conditional overrides with load-time validation (spec 004) are
implemented and tested. **Multi-host** distribution across GPU nodes (spec 003) is
designed but not yet built, and production still runs on Unmanic + the legacy plugin
until the cutover. See [`specs/`](specs/) for the per-feature detail.

## Development

```bash
nix develop                                        # dev shell with all tooling
cargo test --workspace                             # unit + integration tests
cargo clippy --workspace --all-targets -- -D warnings
nix fmt                                             # format everything
```

## License

MIT.
