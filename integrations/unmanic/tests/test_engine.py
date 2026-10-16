"""Tests for the plugin's embedded engine (``_engine``).

Run from the repo root: ``uv run pytest integrations/unmanic/tests``.
"""

import json
import os
import sys

import pytest

PLUGIN_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, PLUGIN_DIR)

from _engine.command import build_encode_command  # noqa: E402
from _engine.config import load_config  # noqa: E402
from _engine.plan import plan_from_probe  # noqa: E402
from _engine.probe import parse_probe_output  # noqa: E402

CONFIG = os.path.join(PLUGIN_DIR, "config.example.yaml")


@pytest.fixture(scope="module")
def cfg():
    return load_config(CONFIG)


def _probe(*streams):
    return parse_probe_output(json.dumps({"streams": list(streams)}))


def _stream(index, codec_type, codec, language=None, title=None, default=0, **extra):
    tags = {}
    if language is not None:
        tags["language"] = language
    if title is not None:
        tags["title"] = title
    return {
        "index": index,
        "codec_type": codec_type,
        "codec_name": codec,
        "tags": tags,
        "disposition": {"default": default},
        **extra,
    }


def _cmd(cfg, profile_name, probe, path="/media/in.mkv", use_cpu=False):
    return build_encode_command(
        path,
        "/cache/out.mkv",
        probe,
        cfg.profiles[profile_name],
        vaapi_device="/dev/dri/renderD128",
        use_cpu=use_cpu,
        hardware_config=cfg.hardware,
    ).build()


def _anime_hevc_with_fonts():
    return _probe(
        _stream(0, "video", "hevc"),
        _stream(1, "audio", "aac", "jpn", channels=2, default=1),
        _stream(2, "subtitle", "ass", "eng", default=1),
        _stream(3, "subtitle", "ass", "fra"),
        _stream(4, "attachment", "ttf"),
    )


def test_hevc_copies_video_and_still_cleans_subtitles(cfg):
    probe = _anime_hevc_with_fonts()
    plan = plan_from_probe("/media/in.mkv", probe, cfg.profiles["anime"])

    assert plan.video.action == "copy"
    assert not plan.should_skip
    assert [s.language for s in plan.subtitles] == ["eng"]

    args = _cmd(cfg, "anime", probe)
    assert args[args.index("-c:v:0") + 1] == "copy"
    assert "0:s:1" not in args  # the French track is not mapped


@pytest.mark.parametrize("use_cpu", [False, True])
def test_mkv_output_keeps_attachments(cfg, use_cpu):
    args = _cmd(cfg, "anime", _anime_hevc_with_fonts(), use_cpu=use_cpu)

    maps = [args[i + 1] for i, a in enumerate(args) if a == "-map"]
    assert "0:t?" in maps
    assert args[args.index("-c:t") + 1] == "copy"
    # Attachments must not disturb the per-type indices of the real streams.
    assert maps[:3] == ["0:v:0", "0:a:0", "0:s:0"]


def test_non_mkv_output_does_not_map_attachments(cfg):
    profile = cfg.profiles["anime"].model_copy(deep=True)
    profile.output.container = "mp4"
    args = build_encode_command(
        "/media/in.mkv",
        "/cache/out.mp4",
        _anime_hevc_with_fonts(),
        profile,
        vaapi_device="/dev/dri/renderD128",
        hardware_config=cfg.hardware,
    ).build()

    assert "0:t?" not in args
    assert "-c:t" not in args


@pytest.mark.parametrize("language", [None, "", "und"])
def test_untagged_audio_gets_a_readable_stable_title(cfg, language):
    probe = _probe(
        _stream(0, "video", "hevc"),
        _stream(1, "audio", "aac", language, channels=2, default=1),
    )
    plan = plan_from_probe("/media/in.mkv", probe, cfg.profiles["tv"])

    assert [a.title_after for a in plan.audio] == ["Unknown / AAC / Stereo"]


def test_untagged_audio_is_compliant_after_its_own_remux(cfg):
    # What the remux of an "und" mp4 track looks like once it is mkv: language
    # dropped to "" and the title written by the previous pass.
    probe = _probe(
        _stream(0, "video", "hevc"),
        _stream(1, "audio", "aac", "", "Unknown / AAC / Stereo", channels=2, default=1),
    )
    plan = plan_from_probe("/media/in.mkv", probe, cfg.profiles["tv"])

    assert plan.should_skip, [r.message for r in plan.reasons]
