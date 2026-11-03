//! The focus thread (ADR 0015, roadmap 9.4b). It reads the control that has focus and publishes
//! `CoreEvent::FocusChanged` to the core's `EventHub`.
//!
//! - MTA COM first; it owns no window and adds no UI Automation event handlers. It has its own
//!   `CUIAutomation8` with the caret thread's 500 ms timeouts, so a hung app's focus read never
//!   delays caret tracking. No COM object leaves the thread.
//! - It hooks foreground and focus WinEvents out of context. The callback only queues the event;
//!   a pure `FocusFilter` in cv-core decides when to read (after the burst settles) and what to
//!   publish.
//! - It reads only while `tts_enabled` or `follow_focus` is true (roadmap 9.9), and never when the
//!   foreground window is ours. With both off it takes the WinEvents and drops them. With only
//!   `follow_focus` on it reads the control and no more: no dialog text, no value (nothing is
//!   spoken, and the magnifier needs only the rectangle).
//! - One read: UI Automation for the control that has focus now (one cached call, no value), MSAA
//!   for the newest event's object, values only after `value_allowed`, then `focus::merge`.
//! - A window that comes to the front is also read as a dialog, once (ADR 0016, dialog.rs): its
//!   text is found by a bounded walk and kept per (window, inner dialog) pair (cache.rs), so every
//!   later read there carries it.
//! - The dialog inside the foreground window that holds the focus (a property page, a XAML prompt)
//!   is found by inner.rs (ADR 0018). `Reading` carries the window and dialog keys so the filter
//!   sees a new window or dialog even when its WinEvent did not come.
//! - A window read (ADR 0019, window.rs) is asked for with [`FocusWaker::read_window`]: the press
//!   time goes into a slot and a `WM_APP` wakes the thread, which reads the foreground window's
//!   controls when no focus read is due and publishes `CoreEvent::WindowRead`. It is dropped when
//!   speech is off, and when a foreground WinEvent arrived while it ran.
//! - It changes nothing system-wide; its hooks are removed when it ends.

mod cache;
mod dialog;
mod inner;
mod map;
mod read;
mod walk;
mod window;

use std::{
    cell::RefCell,
    sync::{Arc, Mutex, OnceLock, PoisonError, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use cv_core::{
    AppId, Control, CoreEvent, EventHub, FocusFilter, Reading, SharedState, Value, WindowInfo,
    focus::{merge, value_allowed},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::{
        Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
        Threading::{GetCurrentProcessId, GetCurrentThreadId, INFINITE},
    },
    UI::{
        Accessibility::{HWINEVENTHOOK, IUIAutomation, SetWinEventHook, UnhookWinEvent},
        WindowsAndMessaging::{
            DispatchMessageW, EVENT_OBJECT_FOCUS, EVENT_SYSTEM_FOREGROUND, GA_ROOT, GetAncestor,
            GetWindowThreadProcessId, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_NOREMOVE, PM_REMOVE,
            PeekMessageW, PostThreadMessageW, QS_ALLINPUT, TranslateMessage,
            WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_APP, WM_QUIT, WM_USER,
        },
    },
};

use crate::caret::{UIA_TIMEOUT_MS, create_uia, exe_name, foreground};
use cache::DialogCache;
use dialog::{Target, window_class};
use map::Plan;
use read::{Requests, Stored, UiaReading};

/// How long `shutdown` waits for the thread. A read blocked in a hung app can take up to about
/// twice `UIA_TIMEOUT_MS`; after this the thread is left to end with the process.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(1);

/// True when `CV_FOCUS_LOG=1`: the `[focus]` lines also give the control's name, and each read
/// gets a line with its timings. Never a value. Read once.
fn log_names() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("CV_FOCUS_LOG").is_ok_and(|v| v == "1"))
}

/// What the hook callback saw, queued for the loop.
struct Event {
    at: Instant,
    foreground: bool,
    stored: Stored,
}

/// When the window-read key was pressed, until the thread takes it (ADR 0019). A newer press
/// overwrites an older one that has not been taken. One focus thread per process, so one slot.
static WINDOW_REQUEST: Mutex<Option<Instant>> = Mutex::new(None);

fn put_window_request(at: Instant) {
    *WINDOW_REQUEST.lock().unwrap_or_else(PoisonError::into_inner) = Some(at);
}

fn take_window_request() -> Option<Instant> {
    WINDOW_REQUEST.lock().unwrap_or_else(PoisonError::into_inner).take()
}

thread_local! {
    /// Filled by `on_win_event`, drained by the loop. Out-of-context events arrive on the thread
    /// that set the hook, inside its message retrieval, so the two never overlap.
    static QUEUE: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
}

/// Handle to the focus thread started by [`spawn_focus_source`].
pub struct FocusHandle {
    thread: JoinHandle<()>,
    /// The thread's id, for `PostThreadMessageW`. 0 if it never reported one.
    tid: u32,
}

/// Wakes the focus thread so it reads `tts_enabled` again. Cheap to copy; safe to call from any
/// thread, also after the thread has ended.
#[derive(Clone, Copy)]
pub struct FocusWaker {
    tid: u32,
}

impl FocusWaker {
    pub fn wake(&self) {
        if self.tid != 0 {
            let _ = unsafe { PostThreadMessageW(self.tid, WM_USER, WPARAM(0), LPARAM(0)) };
        }
    }

    /// Asks the thread to read the foreground window's controls (ADR 0019, Ctrl+Alt+Shift+W). The
    /// answer is a `CoreEvent::WindowRead` on the hub, stamped with the time of this call. Does
    /// nothing while speech is off: the thread drops the request.
    pub fn read_window(&self) {
        put_window_request(Instant::now());
        if self.tid != 0 {
            let _ = unsafe { PostThreadMessageW(self.tid, WM_APP, WPARAM(0), LPARAM(0)) };
        }
    }
}

impl FocusHandle {
    pub fn waker(&self) -> FocusWaker {
        FocusWaker { tid: self.tid }
    }

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
            eprintln!("[focus] thread did not stop in time; it ends with the process");
        }
    }
}

/// Starts the focus thread. Returns once the thread has a message queue, so `shutdown` and
/// `wake` can always reach it.
pub fn spawn_focus_source(hub: Arc<EventHub>, state: SharedState) -> FocusHandle {
    let (ready_tx, ready_rx) = mpsc::channel();
    let thread = thread::spawn(move || run(&hub, &state, ready_tx));
    let tid = ready_rx.recv().unwrap_or(0);
    FocusHandle { thread, tid }
}

fn run(hub: &EventHub, state: &SharedState, ready: mpsc::Sender<u32>) {
    // PostThreadMessageW fails until the thread has a message queue; this creates it.
    let mut msg = MSG::default();
    let _ = unsafe { PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE) };
    let _ = ready.send(unsafe { GetCurrentThreadId() });

    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok();
    if let Err(e) = &com {
        eprintln!("[focus] CoInitializeEx failed: {e}; msaa and uia may not answer");
    }
    let uia = create_uia("focus", "uia off");
    let requests = uia.as_ref().and_then(|uia| match Requests::new(uia) {
        Ok(requests) => Some(requests),
        Err(e) => {
            eprintln!("[focus] cache request failed: {e}; uia off");
            None
        }
    });
    let hooks = install_hooks();

    run_loop(hub, state, uia.as_ref().zip(requests.as_ref()));

    for hook in hooks {
        let _ = unsafe { UnhookWinEvent(hook) };
    }
    drop(requests);
    drop(uia);
    if com.is_ok() {
        unsafe { CoUninitialize() };
    }
}

fn install_hooks() -> Vec<HWINEVENTHOOK> {
    let mut hooks = Vec::new();
    for event in [EVENT_SYSTEM_FOREGROUND, EVENT_OBJECT_FOCUS] {
        let hook = unsafe {
            SetWinEventHook(
                event,
                event,
                None,
                Some(on_win_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if hook.is_invalid() {
            eprintln!("[focus] SetWinEventHook failed for event {event:#x}");
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
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    QUEUE.with(|queue| {
        queue.borrow_mut().push(Event {
            at: Instant::now(),
            foreground: event == EVENT_SYSTEM_FOREGROUND,
            stored: Stored { hwnd, id_object, id_child },
        })
    });
}

/// What the loop keeps between reads.
#[derive(Default)]
struct Session {
    filter: FocusFilter,
    /// The newest event since the last read, for the MSAA half.
    newest: Option<Stored>,
    /// The foreground window a UI Automation call timed out in; skipped until the foreground
    /// window changes.
    hung: Option<isize>,
    /// Foreground WinEvents taken while speech was on: a window read that sees this change while
    /// it runs belongs to a window that is no longer in front (ADR 0019).
    foreground_events: u64,
    /// The text the dialog check gave for each (foreground window, inner dialog) pair, empty for a
    /// window that is not a dialog, so the walk runs once per pair (ADR 0016, 0018).
    dialog: DialogCache,
}

/// Runs messages (the WinEvent callbacks run inside `PeekMessageW`), then moves the queued events
/// into the filter. Returns false on `WM_QUIT`. While speech is off the events are dropped.
fn pump(session: &mut Session, wanted: bool) -> bool {
    let mut msg = MSG::default();
    while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
        if msg.message == WM_QUIT {
            return false;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    for event in QUEUE.take() {
        if wanted {
            session.filter.winevent(event.at, event.foreground);
            if event.foreground {
                session.foreground_events += 1;
            }
            if event.foreground && log_names() {
                eprintln!("[focus] foreground event: class {:?}", window_class(event.stored.hwnd));
            }
            session.newest = Some(event.stored);
        }
    }
    true
}

fn run_loop(hub: &EventHub, state: &SharedState, uia: Option<(&IUIAutomation, &Requests)>) {
    let own_pid = unsafe { GetCurrentProcessId() };
    let mut session = Session::default();
    let mut was_wanted = false;
    let mut was_speech = false;

    loop {
        let (speech, follow) = {
            let s = state.read();
            (s.tts_enabled, s.follow_focus)
        };
        let wanted = speech || follow;
        // Either switch changes what a read does, so the session starts over.
        if wanted != was_wanted || speech != was_speech {
            was_wanted = wanted;
            was_speech = speech;
            session = Session::default();
            if wanted {
                // Read once, as if the foreground window had just changed.
                session.filter.winevent(Instant::now(), true);
            }
        }
        if !pump(&mut session, wanted) {
            return;
        }

        if wanted && session.filter.due(Instant::now()) {
            session.filter.begin_read();
            let newest = session.newest.take();
            let reading = read_focus(uia, own_pid, &mut session.hung, &mut session.dialog, newest, speech);
            // Events that arrived while the read ran make it stale (the filter drops it).
            if !pump(&mut session, wanted) {
                return;
            }
            if let Some(event) = session.filter.finish_read(Instant::now(), reading) {
                log_event(&event);
                hub.publish(event);
            }
            continue;
        }

        // A key press asking for the window read. Taken every time round while speech is off, so
        // a press made then is not answered later. With speech on it waits for the focus read
        // that is due or still settling (at most `SETTLE_MAX`), so the answer and the last focus
        // event describe the same window and the focus announcement does not cut the answer off.
        if !speech {
            let _ = take_window_request();
        } else if session.filter.next_deadline().is_none()
            && let Some(at) = take_window_request()
        {
            let before = session.foreground_events;
            let event = window::read(uia, own_pid, &mut session.hung, &session.dialog, at);
            // Events that arrived while the read ran are counted here.
            if !pump(&mut session, wanted) {
                return;
            }
            if session.foreground_events == before {
                log_event(&event);
                hub.publish(event);
            } else {
                eprintln!("[focus] window read dropped: the foreground window changed while it ran");
            }
            continue;
        }

        let timeout = match session.filter.next_deadline().filter(|_| wanted) {
            Some(deadline) => {
                deadline.saturating_duration_since(Instant::now()).as_millis() as u32
            }
            None => INFINITE,
        };
        let _ = unsafe {
            MsgWaitForMultipleObjectsEx(None, timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
        };
    }
}

fn log_event(event: &CoreEvent) {
    let mut line = format!("[focus] {}", event.describe());
    if log_names()
        && let CoreEvent::FocusChanged { control: Some(c), .. } = event
    {
        line.push_str(&format!(" name {:?}", c.name));
    }
    eprintln!("{line}");
}

/// One read of the foreground window, the dialog inside it that holds the focus, and the focused
/// control. The window stays the foreground window (ADR 0015); `window_key` and `dialog_key` let
/// the filter see a new window or dialog (ADR 0018).
fn read_focus(
    uia: Option<(&IUIAutomation, &Requests)>,
    own_pid: u32,
    hung: &mut Option<isize>,
    cache: &mut DialogCache,
    newest: Option<Stored>,
    speech: bool,
) -> Reading {
    let nothing =
        || Reading { app: None, window: WindowInfo::new(""), control: None, window_key: 0, dialog_key: None };
    let Some(fg) = foreground() else {
        return nothing();
    };
    // The foreground window changed: the old hung mark no longer applies.
    if hung.is_some_and(|h| h != fg.hwnd.0 as isize) {
        *hung = None;
    }
    match map::read_plan(true, fg.pid, own_pid, fg.hwnd.0 as isize, *hung) {
        // No call into our own windows, and nothing to say about them.
        Plan::Own | Plan::Off => return nothing(),
        Plan::Hung | Plan::Read => {}
    }
    if log_names() {
        log_event_root(newest.as_ref(), fg.hwnd);
    }

    let exe = exe_name(fg.pid);
    let mut window = WindowInfo::new(read::window_title(fg.hwnd));
    // The shell's staging window and the desktop have no program name: nothing to say.
    let app = (exe != "?").then_some(AppId { pid: fg.pid, exe });
    let key = fg.hwnd.0 as isize;
    let window_key = key as usize as u64;
    let (control, focused) = if app.is_none() || *hung == Some(key) {
        (None, None)
    } else {
        read_control(uia, &fg, hung, newest, speech)
    };

    // The control is kept if the dialog read hangs (ADR 0016); a hung control read skips it.
    let mut dialog_key = None;
    // Dialog text is for speech only.
    if speech && app.is_some() && *hung != Some(key) {
        // A handle can be reused by another dialog later, so another window's pairs go.
        cache.keep_window(window_key);
        let lookup = inner::find(uia, fg.hwnd, focused.as_ref());
        if log_names() {
            log_lookup(&lookup);
        }
        if lookup.hung {
            eprintln!("[focus] dialog ancestor lookup timed out in {}; skipped until the foreground changes", fg.pid);
            *hung = Some(key);
        } else {
            dialog_key = lookup.inner.as_ref().map(|inner| inner.key);
            if let Some(inner) = &lookup.inner {
                window.dialog_name = inner.name.clone();
            }
            window.dialog_text = match cache.get(window_key, dialog_key) {
                Some(text) => text.to_vec(),
                None => {
                    // The inner dialog if there is one, else the window when the check says it is
                    // a dialog (ADR 0018).
                    let fg_target = Target::Window(fg.hwnd);
                    let target = lookup.inner.as_ref().map_or(&fg_target, |inner| &inner.target);
                    let outcome = dialog::read_dialog(uia, target, &window.title);
                    if outcome.hung {
                        *hung = Some(key);
                    } else {
                        cache.put(window_key, dialog_key, outcome.text.clone());
                    }
                    outcome.text
                }
            };
        }
    }
    Reading { app, window, control, window_key, dialog_key }
}

/// `CV_FOCUS_LOG` only: the newest focus event's window root when it is not the foreground window
/// (ADR 0018). Classes only; titles stay in the `new window` line.
fn log_event_root(newest: Option<&Stored>, fg: HWND) {
    let Some(newest) = newest else {
        return;
    };
    let root = unsafe { GetAncestor(newest.hwnd, GA_ROOT) };
    if !root.is_invalid() && root != fg {
        eprintln!("[focus] event root class {:?} is not the foreground", window_class(root));
    }
}

/// `CV_FOCUS_LOG` only: how the inner dialog lookup went and what it cost. Nothing for a read in
/// which the lookup found nothing and made no UI Automation call.
fn log_lookup(lookup: &inner::Lookup) {
    if lookup.inner.is_none() && lookup.normalize.is_none() {
        return;
    }
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let (via, name) = lookup.inner.as_ref().map_or(("none", ""), |inner| (inner.via, inner.name.as_str()));
    let normalize = lookup.normalize.map_or("not run".to_string(), |d| format!("{:.1} ms", ms(d)));
    eprintln!(
        "[focus] inner dialog via {via}: name {name:?}, chain {:.1} ms, normalize {normalize}",
        ms(lookup.chain),
    );
}

/// Both sources, the values the password rule allows, and the merge.
fn read_control(
    uia: Option<(&IUIAutomation, &Requests)>,
    fg: &crate::caret::Foreground,
    hung: &mut Option<isize>,
    newest: Option<Stored>,
    speech: bool,
) -> (Option<Control>, Option<UiaReading>) {
    let started = Instant::now();
    let mut uia_reading = None;
    let mut uia_error = None;
    if let Some((uia, requests)) = uia {
        match read::uia_read(uia, requests) {
            // The desktop itself as the focused element (Finding 29) is no reading of a control.
            Ok(r) if map::is_desktop_pane(r.control.role, &r.class) => {
                if log_names() {
                    eprintln!("[focus] uia answered the desktop pane (class {:?}); not used", r.class);
                }
            }
            Ok(r) => uia_reading = Some(r),
            Err(e) => {
                if map::is_hung(e.code().0, started.elapsed().as_millis() as u64, UIA_TIMEOUT_MS) {
                    eprintln!("[focus] uia timed out in {}; skipped until the foreground changes", fg.pid);
                    *hung = Some(fg.hwnd.0 as isize);
                    return (None, None);
                }
                uia_error = Some(e);
            }
        }
    }
    let uia_ms = started.elapsed().as_millis();

    // The event's object, only if it belongs to the foreground program: a focus event from a
    // background window is not what the user is looking at.
    let msaa_started = Instant::now();
    let msaa_reading = newest
        .filter(|s| {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(s.hwnd, Some(&mut pid)) };
            pid == fg.pid
        })
        .and_then(|s| read::msaa_read(&s));
    let msaa_ms = msaa_started.elapsed().as_millis();

    // Both password answers first: each one gates the other's value.
    let uia_password = uia_reading.as_ref().and_then(|u| u.password);
    let msaa_protected = msaa_reading.as_ref().and_then(|m| m.protected);

    let uia_control = uia_reading.as_ref().map(|u| {
        let mut control = u.control.clone();
        // A value is read for speech only; following focus needs the rectangle.
        control.value = if !speech {
            Value::None
        } else if value_allowed(uia_password, msaa_protected) {
            match uia.map(|(_, requests)| read::uia_value(u, requests)) {
                Some(Ok(text)) => read::value_of(text),
                Some(Err(e)) => {
                    if map::is_hung(e.code().0, 0, UIA_TIMEOUT_MS) {
                        *hung = Some(fg.hwnd.0 as isize);
                    }
                    Value::None
                }
                None => Value::None,
            }
        } else {
            Value::Withheld
        };
        control
    });
    let msaa_control = msaa_reading.as_ref().map(|m| {
        let mut control = m.control.clone();
        control.value = if !speech {
            Value::None
        } else if value_allowed(msaa_protected, uia_password) {
            read::value_of(read::msaa_value(m))
        } else {
            Value::Withheld
        };
        control
    });

    let merged = merge(uia_control, msaa_control);
    if log_names() {
        let source = merged.as_ref().map_or("none", |c| c.source.name());
        let framework = uia_reading.as_ref().map_or("?", |u| u.framework.as_str());
        let error = uia_error.map_or(String::new(), |e| format!(" (uia error {:#010x})", e.code().0));
        eprintln!(
            "[focus] read: uia {uia_ms} ms{error}, msaa {msaa_ms} ms ({}), uia framework {framework:?}, merged {source}",
            if msaa_reading.is_some() { "answered" } else { "none" },
        );
    }
    (merged, uia_reading)
}
