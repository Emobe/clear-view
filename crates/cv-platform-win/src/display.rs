//! When to re-read the monitor list after `WM_DISPLAYCHANGE` (ADR 0009, roadmap 5.4).
//!
//! A plug or unplug can send several messages while the topology settles, so the list is read
//! at once and once more `SETTLE` after the last message. The caller passes the time in, so the
//! logic needs no clock.

use std::time::{Duration, Instant};

/// How long after the last message the list is read once more.
pub(crate) const SETTLE: Duration = Duration::from_secs(1);

#[derive(Debug, Default)]
pub(crate) struct DisplayWatch {
    /// A message arrived that has not been read for yet.
    immediate: bool,
    /// The settle read is still to come, due `SETTLE` after this time.
    recheck_from: Option<Instant>,
}

impl DisplayWatch {
    /// A `WM_DISPLAYCHANGE` was seen at `now`: read now, and again `SETTLE` after the last one.
    pub(crate) fn on_message(&mut self, now: Instant) {
        self.immediate = true;
        self.recheck_from = Some(now);
    }

    /// The last read failed (no adapter yet while the system reconfigures): try again after
    /// `SETTLE` without reading now.
    pub(crate) fn retry(&mut self, now: Instant) {
        self.recheck_from = Some(now);
    }

    /// True when the list should be read at `now`. Each message gives one immediate read and
    /// one settle read.
    pub(crate) fn poll(&mut self, now: Instant) -> bool {
        if self.immediate {
            self.immediate = false;
            return true;
        }
        match self.recheck_from {
            Some(from) if now.saturating_duration_since(from) >= SETTLE => {
                self.recheck_from = None;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn idle_never_reads() {
        let t0 = Instant::now();
        let mut w = DisplayWatch::default();
        assert!(!w.poll(t0));
        assert!(!w.poll(at(t0, 5000)));
    }

    #[test]
    fn a_message_reads_at_once_then_once_after_the_settle_time() {
        let t0 = Instant::now();
        let mut w = DisplayWatch::default();
        w.on_message(t0);
        assert!(w.poll(t0), "read at once");
        assert!(!w.poll(at(t0, 500)), "not yet");
        assert!(!w.poll(at(t0, 999)));
        assert!(w.poll(at(t0, 1000)), "settle read");
        assert!(!w.poll(at(t0, 1016)), "only once");
        assert!(!w.poll(at(t0, 9000)));
    }

    #[test]
    fn more_messages_push_the_settle_read_back() {
        let t0 = Instant::now();
        let mut w = DisplayWatch::default();
        w.on_message(t0);
        assert!(w.poll(t0));
        w.on_message(at(t0, 600));
        assert!(w.poll(at(t0, 600)), "each message reads at once");
        assert!(!w.poll(at(t0, 1000)), "settle counts from the last message");
        assert!(!w.poll(at(t0, 1599)));
        assert!(w.poll(at(t0, 1600)));
        assert!(!w.poll(at(t0, 1700)));
    }

    #[test]
    fn a_failed_read_is_retried_after_the_settle_time_only() {
        let t0 = Instant::now();
        let mut w = DisplayWatch::default();
        w.on_message(t0);
        assert!(w.poll(t0));
        w.retry(at(t0, 16));
        assert!(!w.poll(at(t0, 500)));
        assert!(!w.poll(at(t0, 1015)));
        assert!(w.poll(at(t0, 1016)));
        assert!(!w.poll(at(t0, 2100)));
    }
}
