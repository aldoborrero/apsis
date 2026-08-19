# Contract: CEL evaluation context (`cel_context_version: 1`)

The **read-only** variables a profile-rule `when` predicate or `set` value is evaluated against.
This is a **stable, versioned public API** — CEL expressions in `scheduler.toml` program against
it. Additive fields bump the minor; a removal/rename bumps `cel_context_version`. Built per-file
by `apsis-engine` from the `Probe` + file facts, immediately before override resolution.

## Top-level variables

| Variable | Type | Source | Notes |
|----------|------|--------|-------|
| `video` | `Video?` | probe video stream | absent (`null`) if the file has no video |
| `audio` | `list<Audio>` | probe audio streams | possibly empty |
| `subtitles` | `list<Subtitle>` | probe subtitle streams | possibly empty |
| `path` | `string` | coordinator-space file path | e.g. `/media/tv/show/ep.mkv` |
| `container` | `string` | source container | e.g. `mkv`, `avi` (no dot) |
| `duration` | `double` | seconds | `0.0` if unknown |
| `size` | `int` | bytes | file size |

## `Video`

| Field | Type | Notes |
|-------|------|-------|
| `codec` | `string` | e.g. `h264`, `hevc`, `av1` |
| `width` | `int` | pixels |
| `height` | `int` | pixels (use for resolution predicates) |
| `bitrate` | `int` | bits/sec (`0` if unknown) |
| `hdr` | `bool` | derived by `Probe::is_hdr()`: raw `color_transfer ∈ {"smpte2084","arib-std-b67"}` |
| `color_transfer` | `string` | **raw** ffprobe value (e.g. `smpte2084`, `bt709`) — compare against these, not `PQ`/`HLG` |
| `bit_depth` | `int` | 8 / 10 / 12 |

## `Audio`

| Field | Type | Notes |
|-------|------|-------|
| `index` | `int` | stream index |
| `codec` | `string` | e.g. `aac`, `eac3`, `truehd` |
| `language` | `string` | ISO code, `und` if unset |
| `channels` | `int` | 2, 6, 8… |
| `title` | `string` | may be empty |
| `default` | `bool` | default disposition (audio has no "forced" concept — that is subtitles) |

## `Subtitle`

| Field | Type | Notes |
|-------|------|-------|
| `index` | `int` | stream index |
| `codec` | `string` | e.g. `subrip`, `hdmv_pgs_subtitle` |
| `language` | `string` | ISO code |
| `forced` | `bool` | forced disposition |

## Supported CEL surface

Standard CEL: boolean/comparison/arithmetic operators, `in`, ternary `cond ? a : b`, `has()`,
`size()`, string methods (`startsWith`, `endsWith`, `contains`), and the comprehension macros
`exists`, `all`, `exists_one`, `filter`, `map`. **No** I/O, custom functions with side effects,
or unbounded loops (pure + terminating — guarantees the reconcile version-gate).

## Example expressions

```cel
# predicates (when)
video.height >= 2160
video != null && video.codec == 'h264' && video.bitrate > 8000000
audio.exists(a, a.codec == 'truehd' && a.channels > 6)
!audio.exists(a, a.language == 'eng')
subtitles.exists(s, s.forced)
path.startsWith('/media/anime')
duration < 300

# computed set values
video.height >= 2160 ? 'av1' : 'hevc'
video.height >= 2160 ? 24 : 22
size(audio) > 4 ? '192k' : '128k'
```

## Load-time validation

Every `when`/`set` CEL string is **compiled** and **canary-evaluated** against a synthetic
context where every field above is present with a typed sample value, at config load (fail-fast).
Unknown fields, syntax errors, and obvious type mismatches stop startup. (See research R2 — this
is the pragmatic stand-in for cel-go static checking, which cel-rust lacks.)
