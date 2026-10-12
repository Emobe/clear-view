//! The OneCore engine (`Windows.Media.SpeechSynthesis`, ADR 0012) with our own WASAPI playback.
//!
//! `speak` starts a synthesis at once. The WinRT completion callback runs on a thread-pool thread,
//! reads the finished stream into plain bytes without blocking, hands them over and signals
//! [`OneCoreBackend::wake_handle`]; no WinRT object reaches the owner thread except the ones it
//! created. The owner thread then calls `take_finished`, which moves the results in, plays the
//! front utterance and reports the ones that ended. Playback is event driven: the owner also waits
//! on [`OneCoreBackend::audio_handle`]. Utterances play in the order `speak` was called.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cv_core::voice::{Engine, ONECORE_RATE_MAX, VoiceId, VoiceInfo, onecore_rate, onecore_volume};
use windows::{
    Media::SpeechSynthesis::{
        SpeechAppendedSilence, SpeechPunctuationSilence, SpeechSynthesisStream, SpeechSynthesizer,
        VoiceInformation,
    },
    Storage::Streams::DataReader,
    Win32::Foundation::HANDLE,
    core::HSTRING,
};
use windows_future::IAsyncOperation;

use crate::SpeechBackend;
use crate::event::Event;
use crate::wasapi::Player;
use crate::wav::parse_wav;

type Bytes = Result<Vec<u8>, String>;

/// Reads all of `stream` into bytes and calls `done` once, on whatever thread finishes the read.
/// Nothing here blocks.
fn read_stream(stream: SpeechSynthesisStream, done: Arc<dyn Fn(Bytes) + Send + Sync>) {
    let start = || -> windows::core::Result<()> {
        let size = stream.Size()? as u32;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        let load = reader.LoadAsync(size)?;
        let done = done.clone();
        load.when(move |loaded| {
            let bytes = loaded.and_then(|got| {
                let mut bytes = vec![0u8; got as usize];
                reader.ReadBytes(&mut bytes)?;
                Ok(bytes)
            });
            let _ = stream.Close();
            done(bytes.map_err(|e| format!("reading the synthesis stream failed: {e}")));
        })
    };
    if let Err(e) = start() {
        done(Err(format!("reading the synthesis stream failed: {e}")));
    }
}

enum State {
    Synthesizing(IAsyncOperation<SpeechSynthesisStream>),
    Ready(Vec<u8>),
    Failed(String),
}

struct Entry {
    id: u32,
    state: State,
    started: Instant,
}

struct Synthesized {
    id: u32,
    bytes: Bytes,
}

pub struct OneCoreBackend {
    synth: SpeechSynthesizer,
    /// Signalled when a synthesis completes.
    wake: Arc<Event>,
    /// Signalled by WASAPI when the output buffer wants data.
    audio: Event,
    results: Arc<Mutex<Vec<Synthesized>>>,
    queue: VecDeque<Entry>,
    playing: Option<(u32, Player)>,
    finished: Vec<u32>,
}

fn voice_info(v: &VoiceInformation) -> Option<VoiceInfo> {
    Some(VoiceInfo {
        id: VoiceId { engine: Engine::OneCore, id: v.Id().ok()?.to_string() },
        name: v.DisplayName().ok()?.to_string(),
        language: v.Language().map(|l| l.to_string()).unwrap_or_default(),
    })
}

impl OneCoreBackend {
    /// Creates the synthesizer. COM must already be initialised (multithreaded).
    pub fn new() -> Result<Self, String> {
        let synth = SpeechSynthesizer::new().map_err(|e| format!("SpeechSynthesizer failed: {e}"))?;
        // The shortest silences, for echo (ADR 0012).
        if let Ok(options) = synth.Options() {
            let _ = options.SetAppendedSilence(SpeechAppendedSilence::Min);
            let _ = options.SetPunctuationSilence(SpeechPunctuationSilence::Min);
        }
        Ok(Self {
            synth,
            wake: Arc::new(Event::new()?),
            audio: Event::new()?,
            results: Arc::default(),
            queue: VecDeque::new(),
            playing: None,
            finished: Vec::new(),
        })
    }

    /// OneCore's default voice, if there is one.
    pub fn default_voice(&self) -> Option<VoiceId> {
        let v = SpeechSynthesizer::DefaultVoice().ok()?;
        Some(VoiceId { engine: Engine::OneCore, id: v.Id().ok()?.to_string() })
    }

    /// Signalled when a synthesis completes. Wait on it and call `take_finished`.
    pub fn wake_handle(&self) -> HANDLE {
        self.wake.handle()
    }

    /// Signalled while audio plays, each time the device wants data. Wait on it and call
    /// `take_finished`.
    pub fn audio_handle(&self) -> HANDLE {
        self.audio.handle()
    }

    /// When the oldest synthesis still running began, for the engine timeout (ADR 0013).
    pub fn oldest_synthesis(&self) -> Option<Instant> {
        self.queue
            .iter()
            .find(|e| matches!(e.state, State::Synthesizing(_)))
            .map(|e| e.started)
    }

    fn all_voices(&self) -> Vec<VoiceInformation> {
        let Ok(list) = SpeechSynthesizer::AllVoices() else { return Vec::new() };
        let count = list.Size().unwrap_or(0);
        (0..count).filter_map(|i| list.GetAt(i).ok()).collect()
    }

    /// Moves finished syntheses in, then plays and reports until nothing more can move.
    fn advance(&mut self) {
        let results = std::mem::take(&mut *self.results.lock().unwrap_or_else(|e| e.into_inner()));
        for r in results {
            // An id that is no longer queued was stopped; drop its result.
            if let Some(entry) = self.queue.iter_mut().find(|e| e.id == r.id) {
                entry.state = match r.bytes {
                    Ok(bytes) => State::Ready(bytes),
                    Err(message) => State::Failed(message),
                };
            }
        }
        loop {
            if let Some((id, player)) = &mut self.playing {
                match player.pump() {
                    Ok(false) => return,
                    Ok(true) => {}
                    Err(e) => eprintln!("[tts] playback of utterance {id} failed: {e}"),
                }
                self.finished.push(*id);
                self.playing = None;
            }
            match self.queue.front().map(|e| &e.state) {
                Some(State::Ready(_)) => {
                    let Some(Entry { id, state: State::Ready(bytes), .. }) = self.queue.pop_front() else {
                        return;
                    };
                    match self.play(&bytes) {
                        Ok(Some(player)) => self.playing = Some((id, player)),
                        Ok(None) => self.finished.push(id),
                        Err(e) => {
                            eprintln!("[tts] utterance {id} dropped: {e}");
                            self.finished.push(id);
                        }
                    }
                }
                Some(State::Failed(_)) => {
                    if let Some(Entry { id, state: State::Failed(message), .. }) = self.queue.pop_front() {
                        eprintln!("[tts] utterance {id} dropped: {message}");
                        self.finished.push(id);
                    }
                }
                _ => return,
            }
        }
    }

    /// A player for the audio in `wav_bytes`, or None when it holds no samples.
    fn play(&self, wav_bytes: &[u8]) -> Result<Option<Player>, String> {
        let wav = parse_wav(wav_bytes)?;
        if wav.data.is_empty() {
            return Ok(None);
        }
        Player::start(&wav, wav_bytes[wav.data.clone()].to_vec(), &self.audio).map(Some)
    }
}

impl SpeechBackend for OneCoreBackend {
    fn voices(&self) -> Vec<VoiceInfo> {
        self.all_voices().iter().filter_map(voice_info).collect()
    }

    fn set_voice(&mut self, voice: &VoiceId) -> bool {
        if voice.engine != Engine::OneCore {
            return false;
        }
        let found = self.all_voices().into_iter().find(|v| v.Id().is_ok_and(|id| id == voice.id.as_str()));
        match found.map(|v| self.synth.SetVoice(&v)) {
            Some(Ok(())) => true,
            Some(Err(e)) => {
                eprintln!("[tts] set voice {} failed: {e}", voice.id);
                false
            }
            None => false,
        }
    }

    fn set_rate(&mut self, neutral: u32) {
        let rate = onecore_rate(neutral, ONECORE_RATE_MAX);
        if let Err(e) = self.synth.Options().and_then(|o| o.SetSpeakingRate(rate)) {
            eprintln!("[tts] SetSpeakingRate({rate}) failed: {e}");
        }
    }

    fn set_volume(&mut self, neutral: u32) {
        if let Err(e) = self.synth.Options().and_then(|o| o.SetAudioVolume(onecore_volume(neutral))) {
            eprintln!("[tts] SetAudioVolume failed: {e}");
        }
    }

    fn speak(&mut self, id: u32, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        let op = match self.synth.SynthesizeTextToStreamAsync(&HSTRING::from(text)) {
            Ok(op) => op,
            Err(e) => {
                eprintln!("[tts] SynthesizeTextToStreamAsync failed: {e}");
                return false;
            }
        };
        let results = self.results.clone();
        let wake = self.wake.clone();
        let deliver: Arc<dyn Fn(Bytes) + Send + Sync> = Arc::new(move |bytes| {
            results.lock().unwrap_or_else(|e| e.into_inner()).push(Synthesized { id, bytes });
            wake.set();
        });
        let on_done = deliver.clone();
        let started = op.when(move |stream| match stream {
            Ok(stream) => read_stream(stream, on_done),
            Err(e) => on_done(Err(format!("synthesis failed: {e}"))),
        });
        if let Err(e) = started {
            eprintln!("[tts] watching the synthesis failed: {e}");
            return false;
        }
        self.queue.push_back(Entry { id, state: State::Synthesizing(op), started: Instant::now() });
        true
    }

    fn stop(&mut self) {
        for entry in self.queue.drain(..) {
            if let State::Synthesizing(op) = entry.state {
                let _ = op.Cancel();
            }
        }
        if let Some((_, player)) = self.playing.take() {
            player.stop();
        }
        self.results.lock().unwrap_or_else(|e| e.into_inner()).clear();
        // Stopped utterances are never reported.
        self.finished.clear();
    }

    fn take_finished(&mut self) -> Vec<u32> {
        self.advance();
        std::mem::take(&mut self.finished)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    use windows::Win32::System::Threading::WaitForSingleObject;

    /// Needs OneCore voices installed; plays nothing: `cargo test -p cv-tts -- --ignored`.
    #[test]
    #[ignore]
    fn onecore_lists_voices_and_synthesizes_without_playing() {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap() };
        {
            let mut backend = OneCoreBackend::new().unwrap();
            let voices = backend.voices();
            assert!(!voices.is_empty());
            assert!(voices.iter().all(|v| v.id.engine == Engine::OneCore && !v.name.is_empty()));
            assert!(backend.default_voice().is_some_and(|d| voices.iter().any(|v| v.id == d)));
            assert!(backend.set_voice(&voices[0].id));
            assert!(!backend.speak(1, ""));
            assert!(backend.speak(2, "Hello."));
            assert!(backend.oldest_synthesis().is_some());
            // Wait for the synthesis (no playback: take_finished is not called).
            unsafe { WaitForSingleObject(backend.wake_handle(), 10_000) };
            let results = backend.results.lock().unwrap();
            let done = results.iter().find(|r| r.id == 2).expect("a result for id 2");
            let bytes = done.bytes.as_ref().expect("synthesis succeeded");
            let wav = parse_wav(bytes).expect("a 16-bit PCM WAV");
            assert!(!wav.data.is_empty());
        }
        unsafe { CoUninitialize() };
    }
}
