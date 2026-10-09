//! Core events (ADR 0004 rules, fields from ADR 0008). Plain data in physical virtual-screen
//! pixels plus a timestamp; no OS handles or COM pointers. Delivered by fan-out through
//! [`EventHub`]: each consumer has its own receiver and never blocks a source.

use parking_lot::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use crate::ScreenRect;

/// Something the core saw happen. New kinds (focus, element, text) are added as new variants.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum CoreEvent {
    /// The caret moved, appeared, or focus moved to a new caret. Sent only when the rectangle,
    /// source or application changes.
    CaretMoved { at: Instant, rect: ScreenRect, source: CaretSource, app: AppId },
    /// The focused element has no caret any more, or no source gives one.
    CaretLost { at: Instant, app: AppId },
}

/// Which API gave a caret position.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaretSource {
    /// The MSAA caret object (`OBJID_CARET`).
    Msaa,
    /// UI Automation text patterns.
    Uia,
    /// `GetGUIThreadInfo`'s caret rectangle.
    Gui,
    /// Not a caret: the rectangle of a focused element that has a caret but no source gave its
    /// position.
    FocusRect,
}

impl CaretSource {
    /// Short name for logs.
    pub fn name(self) -> &'static str {
        match self {
            Self::Msaa => "msaa",
            Self::Uia => "uia",
            Self::Gui => "gui",
            Self::FocusRect => "focus rect",
        }
    }
}

/// The foreground application: process id and executable file name (for example
/// "explorer.exe"). Read once per foreground change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppId {
    pub pid: u32,
    pub exe: String,
}

/// Fan-out delivery of [`CoreEvent`]s. Each [`subscribe`](Self::subscribe) gets its own
/// unbounded receiver, so publishing never blocks on a slow consumer.
#[derive(Default)]
pub struct EventHub {
    senders: Mutex<Vec<Sender<CoreEvent>>>,
}

impl EventHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// A new receiver that gets every event published from now on.
    pub fn subscribe(&self) -> Receiver<CoreEvent> {
        let (tx, rx) = channel();
        self.senders.lock().push(tx);
        rx
    }

    /// Sends `event` to every live receiver and forgets receivers that were dropped.
    pub fn publish(&self, event: CoreEvent) {
        self.senders.lock().retain(|tx| tx.send(event.clone()).is_ok());
    }

    /// How many receivers are still subscribed, as of the last publish.
    pub fn subscribers(&self) -> usize {
        self.senders.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> AppId {
        AppId { pid: 42, exe: "notepad.exe".into() }
    }

    fn moved(left: i32) -> CoreEvent {
        CoreEvent::CaretMoved {
            at: Instant::now(),
            rect: ScreenRect { left, top: 10, width: 2, height: 20 },
            source: CaretSource::Msaa,
            app: app(),
        }
    }

    #[test]
    fn every_subscriber_gets_every_event() {
        let hub = EventHub::new();
        let a = hub.subscribe();
        let b = hub.subscribe();
        let e = moved(1);
        hub.publish(e.clone());
        assert_eq!(a.try_recv().unwrap(), e);
        assert_eq!(b.try_recv().unwrap(), e);
        assert!(a.try_recv().is_err());
    }

    #[test]
    fn events_arrive_in_order() {
        let hub = EventHub::new();
        let rx = hub.subscribe();
        let events: Vec<_> = (0..5).map(moved).collect();
        for e in &events {
            hub.publish(e.clone());
        }
        let got: Vec<_> = rx.try_iter().collect();
        assert_eq!(got, events);
    }

    #[test]
    fn dropped_receivers_are_forgotten_and_others_still_get_events() {
        let hub = EventHub::new();
        let kept = hub.subscribe();
        drop(hub.subscribe());
        assert_eq!(hub.subscribers(), 2);
        hub.publish(moved(1));
        assert_eq!(hub.subscribers(), 1);
        assert!(kept.try_recv().is_ok());
    }

    #[test]
    fn publish_with_no_subscribers_does_nothing() {
        let hub = EventHub::new();
        hub.publish(CoreEvent::CaretLost { at: Instant::now(), app: app() });
        assert_eq!(hub.subscribers(), 0);
    }

    #[test]
    fn a_late_subscriber_gets_only_later_events() {
        let hub = EventHub::new();
        hub.publish(moved(1));
        let rx = hub.subscribe();
        assert!(rx.try_recv().is_err());
        let later = moved(2);
        hub.publish(later.clone());
        assert_eq!(rx.try_recv().unwrap(), later);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn publishing_never_blocks_on_an_unread_receiver() {
        let hub = EventHub::new();
        let _unread = hub.subscribe();
        for i in 0..10_000 {
            hub.publish(moved(i));
        }
        assert_eq!(hub.subscribers(), 1);
    }
}
