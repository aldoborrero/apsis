# Contract: Profile config schema (`scheduler.toml`)

The complete `[profiles.<name>]` surface after this feature. `[new]` = added here; unmarked =
today. All structs are `deny_unknown_fields` (a typo fails load). Profiles are **hardware-agnostic**
(no device/encoder specifics — those live in `worker.toml`).

```toml
[profiles.<name>.video]
codec          = "hevc"                 # hevc | av1
quality        = { mode = "crf", value = 22 }   # mode: qp|crf|bitrate|vmaf ; OR shorthand: quality = 22
bit_depth      = 10
encoder        = "vaapi"                 # vaapi | cpu — UNCHANGED this spec (selects backend today);
                                         #   moving selection to worker.toml is spec 003, not here
skip_codecs    = ["hevc", "av1"]
hdr_policy     = "copy"                  # copy | tonemap | encode
fallback       = "cpu"                   # cpu | none
preset         = "medium"               # [new] encoder speed
max_resolution = "1080p"                # [new] downscale-if-larger ("1080p"|"1920x1080")
crop           = "auto"                 # [new] none | auto
custom_args    = ["-x265-params", "..."]# [new] raw-ffmpeg escape hatch
skip_if_resolution_below = "480p"       # [new] compliance gate
skip_if_bitrate_below    = "2M"         # [new] compliance gate

[profiles.<name>.audio]
keep_languages    = ["eng", "spa", "jpn"]
default_language  = "eng"
priority          = ["eng", "spa", "jpn"]
remove_commentary = true
preserve_surround = true
add_stereo = { codec = "aac", bitrate = 128,   channels = 2, languages = ["eng","spa"] }  # bitrate: int-kbps OR "128k"/"5M"
add_mono   = { codec = "aac", bitrate = "64k", languages = ["eng"] }           # [new]
transcode  = { codec = "opus", bitrate = "160k" }                              # [new] re-encode KEPT tracks
max_channels = 6                                                               # [new]
normalize    = false                                                          # [new] loudnorm

[profiles.<name>.subtitles]
keep_languages    = ["eng", "spa", "jpn"]
default_language  = "eng"
remove_formats    = ["hdmv_pgs_subtitle", "dvd_subtitle"]
remove_commentary = true
order             = ["eng", "spa", "jpn"]   # [new] fixed position
forced_only       = false                   # [new]
extract           = ["srt"]                 # [new] to sidecar

[profiles.<name>.output]
container        = "mkv"
replace_original = true
conform          = true          # [new] drop container-incompatible streams
strip_metadata   = false         # [new]
keep_chapters    = true          # [new]

# [new] conditional overrides — ordered, last-write-wins per field (research R6)
# A `set` value is a literal UNLESS it is a string wrapped in ${…} (a CEL expression).
[[profiles.<name>.rule]]
when = "video.height >= 2160"                       # CEL predicate (contracts/cel-context.md)
set  = { "video.codec" = "av1", "video.quality.value" = "${video.height >= 2160 ? 24 : 22}" }

[[profiles.<name>.rule]]
when = "audio.exists(a, a.codec == 'truehd')"
set  = { "audio.transcode" = { codec = "eac3", bitrate = "640k" } }
```

## Invariants

- **Back-compat**: `quality = N` still loads (→ `{mode=auto,value=N}`); `add_stereo.bitrate`
  still accepts the bare int `128` (now also `"128k"`, via the compatible `Bitrate` migration).
  Existing `scheduler.toml` loads without edits (SC-006).
- **Hardware-agnostic (as far as this spec goes)**: no `device` or `sei` in the profile;
  `quality.mode = auto` resolves to the backend's native RC at command-build (VAAPI qp / CPU crf),
  so the same profile yields an equivalent plan on the **two implemented backends (VAAPI, CPU)**
  (FR-009 / SC-002). `encoder` still lives in the profile (unchanged); moving encoder-selection to
  `worker.toml` — and validating NVENC portability — is **spec 003**.
- **Failsafe**: audio filtering never yields zero audio streams when the source had ≥1 (FR-008).
- **`set` dotted paths** must resolve to a real field; the value is validated as that field
  (e.g. `"video.quality.value"` obeys the 0–51 range). A value is a literal unless it is a
  string wrapped in `${…}`, which is a CEL expression evaluated against the file context — so
  both a literal and a computed value work on the same string-typed field (`"video.codec" =
  "av1"` vs `"video.codec" = "${video.height >= 2160 ? 'av1' : 'hevc'}"`). `${…}` is reserved.
