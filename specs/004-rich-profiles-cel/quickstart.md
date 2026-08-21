# Quickstart: Rich profiles + CEL

Worked example using the reference operator's real libraries, then how to validate.

## 1. A profile with the new coverage fields (no CEL needed)

The `tv` profile, unchanged where it already worked, with the two operator asks (`add_mono`,
subtitle `order`) and a couple of coverage fields:

```toml
[profiles.tv.video]
codec       = "hevc"
quality     = 22                         # shorthand → {mode=auto, value=22}; QP on VAAPI, CRF on CPU
skip_codecs = ["hevc"]
hdr_policy  = "copy"
preset      = "medium"                   # new
max_resolution = "1080p"                 # new (4K sources downscale; 1080p untouched)

[profiles.tv.audio]
keep_languages    = ["eng", "spa", "jpn"]
default_language  = "eng"
remove_commentary = true
preserve_surround = true
add_stereo = { codec = "aac", bitrate = 128, channels = 2, languages = ["eng","spa","jpn"] }  # int-kbps (today) or "128k"
add_mono   = { codec = "aac", bitrate = "64k", languages = ["eng"] }          # new (operator ask)

[profiles.tv.subtitles]
keep_languages    = ["eng", "spa", "jpn"]
remove_formats    = ["hdmv_pgs_subtitle", "dvd_subtitle"]
remove_commentary = true
order             = ["eng", "spa", "jpn"]                                     # new (operator ask)

[profiles.tv.output]
container        = "mkv"
replace_original = true
conform          = true                  # new
```

This runs **unchanged** across the two implemented backends — VAAPI (rhea) and CPU — because the
profile names no hardware (`quality = 22` → QP on VAAPI, CRF on CPU). NVENC (sirius) portability
is added and validated in spec `003`.

## 2. Adding per-file logic with CEL (optional)

Say TV 4K should go to AV1 at slightly lower quality, and any TrueHD 7.1 audio should become
E-AC3 — without touching the engine:

```toml
[[profiles.tv.rule]]
when = "video.height >= 2160"
set  = { "video.codec" = "av1", "video.quality.value" = 24 }

[[profiles.tv.rule]]
when = "audio.exists(a, a.codec == 'truehd' && a.channels > 6)"
set  = { "audio.transcode" = { codec = "eac3", bitrate = "640k" } }
```

A 1080p AAC episode matches neither rule → planned with the base `tv` profile. A 2160p TrueHD-7.1
episode matches both → effective profile = base + AV1 + E-AC3 audio (last-write-wins per field).

Every `set` value above is a **literal**. To *compute* a value from the file, wrap a CEL
expression in `${…}`, e.g. `"video.quality.value" = "${video.height >= 2160 ? 24 : 22}"` or
`"video.codec" = "${video.height >= 2160 ? 'av1' : 'hevc'}"`.

## 3. Validate before deploying

```bash
# fail-fast: a bad field or CEL typo refuses to load
nix develop --command cargo run -p apsis-coordinator -- --check-config scheduler.toml
#   e.g. `when = "video.heigth >= 2160"`  → error: unknown field `heigth` in rule (profile tv)
#   e.g. `quality = { mode = "crf", value = 99 }` → error: video.quality.value must be 0..=51
```

```bash
# unit / golden tests for the new behaviour
nix develop --command cargo test -p apsis-engine profile   # schema + back-compat + QualityMode
nix develop --command cargo test -p apsis-engine overrides # rule layering + CEL eval
nix develop --command cargo test -p apsis-engine plan      # golden: probe+profile(+rules) -> FilePlan
```

## 4. What you do NOT touch

- **Hardware** (VAAPI `sei` device; NVENC — added in spec 003) → `worker.toml`, per host. The
  profile stays agnostic.
- **Hooks** (Sonarr rescan, Bazarr) → per-`[[library]]`, separate spec.
- **The engine's construction** (generating the mono track, ordering subs, conform) → done by
  `apsis-engine`, driven by the fields above. You configure; the engine builds.

## Success signals (maps to spec Success Criteria)

- A complete library-cleanup policy expressible in TOML alone (SC-001).
- Same profile → equivalent plan on the two implemented backends, VAAPI and CPU (SC-002).
- 4K→AV1 / TrueHD→E-AC3 / low-bitrate→skip via config, zero code (SC-003).
- A malformed rule stops startup, not mid-run (SC-004).
- Existing `scheduler.toml` loads unchanged (SC-006).
