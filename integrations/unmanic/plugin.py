#!/usr/bin/env python3
"""
pyflows Transcode — an Unmanic plugin.

The pyflows transcode engine, embedded directly in this plugin: VAAPI HEVC (with
CPU fallback), language keep/drop, AAC stereo downmix, commentary removal, stream
reorder, and image-subtitle removal. Unmanic provides the scanner/queue/worker/
post-processor, so the plugin only carries the planning + ffmpeg-command logic
(the ``_engine`` package next to this file) — no external ``pyflows`` install.

Per file it: picks the matching profile (by which configured *library* the file
lives under), runs ``plan_file()`` to decide skip-vs-process, and on the worker
stage hands Unmanic the exact ffmpeg command from ``build_encode_command()``.

Configuration (environment variables):
  PYFLOWS_UNMANIC_CONFIG  Path to the ``config.yaml`` (default: ``config.yaml``
                          next to this plugin). The config's ``libraries`` (path
                          + profile) drive per-file profile selection, so each
                          library ``path`` must match what Unmanic sees on disk.
  PYFLOWS_FFMPEG          ffmpeg binary  (default: jellyfin-ffmpeg).
  PYFLOWS_FFPROBE         ffprobe binary (default: jellyfin-ffmpeg).
"""

import logging
import os
import shlex
import subprocess
import sys

_HERE = os.path.dirname(os.path.abspath(__file__))
# The embedded engine lives in ``_engine/`` next to this file. Put the plugin dir
# on sys.path so it imports regardless of how Unmanic loads the plugin module.
if _HERE not in sys.path:
    sys.path.insert(0, _HERE)
# The engine's Python deps (pydantic, pyyaml) may be vendored into ``site-packages/``
# next to this plugin — Unmanic's system Python (Debian) ships neither, or ships an
# incompatible pydantic v1. Add the bundle to the path when present; harmless when the
# deps are already importable (global install).
_SITE = os.path.join(_HERE, "site-packages")
if os.path.isdir(_SITE) and _SITE not in sys.path:
    sys.path.insert(0, _SITE)

from unmanic.libs.unplugins.settings import PluginSettings  # noqa: E402

from _engine.command import build_encode_command  # noqa: E402
from _engine.config import load_config  # noqa: E402
from _engine.plan import container_suffix, plan_file  # noqa: E402
from _engine.probe import probe_file  # noqa: E402

logger = logging.getLogger("Unmanic.Plugin.pyflows_transcode")

CONFIG_PATH = os.environ.get("PYFLOWS_UNMANIC_CONFIG") or os.path.join(
    _HERE, "config.yaml"
)
FFMPEG = os.environ.get("PYFLOWS_FFMPEG", "/usr/lib/jellyfin-ffmpeg/ffmpeg")
FFPROBE = os.environ.get("PYFLOWS_FFPROBE", "/usr/lib/jellyfin-ffmpeg/ffprobe")

# Closed-caption recovery is PARKED. The sei=hdr video workaround (command.py) drops
# the in-video CEA-608/708 CC; recovering them needs a clean extraction. ffmpeg's
# subcc->ASS path yields no cues here (subcc->SRT works but carries positioning
# markup). Until the ASS path is understood, keep this off so we don't run a wasted
# extraction per file. The builder's cc_subtitle_path hook + tests stay in place;
# flip this to True to re-enable once extraction is sorted.
_RECOVER_CC = False

# Only ever act on real video files. Without this the Unmanic scanner queues
# every sidecar/artwork file (.nfo/.jpg/.srt/...), wasting effort and failing.
VIDEO_EXTENSIONS = {
    ".mkv",
    ".mp4",
    ".avi",
    ".m4v",
    ".mov",
    ".ts",
    ".m2ts",
    ".wmv",
    ".flv",
    ".webm",
    ".mpg",
    ".mpeg",
    ".mpv",
    ".3gp",
}

_CFG = None


class Settings(PluginSettings):
    settings = {}


def _is_video(path):
    return os.path.splitext(path or "")[1].lower() in VIDEO_EXTENSIONS


def _cfg():
    global _CFG
    if _CFG is None:
        _CFG = load_config(CONFIG_PATH)
    return _CFG


def _profile_for_path(cfg, path):
    """Pick the pyflows profile for a file by the configured library it lives
    under (longest matching ``library.path`` wins)."""
    p = path or ""
    best = None
    for lib in cfg.libraries:
        base = lib.path.rstrip("/")
        if p == base or p.startswith(base + "/"):
            if best is None or len(base) > len(best.path.rstrip("/")):
                best = lib
    if best is None:
        return None, None
    return cfg.profiles.get(best.profile), best.profile


def on_library_management_file_test(data):
    abspath = data.get("path")
    if not _is_video(abspath):
        return data
    try:
        cfg = _cfg()
        profile, name = _profile_for_path(cfg, abspath)
        if profile is None:
            return data
        plan = plan_file(abspath, profile, ffprobe_path=FFPROBE)
        if not plan.should_skip:
            data["add_file_to_pending_tasks"] = True
            logger.info(
                "Queue '%s' (profile=%s): %s",
                abspath,
                name,
                "; ".join(r.message for r in plan.reasons[:4]),
            )
    except Exception:
        logger.exception("file_test failed for '%s'", abspath)
    return data


def _ass_has_dialogue(path: str) -> bool:
    """True if the extracted ASS holds at least one caption cue."""
    try:
        with open(path, encoding="utf-8", errors="ignore") as f:
            return any(line.startswith("Dialogue:") for line in f)
    except OSError:
        return False


def _extract_closed_captions(file_in: str, out_dir: str) -> str | None:
    """Recover embedded CEA-608/708 closed captions as an ASS subtitle file.

    The ``sei=hdr`` video workaround drops the in-video CC (see command.py); this
    preserves them as a real, selectable track. ASS is ffmpeg's native caption
    format, so the CEA positioning stays valid (no markup leaked into an SRT).
    ffmpeg's ``movie`` filter can't parse the ``[]():,`` in library paths, so it
    reads the source through a temp symlink in the (per-task, auto-cleaned) cache
    dir. Returns the ASS path when captions were found, else None. Best-effort —
    never raises; a source without CC just yields None.
    """
    link = os.path.join(out_dir, "pyflows-cc-src")
    cc_path = os.path.join(out_dir, "pyflows-cc.ass")
    try:
        if os.path.islink(link) or os.path.exists(link):
            os.remove(link)
        os.symlink(file_in, link)
        subprocess.run(
            [
                FFMPEG, "-hide_banner", "-y", "-nostdin",
                "-f", "lavfi", "-i", f"movie={link}[out+subcc]",
                "-map", "0:1", cc_path,
            ],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=1800,
        )
        if os.path.isfile(cc_path) and _ass_has_dialogue(cc_path):
            return cc_path
        if os.path.isfile(cc_path):
            os.remove(cc_path)  # header-only (no captions) — discard
    except Exception:
        logger.exception("CC extraction failed for '%s'", file_in)
    finally:
        if os.path.islink(link):
            os.remove(link)
    return None


def on_worker_process(data):
    data["exec_command"] = []
    data["repeat"] = False

    file_in = data.get("file_in")
    file_out = data.get("file_out")
    original = data.get("original_file_path") or file_in
    if not _is_video(original):
        return data
    try:
        cfg = _cfg()
        profile, name = _profile_for_path(cfg, original)
        if profile is None:
            return data

        # Sonarr/Radarr can replace a queued file (upgrade, .mp4 → .mkv import)
        # before a worker reaches it; ffprobe then fails with a traceback.
        if not os.path.isfile(file_in):
            logger.warning("Source vanished before processing, skipping: '%s'", file_in)
            return data

        probe = probe_file(file_in, ffprobe_path=FFPROBE)
        plan = plan_file(file_in, profile, ffprobe_path=FFPROBE)
        if plan.should_skip:
            return data

        # Unmanic names file_out after the *input* extension and the muxer is picked
        # from it, so an .mp4 source stayed mp4, kept failing the container check
        # and was re-queued on every scan. Give file_out the profile's container;
        # Unmanic then moves the result to <original>.mkv and removes the source.
        root, ext = os.path.splitext(file_out)
        target_ext = container_suffix(profile)
        if ext.lower() != target_ext.lower():
            file_out = root + target_ext
            data["file_out"] = file_out

        # Recover embedded closed captions only when the video is actually
        # re-encoded — that is exactly when the sei=hdr workaround drops them (a
        # copied video keeps its own CC). No-op for sources without captions.
        cc_path = None
        if _RECOVER_CC and plan.video.action != "copy":
            cc_path = _extract_closed_captions(file_in, os.path.dirname(file_out))
            if cc_path:
                logger.info("Recovered embedded closed captions for '%s'", original)

        # pyflows applies VAAPI env (e.g. AMD_DEBUG=noefc) to the process;
        # Unmanic runs the command directly, so prepend `env VAR=VAL ...`.
        env_prefix = ["/usr/bin/env"] + [
            f"{k}={v}" for k, v in cfg.hardware.env.items()
        ]

        def build(use_cpu):
            cmd = build_encode_command(
                file_in,
                file_out,
                probe,
                profile,
                vaapi_device=cfg.general.vaapi_device,
                use_cpu=use_cpu,
                ffmpeg_path=FFMPEG,
                hardware_config=cfg.hardware,
                cc_subtitle_path=cc_path,
            )
            return env_prefix + cmd.build()

        vaapi_cmd = build(use_cpu=False)
        if profile.video.encoder == "vaapi" and profile.video.fallback.lower() == "cpu":
            cpu_cmd = build(use_cpu=True)

            def q(args):
                return " ".join(shlex.quote(a) for a in args)

            # VAAPI first; on non-zero exit fall back to CPU (libx265). Both
            # write file_out with -y, so a partial VAAPI output is overwritten.
            shell = (
                "%s || { echo 'pyflows: VAAPI encode failed, falling back to CPU' >&2; %s ; }"
                % (q(vaapi_cmd), q(cpu_cmd))
            )
            data["exec_command"] = ["/bin/sh", "-c", shell]
        else:
            data["exec_command"] = vaapi_cmd
        logger.info("Encoding '%s' (profile=%s)", original, name)
    except Exception:
        logger.exception("worker_process failed for '%s'", original)
        data["exec_command"] = []
    return data
