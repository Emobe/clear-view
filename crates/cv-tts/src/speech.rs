//! The speech thread (roadmap 8.8b, ADR 0013). One thread owns the engines and talks to nothing
//! else: no UI Automation, no MSAA, no pointer reads. What cuts off what is the pure
//! [`SpeechQueue`] in cv-core; this thread gives the engines one utterance at a time and reports
//! back what they finished.
//!
//! The thread is MTA with no window, so a plain `WaitForMultipleObjects` is enough. It waits on
//! the command event, the core-event event and the engines' own handles. That call returns only
//! the lowest signalled index, so every wake runs a whole pass over every source and never
//! trusts which handle fired.
//!
//! With speech off (`tts_enabled` false) no engine exists and no audio device is open. Requests
//! and core events are dropped. Engines are made when speech is first turned on and dropped when
//! it is turned off. `CV_TTS_LOG=1` prints a timestamped `[tts]` line for each step, to measure
//! the stop.

use std::fmt;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cv_core::voice::{
    SpeechState, SpeechStatus, VoiceChoice, VoiceId, choose_voice, neutral_rate_from_legacy,
};
use cv_core::{
    CoreEvent, EventHub, SharedState, SpeechOp, SpeechQueue, SpeechSettings, Utterance, utterances_for,
};
use windows::Win32::Foundation::WAIT_FAILED;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Threading::{INFINITE, WaitForMultipleObjects};

use crate::event::Event;
use crate::{Engines, SpeechBackend, display_language};

/// A OneCore synthesis that has not completed after this long is cancelled (ADR 0013).
const SYNTHESIS_TIMEOUT: Duration = Duration::from_secs(5);
/// How often to ask the engines what finished while something is speaking. Their handles wake
/// the thread too; this is the backstop `SpeechBackend::take_finished` asks for.
const PLAY_POLL: Duration = Duration::from_millis(50);
/// How long [`SpeechHandle::shutdown`] waits for the thread.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(1);

enum Command {
    Say(Vec<Utterance>),
    /// Stop and clear the queue. Carries the time of the request, so a core event older than it
    /// is refused even when the thread handles the command late.
    Silence(Instant),
    /// Speech settings in `AppState` changed.
    Wake,
    /// Read the voice list again (the panel's voice picker was opened).
    RefreshVoices,
    Shutdown,
}

struct Inner {
    tx: Sender<Command>,
    command_event: Arc<Event>,
    status: Arc<Mutex<SpeechStatus>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

/// Talks to the speech thread. Cheap to clone, usable from any thread, and no call blocks: each
/// sends a plain-data request and signals the thread's event. After a failed start, or after
/// [`shutdown`](Self::shutdown), the calls do nothing.
#[derive(Clone)]
pub struct SpeechHandle {
    inner: Option<Arc<Inner>>,
}

impl SpeechHandle {
    /// Speak one utterance by its kind's rule (ignored while speech is off).
    pub fn say(&self, utterance: Utterance) {
        self.send(Command::Say(vec![utterance]));
    }

    /// Speak a list in order; the first item's kind decides what it cuts off.
    pub fn say_batch(&self, utterances: Vec<Utterance>) {
        self.send(Command::Say(utterances));
    }

    /// Stop speaking and clear the queue.
    pub fn silence(&self) {
        self.send(Command::Silence(Instant::now()));
    }

    /// The `tts_*` fields of `AppState` changed: the thread reads them again.
    pub fn wake(&self) {
        self.send(Command::Wake);
    }

    /// Read the voice list again; the status changes if it differs (ADR 0012: not polled).
    pub fn refresh_voices(&self) {
        self.send(Command::RefreshVoices);
    }

    /// What the settings panel shows about voices (roadmap 8.7d). A handle whose thread did not
    /// start reports `Failed`.
    pub fn status(&self) -> SpeechStatus {
        match &self.inner {
            Some(inner) => inner.status.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            None => SpeechStatus { state: SpeechState::Failed, ..SpeechStatus::default() },
        }
    }

    /// Stops the thread and waits up to one second for it; after that it ends with the process.
    pub fn shutdown(&self) {
        let Some(inner) = &self.inner else { return };
        self.send(Command::Shutdown);
        let Some(thread) = inner.thread.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            return;
        };
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        while !thread.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if thread.is_finished() {
            let _ = thread.join();
        } else {
            eprintln!("[tts] speech thread did not stop in time; it ends with the process");
        }
    }

    fn send(&self, command: Command) {
        if let Some(inner) = &self.inner
            && inner.tx.send(command).is_ok()
        {
            inner.command_event.set();
        }
    }
}

/// Starts the speech thread (ADR 0013). It subscribes to `events` with a wake, so core events
/// reach it without polling. `on_change` is called from the speech thread each time the published
/// [`SpeechStatus`] changes (the settings panel repaints from it). If the thread's events cannot
/// be made the handle is inert and one line says so.
pub fn spawn_speech(
    state: SharedState,
    events: &EventHub,
    on_change: impl Fn() + Send + Sync + 'static,
) -> SpeechHandle {
    let (command_event, core_event) = match (Event::new(), Event::new()) {
        (Ok(c), Ok(e)) => (Arc::new(c), Arc::new(e)),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("[tts] speech thread not started: {e}");
            return SpeechHandle { inner: None };
        }
    };
    let (tx, commands) = mpsc::channel();
    let status = Arc::new(Mutex::new(SpeechStatus::default()));
    let publisher = Publisher { status: status.clone(), on_change: Box::new(on_change) };
    let core = {
        let core_event = core_event.clone();
        events.subscribe_with_wake(move || core_event.set())
    };
    let thread = {
        let command_event = command_event.clone();
        thread::Builder::new().name("speech".into()).spawn(move || {
            run_thread(state, commands, core, command_event, core_event, publisher);
        })
    };
    match thread {
        Ok(thread) => SpeechHandle {
            inner: Some(Arc::new(Inner {
                tx,
                command_event,
                status,
                thread: Mutex::new(Some(thread)),
            })),
        },
        Err(e) => {
            eprintln!("[tts] speech thread not started: {e}");
            SpeechHandle { inner: None }
        }
    }
}

fn run_thread(
    state: SharedState,
    commands: Receiver<Command>,
    core: Receiver<CoreEvent>,
    command_event: Arc<Event>,
    core_event: Arc<Event>,
    publisher: Publisher,
) {
    if let Err(e) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
        eprintln!("[tts] speech thread: CoInitializeEx failed: {e}");
        return;
    }
    {
        let mut speaker = Speaker {
            state,
            commands,
            core,
            command_event,
            core_event,
            queue: SpeechQueue::new(),
            engines: None,
            gave_up: false,
            publisher,
            voice_applied: None,
            rate: None,
            volume: None,
            speaking: None,
            log: Log::new(),
        };
        while speaker.pass() {
            speaker.wait();
        }
        speaker.stop_engines();
    }
    unsafe { CoUninitialize() };
}

/// Where the speech thread puts what the panel shows, and how it tells the panel.
struct Publisher {
    status: Arc<Mutex<SpeechStatus>>,
    on_change: Box<dyn Fn() + Send + Sync>,
}

impl Publisher {
    /// Stores `new` and calls `on_change`, only if it differs from what is published.
    fn publish(&self, new: SpeechStatus) {
        {
            let mut current = self.status.lock().unwrap_or_else(|e| e.into_inner());
            if *current == new {
                return;
            }
            *current = new;
        }
        (self.on_change)();
    }

    fn state(&self) -> SpeechState {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).state
    }
}

/// The `CV_TTS_LOG=1` lines: `[tts]`, milliseconds since the thread started, what happened.
struct Log {
    on: bool,
    base: Instant,
}

impl Log {
    fn new() -> Self {
        static ON: OnceLock<bool> = OnceLock::new();
        let on = *ON.get_or_init(|| std::env::var("CV_TTS_LOG").is_ok_and(|v| v == "1"));
        Self { on, base: Instant::now() }
    }

    fn line(&self, what: fmt::Arguments) {
        if self.on {
            eprintln!("[tts] {:>7} ms {what}", self.base.elapsed().as_millis());
        }
    }
}

struct Speaker {
    state: SharedState,
    commands: Receiver<Command>,
    core: Receiver<CoreEvent>,
    command_event: Arc<Event>,
    core_event: Arc<Event>,
    queue: SpeechQueue,
    /// None while speech is off.
    engines: Option<Engines>,
    /// Speech is on but the engines could not start; not retried until it is turned off and on.
    gave_up: bool,
    publisher: Publisher,
    /// The saved voice the engines were last given (None inside: nothing saved). None outside:
    /// no voice chosen yet since the engines started.
    voice_applied: Option<Option<VoiceId>>,
    /// The neutral rate and volume last given to the engines.
    rate: Option<u32>,
    volume: Option<u32>,
    /// The id of the utterance the engines are speaking.
    speaking: Option<u32>,
    log: Log,
}

impl Speaker {
    /// One pass over every source. False once a shutdown was asked for.
    fn pass(&mut self) -> bool {
        let now = Instant::now();
        let commands: Vec<Command> = self.commands.try_iter().collect();
        if commands.iter().any(|c| matches!(c, Command::Shutdown)) {
            return false;
        }
        // Settings before the commands, so a request sent right after speech was turned on finds
        // the engines made.
        self.sync_settings(now);
        for command in commands {
            match command {
                Command::Say(items) => {
                    if self.engines.is_some() {
                        let ops = self.queue.push_batch(now, items);
                        self.run_ops(now, ops);
                    }
                }
                Command::Silence(at) => self.silence(at),
                Command::RefreshVoices => self.refresh_voices(),
                Command::Wake | Command::Shutdown => {}
            }
        }
        let events: Vec<CoreEvent> = self.core.try_iter().collect();
        if self.engines.is_some() {
            let settings = SpeechSettings { enabled: true };
            for event in &events {
                if !self.queue.accepts_event(event.at()) {
                    continue;
                }
                let items = utterances_for(event, &settings);
                if !items.is_empty() {
                    let ops = self.queue.push_batch(now, items);
                    self.run_ops(now, ops);
                }
            }
        }
        self.collect_finished(now);
        self.queue.tick(now);
        self.check_synthesis_timeout(now);
        true
    }

    /// Reads the speech fields, makes or drops the engines when speech goes on or off, gives the
    /// engines a changed voice, and gives a changed rate or volume to the engines (they apply
    /// from the next utterance).
    fn sync_settings(&mut self, now: Instant) {
        let (enabled, rate, volume, voice) = {
            let s = self.state.read();
            (s.tts_enabled, neutral_rate_from_legacy(s.tts_rate), s.tts_volume, s.tts_voice.clone())
        };
        if !enabled {
            self.gave_up = false;
            if self.engines.is_some() {
                self.log.line(format_args!("speech off"));
                let ops = self.queue.silence(now);
                self.run_ops(now, ops);
                self.stop_engines();
            }
            self.publisher.publish(SpeechStatus::default());
            return;
        }
        if self.engines.is_none() && !self.gave_up {
            self.start_engines();
        }
        if self.engines.is_some() && self.voice_applied.as_ref() != Some(&voice) {
            self.apply_voice(&voice, true);
        }
        if let Some(engines) = &mut self.engines {
            if self.rate != Some(rate) {
                engines.set_rate(rate);
                self.rate = Some(rate);
            }
            if self.volume != Some(volume) {
                engines.set_volume(volume);
                self.volume = Some(volume);
            }
        }
    }

    fn start_engines(&mut self) {
        match Engines::new() {
            Ok(engines) => self.engines = Some(engines),
            Err(e) => {
                eprintln!("[tts] speech cannot start: {e}");
                self.gave_up = true;
                self.publisher.publish(SpeechStatus { state: SpeechState::Failed, ..SpeechStatus::default() });
            }
        }
    }

    /// Chooses the voice for the saved one (ADR 0012), gives it to the engines and publishes the
    /// list. A saved voice that is not installed is kept in the settings; one line says so when
    /// `log` is set. With no voices at all speech cannot run: the engines are dropped and it is
    /// not retried until speech is turned off and on.
    fn apply_voice(&mut self, saved: &Option<VoiceId>, log: bool) {
        let Some(engines) = &mut self.engines else { return };
        let voices = engines.voices();
        let default = engines.onecore_default();
        match choose_voice(saved.as_ref(), &voices, default.as_ref(), &display_language()) {
            VoiceChoice::Use { voice, saved_missing } => {
                let set = engines.set_voice(&voice.id);
                if log {
                    if saved_missing {
                        eprintln!("[tts] the saved voice is not installed; using {}", voice.name);
                    }
                    if set {
                        eprintln!("[tts] speech on: {:?} voice {}", voice.id.engine, voice.name);
                    } else {
                        eprintln!("[tts] speech on: could not set voice {}; using the default", voice.name);
                    }
                }
                self.voice_applied = Some(saved.clone());
                self.publisher.publish(SpeechStatus {
                    state: SpeechState::On,
                    voices,
                    speaking_with: set.then_some(voice.id),
                    saved_missing,
                });
            }
            VoiceChoice::NoVoices => {
                eprintln!("[tts] speech cannot start: no voices are installed");
                self.gave_up = true;
                self.stop_engines();
                self.publisher.publish(SpeechStatus { state: SpeechState::NoVoices, ..SpeechStatus::default() });
            }
        }
    }

    /// The panel's voice picker was opened: list the voices again.
    fn refresh_voices(&mut self) {
        if self.engines.is_none() || self.publisher.state() != SpeechState::On {
            return;
        }
        let saved = self.state.read().tts_voice.clone();
        self.apply_voice(&saved, false);
    }

    /// Drops the engines, which closes the audio device.
    fn stop_engines(&mut self) {
        if let Some(engines) = &mut self.engines {
            engines.stop();
        }
        self.engines = None;
        self.voice_applied = None;
        self.rate = None;
        self.volume = None;
        self.speaking = None;
    }

    fn silence(&mut self, at: Instant) {
        if self.engines.is_none() {
            return;
        }
        self.log.line(format_args!("silence handled {} ms after the request", at.elapsed().as_millis()));
        let ops = self.queue.silence(at);
        self.run_ops(Instant::now(), ops);
    }

    /// Does what the queue asked, in order. An utterance the engine refuses counts as finished,
    /// which may ask for the next one.
    fn run_ops(&mut self, now: Instant, ops: Vec<SpeechOp>) {
        let mut pending = std::collections::VecDeque::from(ops);
        while let Some(op) = pending.pop_front() {
            match op {
                SpeechOp::Stop => {
                    let began = Instant::now();
                    if let Some(engines) = &mut self.engines {
                        engines.stop();
                    }
                    self.speaking = None;
                    self.log.line(format_args!("engine stop took {} ms", began.elapsed().as_millis()));
                }
                SpeechOp::Speak { id, text } => {
                    let spoke = self.engines.as_mut().is_some_and(|e| e.speak(id, &text));
                    if spoke {
                        self.speaking = Some(id);
                        self.log.line(format_args!("speak id {id}: {} characters", text.chars().count()));
                    } else {
                        self.log.line(format_args!("speak id {id} refused"));
                        pending.extend(self.queue.finished(now, id));
                    }
                }
            }
        }
    }

    fn collect_finished(&mut self, now: Instant) {
        let done = match &mut self.engines {
            Some(engines) => engines.take_finished(),
            None => return,
        };
        for id in done {
            self.log.line(format_args!("finished id {id}"));
            if self.speaking == Some(id) {
                self.speaking = None;
            }
            let ops = self.queue.finished(now, id);
            self.run_ops(now, ops);
        }
    }

    /// A OneCore synthesis that has run too long is cancelled and counts as finished.
    fn check_synthesis_timeout(&mut self, now: Instant) {
        let Some(started) = self.engines.as_ref().and_then(|e| e.oldest_synthesis()) else {
            return;
        };
        if now.saturating_duration_since(started) < SYNTHESIS_TIMEOUT {
            return;
        }
        eprintln!("[tts] synthesis timed out");
        if let Some(engines) = &mut self.engines {
            engines.stop();
        }
        if let Some(id) = self.speaking.take() {
            let ops = self.queue.finished(now, id);
            self.run_ops(now, ops);
        }
    }

    fn wait(&self) {
        let timeout = wait_timeout(
            Instant::now(),
            self.queue.next_deadline(),
            self.engines.as_ref().and_then(|e| e.oldest_synthesis()),
            self.speaking.is_some(),
        );
        let mut handles = vec![self.command_event.handle(), self.core_event.handle()];
        if let Some(engines) = &self.engines {
            handles.extend(engines.wait_handles());
        }
        // Rounded up, so a wake is never early; INFINITE is reserved for "no deadline".
        let ms = timeout.map_or(INFINITE, |d| d.as_micros().div_ceil(1000).min(u128::from(INFINITE - 1)) as u32);
        if unsafe { WaitForMultipleObjects(&handles, false, ms) } == WAIT_FAILED {
            eprintln!("[tts] waiting for speech events failed");
            thread::sleep(PLAY_POLL);
        }
    }
}

/// How long the thread may sleep: until the queue's next expiry, the synthesis timeout or the
/// next poll while speaking, whichever comes first. None means until something signals.
fn wait_timeout(
    now: Instant,
    queue_deadline: Option<Instant>,
    synthesis_started: Option<Instant>,
    speaking: bool,
) -> Option<Duration> {
    let until = |at: Instant| at.saturating_duration_since(now);
    [
        queue_deadline.map(until),
        synthesis_started.map(|started| until(started + SYNTHESIS_TIMEOUT)),
        speaking.then_some(PLAY_POLL),
    ]
    .into_iter()
    .flatten()
    .min()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn an_idle_thread_sleeps_until_signalled() {
        assert_eq!(wait_timeout(Instant::now(), None, None, false), None);
    }

    #[test]
    fn a_speaking_thread_wakes_at_the_poll_interval() {
        assert_eq!(wait_timeout(Instant::now(), None, None, true), Some(PLAY_POLL));
    }

    #[test]
    fn the_earliest_of_the_queue_deadline_and_the_synthesis_timeout_wins() {
        let now = Instant::now();
        assert_eq!(wait_timeout(now, Some(now + ms(30)), None, true), Some(ms(30)));
        assert_eq!(wait_timeout(now, Some(now + ms(300)), None, true), Some(PLAY_POLL));
        // Started 4.9 s ago: 100 ms left of the 5 s.
        let started = now - ms(4900);
        assert_eq!(wait_timeout(now, None, Some(started), false), Some(ms(100)));
        assert_eq!(wait_timeout(now, Some(now + ms(10)), Some(started), true), Some(ms(10)));
    }

    #[test]
    fn a_deadline_that_has_passed_does_not_wait() {
        let now = Instant::now();
        assert_eq!(wait_timeout(now, Some(now - ms(5)), None, false), Some(Duration::ZERO));
        assert_eq!(wait_timeout(now, None, Some(now - ms(6000)), false), Some(Duration::ZERO));
    }

    fn handle_without_thread() -> (SpeechHandle, Receiver<Command>, Arc<Event>) {
        let (tx, rx) = mpsc::channel();
        let event = Arc::new(Event::new().unwrap());
        let inner = Inner {
            tx,
            command_event: event.clone(),
            status: Arc::default(),
            thread: Mutex::new(None),
        };
        (SpeechHandle { inner: Some(Arc::new(inner)) }, rx, event)
    }

    #[test]
    fn requests_are_queued_in_order_and_signal_the_thread() {
        use cv_core::Kind;
        use windows::Win32::System::Threading::WaitForSingleObject;
        let (handle, rx, event) = handle_without_thread();
        handle.say(Utterance::new(Kind::Test, "one"));
        handle.silence();
        handle.wake();
        handle.refresh_voices();
        assert_eq!(unsafe { WaitForSingleObject(event.handle(), 0) }.0, 0, "the event is signalled");
        let got: Vec<Command> = rx.try_iter().collect();
        assert!(matches!(got.as_slice(), [Command::Say(items), Command::Silence(_), Command::Wake, Command::RefreshVoices] if items.len() == 1));
    }

    #[test]
    fn a_publisher_calls_back_only_when_the_status_changes() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let calls = Arc::new(AtomicU32::new(0));
        let status = Arc::new(Mutex::new(SpeechStatus::default()));
        let publisher = {
            let calls = calls.clone();
            Publisher {
                status: status.clone(),
                on_change: Box::new(move || {
                    calls.fetch_add(1, Ordering::Relaxed);
                }),
            }
        };
        publisher.publish(SpeechStatus::default());
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        let failed = SpeechStatus { state: SpeechState::Failed, ..SpeechStatus::default() };
        publisher.publish(failed.clone());
        publisher.publish(failed.clone());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(*status.lock().unwrap(), failed);
        assert_eq!(publisher.state(), SpeechState::Failed);
    }

    #[test]
    fn the_handle_reports_the_published_status() {
        let (handle, _rx, _event) = handle_without_thread();
        assert_eq!(handle.status().state, SpeechState::Off);
        let inner = handle.inner.as_ref().unwrap();
        inner.status.lock().unwrap().state = SpeechState::NoVoices;
        assert_eq!(handle.status().state, SpeechState::NoVoices);
        assert_eq!(SpeechHandle { inner: None }.status().state, SpeechState::Failed);
    }

    #[test]
    fn an_inert_handle_and_a_clone_after_shutdown_do_nothing() {
        let inert = SpeechHandle { inner: None };
        inert.silence();
        inert.wake();
        inert.refresh_voices();
        inert.shutdown();
        let (handle, rx, _event) = handle_without_thread();
        handle.shutdown();
        assert!(matches!(rx.try_recv(), Ok(Command::Shutdown)));
        handle.clone().shutdown();
    }
}
