"""pyflows transcode engine — the planning + ffmpeg-command-building logic.

This package is the self-contained "brain" of pyflows, embedded directly in the
Unmanic plugin. It deliberately contains no daemon machinery (scanner, queue,
DB, webhook, web UI) — Unmanic provides all of that. What remains is the part
that decides *whether* and *how* to transcode a file:

    config  → load/validate the YAML profile
    probe   → read stream info via ffprobe
    plan    → decide skip-vs-process and which streams to keep/convert
    command → build the exact ffmpeg command (VAAPI or CPU)
"""
