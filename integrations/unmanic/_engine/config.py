"""Configuration models and YAML loader for pyflows."""

import os
import re
from pathlib import Path
from typing import Literal, Union

import yaml  # type: ignore[import-untyped]
from pydantic import BaseModel, Field, field_validator, model_validator

ConfigValue = Union[
    str, dict[str, "ConfigValue"], list["ConfigValue"], int, float, bool, None
]


def expand_env_vars(value: ConfigValue) -> ConfigValue:
    """Recursively expand ${VAR} references from environment."""
    if isinstance(value, str):

        def replace_env(match: re.Match[str]) -> str:
            return os.environ.get(match.group(1), match.group(0))

        return re.sub(r"\$\{([^}]+)\}", replace_env, value)
    elif isinstance(value, dict):
        return {k: expand_env_vars(v) for k, v in value.items()}
    elif isinstance(value, list):
        return [expand_env_vars(item) for item in value]
    return value


# --- Pydantic Models ---


class GeneralConfig(BaseModel):
    # Unmanic owns the queue/cache/scanner, so the daemon-only knobs (workers,
    # retry/scan timing, metrics port, concurrency slots, ...) are gone.
    # `temp_dir`/`db_path` remain optional only so daemon-era configs still load;
    # they are unused by the plugin. Pydantic ignores unknown keys by default.
    temp_dir: str | None = None
    db_path: str | None = None
    log_level: Literal["debug", "info", "warning", "error", "critical"] = "info"
    vaapi_device: str = "/dev/dri/renderD128"
    ffmpeg_path: str = "ffmpeg"
    ffprobe_path: str = "ffprobe"


VideoCodec = Literal["hevc", "av1"]


class VideoConfig(BaseModel):
    codec: VideoCodec
    bit_depth: int = 10
    encoder: Literal["vaapi", "cpu"] = "vaapi"
    quality: int = 22
    fallback: Literal["cpu", "none"] = "cpu"
    skip_codecs: list[str] = []
    hdr_policy: Literal["copy", "tonemap", "encode"] = "copy"

    @field_validator("quality")
    @classmethod
    def _validate_quality(cls, v: int) -> int:
        if not 0 <= v <= 51:
            raise ValueError(f"quality must be between 0 and 51, got {v}")
        return v


class StereoConfig(BaseModel):
    codec: str = "aac"
    bitrate: int = 128
    channels: int = 2
    languages: list[str] = []


class AudioConfig(BaseModel):
    keep_languages: list[str] = Field(default_factory=list)
    default_language: str = "eng"
    priority: list[str] = Field(default_factory=list)
    remove_commentary: bool = True
    add_stereo: StereoConfig = Field(default_factory=StereoConfig)
    preserve_surround: bool = True


class SubtitleConfig(BaseModel):
    keep_languages: list[str] = Field(default_factory=list)
    default_language: str = ""
    remove_formats: list[str] = Field(default_factory=list)
    remove_commentary: bool = True


class OutputConfig(BaseModel):
    container: str = "mkv"
    replace_original: bool = True


class ProfileConfig(BaseModel):
    video: VideoConfig
    audio: AudioConfig
    subtitles: SubtitleConfig
    output: OutputConfig


class LibraryConfig(BaseModel):
    # `path` + `profile` are what the plugin keys on: it picks a profile by which
    # configured library a file lives under. `path` must match what Unmanic sees.
    name: str
    path: str
    profile: str
    extensions: list[str] = ["mkv", "mp4", "avi"]


class VaapiConfig(BaseModel):
    device: str = "va"
    hw_decode_codecs: list[str] = Field(default_factory=lambda: ["hevc", "av1", "vp9"])
    sw_decode_codecs: list[str] = Field(default_factory=lambda: ["h264"])
    upload_filter: str = "format=nv12,hwupload_vaapi"
    async_depth: int = 4


class HardwareConfig(BaseModel):
    env: dict[str, str] = Field(default_factory=lambda: {"AMD_DEBUG": "noefc"})
    vaapi: VaapiConfig = Field(default_factory=VaapiConfig)


class PyflowsConfig(BaseModel):
    general: GeneralConfig = Field(default_factory=GeneralConfig)
    profiles: dict[str, ProfileConfig]
    libraries: list[LibraryConfig] = Field(default_factory=list)
    hardware: HardwareConfig = Field(default_factory=HardwareConfig)

    @model_validator(mode="after")
    def _validate_library_profiles(self) -> "PyflowsConfig":
        for lib in self.libraries:
            if lib.profile not in self.profiles:
                raise ValueError(
                    f"Library '{lib.name}' references profile '{lib.profile}' "
                    f"which does not exist. Available profiles: "
                    f"{list(self.profiles.keys())}"
                )
        return self


def load_config(path: Path) -> PyflowsConfig:
    """Load and validate config from a YAML file."""
    with open(path) as f:
        raw: ConfigValue = yaml.safe_load(f)
    raw = expand_env_vars(raw)
    return PyflowsConfig.model_validate(raw)
