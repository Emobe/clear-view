//! The caret thread (ADR 0008). It reads the caret position of the foreground app and publishes
//! `CoreEvent::CaretMoved` and `CaretLost` to the core's `EventHub`.
//!
//! - MTA COM first; it owns no window and adds no UI Automation event handlers (ADR 0003's rules,
//!   kept for the core). No COM object leaves the thread.
//! - It wakes on out-of-context WinEvents from other processes: foreground, focus, and caret
//!   show, hide and move. Each wake reads the source chain once (read.rs).
//! - While the last answer came from uia or the focus rectangle, which no WinEvent announces,
//!   it also reads every `POLL_EVERY` until focus or the foreground changes.
//! - It changes nothing system-wide, so there is nothing to restore on exit or kill. Its hooks
//!   are removed when it ends.

mod read;
mod tracker;

use std::{
    cell::Cell,
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use cv_core::{AppId, EventHub};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
            Threading::{GetCurrentProcessId, GetCurrentThreadId, INFINITE},
        },
        UI::{
            Accessibility::{
                CUIAutomation8, HWINEVENTHOOK, IUIAutomation, IUIAutomation2, SetWinEventHook,
                UnhookWinEvent,
            },
            WindowsAndMessaging::{
                DispatchMessageW, EVENT_OBJECT_FOCUS, EVENT_OBJECT_HIDE,
                EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_SHOW, EVENT_SYSTEM_FOREGROUND, MSG,
                MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, OBJID_CARET, PM_NOREMOVE,
                PM_REMOVE, PeekMessageW, PostThreadMessageW, QS_ALLINPUT, TranslateMessage,
                WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_QUIT, WM_USER,
            },
        },
    },
    core::Interface,
};

use read::{exe_name, foreground, read_chain};
use tracker::Tracker;

/// UI Automation's connection and transaction timeouts, lowered from 2 s and 20 s so one hung
/// app cannot stall caret tracking for long. A uia read took up to 36 ms in 4.2.
const UIA_TIMEOUT_MS: u32 = 500;
/// How often to read while only uia or the focus rectangle can see the caret (ADR 0008: 20 Hz).
const POLL_EVERY: Duration = Duration::from_millis(50);
/// How long `shutdown` waits for the thread. A read blocked in a hung app can take up to
/// `UIA_TIMEOUT_MS`; after this the thread is left to end with the process.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(1);

/// The WinEvents that wake the thread, as (min, max) ranges.
const HOOKS: [(u32, u32); 4] = [
    (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
    (EVENT_OBJECT_FOCUS, EVENT_OBJECT_FOCUS),
    (EVENT_OBJECT_SHOW, EVENT_OBJECT_HIDE),
    (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
];

/// What the WinEvents since the last read asked for.
#[derive(Clone, Copy, Default)]
struct Wake {
    foreground: bool,
    focus: bool,
    caret: bool,
}

impl Wake {
    fn any(self) -> bool {
        self.foreground || self.focus || self.caret
    }
}

thread_local! {
    /// Set by `on_win_event`, taken by the loop. Out-of-context events arrive on the thread that
    /// set the hook, inside its message retrieval, so the loop and the callback never overlap.
    static WAKE: Cell<Wake> = Cell::new(Wake::default());
}

/// Handle to the caret thread started by [`spawn_caret_source`].
pub struct CaretHandle {
    thread: JoinHandle<()>,
    /// The thread's id, for `PostThreadMessageW`. 0 if it never reported one.
    tid: u32,
}

impl CaretHandle {
    /// Asks the thread to unhook and exit, then waits up to `SHUTDOWN_WAIT` for it.
    pub fn shutdown(self) {
        if self.tid != 0 {
            let _ = unsafe { PostThreadMessageW(self.tid, WM_QUIT, WPARAM(0), LPARAM(0)) };
        }
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        while !self.thread.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if self.thread.is_finished() {
            let _ = self.thread.join();
        } else {
            eprintln!("[caret] thread did not stop in time; it ends with the process");
        }
    }
}

/// Starts the caret thread. Returns once the thread has a message queue, so `shutdown` can
/// always reach it.
pub fn spawn_caret_source(hub: Arc<EventHub>) -> CaretHandle {
    let (ready_tx, ready_rx) = mpsc::channel();
    let thread = thread::spawn(move || run(&hub, ready_tx));
    let tid = ready_rx.recv().unwrap_or(0);
    CaretHandle { thread, tid }
}

fn run(hub: &EventHub, ready: mpsc::Sender<u32>) {
    // PostThreadMessageW fails until the thread has a message queue; this creates it
    // (Microsoft docs).
    let mut msg = MSG::default();
    let _ = unsafe { PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE) };
    let _ = ready.send(unsafe { GetCurrentThreadId() });

    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok();
    if let Err(e) = &com {
        eprintln!("[caret] CoInitializeEx failed: {e}; msaa and uia may not answer");
    }
    let uia = create_uia();
    let hooks = install_hooks();

    run_loop(hub, uia.as_ref());

    for hook in hooks {
        let _ = unsafe { UnhookWinEvent(hook) };
    }
    drop(uia);
    if com.is_ok() {
        unsafe { CoUninitialize() };
    }
}

fn create_uia() -> Option<IUIAutomation> {
    let uia: IUIAutomation =
        match unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) } {
            Ok(uia) => uia,
            Err(e) => {
                eprintln!("[caret] UI Automation unavailable: {e}; uia and focus rect off");
                return None;
            }
        };
    match uia.cast::<IUIAutomation2>() {
        Ok(uia2) => unsafe {
            let _ = uia2.SetConnectionTimeout(UIA_TIMEOUT_MS);
            let _ = uia2.SetTransactionTimeout(UIA_TIMEOUT_MS);
            let ms = |r: windows::core::Result<u32>| {
                r.map(|v| format!("{v} ms")).unwrap_or_else(|_| "?".into())
            };
            eprintln!(
                "[caret] UIA timeouts: connection {}, transaction {}",
                ms(uia2.ConnectionTimeout()),
                ms(uia2.TransactionTimeout())
            );
        },
        Err(e) => eprintln!("[caret] IUIAutomation2 unavailable ({e}); UIA timeouts stay 2 s and 20 s"),
    }
    Some(uia)
}

fn install_hooks() -> Vec<HWINEVENTHOOK> {
    let mut hooks = Vec::new();
    for (min, max) in HOOKS {
        let hook = unsafe {
            SetWinEventHook(
                min,
                max,
                None,
                Some(on_win_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if hook.is_invalid() {
            eprintln!("[caret] SetWinEventHook failed for events {min:#x}..{max:#x}");
        } else {
            hooks.push(hook);
        }
    }
    hooks
}

/// Only records what happened: out-of-context callbacks must return quickly (Microsoft docs),
/// and the loop does the reading.
unsafe extern "system" fn on_win_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    _hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    WAKE.with(|wake| {
        let mut w = wake.get();
        match event {
            EVENT_SYSTEM_FOREGROUND => w.foreground = true,
            EVENT_OBJECT_FOCUS => w.focus = true,
            _ if id_object == OBJID_CARET.0 => w.caret = true,
            _ => return,
        }
        wake.set(w);
    });
}

/// Reads on each wake and poll, then waits for the next message or poll. Returns on `WM_QUIT`.
fn run_loop(hub: &EventHub, uia: Option<&IUIAutomation>) {
    let own_pid = unsafe { GetCurrentProcessId() };
    let mut tracker = Tracker::new();
    let mut app: Option<AppId> = None;
    let mut next_poll: Option<Instant> = None;
    // Read once at startup, as if everything had just changed.
    let mut wake = Wake { foreground: true, focus: true, caret: true };
    let mut msg = MSG::default();

    loop {
        let poll_due = next_poll.is_some_and(|t| Instant::now() >= t);
        if wake.any() || poll_due {
            if wake.foreground {
                tracker.foreground_changed();
            } else if wake.focus {
                tracker.focus_changed();
            }
            read_once(hub, uia, own_pid, &mut tracker, &mut app);
            next_poll = tracker.polling().then(|| Instant::now() + POLL_EVERY);
        }

        let timeout = match next_poll {
            Some(t) => t.saturating_duration_since(Instant::now()).as_millis() as u32,
            None => INFINITE,
        };
        let _ = unsafe { MsgWaitForMultipleObjectsEx(None, timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE) };
        // WinEvent callbacks run inside PeekMessageW.
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_QUIT {
                return;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        wake = WAKE.take();
    }
}

/// One read of the foreground app's caret, published and logged through the tracker.
fn read_once(
    hub: &EventHub,
    uia: Option<&IUIAutomation>,
    own_pid: u32,
    tracker: &mut Tracker,
    app: &mut Option<AppId>,
) {
    let Some(fg) = foreground() else {
        return;
    };
    let at = Instant::now();
    // Read once per foreground process.
    let app = match app {
        Some(a) if a.pid == fg.pid => a,
        _ => app.insert(AppId { pid: fg.pid, exe: exe_name(fg.pid) }),
    };
    // Our own windows (the settings panel) have no caret to follow. Reading them would also
    // send WM_GETOBJECT to the main thread, which at exit is waiting for this one.
    let found = if fg.pid == own_pid { None } else { read_chain(uia, &fg) };
    let outcome = tracker.update(at, app, found);
    if let Some(line) = outcome.log {
        eprintln!("{line}");
    }
    if let Some(event) = outcome.event {
        hub.publish(event);
    }
}
