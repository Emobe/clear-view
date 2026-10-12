//! Speech engines (ADR 0012). Two backends behind one interface, [`SpeechBackend`]: the SAPI 5
//! backend (8.7b) and the OneCore backend with WASAPI playback (8.7c), and [`Engines`], which puts
//! them behind one voice list. A backend lists voices, picks one, sets rate and volume, speaks one
//! string at a time and stops. None of them owns a thread: the speech thread (8.8b, ADR 0013) owns
//! the backends and waits on their handles, and until then a dev example drives them.
//!
//! The interface uses cv-core voice types and no Windows types, so a Linux backend can implement
//! it later. The caller initialises COM (multithreaded) on the thread that uses a backend.

mod engines;
mod event;
mod onecore;
mod wasapi;
mod wav;

pub use engines::{Engines, display_language};
pub use onecore::OneCoreBackend;

use cv_core::voice::{Engine, VoiceId, VoiceInfo, sapi_rate, sapi_volume};
use windows::{
    Win32::Foundation::HANDLE,
    Win32::Globalization::LCIDToLocaleName,
    Win32::Media::Speech::{
        ISpObjectToken, ISpObjectTokenCategory, ISpVoice, SPCAT_VOICES, SPEI_END_INPUT_STREAM,
        SPEVENT, SPF_ASYNC, SPF_PURGEBEFORESPEAK, SpObjectToken, SpObjectTokenCategory, SpVoice,
    },
    Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree},
    core::{HSTRING, IUnknown, PCWSTR, PWSTR},
};

/// What the speech thread needs from an engine (ADR 0012, ADR 0013).
///
/// Utterance ids are the caller's: `speak` is given one, and `take_finished` reports it back. The
/// caller keeps them unique. `take_finished` does not block, but it is also what moves OneCore
/// along (collecting synthesis, feeding the audio device), so call it each time a wait handle of
/// the backend is signalled and at least every few tens of milliseconds while audio plays.
pub trait SpeechBackend {
    /// Every installed voice of this engine.
    fn voices(&self) -> Vec<VoiceInfo>;
    /// Speaks with `voice` from the next utterance. False if it is not this engine's voice or the
    /// engine refuses it.
    fn set_voice(&mut self, voice: &VoiceId) -> bool;
    /// Neutral rate 0 to 100 (cv-core `voice`), applied from the next utterance.
    fn set_rate(&mut self, neutral: u32);
    /// Neutral volume 0 to 100 (cv-core `voice`), applied from the next utterance.
    fn set_volume(&mut self, neutral: u32);
    /// Queues `text` as utterance `id`; several calls play in order. False for empty text or if the
    /// engine refuses it.
    fn speak(&mut self, id: u32, text: &str) -> bool;
    /// Stops what is speaking and discards what is queued. Discarded ids are never reported.
    fn stop(&mut self);
    /// The ids whose speech has ended since the last call.
    fn take_finished(&mut self) -> Vec<u32>;
}

/// Maps an engine's own utterance numbers to the caller's ids.
#[derive(Default)]
struct IdMap(Vec<(u32, u32)>);

impl IdMap {
    fn insert(&mut self, engine: u32, id: u32) {
        self.0.push((engine, id));
    }

    /// The caller's id for the engine's number, forgetting the pair. None if it is not known, for
    /// example after `clear`.
    fn take(&mut self, engine: u32) -> Option<u32> {
        let at = self.0.iter().position(|&(e, _)| e == engine)?;
        Some(self.0.remove(at).1)
    }

    fn clear(&mut self) {
        self.0.clear();
    }
}

/// `SPFEI_FLAGCHECK`: SAPI wants these two reserved bits set in every interest mask.
const SPFEI_FLAGCHECK: u64 = (1 << 30) | (1 << 33);
const LOCALE_NAME_MAX_LENGTH: usize = 85;

/// Interest mask for the given `SPEVENTENUM` ids (the `SPFEI` macro).
fn event_interest(ids: &[i32]) -> u64 {
    ids.iter().fold(SPFEI_FLAGCHECK, |mask, id| mask | (1u64 << id))
}

/// The event id in an `SPEVENT`: the low 16 bits of its first bitfield word.
fn event_id(bitfield: i32) -> i32 {
    bitfield & 0xFFFF
}

/// The LCIDs in a voice token's `Language` attribute: hex numbers separated by `;`, for example
/// "409;9". Entries that are not hex are skipped.
fn parse_lcids(value: &str) -> Vec<u32> {
    value.split(';').filter_map(|part| u32::from_str_radix(part.trim(), 16).ok()).collect()
}

/// The last part of a token id, used as the name when a token has none:
/// `HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Speech\Voices\Tokens\TTS_MS_EN-US_ZIRA_11.0` gives
/// `TTS_MS_EN-US_ZIRA_11.0`.
fn name_from_id(id: &str) -> &str {
    id.rsplit('\\').next().unwrap_or(id)
}

pub(crate) fn lcid_to_tag(lcid: u32) -> Option<String> {
    let mut buf = [0u16; LOCALE_NAME_MAX_LENGTH];
    let len = unsafe { LCIDToLocaleName(lcid, Some(&mut buf), 0) };
    // The length includes the terminating zero; zero means the LCID is unknown.
    (len > 1).then(|| String::from_utf16_lossy(&buf[..len as usize - 1]))
}

/// Takes the text of a COM-allocated string and frees it.
fn take_pwstr(p: PWSTR) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { p.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
    text
}

fn token_string(token: &ISpObjectToken, value_name: PCWSTR) -> Option<String> {
    take_pwstr(unsafe { token.GetStringValue(value_name) }.ok()?)
}

fn token_language(token: &ISpObjectToken) -> String {
    let language = unsafe { token.OpenKey(windows::core::w!("Attributes")) }
        .ok()
        .and_then(|key| take_pwstr(unsafe { key.GetStringValue(windows::core::w!("Language")) }.ok()?));
    language
        .and_then(|value| parse_lcids(&value).into_iter().find_map(lcid_to_tag))
        .unwrap_or_default()
}

fn voice_info(token: &ISpObjectToken) -> Option<VoiceInfo> {
    let id = take_pwstr(unsafe { token.GetId() }.ok()?)?;
    let name = token_string(token, PCWSTR::null()).unwrap_or_else(|| name_from_id(&id).to_string());
    Some(VoiceInfo {
        language: token_language(token),
        id: VoiceId { engine: Engine::Sapi, id },
        name,
    })
}

/// The SAPI 5 engine (`ISpVoice`). Speech is asynchronous: `speak` returns at once, and
/// `take_finished` reports the ids whose speech has ended.
pub struct SapiBackend {
    voice: ISpVoice,
    streams: IdMap,
}

impl SapiBackend {
    /// Creates the voice on the default audio output. COM must already be initialised.
    pub fn new() -> Result<Self, String> {
        let voice: ISpVoice = unsafe { CoCreateInstance(&SpVoice, None, CLSCTX_ALL) }
            .map_err(|e| format!("CoCreateInstance ISpVoice failed: {e}"))?;
        unsafe {
            // The default audio device (ADR 0012).
            voice
                .SetOutput(None::<&IUnknown>, true)
                .map_err(|e| format!("SetOutput failed: {e}"))?;
            let interest = event_interest(&[SPEI_END_INPUT_STREAM.0]);
            voice.SetInterest(interest, interest).map_err(|e| format!("SetInterest failed: {e}"))?;
            voice.SetNotifyWin32Event().map_err(|e| format!("SetNotifyWin32Event failed: {e}"))?;
        }
        Ok(Self { voice, streams: IdMap::default() })
    }

    /// Signalled when SAPI has an event to report. Wait on it and call `take_finished`.
    pub fn wait_handle(&self) -> HANDLE {
        unsafe { self.voice.GetNotifyEventHandle() }
    }
}

impl SpeechBackend for SapiBackend {
    /// In the order SAPI lists them.
    fn voices(&self) -> Vec<VoiceInfo> {
        let mut out = Vec::new();
        let category: ISpObjectTokenCategory =
            match unsafe { CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL) } {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("[tts] voice category failed: {e}");
                    return out;
                }
            };
        let tokens = unsafe {
            category
                .SetId(SPCAT_VOICES, false)
                .and_then(|()| category.EnumTokens(PCWSTR::null(), PCWSTR::null()))
        };
        let tokens = match tokens {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[tts] voice list failed: {e}");
                return out;
            }
        };
        loop {
            let mut token: Option<ISpObjectToken> = None;
            let mut fetched = 0u32;
            if unsafe { tokens.Next(1, &mut token, Some(&mut fetched)) }.is_err() || fetched == 0 {
                break;
            }
            if let Some(info) = token.as_ref().and_then(voice_info) {
                out.push(info);
            }
        }
        out
    }

    fn set_voice(&mut self, voice: &VoiceId) -> bool {
        if voice.engine != Engine::Sapi {
            return false;
        }
        let result = (|| -> windows::core::Result<()> {
            let token: ISpObjectToken = unsafe { CoCreateInstance(&SpObjectToken, None, CLSCTX_ALL) }?;
            unsafe {
                token.SetId(SPCAT_VOICES, &HSTRING::from(voice.id.as_str()), false)?;
                self.voice.SetVoice(&token)
            }
        })();
        if let Err(e) = &result {
            eprintln!("[tts] set voice {} failed: {e}", voice.id);
        }
        result.is_ok()
    }

    fn set_rate(&mut self, neutral: u32) {
        if let Err(e) = unsafe { self.voice.SetRate(sapi_rate(neutral)) } {
            eprintln!("[tts] SetRate failed: {e}");
        }
    }

    fn set_volume(&mut self, neutral: u32) {
        if let Err(e) = unsafe { self.voice.SetVolume(sapi_volume(neutral)) } {
            eprintln!("[tts] SetVolume failed: {e}");
        }
    }

    /// Several calls queue in order; stopping first is the caller's rule (ADR 0013).
    fn speak(&mut self, id: u32, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        let mut stream = 0u32;
        let result = unsafe {
            self.voice.Speak(&HSTRING::from(text), SPF_ASYNC.0 as u32, Some(&mut stream))
        };
        match result {
            Ok(()) => {
                self.streams.insert(stream, id);
                true
            }
            Err(e) => {
                eprintln!("[tts] Speak failed: {e}");
                false
            }
        }
    }

    fn stop(&mut self) {
        let result = unsafe {
            self.voice.Speak(PCWSTR::null(), SPF_PURGEBEFORESPEAK.0 as u32, None)
        };
        if let Err(e) = result {
            eprintln!("[tts] stop failed: {e}");
        }
        // SAPI may still report the purged streams; with the pairs forgotten they map to nothing.
        self.streams.clear();
    }

    fn take_finished(&mut self) -> Vec<u32> {
        let mut finished = Vec::new();
        loop {
            let mut events = [SPEVENT::default(); 8];
            let mut fetched = 0u32;
            let got = unsafe { self.voice.GetEvents(events.len() as u32, events.as_mut_ptr(), &mut fetched) };
            if got.is_err() || fetched == 0 {
                break;
            }
            finished.extend(
                events[..fetched as usize]
                    .iter()
                    .filter(|e| event_id(e._bitfield) == SPEI_END_INPUT_STREAM.0)
                    .filter_map(|e| self.streams.take(e.ulStreamNum)),
            );
        }
        finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

    #[test]
    fn interest_sets_the_flag_check_bits_and_the_event() {
        let mask = event_interest(&[SPEI_END_INPUT_STREAM.0]);
        assert_eq!(mask, (1 << 2) | (1 << 30) | (1 << 33));
        assert_eq!(event_interest(&[]), SPFEI_FLAGCHECK);
    }

    #[test]
    fn engine_numbers_map_to_caller_ids_once() {
        let mut map = IdMap::default();
        map.insert(7, 100);
        map.insert(8, 101);
        assert_eq!(map.take(8), Some(101));
        assert_eq!(map.take(8), None);
        assert_eq!(map.take(9), None);
        assert_eq!(map.take(7), Some(100));
    }

    #[test]
    fn cleared_engine_numbers_map_to_nothing() {
        let mut map = IdMap::default();
        map.insert(7, 100);
        map.clear();
        assert_eq!(map.take(7), None);
    }

    #[test]
    fn event_id_is_the_low_sixteen_bits() {
        assert_eq!(event_id(0x0001_0002), 2);
        assert_eq!(event_id(2), 2);
    }

    #[test]
    fn lcids_are_hex_and_semicolon_separated() {
        assert_eq!(parse_lcids("409"), vec![0x409]);
        assert_eq!(parse_lcids("809;9"), vec![0x809, 0x9]);
        assert_eq!(parse_lcids(" 409 ; x ;;"), vec![0x409]);
        assert!(parse_lcids("").is_empty());
    }

    #[test]
    fn a_name_falls_back_to_the_last_part_of_the_id() {
        assert_eq!(
            name_from_id(r"HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Speech\Voices\Tokens\TTS_MS_EN-US_ZIRA_11.0"),
            "TTS_MS_EN-US_ZIRA_11.0"
        );
        assert_eq!(name_from_id("plain"), "plain");
    }

    #[test]
    fn lcids_become_language_tags() {
        assert_eq!(lcid_to_tag(0x809).as_deref(), Some("en-GB"));
        assert_eq!(lcid_to_tag(0x409).as_deref(), Some("en-US"));
    }

    /// Needs SAPI voices installed: `cargo test -p cv-tts -- --ignored`.
    #[test]
    #[ignore]
    fn sapi_lists_voices() {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap() };
        {
            let mut backend = SapiBackend::new().unwrap();
            let voices = backend.voices();
            assert!(!voices.is_empty());
            assert!(voices.iter().all(|v| v.id.engine == Engine::Sapi && !v.name.is_empty()));
            assert!(backend.set_voice(&voices[0].id));
            assert!(!backend.speak(1, ""));
        }
        unsafe { CoUninitialize() };
    }
}
