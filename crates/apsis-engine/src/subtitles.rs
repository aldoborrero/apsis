//! Subtitle stream filtering by format, language, and title (port of
//! `_engine/subtitles.py`).

use std::collections::HashSet;

use crate::config::SubtitleConfig;
use crate::constants::is_commentary;
use crate::probe::StreamInfo;

/// Expand short format names to ffprobe codec names (Python `FORMAT_ALIASES`).
fn banned_codecs(remove_formats: &[String]) -> HashSet<String> {
    let mut banned = HashSet::new();
    for fmt in remove_formats {
        match fmt.as_str() {
            "pgs" | "hdmv_pgs_subtitle" => banned.insert("hdmv_pgs_subtitle".to_string()),
            other => banned.insert(other.to_string()),
        };
    }
    banned
}

/// Filter subtitle streams: remove banned formats → keep matching languages →
/// remove commentary → `forced_only` → `order` (spec 004 pipeline order). Unlike
/// audio there is no "keep ≥1" failsafe — zero kept subtitles is a valid outcome.
#[must_use]
pub fn filter_subtitles(streams: &[StreamInfo], config: &SubtitleConfig) -> Vec<StreamInfo> {
    let banned = banned_codecs(&config.remove_formats);
    let mut kept: Vec<StreamInfo> = streams
        .iter()
        .filter(|s| {
            if banned.contains(&s.codec) {
                return false;
            }
            if !config.keep_languages.is_empty() && !config.keep_languages.contains(&s.language) {
                return false;
            }
            if config.remove_commentary && is_commentary(&s.title) {
                return false;
            }
            if config.forced_only && !s.forced {
                return false;
            }
            true
        })
        .cloned()
        .collect();

    // Reorder by the configured language priority (stable; unlisted languages last).
    if !config.order.is_empty() {
        kept.sort_by_key(|s| {
            config
                .order
                .iter()
                .position(|l| *l == s.language)
                .unwrap_or(usize::MAX)
        });
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(json: &str) -> SubtitleConfig {
        serde_json::from_str(json).unwrap()
    }

    fn sub(index: u32, lang: &str, codec: &str, title: &str) -> StreamInfo {
        StreamInfo {
            index,
            codec_type: "subtitle".into(),
            codec: codec.into(),
            language: lang.into(),
            title: title.into(),
            ..Default::default()
        }
    }

    #[test]
    fn forced_only_and_order() {
        let mut forced = sub(2, "eng", "subrip", "");
        forced.forced = true;
        let streams = [
            sub(0, "spa", "subrip", ""),
            sub(1, "eng", "subrip", ""),
            forced,
        ];
        // forced_only keeps only the forced track
        let kept = filter_subtitles(&streams, &cfg(r#"{"forced_only":true}"#));
        assert_eq!(kept.len(), 1);
        assert!(kept[0].forced);
        // order reorders kept subs by language priority (eng before spa)
        let kept = filter_subtitles(&streams, &cfg(r#"{"order":["eng","spa"]}"#));
        assert_eq!(
            kept.iter().map(|s| s.language.as_str()).collect::<Vec<_>>(),
            ["eng", "eng", "spa"]
        );
    }

    #[test]
    fn removes_image_formats_by_alias() {
        let streams = [
            sub(0, "eng", "subrip", ""),
            sub(1, "eng", "hdmv_pgs_subtitle", ""),
            sub(2, "eng", "dvd_subtitle", ""),
        ];
        let kept = filter_subtitles(
            &streams,
            &cfg(r#"{"remove_formats":["pgs","dvd_subtitle"]}"#),
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].codec, "subrip");
    }

    #[test]
    fn keeps_only_matching_languages() {
        let streams = [sub(0, "eng", "subrip", ""), sub(1, "spa", "subrip", "")];
        let kept = filter_subtitles(&streams, &cfg(r#"{"keep_languages":["spa"]}"#));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].language, "spa");
    }

    #[test]
    fn removes_commentary() {
        let streams = [
            sub(0, "eng", "subrip", "Full"),
            sub(1, "eng", "subrip", "Director's notes"),
        ];
        let kept = filter_subtitles(&streams, &cfg("{}"));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].title, "Full");
    }
}
