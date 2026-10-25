//! Core events (ADR 0004 rules, fields from ADR 0008). Plain data in physical virtual-screen
//! pixels plus a timestamp; no OS handles or COM pointers. Delivered by fan-out through
//! [`EventHub`]: each consumer has its own receiver and never blocks a source.

use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use crate::ScreenRect;
use crate::focus::{Control, Value, WindowInfo};

/// Something the core saw happen. New kinds (element, text) are added as new variants.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum CoreEvent {
    /// The caret moved, appeared, or focus moved to a new caret. Sent only when the rectangle,
    /// source or application changes.
    CaretMoved { at: Instant, rect: ScreenRect, source: CaretSource, app: AppId },
    /// The focused element has no caret any more, or no source gives one.
    CaretLost { at: Instant, app: AppId },
    /// Focus moved to a new control, or the foreground window changed (ADR 0015). Sent only
    /// after the focus filter: never for our own process, never twice for the same thing.
    FocusChanged {
        at: Instant,
        app: AppId,
        window: WindowInfo,
        /// The foreground window changed since the last `FocusChanged`.
        window_changed: bool,
        /// `None`: neither source could read the focused control (Task Manager, a hung window).
        control: Option<Control>,
    },
}

impl CoreEvent {
    /// When it happened, for the speech queue's late-event rule (ADR 0013).
    pub fn at(&self) -> Instant {
        match self {
            Self::CaretMoved { at, .. }
            | Self::CaretLost { at, .. }
            | Self::FocusChanged { at, .. } => *at,
        }
    }

    /// One line for logs, for example `msaa (812, 64) 1240x32 in explorer.exe` or
    /// `lost in explorer.exe`. A focus event gives the role and source, never the name or the
    /// value.
    pub fn describe(&self) -> String {
        match self {
            Self::CaretMoved { rect, source, app, .. } => {
                format!("{} {rect} in {}", source.name(), app.exe)
            }
            Self::CaretLost { app, .. } => format!("lost in {}", app.exe),
            Self::FocusChanged { app, window_changed, control, .. } => {
                let new_window = if *window_changed { ", new window" } else { "" };
                match control {
                    Some(c) => {
                        let withheld = if c.value == Value::Withheld { " value withheld" } else { "" };
                        format!("focus {:?} ({}){withheld} in {}{new_window}", c.role, c.source.name(), app.exe)
                    }
                    None => format!("focus no control in {}{new_window}", app.exe),
                }
            }
        }
    }
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

/// Called after an event is sent to a receiver made by [`EventHub::subscribe_with_wake`].
type Wake = Arc<dyn Fn() + Send + Sync>;

struct Subscriber {
    tx: Sender<CoreEvent>,
    wake: Option<Wake>,
}

/// Fan-out delivery of [`CoreEvent`]s. Each [`subscribe`](Self::subscribe) gets its own
/// unbounded receiver, so publishing never blocks on a slow consumer.
#[derive(Default)]
pub struct EventHub {
    senders: Mutex<Vec<Subscriber>>,
}

impl EventHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// A new receiver that gets every event published from now on.
    pub fn subscribe(&self) -> Receiver<CoreEvent> {
        let (tx, rx) = channel();
        self.senders.lock().push(Subscriber { tx, wake: None });
        rx
    }

    /// Like [`subscribe`](Self::subscribe), and `wake` is called on the publishing thread after
    /// each event has been sent to the receiver (ADR 0013). A subscriber that waits on something
    /// other than the channel (the speech thread waits on Win32 handles) uses it to wake up.
    /// `wake` must be cheap and must not block; the hub's lock is not held while it runs.
    pub fn subscribe_with_wake(
        &self,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Receiver<CoreEvent> {
        let (tx, rx) = channel();
        self.senders.lock().push(Subscriber { tx, wake: Some(Arc::new(wake)) });
        rx
    }

    /// Sends `event` to every live receiver and forgets receivers that were dropped.
    pub fn publish(&self, event: CoreEvent) {
        let mut wakes = Vec::new();
        self.senders.lock().retain(|sub| {
            let alive = sub.tx.send(event.clone()).is_ok();
            if alive && let Some(wake) = &sub.wake {
                wakes.push(wake.clone());
            }
            alive
        });
        for wake in wakes {
            wake();
        }
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
    fn at_is_the_time_each_kind_carries() {
        let at = Instant::now();
        assert_eq!(CoreEvent::CaretLost { at, app: app() }.at(), at);
        let e = CoreEvent::CaretMoved {
            at,
            rect: ScreenRect { left: 1, top: 2, width: 3, height: 4 },
            source: CaretSource::Gui,
            app: app(),
        };
        assert_eq!(e.at(), at);
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

    #[test]
    fn a_wake_is_called_once_per_event_after_the_event_is_queued() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let hub = EventHub::new();
        let woken = Arc::new(AtomicUsize::new(0));
        let queued_at_wake = Arc::new(Mutex::new(Vec::new()));
        let slot: Arc<Mutex<Option<Receiver<CoreEvent>>>> = Arc::new(Mutex::new(None));
        let rx = {
            let (woken, seen, slot) = (woken.clone(), queued_at_wake.clone(), slot.clone());
            hub.subscribe_with_wake(move || {
                woken.fetch_add(1, Ordering::SeqCst);
                if let Some(rx) = slot.lock().as_ref() {
                    seen.lock().push(rx.try_recv().is_ok());
                }
            })
        };
        *slot.lock() = Some(rx);
        hub.publish(moved(1));
        hub.publish(moved(2));
        assert_eq!(woken.load(Ordering::SeqCst), 2);
        assert_eq!(*queued_at_wake.lock(), vec![true, true]);
    }

    #[test]
    fn plain_subscribers_and_wake_subscribers_both_get_events() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let hub = EventHub::new();
        let woken = Arc::new(AtomicUsize::new(0));
        let plain = hub.subscribe();
        let counter = woken.clone();
        let waking = hub.subscribe_with_wake(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let e = moved(3);
        hub.publish(e.clone());
        assert_eq!(plain.try_recv().unwrap(), e);
        assert_eq!(waking.try_recv().unwrap(), e);
        assert_eq!(woken.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_dropped_wake_receiver_is_forgotten_and_not_woken() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let hub = EventHub::new();
        let woken = Arc::new(AtomicUsize::new(0));
        let counter = woken.clone();
        drop(hub.subscribe_with_wake(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        hub.publish(moved(1));
        assert_eq!(hub.subscribers(), 0);
        assert_eq!(woken.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_wake_may_subscribe_without_deadlocking_the_hub() {
        let hub = Arc::new(EventHub::new());
        let inner = hub.clone();
        let _rx = hub.subscribe_with_wake(move || {
            drop(inner.subscribe());
        });
        hub.publish(moved(1));
        assert!(hub.subscribers() >= 1);
    }

    #[test]
    fn describe_names_source_rect_and_app() {
        assert_eq!(moved(5).describe(), "msaa (5, 10) 2x20 in notepad.exe");
        let lost = CoreEvent::CaretLost { at: Instant::now(), app: app() };
        assert_eq!(lost.describe(), "lost in notepad.exe");
    }
}
