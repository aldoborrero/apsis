//! Profile-rule override resolution (US2). Pure — no NATS, no I/O.
//!
//! The coordinator builds a CEL context per file (from the [`Probe`] + file facts),
//! then [`resolve_effective_profile`] layers each matching rule's `set` over the base
//! profile — ordered, last-write-wins — into an *effective* [`Profile`] that feeds the
//! single `plan()` call. The worker never sees rules (Constitution III).
//!
//! **Mechanism**: the base profile is serialized to JSON, each `set` is applied by its
//! dotted path, and the result is deserialized back to a `Profile` — so the effective
//! profile is revalidated by the exact same rules as a static one (`deny_unknown_fields`
//! turns a mistyped path like `video.heigth` into a hard error; range checks re-run).
//!
//! **Literal vs CEL `set` value**: a `set` value is a CEL expression iff it is a string
//! wrapped in `${…}` (e.g. `"${video.height >= 2160 ? 24 : 22}"`); every other value —
//! a bare string (`"av1"`), an int, a bool, a table — is a literal assigned verbatim.
//! The explicit marker is unambiguous where a type-directed heuristic could not be: both
//! `"video.codec" = "av1"` (literal on a string field) and
//! `"add_stereo.bitrate" = "${… ? '192k' : '128k'}"` (computed on a string field) target
//! string-typed fields, so nothing but an explicit marker can tell them apart. `${…}` is
//! therefore reserved — a profile value that must be the literal text `${x}` is not
//! expressible (no real codec/preset/bitrate value looks like that).

use cel::{Context, Program, Value as CelValue};
use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::config::{Profile, ProfileRule, SetValue};
use crate::error::EngineError;
use crate::probe::{Probe, StreamInfo};

/// The `cel_context_version` this builder implements (contracts/cel-context.md).
/// Additive fields bump the doc's minor; a removal/rename bumps this.
pub const CEL_CONTEXT_VERSION: u32 = 1;

/// Per-file facts the CEL context needs that the [`Probe`] does not carry (the
/// coordinator supplies them from the filesystem + container detection).
pub struct FileFacts<'a> {
    pub path: &'a str,
    pub container: &'a str,
    pub duration: f64,
    pub size: i64,
}

// --- CEL activation structs: these mirror contracts/cel-context.md field-for-field.
//     Renaming/removing a field here is a breaking context-version change. ---

// Integer fields are `i64`, not `u32`: the `cel` crate serializes `u32` to `Value::UInt`,
// and this CEL implementation only does arithmetic between matching operand types — so a
// `UInt` context field would fail to add/subtract/divide against an `Int` literal (e.g.
// `video.height / 90`). `i64` → `Value::Int` matches CEL literals and the contract (`int`).
#[derive(Serialize)]
struct CtxVideo {
    codec: String,
    width: i64,
    height: i64,
    bitrate: i64,
    hdr: bool,
    color_transfer: String,
    bit_depth: i64,
}

#[derive(Serialize)]
struct CtxAudio {
    index: i64,
    codec: String,
    language: String,
    channels: i64,
    title: String,
    default: bool,
}

#[derive(Serialize)]
struct CtxSubtitle {
    index: i64,
    codec: String,
    language: String,
    forced: bool,
}

impl CtxVideo {
    fn from_stream(s: &StreamInfo, hdr: bool) -> Self {
        Self {
            codec: s.codec.clone(),
            width: i64::from(s.width),
            height: i64::from(s.height),
            bitrate: i64::from(s.bitrate),
            hdr,
            color_transfer: s.color_transfer.clone(),
            bit_depth: i64::from(s.bit_depth),
        }
    }
}

impl CtxAudio {
    fn from_stream(s: &StreamInfo) -> Self {
        Self {
            index: i64::from(s.index),
            codec: s.codec.clone(),
            language: s.language.clone(),
            channels: i64::from(s.channels),
            title: s.title.clone(),
            default: s.is_default,
        }
    }
}

impl CtxSubtitle {
    fn from_stream(s: &StreamInfo) -> Self {
        Self {
            index: i64::from(s.index),
            codec: s.codec.clone(),
            language: s.language.clone(),
            forced: s.forced,
        }
    }
}

/// Build the read-only CEL context for one file (contracts/cel-context.md).
///
/// # Errors
/// [`EngineError::Override`] if a context variable cannot be serialized into a CEL
/// value (not expected for these plain structs, but surfaced rather than panicked).
pub fn build_context(
    probe: &Probe,
    facts: &FileFacts<'_>,
) -> Result<Context<'static>, EngineError> {
    let mut ctx = Context::default();
    let add_err =
        |what: &'static str| move |e| EngineError::Override(format!("cel context `{what}`: {e}"));

    let video = probe
        .video
        .as_ref()
        .map(|v| CtxVideo::from_stream(v, probe.is_hdr()));
    ctx.add_variable("video", video).map_err(add_err("video"))?;

    let audio: Vec<CtxAudio> = probe.audio.iter().map(CtxAudio::from_stream).collect();
    ctx.add_variable("audio", audio).map_err(add_err("audio"))?;

    let subtitles: Vec<CtxSubtitle> = probe
        .subtitles
        .iter()
        .map(CtxSubtitle::from_stream)
        .collect();
    ctx.add_variable("subtitles", subtitles)
        .map_err(add_err("subtitles"))?;

    ctx.add_variable("path", facts.path.to_string())
        .map_err(add_err("path"))?;
    ctx.add_variable("container", facts.container.to_string())
        .map_err(add_err("container"))?;
    ctx.add_variable("duration", facts.duration)
        .map_err(add_err("duration"))?;
    ctx.add_variable("size", facts.size)
        .map_err(add_err("size"))?;
    Ok(ctx)
}

/// Layer matching rules' `set` over `base` into an effective profile.
///
/// Rules are applied in order; a later `set` on the same dotted path wins. The result
/// is re-validated as a full profile (ranges + unknown-field rejection).
///
/// # Errors
/// [`EngineError::Override`] on a CEL compile/eval failure, an unknown/ill-typed `set`
/// path, or an effective profile that fails validation; [`EngineError::ParseJson`] if
/// the base profile cannot be serialized.
pub fn resolve_effective_profile(
    base: &Profile,
    rules: &[ProfileRule],
    ctx: &Context<'_>,
) -> Result<Profile, EngineError> {
    if rules.is_empty() {
        return Ok(base.clone());
    }
    let mut json = serde_json::to_value(base)?;
    for (ri, rule) in rules.iter().enumerate() {
        if rule_matches(rule, ctx, ri)? {
            for (path, sv) in &rule.set {
                let value = resolve_set_value(sv, path, ctx, ri)?;
                apply_dotted(&mut json, path, value, ri)?;
            }
        }
    }
    serde_json::from_value(json).map_err(|e| {
        EngineError::Override(format!("effective profile invalid after overrides: {e}"))
    })
}

fn rule_matches(rule: &ProfileRule, ctx: &Context<'_>, ri: usize) -> Result<bool, EngineError> {
    let when = &rule.when;
    let prog = Program::compile(when).map_err(|e| {
        EngineError::Override(format!("rule[{ri}] `when` {when:?} failed to compile: {e}"))
    })?;
    match prog.execute(ctx) {
        Ok(CelValue::Bool(b)) => Ok(b),
        Ok(other) => Err(EngineError::Override(format!(
            "rule[{ri}] `when` {when:?} must evaluate to a bool, got {other:?}"
        ))),
        Err(e) => Err(EngineError::Override(format!(
            "rule[{ri}] `when` {when:?} failed to evaluate: {e}"
        ))),
    }
}

/// Resolve one `set` value to the JSON to assign — a `${…}`-wrapped string is a CEL
/// expression (evaluated); everything else is a literal assigned verbatim (module doc).
fn resolve_set_value(
    sv: &SetValue,
    path: &str,
    ctx: &Context<'_>,
    ri: usize,
) -> Result<JsonValue, EngineError> {
    // A CEL expression is a string wrapped in `${…}`; every other value is a literal.
    let Some(expr) = sv.0.as_str().and_then(cel_expr) else {
        return Ok(sv.0.clone());
    };
    let prog = Program::compile(expr).map_err(|e| {
        EngineError::Override(format!(
            "rule[{ri}] `set` {path:?} = ${{{expr}}} failed to compile: {e}"
        ))
    })?;
    let val = prog.execute(ctx).map_err(|e| {
        EngineError::Override(format!("rule[{ri}] `set` {path:?} failed to evaluate: {e}"))
    })?;
    val.json().map_err(|e| {
        EngineError::Override(format!(
            "rule[{ri}] `set` {path:?}: CEL result is not a profile value: {e:?}"
        ))
    })
}

/// The inner CEL expression of a `${…}`-wrapped string, or `None` for a literal.
/// The value is trimmed first, so a stray leading/trailing space around the marker
/// (`"${expr} "`) is still recognized as CEL rather than silently taken literally.
fn cel_expr(s: &str) -> Option<&str> {
    s.trim()
        .strip_prefix("${")
        .and_then(|r| r.strip_suffix('}'))
}

/// Assign `value` at `path`, creating intermediate objects for absent (`null`) fields.
/// A path segment that lands on a non-object is an error; a leaf key that does not
/// exist on the target object is still inserted, so `deny_unknown_fields` rejects a
/// mistyped path when the effective profile is re-deserialized.
fn apply_dotted(
    root: &mut JsonValue,
    path: &str,
    value: JsonValue,
    ri: usize,
) -> Result<(), EngineError> {
    let bad = |msg: &str| EngineError::Override(format!("rule[{ri}] `set` {path:?}: {msg}"));
    let segs: Vec<&str> = path.split('.').collect();
    if segs.iter().any(|s| s.is_empty()) {
        return Err(bad("empty path segment"));
    }
    let (last, parents) = segs.split_last().expect("path has ≥1 segment");
    let mut cur = root;
    for seg in parents {
        let obj = cur
            .as_object_mut()
            .ok_or_else(|| bad("parent is not an object"))?;
        let entry = obj
            .entry((*seg).to_string())
            .or_insert_with(|| JsonValue::Object(serde_json::Map::new()));
        if entry.is_null() {
            *entry = JsonValue::Object(serde_json::Map::new());
        }
        cur = entry;
    }
    cur.as_object_mut()
        .ok_or_else(|| bad("parent is not an object"))?
        .insert((*last).to_string(), value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{QualityKind, QualityValue, VideoCodec};

    fn base_profile() -> Profile {
        serde_json::from_str(r#"{"video":{"codec":"hevc"},"audio":{},"subtitles":{},"output":{}}"#)
            .unwrap()
    }

    fn rules(json: &str) -> Vec<ProfileRule> {
        serde_json::from_str(json).unwrap()
    }

    fn video(codec: &str, width: u32, height: u32) -> StreamInfo {
        StreamInfo {
            codec_type: "video".into(),
            codec: codec.into(),
            width,
            height,
            ..Default::default()
        }
    }

    fn audio(codec: &str, channels: u32) -> StreamInfo {
        StreamInfo {
            codec_type: "audio".into(),
            codec: codec.into(),
            channels,
            ..Default::default()
        }
    }

    fn facts() -> FileFacts<'static> {
        FileFacts {
            path: "/media/tv/show/ep.mkv",
            container: "mkv",
            duration: 1800.0,
            size: 4_000_000_000,
        }
    }

    // The two-rule profile from the spec quickstart/schema. `video.codec` is a literal;
    // `video.quality.value` is a `${…}`-wrapped CEL expression.
    const SPEC_RULES: &str = r#"[
        {"when":"video.height >= 2160",
         "set":{"video.codec":"av1","video.quality.value":"${video.height >= 2160 ? 24 : 22}"}},
        {"when":"audio.exists(a, a.codec == 'truehd' && a.channels > 6)",
         "set":{"audio.transcode":{"codec":"eac3","bitrate":"640k"}}}
    ]"#;

    #[test]
    fn matching_file_layers_overrides() {
        // 2160p + TrueHD-7.1 → both rules fire: AV1, computed crf 24, E-AC3 audio.
        let probe = Probe {
            video: Some(video("h264", 3840, 2160)),
            audio: vec![audio("truehd", 8)],
            ..Default::default()
        };
        let ctx = build_context(&probe, &facts()).unwrap();
        let eff = resolve_effective_profile(&base_profile(), &rules(SPEC_RULES), &ctx).unwrap();

        assert_eq!(eff.video.codec, VideoCodec::Av1, "rule set video.codec");
        assert_eq!(eff.video.quality.mode, QualityKind::Auto, "mode untouched");
        assert_eq!(
            eff.video.quality.value,
            QualityValue::Num(24),
            "computed CEL value"
        );
        let tc = eff.audio.transcode.as_ref().expect("audio.transcode set");
        assert_eq!(tc.codec, "eac3");
        assert_eq!(tc.bitrate.as_arg(), "640k");
    }

    #[test]
    fn non_matching_file_is_unchanged() {
        // 1080p AAC → neither rule fires → effective == base.
        let probe = Probe {
            video: Some(video("h264", 1920, 1080)),
            audio: vec![audio("aac", 2)],
            ..Default::default()
        };
        let ctx = build_context(&probe, &facts()).unwrap();
        let base = base_profile();
        let eff = resolve_effective_profile(&base, &rules(SPEC_RULES), &ctx).unwrap();
        assert_eq!(eff, base, "no rule matched → base profile verbatim");
    }

    #[test]
    fn literal_string_is_not_evaluated_as_cel() {
        // No `${…}` marker → "av1" is a literal, never compiled as CEL (a bare `av1`
        // identifier would fail to resolve if it were).
        let probe = Probe {
            video: Some(video("h264", 3840, 2160)),
            ..Default::default()
        };
        let ctx = build_context(&probe, &facts()).unwrap();
        let r = rules(r#"[{"when":"true","set":{"video.codec":"av1"}}]"#);
        let eff = resolve_effective_profile(&base_profile(), &r, &ctx).unwrap();
        assert_eq!(eff.video.codec, VideoCodec::Av1);
    }

    #[test]
    fn literal_on_absent_optional_field_stays_literal() {
        // Regression: an absent Option<String> serializes to null, which the old
        // type-directed heuristic mis-read as "not a string" → CEL. With the `${…}`
        // marker, a bare literal on an unset field is unambiguously a literal.
        let probe = Probe {
            video: Some(video("h264", 3840, 2160)),
            ..Default::default()
        };
        let ctx = build_context(&probe, &facts()).unwrap();
        let r = rules(
            r#"[{"when":"true","set":{"video.max_resolution":"1080p","video.preset":"medium"}}]"#,
        );
        let eff = resolve_effective_profile(&base_profile(), &r, &ctx).unwrap();
        assert_eq!(eff.video.max_resolution.as_deref(), Some("1080p"));
        assert_eq!(
            eff.video.preset.as_deref(),
            Some("medium"),
            "not the CEL var"
        );
    }

    #[test]
    fn cel_marker_computes_a_string_typed_field() {
        // Regression: a computed value on a string-typed field — impossible under the
        // old heuristic — works with the explicit marker.
        let probe = Probe {
            video: Some(video("h264", 3840, 2160)),
            ..Default::default()
        };
        let ctx = build_context(&probe, &facts()).unwrap();
        let r = rules(
            r#"[{"when":"true","set":{"video.max_resolution":"${video.height >= 2160 ? '2160p' : '1080p'}"}}]"#,
        );
        let eff = resolve_effective_profile(&base_profile(), &r, &ctx).unwrap();
        assert_eq!(eff.video.max_resolution.as_deref(), Some("2160p"));
    }

    #[test]
    fn cel_arithmetic_on_context_ints() {
        // Regression (F1): context numeric fields are Int, so arithmetic against CEL
        // integer literals works (a UInt field would fail `div`/`add`/`sub`).
        let probe = Probe {
            video: Some(video("h264", 3840, 2160)),
            ..Default::default()
        };
        let ctx = build_context(&probe, &facts()).unwrap();
        let r = rules(r#"[{"when":"true","set":{"video.quality.value":"${video.height / 90}"}}]"#);
        let eff = resolve_effective_profile(&base_profile(), &r, &ctx).unwrap();
        assert_eq!(eff.video.quality.value, QualityValue::Num(24), "2160 / 90");
    }

    #[test]
    fn cel_marker_tolerates_surrounding_whitespace() {
        // A stray space around the marker still parses as CEL (not silently literal).
        let probe = Probe {
            video: Some(video("h264", 3840, 2160)),
            ..Default::default()
        };
        let ctx = build_context(&probe, &facts()).unwrap();
        let r =
            rules(r#"[{"when":"true","set":{"video.quality.value":"  ${video.height / 90} "}}]"#);
        let eff = resolve_effective_profile(&base_profile(), &r, &ctx).unwrap();
        assert_eq!(eff.video.quality.value, QualityValue::Num(24));
    }

    #[test]
    fn last_write_wins_across_rules() {
        let probe = Probe {
            video: Some(video("h264", 3840, 2160)),
            ..Default::default()
        };
        let ctx = build_context(&probe, &facts()).unwrap();
        let r = rules(
            r#"[{"when":"true","set":{"video.codec":"av1"}},
                {"when":"true","set":{"video.codec":"hevc"}}]"#,
        );
        let eff = resolve_effective_profile(&base_profile(), &r, &ctx).unwrap();
        assert_eq!(eff.video.codec, VideoCodec::Hevc, "later rule wins");
    }

    #[test]
    fn unknown_set_path_is_rejected() {
        // A mistyped field becomes an unknown key → deny_unknown_fields rejects it.
        let probe = Probe {
            video: Some(video("h264", 3840, 2160)),
            ..Default::default()
        };
        let ctx = build_context(&probe, &facts()).unwrap();
        let r = rules(r#"[{"when":"true","set":{"video.heigth":"1080p"}}]"#);
        let err = resolve_effective_profile(&base_profile(), &r, &ctx).unwrap_err();
        assert!(
            matches!(err, EngineError::Override(_)),
            "expected Override error, got {err:?}"
        );
    }

    #[test]
    fn bad_cel_predicate_errors() {
        let probe = Probe {
            video: Some(video("h264", 1920, 1080)),
            ..Default::default()
        };
        let ctx = build_context(&probe, &facts()).unwrap();
        // `heigth` is not a context field → evaluation error surfaces (not silent).
        let r = rules(r#"[{"when":"video.heigth > 0","set":{"video.codec":"av1"}}]"#);
        assert!(resolve_effective_profile(&base_profile(), &r, &ctx).is_err());
    }
}
