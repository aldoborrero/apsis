"""Build the ffmpeg command for a planned transcode.

Extracted from pyflows' encode pipeline — only the command-*building* half. The
run/verify/replace-original half lived in the daemon's pipeline and is now
Unmanic's job (worker + post-processor), so it is intentionally omitted here.
"""

from .config import HardwareConfig, ProfileConfig
from .ffmpeg import FFmpegCommand
from .plan import FilePlan, plan_from_probe
from .probe import ProbeResult, StreamInfo

# Codec → encoder mapping
VAAPI_ENCODERS = {"hevc": "hevc_vaapi", "av1": "av1_vaapi"}
CPU_ENCODERS = {"hevc": "libx265", "av1": "libsvtav1"}
# Subtitle codecs Matroska cannot store → the encoder to convert them with.
MATROSKA_SUBTITLE_CONVERSIONS = {"mov_text": "srt"}


def build_encode_command_from_plan(
    plan: FilePlan,
    profile: ProfileConfig,
    vaapi_device: str,
    use_cpu: bool = False,
    ffmpeg_path: str = "ffmpeg",
    hardware_config: HardwareConfig | None = None,
    cc_subtitle_path: str | None = None,
) -> FFmpegCommand:
    """Build the ffmpeg command from a FilePlan.

    ``cc_subtitle_path``: optional SRT extracted from the source's embedded
    CEA-608/708 closed captions. When set, it is muxed as an extra English
    subtitle track — the ``sei=hdr`` video workaround drops the in-video CC, so
    this preserves them as a real (selectable, non-default) subtitle stream.
    """
    cmd = FFmpegCommand()
    cmd.set_ffmpeg_path(ffmpeg_path)

    use_vaapi = profile.video.encoder == "vaapi" and not use_cpu
    if hardware_config is not None:
        cmd.configure_hardware(hardware_config)
    if use_vaapi:
        cmd.set_vaapi_device(vaapi_device)
        if plan.source_probe.video:
            cmd.set_input_codec(plan.source_probe.video.codec)

    cmd.add_input(plan.output.input_path)

    # --- Video ---
    v_idx = cmd.map_stream("0:v:0")
    if plan.video.action == "copy":
        cmd.set_codec(v_idx, "copy")
    elif use_vaapi:
        codec_name = profile.video.codec
        encoder = VAAPI_ENCODERS.get(codec_name, f"{codec_name}_vaapi")
        async_depth = (
            hardware_config.vaapi.async_depth if hardware_config is not None else 4
        )
        opts = {"qp": profile.video.quality, "async_depth": async_depth}
        # AMD Mesa VCN HEVC/H.264 encoders stall ~40x (0.6 fps) when re-embedding the
        # a53_cc SEI — i.e. whenever the source carries embedded CEA-608/708 closed
        # captions (US-broadcast H.264, e.g. Mr. Robot). Emit HDR SEI only, dropping
        # a53_cc, which restores full ~20x hardware speed. The captions are not lost:
        # they are extracted and muxed as a real subtitle track (see plugin CC step).
        # AV1 has no SEI concept, so scope this to hevc/h264. See homelab docs/GOTCHAS.md.
        if codec_name in ("hevc", "h264"):
            opts["sei"] = "hdr"
        cmd.set_codec(v_idx, encoder, **opts)
    else:
        codec_name = profile.video.codec
        encoder = CPU_ENCODERS.get(codec_name, f"lib{codec_name}")
        cmd.set_codec(v_idx, encoder, crf=profile.video.quality, preset="medium")
    cmd.set_metadata(v_idx, "title", "")

    # --- Audio ---
    for audio_item in plan.audio:
        assert audio_item.source_index is not None
        a_idx = cmd.map_stream(
            f"0:a:{_audio_input_index_by_source_index(plan.source_probe.audio, audio_item.source_index)}"
        )
        if audio_item.action == "copy":
            cmd.set_codec(a_idx, "copy")
        else:
            cmd.set_codec(
                a_idx,
                audio_item.target_codec,
                ac=audio_item.target_channels,
                b=f"{_default_audio_bitrate(profile, audio_item.language)}k",
            )
        cmd.set_metadata(a_idx, "title", audio_item.title_after)
        cmd.set_disposition(a_idx, "default" if audio_item.default else "0")

    # --- Subtitles ---
    for subtitle_item in plan.subtitles:
        assert subtitle_item.source_index is not None
        s_idx = cmd.map_stream(
            f"0:s:{_sub_input_index_by_source_index(plan.source_probe.subtitles, subtitle_item.source_index)}"
        )
        cmd.set_codec(s_idx, _subtitle_codec(subtitle_item.source_codec, profile))
        cmd.set_disposition(s_idx, "default" if subtitle_item.default else "0")

    # --- Recovered closed captions (extra English subtitle track) ---
    # The CC are extracted (in the plugin) as an ASS file — ASS is ffmpeg's native
    # caption representation, so it carries the CEA-608/708 positioning as valid ASS
    # tags rather than markup leaked into an SRT. Copy it in as-is (no re-encode).
    if cc_subtitle_path:
        cc_input = cmd.num_inputs  # index of this extra input (main file is 0)
        cmd.add_input(cc_subtitle_path)
        cc_idx = cmd.map_stream(f"{cc_input}:s:0")
        cmd.set_codec(cc_idx, "copy")
        cmd.set_metadata(cc_idx, "language", "eng")
        cmd.set_metadata(cc_idx, "title", "Closed Captions")
        cmd.set_disposition(cc_idx, "0")  # never the default track

    cmd.set_output(plan.output.output_path)
    return cmd


def build_encode_command(
    input_path: str,
    output_path: str,
    probe: ProbeResult,
    profile: ProfileConfig,
    vaapi_device: str,
    use_cpu: bool = False,
    ffmpeg_path: str = "ffmpeg",
    hardware_config: HardwareConfig | None = None,
    cc_subtitle_path: str | None = None,
) -> FFmpegCommand:
    """Build the ffmpeg command from probe data and profile config."""
    plan = plan_from_probe(input_path, probe, profile)
    if output_path != plan.output.output_path:
        plan.output.output_path = output_path
    return build_encode_command_from_plan(
        plan,
        profile,
        vaapi_device,
        use_cpu=use_cpu,
        ffmpeg_path=ffmpeg_path,
        hardware_config=hardware_config,
        cc_subtitle_path=cc_subtitle_path,
    )


def _audio_input_index_by_source_index(
    all_audio: list[StreamInfo], source_index: int
) -> int:
    """Get the audio-relative index (0:a:N) for a source stream index."""
    indices = [s.index for s in all_audio]
    try:
        return indices.index(source_index)
    except ValueError:
        raise ValueError(
            f"Audio stream index {source_index} not found in available audio streams: {indices}"
        ) from None


def _sub_input_index_by_source_index(
    all_subs: list[StreamInfo], source_index: int
) -> int:
    """Get the subtitle-relative index (0:s:N) for a source stream index."""
    indices = [s.index for s in all_subs]
    try:
        return indices.index(source_index)
    except ValueError:
        raise ValueError(
            f"Subtitle stream index {source_index} not found in available subtitle streams: {indices}"
        ) from None


def _subtitle_codec(source_codec: str | None, profile: ProfileConfig) -> str:
    """Copy subtitles, except MP4-only text tracks that Matroska cannot hold.

    ``mov_text`` (tx3g) copied into mkv aborts the mux with "Subtitle codec 94213
    is not supported", failing the whole task — convert it to SRT instead.
    """
    container = profile.output.container.strip().lstrip(".").lower()
    if container == "mkv":
        return MATROSKA_SUBTITLE_CONVERSIONS.get(source_codec or "", "copy")
    return "copy"


def _default_audio_bitrate(profile: ProfileConfig, language: str) -> int:
    return profile.audio.add_stereo.bitrate
