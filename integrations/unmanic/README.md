# pyflows Transcode — Unmanic plugin

A thin [Unmanic](https://github.com/Unmanic/unmanic) plugin that drives the
**pyflows** transcode engine, so Unmanic's scanner/queue/worker produce the
exact same output as the pyflows daemon. The engine (config, probe, plan, command,
ffmpeg, audio, subtitles) ships **inside the plugin** as `_engine/`, so Unmanic needs no
`pyflows` install — only the engine's own deps (pydantic v2, pyyaml) in `site-packages/`.

This directory is what runs on the rhea deployment (Unmanic CT 2004), brought back
from the container in October 2026. It adds over the June version:

- `_engine/command.py`: builds the ffmpeg command. On `hevc_vaapi`/`h264_vaapi` it sets
  **`sei=hdr`**, dropping the `a53_cc` SEI that re-embedded closed captions on every frame
  and slowed VAAPI encodes ~20–40× (HDR mastering/CLL SEI is kept).
- Closed-caption recovery (`_extract_closed_captions` in `plugin.py`): CEA-608/708 captions
  saved as an extra English ASS track, since `sei=hdr` drops them from the video.
  **Parked** — see the note in `plugin.py`.
- ASS font attachments are carried over (`copy_attachments`).
- Untagged tracks (`""`/`und`) are titled "Unknown" on both mp4 and mkv, so a remuxed
  file is not re-queued forever.

`_engine/` is a trimmed copy of `pyflows/` (no daemon, DB or concurrency settings) with
these changes not yet folded back into `pyflows/`.

Per file it:
1. picks the pyflows **profile** by which configured library the file lives under,
2. runs `plan_file()` to skip already-compliant files (`on_library_management_file_test`),
3. emits pyflows' exact ffmpeg command, with **VAAPI → CPU fallback** (`on_worker_process`).

Non-video files (`.nfo/.jpg/.srt/...`) are ignored.

## Install

1. Copy this directory into Unmanic's plugins dir as `pyflows_transcode`:
   ```
   <unmanic-config>/plugins/pyflows_transcode/
   ```
2. Install the engine's deps for Unmanic's Python (match its version — compiled wheels):
   `pip install --target plugins/pyflows_transcode/site-packages 'pydantic>=2' pyyaml`
   (the plugin adds a sibling `site-packages/` to `sys.path`).
3. Provide a pyflows `config.yaml`. By default the plugin reads
   `config.yaml` next to `plugin.py`; override with the env var
   `PYFLOWS_UNMANIC_CONFIG=/path/to/config.yaml`. Start from
   [`config.example.yaml`](config.example.yaml).
   **The library `path`s must match what Unmanic sees on disk.**
4. Register the plugin and add it to each library's flow under both
   **Library File Tests** and **Worker** stages (or insert
   `library_management.file_test` + `worker.process` rows into
   `librarypluginflow`).

Optional env: `PYFLOWS_FFMPEG`, `PYFLOWS_FFPROBE` (default: jellyfin-ffmpeg).

## Operational notes

- **Replace-original** is handled by Unmanic's post-processor (output container
  is mkv); the plugin writes to `file_out` and lets Unmanic move it back.
- **Cache:** point Unmanic's `cache_path` at the same disk as the media — a full
  HEVC temp is several GB and will fill a small root disk.
- **Shared GPU:** keep the worker count low (1–2). Unmanic ties concurrency to
  worker count, so that throttles concurrent VAAPI encodes.
- **Audio safety:** pyflows never drops *all* audio — if no track matches a
  profile's `keep_languages` (e.g. untagged `und` audio), it keeps the original
  tracks rather than emitting a video-only file.

## Not handled here

Sonarr/Radarr **rescan + rename** after transcode (so the *arr filenames /
mediainfo match the new codec) is a separate concern — wire it as an Unmanic
post-processor or via the *arr APIs. See the project docs.
