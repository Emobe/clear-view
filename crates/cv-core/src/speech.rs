//! Speech queue and event-to-utterance rules (roadmap 8.8a, ADR 0013). Pure logic with no Windows
//! types, threads or I/O: the speech thread in cv-tts feeds it requests and does what it returns.
//!
//! - Speech has two levels. Foreground speech (anything the user did) cuts off everything.
//!   Background speech (pointer echo, continuous reading) never interrupts foreground speech.
//! - The queue is ours, not the engine's: the backend is given one utterance at a time, each with
//!   an id, and reports the id when it has finished. A report for any other id is ignored, so a
//!   late report for an utterance that was cut off cannot disturb the one that replaced it.
//! - A batch is a list that queues in order behind its first item, which decides what it cuts off.
//! - No resume. A cut-off utterance is gone (ADR 0013; continuous reading decides in 12.1).
//!
//! The numbers ([`QUEUE_MAX`], [`POINTER_EXPIRY`]) are starting values from ADR 0013.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::events::CoreEvent;
use crate::wording::{Verbosity, focus_utterances};

/// The queue holds at most this many waiting utterances; the oldest are dropped first.
pub const QUEUE_MAX: usize = 32;
/// A queued [`Kind::Pointer`] utterance that has not started this long after it was queued is
/// dropped: it describes where the pointer was.
pub const POINTER_EXPIRY: Duration = Duration::from_secs(1);

/// What an utterance is for (ADR 0013). The kind decides what it cuts off and what cuts it off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A command's answer: read title, repeat (9.8).
    Command,
    /// Focus, window and dialog changes (9.5, 9.6).
    Focus,
    /// Typing echo (10.x).
    Typing,
    /// Caret moves (10.4).
    Caret,
    /// The panel's "Test voice" (8.7d).
    Test,
    /// Mouse echo (11.x).
    Pointer,
    /// Continuous reading (12.x).
    Reading,
}

/// Which side of the interruption rule a [`Kind`] is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Cuts off everything.
    Foreground,
    /// Never interrupts foreground speech.
    Background,
}

impl Kind {
    pub fn level(self) -> Level {
        match self {
            Self::Command | Self::Focus | Self::Typing | Self::Caret | Self::Test => {
                Level::Foreground
            }
            Self::Pointer | Self::Reading => Level::Background,
        }
    }
}

/// Text to speak and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utterance {
    pub text: String,
    pub kind: Kind,
}

impl Utterance {
    pub fn new(kind: Kind, text: impl Into<String>) -> Self {
        Self { text: text.into(), kind }
    }
}

/// What the backend must do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeechOp {
    /// Start speaking `text` as utterance `id`. The backend reports `id` when it has finished.
    Speak { id: u32, text: String },
    /// Stop what is speaking now. Anything not yet given to the backend is already gone.
    Stop,
}

struct Waiting {
    text: String,
    kind: Kind,
    queued_at: Instant,
}

impl Waiting {
    fn expired(&self, now: Instant) -> bool {
        self.kind == Kind::Pointer
            && now.saturating_duration_since(self.queued_at) >= POINTER_EXPIRY
    }
}

/// The queue. Every input takes `now` so tests control time; none of them blocks or sleeps.
#[derive(Default)]
pub struct SpeechQueue {
    current: Option<(u32, Kind)>,
    waiting: VecDeque<Waiting>,
    next_id: u32,
    silenced_at: Option<Instant>,
}

impl SpeechQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Speak one utterance by its kind's rule.
    pub fn push(&mut self, now: Instant, utterance: Utterance) -> Vec<SpeechOp> {
        self.push_batch(now, vec![utterance])
    }

    /// Speak a list in order. The first item's kind decides what the batch cuts off, or whether
    /// the whole batch is dropped. Items with no text to speak are skipped.
    pub fn push_batch(&mut self, now: Instant, items: Vec<Utterance>) -> Vec<SpeechOp> {
        let items: Vec<Utterance> =
            items.into_iter().filter(|u| !u.text.trim().is_empty()).collect();
        let Some(first) = items.first().map(|u| u.kind) else {
            return Vec::new();
        };

        let mut ops = Vec::new();
        match first {
            Kind::Pointer => {
                // Pointer echo waits for nothing: foreground speech and reading both win.
                if self.foreground_active() || self.reading_active() {
                    return ops;
                }
                self.cut_off(&mut ops, |k| k == Kind::Pointer);
            }
            Kind::Reading => {
                if self.foreground_active() {
                    // Never interrupt foreground speech: older background speech goes, the new
                    // reading waits its turn.
                    self.waiting.retain(|w| w.kind.level() == Level::Foreground);
                } else {
                    self.cut_off(&mut ops, |_| true);
                }
            }
            _ => self.cut_off(&mut ops, |_| true),
        }

        for u in items {
            self.waiting.push_back(Waiting { text: u.text, kind: u.kind, queued_at: now });
        }
        while self.waiting.len() > QUEUE_MAX {
            self.waiting.pop_front();
        }
        ops.extend(self.start_next(now));
        ops
    }

    /// Stop speaking and clear the queue, whatever is in it. Events older than this are refused
    /// by [`accepts_event`](Self::accepts_event) from now on.
    pub fn silence(&mut self, now: Instant) -> Vec<SpeechOp> {
        self.silenced_at = Some(now);
        self.waiting.clear();
        match self.current.take() {
            Some(_) => vec![SpeechOp::Stop],
            None => Vec::new(),
        }
    }

    /// The backend finished utterance `id`. Gives the next one to speak, if any. An id that is
    /// not the one speaking now (cut off, or never given out) is ignored.
    pub fn finished(&mut self, now: Instant, id: u32) -> Vec<SpeechOp> {
        match self.current {
            Some((current, _)) if current == id => {
                self.current = None;
                self.start_next(now).into_iter().collect()
            }
            _ => Vec::new(),
        }
    }

    /// Drops queued pointer utterances that have waited [`POINTER_EXPIRY`]. Starting an
    /// utterance checks this too, so `tick` only keeps the queue and
    /// [`next_deadline`](Self::next_deadline) honest.
    pub fn tick(&mut self, now: Instant) {
        self.waiting.retain(|w| !w.expired(now));
    }

    /// When a queued pointer utterance next expires, for the speech thread's wait timeout.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.waiting
            .iter()
            .filter(|w| w.kind == Kind::Pointer)
            .map(|w| w.queued_at + POINTER_EXPIRY)
            .min()
    }

    /// False for a core event that happened before the last silence: a slow app's late focus
    /// event must not speak after the user silenced speech. Check it when an event is drained.
    pub fn accepts_event(&self, at: Instant) -> bool {
        self.silenced_at.is_none_or(|silenced| at >= silenced)
    }

    /// An utterance has been given to the backend and not reported finished.
    pub fn is_speaking(&self) -> bool {
        self.current.is_some()
    }

    /// How many utterances wait behind the one speaking.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    fn foreground_active(&self) -> bool {
        self.current.is_some_and(|(_, k)| k.level() == Level::Foreground)
            || self.waiting.iter().any(|w| w.kind.level() == Level::Foreground)
    }

    fn reading_active(&self) -> bool {
        self.current.is_some_and(|(_, k)| k == Kind::Reading)
            || self.waiting.iter().any(|w| w.kind == Kind::Reading)
    }

    /// Removes the queued utterances `cuts` selects, and stops the one speaking if it is
    /// selected.
    fn cut_off(&mut self, ops: &mut Vec<SpeechOp>, cuts: impl Fn(Kind) -> bool) {
        self.waiting.retain(|w| !cuts(w.kind));
        if self.current.is_some_and(|(_, k)| cuts(k)) {
            self.current = None;
            ops.push(SpeechOp::Stop);
        }
    }

    /// If nothing is speaking, takes the first waiting utterance that has not expired.
    fn start_next(&mut self, now: Instant) -> Option<SpeechOp> {
        if self.current.is_some() {
            return None;
        }
        while let Some(w) = self.waiting.pop_front() {
            if w.expired(now) {
                continue;
            }
            self.next_id = self.next_id.wrapping_add(1);
            self.current = Some((self.next_id, w.kind));
            return Some(SpeechOp::Speak { id: self.next_id, text: w.text });
        }
        None
    }
}

/// The speech fields the rules need. Later steps add the echo switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpeechSettings {
    pub enabled: bool,
    pub verbosity: Verbosity,
}

/// What a core event says. A focus event speaks its control (9.5, `wording`); the window title
/// and dialog text are 9.6. Caret moves speak nothing until 10.4 (ADR 0013, ADR 0015). Events
/// carry the text they need, so this stays pure.
pub fn utterances_for(event: &CoreEvent, settings: &SpeechSettings) -> Vec<Utterance> {
    if !settings.enabled {
        return Vec::new();
    }
    match event {
        CoreEvent::FocusChanged { control, .. } => {
            focus_utterances(control.as_ref(), settings.verbosity)
        }
        CoreEvent::CaretMoved { .. } | CoreEvent::CaretLost { .. } => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{AppId, CaretSource};
    use crate::focus::{Control, FocusSource, Password, Role, WindowInfo};
    use crate::ScreenRect;

    fn t0() -> Instant {
        // One fixed base per test; offsets are added to it.
        Instant::now()
    }

    fn ms(base: Instant, n: u64) -> Instant {
        base + Duration::from_millis(n)
    }

    fn u(kind: Kind, text: &str) -> Utterance {
        Utterance::new(kind, text)
    }

    fn speak(id: u32, text: &str) -> SpeechOp {
        SpeechOp::Speak { id, text: text.into() }
    }

    #[test]
    fn levels_follow_the_adr_table() {
        for k in [Kind::Command, Kind::Focus, Kind::Typing, Kind::Caret, Kind::Test] {
            assert_eq!(k.level(), Level::Foreground, "{k:?}");
        }
        for k in [Kind::Pointer, Kind::Reading] {
            assert_eq!(k.level(), Level::Background, "{k:?}");
        }
    }

    #[test]
    fn an_idle_queue_speaks_at_once_with_an_id() {
        let mut q = SpeechQueue::new();
        let ops = q.push(t0(), u(Kind::Focus, "OK button"));
        assert_eq!(ops, vec![speak(1, "OK button")]);
        assert!(q.is_speaking());
        assert_eq!(q.waiting(), 0);
    }

    #[test]
    fn empty_text_speaks_nothing_and_cuts_nothing_off() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push(b, u(Kind::Focus, "one"));
        assert!(q.push(b, u(Kind::Command, "")).is_empty());
        assert!(q.push(b, u(Kind::Command, "  \n")).is_empty());
        assert!(q.push_batch(b, vec![]).is_empty());
        assert!(q.is_speaking());
    }

    #[test]
    fn foreground_cuts_off_the_speaking_one_and_everything_queued() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push_batch(b, vec![u(Kind::Focus, "one"), u(Kind::Typing, "two"), u(Kind::Typing, "three")]);
        assert_eq!(q.waiting(), 2);
        let ops = q.push(b, u(Kind::Command, "four"));
        assert_eq!(ops, vec![SpeechOp::Stop, speak(2, "four")]);
        assert_eq!(q.waiting(), 0);
    }

    #[test]
    fn every_foreground_kind_cuts_off_every_kind() {
        let b = t0();
        for first in [Kind::Command, Kind::Focus, Kind::Typing, Kind::Caret, Kind::Test, Kind::Reading]
        {
            for second in [Kind::Command, Kind::Focus, Kind::Typing, Kind::Caret, Kind::Test] {
                let mut q = SpeechQueue::new();
                q.push(b, u(first, "a"));
                let ops = q.push(b, u(second, "b"));
                assert_eq!(ops, vec![SpeechOp::Stop, speak(2, "b")], "{first:?} then {second:?}");
            }
        }
    }

    #[test]
    fn queued_utterances_play_in_order_as_finished_reports_arrive() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push_batch(b, vec![u(Kind::Focus, "title"), u(Kind::Focus, "text"), u(Kind::Focus, "end")]);
        assert_eq!(q.waiting(), 2);
        assert_eq!(q.finished(b, 1), vec![speak(2, "text")]);
        assert_eq!(q.finished(b, 2), vec![speak(3, "end")]);
        assert_eq!(q.finished(b, 3), vec![]);
        assert!(!q.is_speaking());
    }

    #[test]
    fn a_batch_cuts_off_what_its_first_item_says() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push(b, u(Kind::Typing, "x"));
        let ops = q.push_batch(b, vec![u(Kind::Focus, "dialog"), u(Kind::Focus, "text")]);
        assert_eq!(ops, vec![SpeechOp::Stop, speak(2, "dialog")]);
        assert_eq!(q.waiting(), 1);
    }

    #[test]
    fn a_later_foreground_push_cuts_off_the_rest_of_a_batch() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push_batch(b, vec![u(Kind::Focus, "title"), u(Kind::Focus, "text")]);
        let ops = q.push(b, u(Kind::Typing, "a"));
        assert_eq!(ops, vec![SpeechOp::Stop, speak(2, "a")]);
        assert_eq!(q.finished(b, 2), vec![]);
    }

    #[test]
    fn pointer_is_dropped_while_foreground_speaks() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push(b, u(Kind::Focus, "dialog"));
        assert_eq!(q.push(b, u(Kind::Pointer, "button")), vec![]);
        assert_eq!(q.waiting(), 0);
        // The foreground utterance finishes with nothing queued behind it.
        assert_eq!(q.finished(b, 1), vec![]);
    }

    #[test]
    fn pointer_is_dropped_while_foreground_is_queued() {
        let b = t0();
        let mut q = SpeechQueue::new();
        // A pointer utterance speaks, then a foreground batch cuts it off and queues behind its
        // first item: foreground is both speaking and queued here, so check each state.
        q.push(b, u(Kind::Pointer, "link"));
        q.push_batch(b, vec![u(Kind::Focus, "title"), u(Kind::Focus, "text")]);
        assert_eq!(q.push(b, u(Kind::Pointer, "button")), vec![]);
        assert_eq!(q.waiting(), 1);
        // Once "title" finishes, "text" is speaking and still foreground.
        q.finished(b, 2);
        assert_eq!(q.push(b, u(Kind::Pointer, "button")), vec![]);
    }

    #[test]
    fn pointer_replaces_pointer_and_speaks_when_idle() {
        let b = t0();
        let mut q = SpeechQueue::new();
        assert_eq!(q.push(b, u(Kind::Pointer, "one")), vec![speak(1, "one")]);
        assert_eq!(
            q.push(ms(b, 100), u(Kind::Pointer, "two")),
            vec![SpeechOp::Stop, speak(2, "two")]
        );
        assert_eq!(q.waiting(), 0);
    }

    #[test]
    fn a_pointer_batch_replaces_pointer_speech_and_queues_in_order() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push(b, u(Kind::Pointer, "old"));
        let ops = q.push_batch(b, vec![u(Kind::Pointer, "name"), u(Kind::Pointer, "role")]);
        assert_eq!(ops, vec![SpeechOp::Stop, speak(2, "name")]);
        assert_eq!(q.finished(b, 2), vec![speak(3, "role")]);
    }

    #[test]
    fn a_dropped_pointer_batch_is_dropped_whole() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push(b, u(Kind::Command, "title"));
        assert_eq!(q.push_batch(b, vec![u(Kind::Pointer, "a"), u(Kind::Pointer, "b")]), vec![]);
        assert_eq!(q.waiting(), 0);
    }

    #[test]
    fn a_waiting_pointer_utterance_expires_after_one_second() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push_batch(b, vec![u(Kind::Pointer, "name"), u(Kind::Pointer, "role")]);
        // "role" waits behind "name". Just before the limit it still speaks.
        let mut early = SpeechQueue::new();
        early.push_batch(b, vec![u(Kind::Pointer, "name"), u(Kind::Pointer, "role")]);
        assert_eq!(early.finished(ms(b, 999), 1), vec![speak(2, "role")]);
        // At the limit it is dropped instead.
        assert_eq!(q.finished(ms(b, 1000), 1), vec![]);
        assert!(!q.is_speaking());
        assert_eq!(q.waiting(), 0);
    }

    #[test]
    fn expiry_applies_to_pointer_only() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push_batch(b, vec![u(Kind::Focus, "title"), u(Kind::Focus, "text")]);
        assert_eq!(q.finished(ms(b, 60_000), 1), vec![speak(2, "text")]);
    }

    #[test]
    fn tick_prunes_expired_pointer_utterances_and_next_deadline_names_them() {
        let b = t0();
        let mut q = SpeechQueue::new();
        assert_eq!(q.next_deadline(), None);
        q.push_batch(b, vec![u(Kind::Pointer, "name"), u(Kind::Pointer, "role")]);
        assert_eq!(q.next_deadline(), Some(ms(b, 1000)));
        q.tick(ms(b, 999));
        assert_eq!(q.waiting(), 1);
        q.tick(ms(b, 1000));
        assert_eq!(q.waiting(), 0);
        assert_eq!(q.next_deadline(), None);
        assert!(q.is_speaking());
    }

    #[test]
    fn foreground_never_expires_and_has_no_deadline() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push_batch(b, vec![u(Kind::Focus, "a"), u(Kind::Focus, "b")]);
        assert_eq!(q.next_deadline(), None);
        q.tick(ms(b, 10_000));
        assert_eq!(q.waiting(), 1);
    }

    #[test]
    fn reading_speaks_when_idle_and_a_new_reading_replaces_it() {
        let b = t0();
        let mut q = SpeechQueue::new();
        assert_eq!(q.push(b, u(Kind::Reading, "page one")), vec![speak(1, "page one")]);
        assert_eq!(
            q.push(b, u(Kind::Reading, "page two")),
            vec![SpeechOp::Stop, speak(2, "page two")]
        );
    }

    #[test]
    fn reading_cuts_off_pointer_speech() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push(b, u(Kind::Pointer, "link"));
        assert_eq!(
            q.push(b, u(Kind::Reading, "text")),
            vec![SpeechOp::Stop, speak(2, "text")]
        );
    }

    #[test]
    fn pointer_never_interrupts_reading() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push_batch(b, vec![u(Kind::Reading, "one"), u(Kind::Reading, "two")]);
        assert_eq!(q.push(b, u(Kind::Pointer, "link")), vec![]);
        assert_eq!(q.finished(b, 1), vec![speak(2, "two")]);
    }

    #[test]
    fn reading_waits_behind_foreground_and_a_later_foreground_drops_it() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push(b, u(Kind::Command, "reading from caret"));
        assert_eq!(q.push(b, u(Kind::Reading, "text")), vec![]);
        assert_eq!(q.waiting(), 1);
        // Foreground finishes: reading starts.
        assert_eq!(q.finished(b, 1), vec![speak(2, "text")]);

        // Reading queued behind foreground is dropped by the next foreground push.
        let mut q = SpeechQueue::new();
        q.push(b, u(Kind::Command, "a"));
        q.push(b, u(Kind::Reading, "text"));
        assert_eq!(q.push(b, u(Kind::Typing, "b")), vec![SpeechOp::Stop, speak(2, "b")]);
        assert_eq!(q.finished(b, 2), vec![]);
    }

    #[test]
    fn a_new_reading_behind_foreground_replaces_the_waiting_one() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push(b, u(Kind::Command, "a"));
        q.push(b, u(Kind::Reading, "old"));
        q.push(b, u(Kind::Reading, "new"));
        assert_eq!(q.waiting(), 1);
        assert_eq!(q.finished(b, 1), vec![speak(2, "new")]);
    }

    #[test]
    fn silence_stops_the_speaking_one_and_clears_the_queue() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push_batch(b, vec![u(Kind::Focus, "a"), u(Kind::Focus, "b")]);
        assert_eq!(q.silence(ms(b, 10)), vec![SpeechOp::Stop]);
        assert!(!q.is_speaking());
        assert_eq!(q.waiting(), 0);
        // The cut-off utterance's report arrives late and does nothing.
        assert_eq!(q.finished(ms(b, 20), 1), vec![]);
    }

    #[test]
    fn silence_with_nothing_speaking_sends_no_stop() {
        let mut q = SpeechQueue::new();
        assert_eq!(q.silence(t0()), vec![]);
    }

    #[test]
    fn speech_works_again_after_silence() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push(b, u(Kind::Focus, "a"));
        q.silence(ms(b, 1));
        assert_eq!(q.push(ms(b, 2), u(Kind::Focus, "b")), vec![speak(2, "b")]);
    }

    #[test]
    fn events_before_the_last_silence_are_refused() {
        let b = t0();
        let mut q = SpeechQueue::new();
        assert!(q.accepts_event(b), "nothing silenced yet");
        q.silence(ms(b, 100));
        assert!(!q.accepts_event(ms(b, 99)));
        assert!(q.accepts_event(ms(b, 100)), "an event at the same instant is not late");
        assert!(q.accepts_event(ms(b, 101)));
        // A later silence moves the line.
        q.silence(ms(b, 500));
        assert!(!q.accepts_event(ms(b, 499)));
        assert!(q.accepts_event(ms(b, 500)));
    }

    #[test]
    fn a_stale_or_unknown_finished_id_changes_nothing() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push(b, u(Kind::Focus, "a"));
        q.push(b, u(Kind::Focus, "b")); // cuts "a" off; "b" is id 2
        assert_eq!(q.finished(b, 1), vec![], "the cut-off utterance");
        assert_eq!(q.finished(b, 99), vec![], "an id never given out");
        assert!(q.is_speaking());
        assert_eq!(q.finished(b, 2), vec![]);
        assert!(!q.is_speaking());
        assert_eq!(q.finished(b, 2), vec![], "reported twice");
    }

    #[test]
    fn a_stale_report_does_not_start_the_next_utterance_early() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.push_batch(b, vec![u(Kind::Focus, "a"), u(Kind::Focus, "b")]);
        q.push_batch(b, vec![u(Kind::Focus, "c"), u(Kind::Focus, "d")]); // "c" is id 2
        assert_eq!(q.finished(b, 1), vec![]);
        assert_eq!(q.waiting(), 1, "d still waits behind c");
        assert_eq!(q.finished(b, 2), vec![speak(3, "d")]);
    }

    #[test]
    fn the_queue_keeps_at_most_32_and_drops_the_oldest() {
        let b = t0();
        let mut q = SpeechQueue::new();
        let batch: Vec<_> = (0..40).map(|i| u(Kind::Reading, &format!("s{i}"))).collect();
        let ops = q.push_batch(b, batch);
        assert_eq!(q.waiting(), QUEUE_MAX - 1, "one is speaking, 31 wait");
        // The oldest eight were dropped before anything started: s8 speaks first.
        assert_eq!(ops, vec![speak(1, "s8")]);
        assert_eq!(q.finished(b, 1), vec![speak(2, "s9")]);
    }

    #[test]
    fn ids_wrap_around() {
        let b = t0();
        let mut q = SpeechQueue::new();
        q.next_id = u32::MAX;
        assert_eq!(q.push(b, u(Kind::Focus, "a")), vec![speak(0, "a")]);
        assert_eq!(q.push(b, u(Kind::Focus, "b")), vec![SpeechOp::Stop, speak(1, "b")]);
    }

    #[test]
    fn caret_events_say_nothing_yet() {
        let at = t0();
        let app = AppId { pid: 1, exe: "notepad.exe".into() };
        let rect = ScreenRect { left: 1, top: 2, width: 3, height: 4 };
        let settings = SpeechSettings { enabled: true, ..SpeechSettings::default() };
        for event in [
            CoreEvent::CaretMoved { at, rect, source: CaretSource::Msaa, app: app.clone() },
            CoreEvent::CaretLost { at, app },
        ] {
            assert!(utterances_for(&event, &settings).is_empty());
            assert!(utterances_for(&event, &SpeechSettings::default()).is_empty());
        }
    }

    fn focus_event(control: Option<Control>) -> CoreEvent {
        CoreEvent::FocusChanged {
            at: t0(),
            app: AppId { pid: 1, exe: "notepad.exe".into() },
            window: WindowInfo::new("Untitled - Notepad"),
            window_changed: true,
            control,
        }
    }

    fn on(verbosity: Verbosity) -> SpeechSettings {
        SpeechSettings { enabled: true, verbosity }
    }

    #[test]
    fn a_focus_event_speaks_its_control_as_a_focus_utterance() {
        let mut c = Control::new("Save", Role::Button, FocusSource::Both);
        c.states.password = Password::No;
        let items = utterances_for(&focus_event(Some(c)), &on(Verbosity::Normal));
        assert_eq!(items, vec![u(Kind::Focus, "Save, button")]);
    }

    #[test]
    fn the_verbosity_setting_reaches_the_wording() {
        let mut c = Control::new("Save", Role::Button, FocusSource::Both);
        c.states.password = Password::No;
        let items = utterances_for(&focus_event(Some(c)), &on(Verbosity::Brief));
        assert_eq!(items, vec![u(Kind::Focus, "Save")]);
    }

    #[test]
    fn a_focus_event_with_nothing_to_say_or_speech_off_speaks_nothing() {
        assert!(utterances_for(&focus_event(None), &on(Verbosity::Normal)).is_empty());
        let pane = Control::new("", Role::Pane, FocusSource::Msaa);
        assert!(utterances_for(&focus_event(Some(pane)), &on(Verbosity::Full)).is_empty());
        let button = Control::new("Save", Role::Button, FocusSource::Both);
        assert!(utterances_for(&focus_event(Some(button)), &SpeechSettings::default()).is_empty());
    }
}
