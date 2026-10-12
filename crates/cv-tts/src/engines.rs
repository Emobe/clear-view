//! Both engines behind one voice list (ADR 0012): OneCore voices first, then SAPI voices. A voice
//! picks the engine that speaks. Before one is picked, OneCore speaks with its own default voice.

use cv_core::voice::{Engine, VoiceId, VoiceInfo};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Globalization::GetUserDefaultUILanguage;

use crate::{OneCoreBackend, SapiBackend, SpeechBackend, lcid_to_tag};

/// OneCore voices, then SAPI voices, each list in its own order.
fn ordered(mut onecore: Vec<VoiceInfo>, sapi: Vec<VoiceInfo>) -> Vec<VoiceInfo> {
    onecore.extend(sapi);
    onecore
}

/// The Windows display language as a BCP-47 tag, for `cv_core::voice::choose_voice`. Empty if
/// Windows does not say.
pub fn display_language() -> String {
    lcid_to_tag(u32::from(unsafe { GetUserDefaultUILanguage() })).unwrap_or_default()
}

pub struct Engines {
    onecore: Option<OneCoreBackend>,
    sapi: Option<SapiBackend>,
    active: Engine,
}

impl Engines {
    /// Creates whichever engines start. An engine that fails is logged and left out; an error only
    /// if neither starts. COM must already be initialised (multithreaded).
    pub fn new() -> Result<Self, String> {
        let onecore = OneCoreBackend::new().map_err(|e| eprintln!("[tts] OneCore unavailable: {e}")).ok();
        let sapi = SapiBackend::new().map_err(|e| eprintln!("[tts] SAPI unavailable: {e}")).ok();
        if onecore.is_none() && sapi.is_none() {
            return Err("neither OneCore nor SAPI speech started".into());
        }
        let active = if onecore.is_some() { Engine::OneCore } else { Engine::Sapi };
        Ok(Self { onecore, sapi, active })
    }

    /// OneCore's default voice, for `cv_core::voice::choose_voice`.
    pub fn onecore_default(&self) -> Option<VoiceId> {
        self.onecore.as_ref()?.default_voice()
    }

    /// Every handle the speech thread waits on: OneCore's synthesis and audio events, SAPI's
    /// notify event. Signalled means call `take_finished`.
    pub fn wait_handles(&self) -> Vec<HANDLE> {
        let mut handles = Vec::new();
        if let Some(o) = &self.onecore {
            handles.push(o.wake_handle());
            handles.push(o.audio_handle());
        }
        if let Some(s) = &self.sapi {
            handles.push(s.wait_handle());
        }
        handles
    }

    fn both(&mut self) -> impl Iterator<Item = &mut dyn SpeechBackend> {
        let onecore = self.onecore.as_mut().map(|b| b as &mut dyn SpeechBackend);
        let sapi = self.sapi.as_mut().map(|b| b as &mut dyn SpeechBackend);
        onecore.into_iter().chain(sapi)
    }

    fn active_backend(&mut self) -> Option<&mut dyn SpeechBackend> {
        match self.active {
            Engine::OneCore => self.onecore.as_mut().map(|b| b as &mut dyn SpeechBackend),
            Engine::Sapi => self.sapi.as_mut().map(|b| b as &mut dyn SpeechBackend),
        }
    }
}

impl SpeechBackend for Engines {
    fn voices(&self) -> Vec<VoiceInfo> {
        ordered(
            self.onecore.as_ref().map(|b| b.voices()).unwrap_or_default(),
            self.sapi.as_ref().map(|b| b.voices()).unwrap_or_default(),
        )
    }

    fn set_voice(&mut self, voice: &VoiceId) -> bool {
        let ok = match voice.engine {
            Engine::OneCore => self.onecore.as_mut().is_some_and(|b| b.set_voice(voice)),
            Engine::Sapi => self.sapi.as_mut().is_some_and(|b| b.set_voice(voice)),
        };
        if ok {
            self.active = voice.engine;
        }
        ok
    }

    fn set_rate(&mut self, neutral: u32) {
        self.both().for_each(|b| b.set_rate(neutral));
    }

    fn set_volume(&mut self, neutral: u32) {
        self.both().for_each(|b| b.set_volume(neutral));
    }

    fn speak(&mut self, id: u32, text: &str) -> bool {
        self.active_backend().is_some_and(|b| b.speak(id, text))
    }

    fn stop(&mut self) {
        self.both().for_each(|b| b.stop());
    }

    fn take_finished(&mut self) -> Vec<u32> {
        self.both().flat_map(|b| b.take_finished()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(engine: Engine, id: &str) -> VoiceInfo {
        VoiceInfo { id: VoiceId { engine, id: id.into() }, name: id.into(), language: "en-GB".into() }
    }

    #[test]
    fn onecore_voices_come_before_sapi_voices() {
        let list = ordered(
            vec![voice(Engine::OneCore, "a"), voice(Engine::OneCore, "b")],
            vec![voice(Engine::Sapi, "c")],
        );
        let ids: Vec<_> = list.iter().map(|v| v.id.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
        assert!(ordered(Vec::new(), Vec::new()).is_empty());
    }
}
