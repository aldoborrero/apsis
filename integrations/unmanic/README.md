# pyflows Transcode — Unmanic plugin

An [Unmanic](https://github.com/Unmanic/unmanic) plugin carrying the **pyflows**
transcode engine. The planning and ffmpeg-command logic is embedded in `_engine/`
(no pyflows install needed); Unmanic supplies the scanner, queue, worker and
post-processor.

Per file it:
1. picks the pyflows **profile** by which configured library the file lives under,
2. runs `plan_file()` to skip already-compliant files (`on_library_management_file_test`),
3. emits pyflows' exact ffmpeg command, with **VAAPI → CPU fallback** (`on_worker_process`).

Non-video files (`.nfo/.jpg/.srt/...`) are ignored.

A file whose video is already in `skip_codecs` (e.g. HEVC) is still processed when
its audio or subtitles do not match the profile: the video is stream-copied and
only the audio/subtitle tracks are normalised. With an mkv output the source's
attachments (ASS fonts) are copied along.

## Install

1. Copy this directory into Unmanic's plugins dir as `pyflows_transcode`:
   ```
   <unmanic-config>/plugins/pyflows_transcode/
   ```
2. Make the engine's deps (`pydantic>=2`, `pyyaml`) importable. Debian's system
   Python blocks `pip` (PEP 668) and ships pydantic v1, so vendor them next to the
   plugin: `pip install --target plugins/pyflows_transcode/site-packages pydantic pyyaml`
   (the plugin adds a sibling `site-packages/` to `sys.path` if present).
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

## Tests

```
pytest integrations/unmanic/tests
```

Needs `pydantic` and `pyyaml`; the tests import `_engine` directly, not Unmanic.

## Not handled here

Sonarr/Radarr **rescan + rename** after transcode (so the *arr filenames /
mediainfo match the new codec) is a separate concern — wire it as an Unmanic
post-processor or via the *arr APIs. See the project docs.
