# Phase 1 Data Model: Rich profiles + CEL

Entities are `serde`-deserialized config types in `apsis-engine::config` (+ probe types in
`apsis-engine::probe`). "Validation" = deserialize-time (`serde` custom + `garde`) so an invalid
profile cannot be constructed — matching the existing engine convention (`deny_unknown_fields`,
`de_quality`). No runtime/DB state.

## Profile (extended)

The existing `Profile { video, audio, subtitles, output }` gains fields + a `rules` list.

### VideoConfig (extended)

| Field | Type | Default | Validation | Notes |
|-------|------|---------|-----------|-------|
| `codec` | enum `hevc\|av1` | — | required | (today) target codec |
| `quality` | `QualityMode` | `{auto,22}` | see QualityMode | **CHANGED**: was `u8`; back-compat shorthand `quality = 22` |
| `bit_depth` | u8 | 10 | — | (today) |
| `skip_codecs` | `[string]` | `[]` | — | (today) compliance gate |
| `hdr_policy` | enum | `copy` | — | (today) copy\|tonemap\|encode |
| `fallback` | enum | `cpu` | — | (today) cpu\|none |
| `preset` | string (opt) | none | — | **NEW** encoder speed (e.g. medium, slow) |
| `max_resolution` | string (opt) | none | `WxH` or `1080p\|720p…` | **NEW** downscale-if-larger |
| `crop` | enum `none\|auto` | `none` | — | **NEW** black-bar autocrop |
| `custom_args` | `[string]` | `[]` | — | **NEW** raw-ffmpeg escape hatch |
| `encoder` | enum `vaapi\|cpu` | `vaapi` | — | (today) **unchanged in this spec** — still selects the backend (`command.rs`). Moving encoder-selection to `worker.toml` for true portability is **spec 003** work, out of scope here; do NOT deprecate or silently ignore it now. |

### QualityMode (new)

`{ mode: qp\|crf\|bitrate\|vmaf, value: <int or bitrate-string> }`.

- Shorthand: a bare integer `quality = N` → `{ mode = auto, value = N }`.
- Validation: for `qp`/`crf`/`auto`, `value` is `0..=51` (reuses `de_quality`). For `bitrate`,
  `value` is a bitrate string (`"5M"`, `"8000k"`). `vmaf` accepted, materialization deferred (R5).
- `mode = auto` → backend picks qp (VAAPI) or crf (CPU) at command-build (R3), preserving today's
  behaviour.

### AudioConfig (extended)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `keep_languages`, `default_language`, `priority`, `remove_commentary`, `add_stereo`, `preserve_surround` | | | (today) |
| `transcode` | `{ codec: string, bitrate: Bitrate }` (opt) | none | **NEW** re-encode the KEPT tracks (not just the stereo clone) |
| `add_mono` | `{ codec, bitrate: Bitrate, languages: [string] }` (opt) | none | **NEW** generate a mono track (mirrors `add_stereo`) |
| `max_channels` | u32 (opt) | none | **NEW** cap channel count |
| `normalize` | bool | false | **NEW** loudnorm |

> **`StereoConfig.bitrate` migrates to `Bitrate` (compatible).** Today it is a bare `u32`
> (kbps, e.g. `128`). It becomes the `Bitrate` type below, which still accepts the bare int —
> so existing `add_stereo = { bitrate = 128, … }` loads unchanged — and additionally accepts
> the string forms. This is a **compatible change to an existing field** (called out per the
> reviewer), keeping one bitrate convention across `add_stereo`/`add_mono`/`transcode`/`quality`.

### Bitrate (new type)

A value accepted as **either** a bare integer (kbps: `128` → `128k`) **or** a string with a
unit (`"128k"`, `"5M"`, `"8000k"`). Custom `Deserialize`. Used by `add_stereo`, `add_mono`,
`audio.transcode`, and `QualityMode` (`mode = "bitrate"`). Back-compat: the bare int keeps
today's `add_stereo.bitrate = 128` working (SC-006).

### SubtitleConfig (extended)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `keep_languages`, `default_language`, `remove_formats`, `remove_commentary` | | | (today) |
| `order` | `[string]` | `[]` | **NEW** fixed positional order (language codes) |
| `forced_only` | bool | false | **NEW** keep/flag forced subs only |
| `extract` | `[string]` | `[]` | **NEW** extract to sidecar (e.g. `["srt"]`) |

### OutputConfig (extended)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `container`, `replace_original` | | | (today) |
| `conform` | bool | false | **NEW** drop streams the target container can't hold |
| `strip_metadata` | bool | false | **NEW** |
| `keep_chapters` | bool | true | **NEW** |

### Skip gates (fields on `VideoConfig` — decided; not a separate block)

| Field | Type | Notes |
|-------|------|-------|
| `skip_if_resolution_below` | string (opt) | **NEW** e.g. `"480p"`; **strictly below** — a source whose height is `< 480` is skipped; exactly 480 is processed |
| `skip_if_bitrate_below` | `Bitrate` (opt) | **NEW** e.g. `"2M"`; **strictly below** — bitrate `< 2M` is skipped |

They live directly under `[profiles.<name>.video]` (single committed location — since every
struct is `deny_unknown_fields`, the location is part of the public API and is fixed here, not
left to the implementer). *(skip-if-already-processed is NOT here — the coordinator's
`mtime:size` version-gate covers it.)*

## ProfileRule (new)

`Profile.rules: Vec<ProfileRule>` — TOML `[[profiles.<name>.rule]]`.

| Field | Type | Validation | Notes |
|-------|------|-----------|-------|
| `when` | string (CEL) | compile + canary-eval at load (R2) | predicate over the CEL context |
| `set` | map `dotted-field -> value` | dotted path must resolve to a known field; value validated as that field; a value may itself be a CEL string (computed) | last-write-wins layering (R6) |

**Relationship**: rules belong to a Profile; resolution layers matching rules' `set` over the base
(R6) → an **effective Profile** (same type), which is what `plan()` receives.

## CEL context (new — see contracts/cel-context.md)

The read-only activation CEL evaluates against, built per-file from the `Probe` + file facts.
Versioned, stable API. Summarised: `video{codec,width,height,bitrate,hdr,color_transfer,bit_depth}`,
`audio[]{index,codec,language,channels,title}`, `subtitles[]{index,codec,language,forced}`,
`path`, `container`, `duration`, `size`.

## Effective Profile (derived, not persisted)

`resolve_effective_profile(base: &Profile, rules: &[ProfileRule], ctx: &CelContext) -> Profile`
(pure, in `apsis-engine::overrides`). Steps: start from `base` → for each rule in order, if
`when(ctx)` → merge `set` (evaluating any CEL `set` values against `ctx`) last-write-wins →
validate the result (same rules as static fields) → return. Feeds the single `plan()` call.

## Resolution & construction order (defined — no implementer discretion)

The pipeline is fixed and testable; field interactions the reviewer flagged are pinned here.

**A. Per-file resolution (coordinator, before `plan()`):**
1. Build the CEL context from the probe.
2. **Layer rules** (R6) → effective profile.
3. **Skip evaluation** on the *effective* profile (so a rule may set a value a gate reads):
   `skip_codecs` first (already-compliant codec) → then `skip_if_resolution_below` /
   `skip_if_bitrate_below`. Any gate true ⇒ `should_skip`, no plan built.

**B. Construction (engine `plan()`), per stream kind:**
- **Video**: decide encode/copy → **crop → scale (`max_resolution`)** (crop-then-scale, so the
  downscale sees the cropped frame) → quality/preset → `custom_args` appended last, into the one
  command.
- **Audio**: language/commentary filter → **`transcode`** (re-encode kept tracks) → generate
  `add_stereo` / `add_mono` (from the best *kept* source; generated tracks are produced at their
  own `codec`/`bitrate`, not re-run through `transcode`) → `max_channels` cap → **failsafe** (if
  now empty and source had ≥1, restore the highest-priority source track, copied) → `normalize`.
- **Subtitles**: `remove_formats` / language / `remove_commentary` filter → `forced_only` →
  **`order`** (positions the survivors) → `extract` (a sidecar copy; the in-container track is
  still kept unless filtered out).
- **Output**: `conform` runs **after** all stream selection (drops any survivor the container
  can't hold) — the audio failsafe runs before `conform`, and `conform` MUST NOT drop the last
  audio stream (failsafe wins).

## Engine invariant (not a field)

**Keep ≥1 audio** (R8/FR-008): audio filtering never yields zero audio streams when the source
had ≥1. Enforced in `build_audio_plan`, unit-tested. This **consolidates and extends** the two
guards already in `audio.rs` (empty-after-language-filter, empty-after-commentary-removal) to
also cover the new filters (`max_channels`, `conform`) — one guard, not three.

## Validation summary

- Deserialize-time: `deny_unknown_fields` on every struct (a typo fails load); `QualityMode` range;
  bitrate-string parse; `max_resolution`/`skip_if_*` format parse.
- CEL: compile + canary-eval each `when`/`set` expression at load (R2).
- Post-resolution: the effective profile re-validated before planning (FR-015).
- Back-compat: `quality = N` and all existing fields load unchanged (SC-006).
