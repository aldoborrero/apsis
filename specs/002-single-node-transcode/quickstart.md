# Quickstart — Single-node apsis (spec 002)

## Prerequisites

- A `nats-server` with JetStream on rhea (its own LXC/container; JetStream file store on a
  persistent path). Single node — no cluster.
- ffmpeg + ffprobe with VAAPI (`/dev/dri/renderD128`) on the worker host.
- A **test** library (not prod) mapped to a profile — run alongside Unmanic during validation.

## Config

`scheduler.toml` (coordinator):
```toml
[reconcile]
scan_interval = "5m"
debounce      = "60s"
inotify       = true

[[library]]
name = "tv-test"
path = "/hdd/media/test/tv"
profile = "tv"

[profiles.tv.video]   # = apsis_engine::Profile (spec 001)
codec = "hevc"
skip_codecs = ["hevc", "av1"]
# ... audio/subtitles/output as in the engine profile
```

`worker.toml` (worker):
```toml
concurrency = 1                      # AMD VCN HEVC → single session

[verify]
duration_tolerance = "1s"
max_size_ratio = 1.5

[[backend]]
kind = "vaapi"
device = "/dev/dri/renderD128"

[[backend]]
kind = "cpu"                         # fallback

# path_map identity on rhea; real on sirius (spec 003)
[path_map]
```

## Run

```bash
# provisions stream/KV/consumer if absent, then reconciles + serves the loop
apsis-coordinator --config scheduler.toml --nats nats://rhea:4222
apsis-worker      --config worker.toml     --nats nats://rhea:4222
```

## What happens

1. Coordinator discovers files (inotify + periodic walk), skips non-video and still-being-
   written files (debounce), picks each file's profile by longest-matching path.
2. For a changed/unseen file it runs `apsis_engine::plan`; compliant → KV `Done`; drift →
   KV `Pending` (CAS) + publishes a `Job`.
3. Worker pulls the job (lease via `AckWait`), builds the ffmpeg command for VAAPI (CPU on
   fallback), transcodes to `.apsis-tmp-<ulid>` on the same filesystem.
4. ffprobe verify (streams/duration/not-truncated/size) → atomic `rename` + restore
   owner/mtime/perms → `ack` + `TranscodeResult`. Verify fail → discard temp, original
   untouched, `nak`/`term`.
5. Metrics update in Grafana; a second reconcile with no changes creates **0** jobs.

## Verifying the success criteria

- **SC-001**: drop an H.264 sample → after a cycle it's HEVC and plays; original replaced.
- **SC-002**: reconcile a compliant library twice → 0 jobs both times.
- **SC-003**: `kill -9` the worker mid-transcode → original intact; job redelivered and
  completes on retry.
- **SC-004**: a deliberately-unencodable file is retried `max_deliver` times → `Failed`,
  then suppressed until its bytes change.
- **SC-005**: scrape metrics → queue depth/in-flight/outcomes present and current.

## Test locally without prod media

```bash
# tiny sample clips for US1/US3
ffmpeg -f lavfi -i testsrc=d=5:s=320x240 -f lavfi -i sine=d=5 -c:v libx264 sample.mkv
```
Point a throwaway library at a temp dir of these; the safe-replace and fallback paths are
exercised without touching real media.
