//! Voice settings and choice (roadmap 8.7a, ADR 0012): the neutral rate and volume each speech
//! engine is driven from, and which voice speaks when a saved voice is missing or none was saved.
//! Pure logic with no Windows types; the backends in cv-tts read the lists and call these.
//!
//! - Rate is one neutral setting, 0 to 100, with 50 the voice's normal rate. It means a position
//!   on the voice's own range, not words per minute, which neither engine documents.
//! - Volume is 0 to 100.
//! - A saved voice that is not in the list never changes the saved setting: the caller speaks with
//!   the voice returned here and shows [`VoiceChoice::Use::saved_missing`] in the panel.

use serde::{Deserialize, Serialize};

/// The neutral rate and volume range top (ADR 0012).
pub const NEUTRAL_MAX: u32 = 100;
/// The neutral rate that means the voice's normal rate.
pub const NEUTRAL_RATE_NORMAL: u32 = 50;
/// SAPI 5 `ISpVoice::SetRate` range: -10 to 10 (archived SAPI 5.4 docs).
pub const SAPI_RATE_MIN: i32 = -10;
pub const SAPI_RATE_MAX: i32 = 10;
/// OneCore `SpeakingRate` is 0.5 to 6.0 with 1.0 normal; some voices allow less.
pub const ONECORE_RATE_MIN: f64 = 0.5;
pub const ONECORE_RATE_NORMAL: f64 = 1.0;
/// Where the neutral rate 100 lands on OneCore. A starting value: 8.3 and the tester's ear decide
/// it (ADR 0012). The platform allows up to 6.0.
pub const ONECORE_RATE_MAX: f64 = 3.0;

/// The speech engine a voice belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Engine {
    OneCore,
    Sapi,
}

/// A voice as it is saved: the engine and the engine's own id (the SAPI token id, the OneCore
/// `VoiceInformation.Id`), never the display name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VoiceId {
    pub engine: Engine,
    pub id: String,
}

/// An installed voice, as the backends list it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceInfo {
    pub id: VoiceId,
    pub name: String,
    /// BCP-47 language tag, for example "en-GB".
    pub language: String,
}

/// SAPI `SetRate` value for a neutral rate: linear, 0 to -10, 50 to 0, 100 to 10. Above 100 is 100.
pub fn sapi_rate(neutral: u32) -> i32 {
    let n = neutral.min(NEUTRAL_MAX) as f64;
    let steps = (n - NEUTRAL_RATE_NORMAL as f64) / 5.0;
    (steps.round() as i32).clamp(SAPI_RATE_MIN, SAPI_RATE_MAX)
}

/// OneCore `SpeakingRate` for a neutral rate: 0.5 to 1.0 across 0 to 50, then 1.0 to `max` across
/// 50 to 100. A `max` below 1.0 or not a number counts as 1.0. The voice's own narrower limits
/// are the OneCore backend's to apply.
pub fn onecore_rate(neutral: u32, max: f64) -> f64 {
    let n = neutral.min(NEUTRAL_MAX) as f64;
    let half = NEUTRAL_RATE_NORMAL as f64;
    if n <= half {
        ONECORE_RATE_MIN + (ONECORE_RATE_NORMAL - ONECORE_RATE_MIN) * (n / half)
    } else {
        let max = max.max(ONECORE_RATE_NORMAL);
        ONECORE_RATE_NORMAL + (max - ONECORE_RATE_NORMAL) * ((n - half) / (NEUTRAL_MAX as f64 - half))
    }
}

/// SAPI `SetVolume` value: 0 to 100 as is. Above 100 is 100.
pub fn sapi_volume(neutral: u32) -> u16 {
    neutral.min(NEUTRAL_MAX) as u16
}

/// OneCore `AudioVolume`: 0.0 to 1.0. Above 100 is 1.0.
pub fn onecore_volume(neutral: u32) -> f64 {
    neutral.min(NEUTRAL_MAX) as f64 / NEUTRAL_MAX as f64
}

/// Which voice speaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceChoice {
    /// Speak with `voice`. `saved_missing` is true when a voice was saved but is not installed
    /// now (uninstalled, or its engine is gone): log it once and show it in the panel, and keep
    /// the saved setting so a reinstalled voice comes back.
    Use { voice: VoiceInfo, saved_missing: bool },
    /// No voices at all: speech cannot be turned on.
    NoVoices,
}

/// Choose the voice (ADR 0012, "Voices"), in this order:
///
/// 1. the saved voice, if it is in `voices`;
/// 2. the OneCore default (`onecore_default`), if it is in `voices`;
/// 3. the first SAPI voice in the Windows display language (`display_language`, a BCP-47 tag):
///    an exact tag match, then the same primary language;
/// 4. the first SAPI voice;
/// 5. the first voice in the list (only OneCore voices and no default given; not in the ADR, so
///    that a non-empty list never gives [`VoiceChoice::NoVoices`]).
pub fn choose_voice(
    saved: Option<&VoiceId>,
    voices: &[VoiceInfo],
    onecore_default: Option<&VoiceId>,
    display_language: &str,
) -> VoiceChoice {
    let find = |id: &VoiceId| voices.iter().find(|v| &v.id == id);
    let saved_found = saved.and_then(find);
    let saved_missing = saved.is_some() && saved_found.is_none();
    let found = saved_found
        .or_else(|| onecore_default.and_then(find))
        .or_else(|| sapi_for_language(voices, display_language))
        .or_else(|| voices.iter().find(|v| v.id.engine == Engine::Sapi))
        .or_else(|| voices.first());
    match found {
        Some(voice) => VoiceChoice::Use { voice: voice.clone(), saved_missing },
        None => VoiceChoice::NoVoices,
    }
}

fn sapi_for_language<'a>(voices: &'a [VoiceInfo], language: &str) -> Option<&'a VoiceInfo> {
    let sapi = || voices.iter().filter(|v| v.id.engine == Engine::Sapi);
    let primary = |tag: &str| tag.split('-').next().unwrap_or("").to_ascii_lowercase();
    sapi()
        .find(|v| v.language.eq_ignore_ascii_case(language))
        .or_else(|| {
            let want = primary(language);
            if want.is_empty() {
                return None;
            }
            sapi().find(|v| primary(&v.language) == want)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(engine: Engine, id: &str, language: &str) -> VoiceInfo {
        VoiceInfo { id: VoiceId { engine, id: id.into() }, name: id.into(), language: language.into() }
    }

    fn id(engine: Engine, id: &str) -> VoiceId {
        VoiceId { engine, id: id.into() }
    }

    fn used(choice: &VoiceChoice) -> (&str, bool) {
        match choice {
            VoiceChoice::Use { voice, saved_missing } => (voice.id.id.as_str(), *saved_missing),
            VoiceChoice::NoVoices => panic!("expected a voice"),
        }
    }

    // The dev PC's lists (ADR 0012), cut down.
    fn voices() -> Vec<VoiceInfo> {
        vec![
            voice(Engine::OneCore, "oc-george", "en-GB"),
            voice(Engine::OneCore, "oc-hazel", "en-GB"),
            voice(Engine::OneCore, "oc-david", "en-US"),
            voice(Engine::Sapi, "sapi-hazel", "en-GB"),
            voice(Engine::Sapi, "sapi-david", "en-US"),
            voice(Engine::Sapi, "sapi-haruka", "ja-JP"),
        ]
    }

    #[test]
    fn sapi_rate_ends_and_middle() {
        assert_eq!(sapi_rate(0), -10);
        assert_eq!(sapi_rate(50), 0);
        assert_eq!(sapi_rate(100), 10);
    }

    #[test]
    fn sapi_rate_rounds_to_the_nearest_step() {
        assert_eq!(sapi_rate(52), 0);
        assert_eq!(sapi_rate(53), 1);
        assert_eq!(sapi_rate(47), -1);
        assert_eq!(sapi_rate(48), 0);
        assert_eq!(sapi_rate(46), -1);
    }

    #[test]
    fn sapi_rate_never_goes_down_as_the_setting_goes_up() {
        let mut last = i32::MIN;
        for n in 0..=100 {
            let r = sapi_rate(n);
            assert!(r >= last, "rate fell at {n}");
            assert!((SAPI_RATE_MIN..=SAPI_RATE_MAX).contains(&r));
            last = r;
        }
    }

    #[test]
    fn sapi_rate_above_100_is_100() {
        assert_eq!(sapi_rate(101), 10);
        assert_eq!(sapi_rate(u32::MAX), 10);
    }

    #[test]
    fn onecore_rate_ends_and_middle() {
        assert_eq!(onecore_rate(0, ONECORE_RATE_MAX), 0.5);
        assert_eq!(onecore_rate(50, ONECORE_RATE_MAX), 1.0);
        assert_eq!(onecore_rate(100, ONECORE_RATE_MAX), ONECORE_RATE_MAX);
    }

    #[test]
    fn onecore_rate_is_linear_on_each_side_of_normal() {
        assert!((onecore_rate(25, 3.0) - 0.75).abs() < 1e-12);
        assert!((onecore_rate(75, 3.0) - 2.0).abs() < 1e-12);
        assert!((onecore_rate(75, 6.0) - 3.5).abs() < 1e-12);
    }

    #[test]
    fn onecore_rate_never_goes_down_and_stays_in_range() {
        for max in [1.0, 1.5, ONECORE_RATE_MAX, 6.0] {
            let mut last = 0.0;
            for n in 0..=100 {
                let r = onecore_rate(n, max);
                assert!(r >= last, "rate fell at {n} with max {max}");
                assert!((0.5..=max).contains(&r));
                last = r;
            }
        }
    }

    #[test]
    fn onecore_rate_bad_max_counts_as_normal() {
        assert_eq!(onecore_rate(100, 0.2), 1.0);
        assert_eq!(onecore_rate(100, f64::NAN), 1.0);
        assert_eq!(onecore_rate(100, -5.0), 1.0);
    }

    #[test]
    fn onecore_rate_above_100_is_100() {
        assert_eq!(onecore_rate(500, 3.0), 3.0);
    }

    #[test]
    fn volumes_map_each_engine() {
        assert_eq!(sapi_volume(0), 0);
        assert_eq!(sapi_volume(80), 80);
        assert_eq!(sapi_volume(100), 100);
        assert_eq!(onecore_volume(0), 0.0);
        assert_eq!(onecore_volume(50), 0.5);
        assert_eq!(onecore_volume(100), 1.0);
    }

    #[test]
    fn volumes_above_100_are_100() {
        assert_eq!(sapi_volume(101), 100);
        assert_eq!(sapi_volume(u32::MAX), 100);
        assert_eq!(onecore_volume(101), 1.0);
        assert_eq!(onecore_volume(u32::MAX), 1.0);
    }

    #[test]
    fn a_saved_voice_that_is_installed_is_used() {
        let saved = id(Engine::Sapi, "sapi-david");
        let c = choose_voice(Some(&saved), &voices(), Some(&id(Engine::OneCore, "oc-george")), "en-GB");
        assert_eq!(used(&c), ("sapi-david", false));
    }

    #[test]
    fn no_saved_voice_uses_the_onecore_default() {
        let c = choose_voice(None, &voices(), Some(&id(Engine::OneCore, "oc-george")), "en-GB");
        assert_eq!(used(&c), ("oc-george", false));
    }

    #[test]
    fn a_missing_saved_voice_falls_back_and_says_so() {
        let saved = id(Engine::Sapi, "sapi-gone");
        let c = choose_voice(Some(&saved), &voices(), Some(&id(Engine::OneCore, "oc-george")), "en-GB");
        assert_eq!(used(&c), ("oc-george", true));
    }

    #[test]
    fn a_saved_voice_of_an_engine_that_is_gone_is_missing() {
        let list: Vec<_> = voices().into_iter().filter(|v| v.id.engine == Engine::Sapi).collect();
        let saved = id(Engine::OneCore, "oc-hazel");
        let c = choose_voice(Some(&saved), &list, None, "en-GB");
        assert_eq!(used(&c), ("sapi-hazel", true));
    }

    #[test]
    fn the_same_id_on_the_other_engine_is_not_the_saved_voice() {
        let list = vec![voice(Engine::Sapi, "hazel", "en-GB"), voice(Engine::OneCore, "hazel", "en-GB")];
        let saved = id(Engine::OneCore, "hazel");
        let c = choose_voice(Some(&saved), &list, None, "en-GB");
        match c {
            VoiceChoice::Use { voice, saved_missing } => {
                assert_eq!(voice.id.engine, Engine::OneCore);
                assert!(!saved_missing);
            }
            VoiceChoice::NoVoices => panic!("expected a voice"),
        }
    }

    #[test]
    fn an_onecore_default_not_in_the_list_is_ignored() {
        let c = choose_voice(None, &voices(), Some(&id(Engine::OneCore, "oc-gone")), "en-US");
        assert_eq!(used(&c), ("sapi-david", false));
    }

    #[test]
    fn without_onecore_the_first_sapi_voice_in_the_display_language_is_used() {
        let list: Vec<_> = voices().into_iter().filter(|v| v.id.engine == Engine::Sapi).collect();
        assert_eq!(used(&choose_voice(None, &list, None, "en-US")), ("sapi-david", false));
        assert_eq!(used(&choose_voice(None, &list, None, "ja-JP")), ("sapi-haruka", false));
        assert_eq!(used(&choose_voice(None, &list, None, "EN-gb")), ("sapi-hazel", false));
    }

    #[test]
    fn the_display_language_falls_back_to_the_same_primary_language() {
        let list: Vec<_> = voices().into_iter().filter(|v| v.id.engine == Engine::Sapi).collect();
        assert_eq!(used(&choose_voice(None, &list, None, "en-AU")), ("sapi-hazel", false));
        assert_eq!(used(&choose_voice(None, &list, None, "ja")), ("sapi-haruka", false));
    }

    #[test]
    fn an_unmatched_display_language_uses_the_first_sapi_voice() {
        let list: Vec<_> = voices().into_iter().filter(|v| v.id.engine == Engine::Sapi).collect();
        assert_eq!(used(&choose_voice(None, &list, None, "de-DE")), ("sapi-hazel", false));
        assert_eq!(used(&choose_voice(None, &list, None, "")), ("sapi-hazel", false));
    }

    #[test]
    fn a_sapi_voice_is_preferred_to_onecore_voices_when_there_is_no_default() {
        let list = vec![voice(Engine::OneCore, "oc-a", "en-GB"), voice(Engine::Sapi, "sapi-a", "de-DE")];
        assert_eq!(used(&choose_voice(None, &list, None, "en-GB")), ("sapi-a", false));
    }

    #[test]
    fn onecore_voices_only_and_no_default_uses_the_first_voice() {
        let list: Vec<_> = voices().into_iter().filter(|v| v.id.engine == Engine::OneCore).collect();
        assert_eq!(used(&choose_voice(None, &list, None, "en-US")), ("oc-george", false));
    }

    #[test]
    fn no_voices_means_no_voices_with_or_without_a_saved_voice() {
        let saved = id(Engine::Sapi, "sapi-david");
        assert_eq!(choose_voice(None, &[], None, "en-GB"), VoiceChoice::NoVoices);
        assert_eq!(choose_voice(Some(&saved), &[], None, "en-GB"), VoiceChoice::NoVoices);
    }

    #[test]
    fn voice_id_round_trips_through_json() {
        let v = id(Engine::OneCore, "x");
        let s = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<VoiceId>(&s).unwrap(), v);
    }
}
