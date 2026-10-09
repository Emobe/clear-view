//! What the caret thread does with each read, as pure logic: which event to publish, which line
//! to log, and whether to keep polling (ADR 0008). No Windows calls, so it is unit-tested.

use std::time::Instant;

use cv_core::{AppId, CaretSource, CoreEvent, ScreenRect};

/// One answer from the source chain: where the caret is and which source said so.
pub(crate) type Found = (ScreenRect, CaretSource);

/// What one read leads to.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Outcome {
    pub event: Option<CoreEvent>,
    pub log: Option<String>,
}

#[derive(Default)]
pub(crate) struct Tracker {
    /// The last caret published, and in which app. `None` after `CaretLost` or before the first.
    last: Option<(Found, AppId)>,
    /// The last source logged since the foreground changed.
    logged_source: Option<CaretSource>,
    /// Whether "no source" was logged since the foreground changed.
    logged_none: bool,
    /// Set once uia or the focus rectangle answers; cleared by a focus or foreground change.
    polling: bool,
}

impl Tracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the thread should read again on a timer: no WinEvent announces a caret that
    /// only uia or the focus rectangle can see.
    pub fn polling(&self) -> bool {
        self.polling
    }

    /// Keyboard focus moved. Call before the read it triggers.
    pub fn focus_changed(&mut self) {
        self.polling = false;
    }

    /// The foreground window changed. Call before the read it triggers. Logging starts over, so
    /// each app gets its source line and at most one "no source" line.
    pub fn foreground_changed(&mut self) {
        self.polling = false;
        self.logged_source = None;
        self.logged_none = false;
    }

    /// Takes one read of the chain for `app` and says what to publish and log. Events go out
    /// only on change: a new rectangle, source or app, or the caret going away.
    pub fn update(&mut self, at: Instant, app: &AppId, found: Option<Found>) -> Outcome {
        let mut outcome = Outcome::default();
        match found {
            Some(found) => {
                if matches!(found.1, CaretSource::Uia | CaretSource::FocusRect) {
                    self.polling = true;
                }
                if self.logged_source != Some(found.1) {
                    self.logged_source = Some(found.1);
                    outcome.log = Some(format!("[caret] {} in {}", found.1.name(), app.exe));
                }
                let current = (found, app.clone());
                if self.last.as_ref() != Some(&current) {
                    outcome.event = Some(CoreEvent::CaretMoved {
                        at,
                        rect: found.0,
                        source: found.1,
                        app: app.clone(),
                    });
                    self.last = Some(current);
                }
            }
            None => {
                if !self.logged_none {
                    self.logged_none = true;
                    outcome.log = Some(format!("[caret] no source in {}", app.exe));
                }
                if self.last.take().is_some() {
                    outcome.event = Some(CoreEvent::CaretLost { at, app: app.clone() });
                }
            }
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(exe: &str, pid: u32) -> AppId {
        AppId { pid, exe: exe.into() }
    }

    fn rect(left: i32) -> ScreenRect {
        ScreenRect { left, top: 100, width: 2, height: 20 }
    }

    fn is_moved(o: &Outcome, want: ScreenRect, want_source: CaretSource, want_app: &AppId) -> bool {
        matches!(&o.event, Some(CoreEvent::CaretMoved { rect, source, app, .. })
            if *rect == want && *source == want_source && app == want_app)
    }

    fn is_lost(o: &Outcome, want_app: &AppId) -> bool {
        matches!(&o.event, Some(CoreEvent::CaretLost { app, .. }) if app == want_app)
    }

    #[test]
    fn first_caret_is_published_and_logged() {
        let mut t = Tracker::new();
        let notepad = app("notepad.exe", 1);
        let o = t.update(Instant::now(), &notepad, Some((rect(10), CaretSource::Msaa)));
        assert!(is_moved(&o, rect(10), CaretSource::Msaa, &notepad));
        assert_eq!(o.log.as_deref(), Some("[caret] msaa in notepad.exe"));
    }

    #[test]
    fn same_caret_again_publishes_nothing() {
        let mut t = Tracker::new();
        let notepad = app("notepad.exe", 1);
        t.update(Instant::now(), &notepad, Some((rect(10), CaretSource::Msaa)));
        let o = t.update(Instant::now(), &notepad, Some((rect(10), CaretSource::Msaa)));
        assert_eq!(o, Outcome::default());
    }

    #[test]
    fn a_move_is_published_but_not_logged() {
        let mut t = Tracker::new();
        let notepad = app("notepad.exe", 1);
        t.update(Instant::now(), &notepad, Some((rect(10), CaretSource::Msaa)));
        let o = t.update(Instant::now(), &notepad, Some((rect(18), CaretSource::Msaa)));
        assert!(is_moved(&o, rect(18), CaretSource::Msaa, &notepad));
        assert_eq!(o.log, None);
    }

    #[test]
    fn a_new_source_at_the_same_place_is_published_and_logged() {
        let mut t = Tracker::new();
        let notepad = app("notepad.exe", 1);
        t.update(Instant::now(), &notepad, Some((rect(10), CaretSource::Msaa)));
        let o = t.update(Instant::now(), &notepad, Some((rect(10), CaretSource::Uia)));
        assert!(is_moved(&o, rect(10), CaretSource::Uia, &notepad));
        assert_eq!(o.log.as_deref(), Some("[caret] uia in notepad.exe"));
    }

    #[test]
    fn a_new_app_at_the_same_place_is_published() {
        let mut t = Tracker::new();
        let a = app("notepad.exe", 1);
        let b = app("notepad.exe", 2);
        t.update(Instant::now(), &a, Some((rect(10), CaretSource::Msaa)));
        let o = t.update(Instant::now(), &b, Some((rect(10), CaretSource::Msaa)));
        assert!(is_moved(&o, rect(10), CaretSource::Msaa, &b));
    }

    #[test]
    fn losing_the_caret_publishes_caret_lost_once() {
        let mut t = Tracker::new();
        let notepad = app("notepad.exe", 1);
        t.update(Instant::now(), &notepad, Some((rect(10), CaretSource::Msaa)));
        let o = t.update(Instant::now(), &notepad, None);
        assert!(is_lost(&o, &notepad));
        let o = t.update(Instant::now(), &notepad, None);
        assert_eq!(o.event, None);
    }

    #[test]
    fn no_caret_from_the_start_publishes_nothing() {
        let mut t = Tracker::new();
        let o = t.update(Instant::now(), &app("soffice.bin", 3), None);
        assert_eq!(o.event, None);
    }

    #[test]
    fn caret_lost_names_the_current_app() {
        let mut t = Tracker::new();
        t.update(Instant::now(), &app("notepad.exe", 1), Some((rect(10), CaretSource::Msaa)));
        t.foreground_changed();
        let office = app("soffice.bin", 3);
        let o = t.update(Instant::now(), &office, None);
        assert!(is_lost(&o, &office));
    }

    #[test]
    fn caret_back_after_lost_is_published_again() {
        let mut t = Tracker::new();
        let notepad = app("notepad.exe", 1);
        t.update(Instant::now(), &notepad, Some((rect(10), CaretSource::Msaa)));
        t.update(Instant::now(), &notepad, None);
        let o = t.update(Instant::now(), &notepad, Some((rect(10), CaretSource::Msaa)));
        assert!(is_moved(&o, rect(10), CaretSource::Msaa, &notepad));
    }

    #[test]
    fn no_source_is_logged_once_per_foreground_change() {
        let mut t = Tracker::new();
        let office = app("soffice.bin", 3);
        let o = t.update(Instant::now(), &office, None);
        assert_eq!(o.log.as_deref(), Some("[caret] no source in soffice.bin"));
        assert_eq!(t.update(Instant::now(), &office, None).log, None);
        t.focus_changed();
        assert_eq!(t.update(Instant::now(), &office, None).log, None);
        t.foreground_changed();
        let o = t.update(Instant::now(), &office, None);
        assert_eq!(o.log.as_deref(), Some("[caret] no source in soffice.bin"));
    }

    #[test]
    fn the_same_source_is_not_logged_again_after_a_gap() {
        let mut t = Tracker::new();
        let edge = app("msedge.exe", 4);
        t.update(Instant::now(), &edge, Some((rect(10), CaretSource::Msaa)));
        t.update(Instant::now(), &edge, None);
        let o = t.update(Instant::now(), &edge, Some((rect(30), CaretSource::Msaa)));
        assert_eq!(o.log, None);
    }

    #[test]
    fn a_foreground_change_logs_the_source_again() {
        let mut t = Tracker::new();
        let a = app("notepad.exe", 1);
        let b = app("notepad.exe", 2);
        t.update(Instant::now(), &a, Some((rect(10), CaretSource::Msaa)));
        t.foreground_changed();
        let o = t.update(Instant::now(), &b, Some((rect(40), CaretSource::Msaa)));
        assert_eq!(o.log.as_deref(), Some("[caret] msaa in notepad.exe"));
    }

    #[test]
    fn polling_starts_with_uia_or_focus_rect_only() {
        let notepad = app("notepad.exe", 1);
        for (source, polls) in [
            (CaretSource::Msaa, false),
            (CaretSource::Gui, false),
            (CaretSource::Uia, true),
            (CaretSource::FocusRect, true),
        ] {
            let mut t = Tracker::new();
            t.update(Instant::now(), &notepad, Some((rect(10), source)));
            assert_eq!(t.polling(), polls, "{source:?}");
        }
    }

    #[test]
    fn polling_survives_a_lost_caret_until_focus_or_foreground_changes() {
        let term = app("WindowsTerminal.exe", 5);
        let mut t = Tracker::new();
        t.update(Instant::now(), &term, Some((rect(10), CaretSource::Uia)));
        t.update(Instant::now(), &term, None);
        assert!(t.polling());
        t.update(Instant::now(), &term, Some((rect(10), CaretSource::Msaa)));
        assert!(t.polling());
        t.focus_changed();
        assert!(!t.polling());

        t.update(Instant::now(), &term, Some((rect(10), CaretSource::Uia)));
        assert!(t.polling());
        t.foreground_changed();
        assert!(!t.polling());
    }
}
