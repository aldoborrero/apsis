//! Audio track selection, filtering, and stereo creation (port of `_engine/audio.py`).

use std::collections::{HashMap, HashSet};

use crate::config::AudioConfig;
use crate::constants::is_commentary;
use crate::probe::StreamInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioActionKind {
    Copy,
    Encode,
}

/// One planned output audio track: `Copy` (passthrough) or `Encode` (new AAC stereo).
/// The encode bitrate is not carried here — `command.rs` reads it from the profile's
/// `add_stereo` (the single source of truth), so no plan/command divergence is possible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioAction {
    pub stream: StreamInfo,
    pub action: AudioActionKind,
    pub codec: String,
    pub channels: u32,
}

/// Build the ordered list of audio actions from probe data and config.
#[must_use]
pub fn build_audio_plan(streams: &[StreamInfo], config: &AudioConfig) -> Vec<AudioAction> {
    // 1. Filter to keep_languages (empty = keep all); never drop ALL audio.
    let mut kept: Vec<&StreamInfo> = if config.keep_languages.is_empty() {
        streams.iter().collect()
    } else {
        let k: Vec<&StreamInfo> = streams
            .iter()
            .filter(|s| config.keep_languages.contains(&s.language))
            .collect();
        if k.is_empty() && !streams.is_empty() {
            streams.iter().collect()
        } else {
            k
        }
    };

    // 2. Remove commentary (fall back to the pre-filter list if all removed).
    if config.remove_commentary {
        let before = kept.clone();
        kept.retain(|s| !is_commentary(&s.title));
        if kept.is_empty() && !before.is_empty() {
            kept = before;
        }
    }

    // 3. Sort by priority (then original index).
    let priority: HashMap<&str, usize> = config
        .priority
        .iter()
        .enumerate()
        .map(|(i, lang)| (lang.as_str(), i))
        .collect();
    kept.sort_by_key(|s| {
        (
            priority.get(s.language.as_str()).copied().unwrap_or(999),
            s.index,
        )
    });

    // 4. Build actions: original (copy) then optional stereo/mono downmix per lang.
    let stereo_langs: HashSet<&str> = config
        .add_stereo
        .languages
        .iter()
        .map(String::as_str)
        .collect();
    let mut stereo_present: HashSet<String> = stereo_langs
        .iter()
        .filter(|&&lang| {
            kept.iter()
                .any(|s| s.language == lang && s.channels <= config.add_stereo.channels)
        })
        .map(|&lang| lang.to_string())
        .collect();

    // add_mono mirrors add_stereo but downmixes to 1 channel; skip if a mono track
    // in that language is already present.
    let mono_langs: HashSet<&str> = config
        .add_mono
        .as_ref()
        .map(|m| m.languages.iter().map(String::as_str).collect())
        .unwrap_or_default();
    let mut mono_present: HashSet<String> = mono_langs
        .iter()
        .filter(|&&lang| kept.iter().any(|s| s.language == lang && s.channels <= 1))
        .map(|&lang| lang.to_string())
        .collect();

    let mut actions = Vec::new();
    for s in kept {
        if s.channels <= 2 || config.preserve_surround {
            actions.push(AudioAction {
                stream: s.clone(),
                action: AudioActionKind::Copy,
                codec: String::new(),
                channels: 0,
            });
        }
        if s.channels > config.add_stereo.channels
            && stereo_langs.contains(s.language.as_str())
            && !stereo_present.contains(&s.language)
        {
            actions.push(AudioAction {
                stream: s.clone(),
                action: AudioActionKind::Encode,
                codec: config.add_stereo.codec.clone(),
                channels: config.add_stereo.channels,
            });
            stereo_present.insert(s.language.clone());
        }
        if let Some(mono) = &config.add_mono
            && s.channels > 1
            && mono_langs.contains(s.language.as_str())
            && !mono_present.contains(&s.language)
        {
            actions.push(AudioAction {
                stream: s.clone(),
                action: AudioActionKind::Encode,
                codec: mono.codec.clone(),
                channels: 1,
            });
            mono_present.insert(s.language.clone());
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(json: &str) -> AudioConfig {
        serde_json::from_str(json).unwrap()
    }

    fn audio(index: u32, lang: &str, channels: u32, title: &str) -> StreamInfo {
        StreamInfo {
            index,
            codec_type: "audio".into(),
            codec: "eac3".into(),
            language: lang.into(),
            title: title.into(),
            channels,
            ..Default::default()
        }
    }

    #[test]
    fn keep_languages_filters() {
        let streams = [audio(0, "eng", 2, ""), audio(1, "spa", 2, "")];
        let plan = build_audio_plan(&streams, &cfg(r#"{"keep_languages":["eng"]}"#));
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].stream.language, "eng");
    }

    #[test]
    fn never_drops_all_audio() {
        // keep_languages matches nothing → fall back to keeping every track.
        let streams = [audio(0, "eng", 2, ""), audio(1, "spa", 2, "")];
        let plan = build_audio_plan(&streams, &cfg(r#"{"keep_languages":["fre"]}"#));
        assert_eq!(plan.len(), 2);
    }

    #[test]
    fn add_mono_generates_a_one_channel_track() {
        // 5.1 English source + add_mono(eng) → copy 5.1 + a generated 1-channel encode.
        let streams = [audio(0, "eng", 6, "")];
        let plan = build_audio_plan(
            &streams,
            &cfg(r#"{"preserve_surround":true,"add_mono":{"languages":["eng"]}}"#),
        );
        assert_eq!(plan.len(), 2, "copy 5.1 + generated mono");
        assert_eq!(plan[0].action, AudioActionKind::Copy);
        assert_eq!(plan[1].action, AudioActionKind::Encode);
        assert_eq!(plan[1].channels, 1, "mono downmix");
        // a source already ≤1ch in that language is not duplicated
        let mono = [audio(0, "eng", 1, "")];
        let plan = build_audio_plan(&mono, &cfg(r#"{"add_mono":{"languages":["eng"]}}"#));
        assert!(plan.iter().all(|a| a.action == AudioActionKind::Copy));
    }

    #[test]
    fn removes_commentary_but_keeps_all_if_only_commentary() {
        let mixed = [
            audio(0, "eng", 2, "Main"),
            audio(1, "eng", 2, "Director Commentary"),
        ];
        let plan = build_audio_plan(&mixed, &cfg("{}"));
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].stream.title, "Main");

        let only_comm = [audio(0, "eng", 2, "Commentary")];
        let plan = build_audio_plan(&only_comm, &cfg("{}"));
        assert_eq!(plan.len(), 1); // fallback: don't drop everything
    }

    #[test]
    fn sorts_by_priority() {
        let streams = [audio(0, "eng", 2, ""), audio(1, "spa", 2, "")];
        let plan = build_audio_plan(&streams, &cfg(r#"{"priority":["spa","eng"]}"#));
        assert_eq!(plan[0].stream.language, "spa");
        assert_eq!(plan[1].stream.language, "eng");
    }

    #[test]
    fn adds_stereo_downmix_for_surround() {
        let streams = [audio(0, "eng", 6, "Surround")];
        let plan = build_audio_plan(&streams, &cfg(r#"{"add_stereo":{"languages":["eng"]}}"#));
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].action, AudioActionKind::Copy); // keep 5.1 (preserve_surround default)
        assert_eq!(plan[1].action, AudioActionKind::Encode);
        assert_eq!(plan[1].channels, 2);
        assert_eq!(plan[1].codec, "aac");
    }

    #[test]
    fn no_extra_stereo_when_one_present() {
        let streams = [audio(0, "eng", 6, "Surround"), audio(1, "eng", 2, "Stereo")];
        let plan = build_audio_plan(&streams, &cfg(r#"{"add_stereo":{"languages":["eng"]}}"#));
        // 5.1 copy + stereo copy, but NO generated stereo (one already present).
        assert_eq!(
            plan.iter()
                .filter(|a| a.action == AudioActionKind::Encode)
                .count(),
            0
        );
        assert_eq!(plan.len(), 2);
    }

    #[test]
    fn drops_surround_when_preserve_off() {
        let streams = [audio(0, "eng", 6, "Surround")];
        let plan = build_audio_plan(
            &streams,
            &cfg(r#"{"preserve_surround":false,"add_stereo":{"languages":["eng"]}}"#),
        );
        // original 5.1 dropped, only the generated stereo remains.
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].action, AudioActionKind::Encode);
    }
}
