use std::{
    ffi::c_void,
    mem::size_of,
    num::NonZeroIsize,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering},
        mpsc::Receiver,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        System::{Console::SetConsoleCtrlHandler, LibraryLoader::GetModuleHandleW},
        UI::{
            Magnification::{MagInitialize, MagShowSystemCursor, MagUninitialize},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
                HTTRANSPARENT, IDC_ARROW, KillTimer, LWA_ALPHA, LoadCursorW, MSG, PostMessageW,
                PostQuitMessage, RegisterClassExW, SW_HIDE, SW_SHOW, SWP_NOACTIVATE,
                SWP_NOZORDER, SetLayeredWindowAttributes, SetTimer, SetWindowDisplayAffinity,
                SetWindowPos, ShowWindow, TranslateMessage, UnregisterClassW,
                WDA_EXCLUDEFROMCAPTURE, WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_NCHITTEST, WM_TIMER,
                WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOPMOST,
                WS_EX_TRANSPARENT, WS_POPUP,
            },
        },
    },
    core::{BOOL, PCWSTR, w},
};

use cv_core::{
    CoreEvent, FrameState, OutputInfo, PointerSource, ScreenPoint, ScreenRect, SharedState,
    geometry,
};
use cv_magnifier::{Layout, Magnifier, OverlayHost};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle};

use crate::pointer::CursorPointer;

const CLASS_NAME: PCWSTR = w!("clear_view_overlay");

/// The tick timer on the overlay window.
const TIMER_ID: usize = 1;
const TICK_MS: u32 = 16;

/// The overlay window, as an integer so other threads can post `WM_CLOSE` to it. 0 = none.
/// There is one overlay per process.
static OVERLAY_HWND: AtomicIsize = AtomicIsize::new(0);
/// Set when `teardown` has finished, so the console handler knows it may return.
static TEARDOWN_DONE: AtomicBool = AtomicBool::new(false);

/// How long to wait for the render thread to restore the machine before giving up.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// Handle to the render thread started by [`spawn_overlay`].
pub struct OverlayHandle {
    thread: JoinHandle<()>,
}

impl OverlayHandle {
    /// Asks the render thread to restore the machine and exit, then waits for it.
    /// If the thread does not finish in time, shows the system cursor from the calling thread
    /// (it is system-wide).
    pub fn shutdown(self) {
        request_close();
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        while !self.thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if self.thread.is_finished() {
            let _ = self.thread.join();
        } else {
            eprintln!("[render] thread did not stop in time; restoring cursor only");
            let _ = unsafe { MagShowSystemCursor(true) };
        }
    }
}

/// Starts the render thread: it creates the overlay window and runs its message loop.
/// It also installs a console handler so Ctrl+C and closing the console window restore the
/// machine before the process ends.
///
/// `events` is the render thread's core event subscription; each tick drains it (ADR 0008).
pub fn spawn_overlay(
    frame_state: FrameState,
    app_state: SharedState,
    outputs: Vec<OutputInfo>,
    desired_output: Arc<AtomicU32>,
    events: Receiver<CoreEvent>,
) -> OverlayHandle {
    // Best-effort: with no console attached there is nothing to hook.
    let _ = unsafe { SetConsoleCtrlHandler(Some(console_ctrl_handler), true) };
    let thread = std::thread::spawn(move || {
        run_overlay(frame_state, app_state, outputs, desired_output, events);
    });
    OverlayHandle { thread }
}

/// Posts `WM_CLOSE` to the overlay window. Returns false if there is no window (yet, or any more).
fn request_close() -> bool {
    let raw = OVERLAY_HWND.load(Ordering::SeqCst);
    if raw == 0 {
        return false;
    }
    let hwnd = HWND(raw as *mut c_void);
    unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_ok()
}

/// Runs on a thread the system creates for the event. Waits for the render thread to restore
/// the machine, then returns FALSE so the default handler ends the process.
unsafe extern "system" fn console_ctrl_handler(_event: u32) -> BOOL {
    if request_close() {
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        while !TEARDOWN_DONE.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    BOOL(0)
}

/// Restores what the overlay changed, the system cursor, then destroys the window. Runs from
/// `WinHost`'s `Drop`, so it also runs when the render thread panics. The work area and the
/// cursor clip are never changed (ADR 0007), so there is nothing to restore for them; releasing
/// the clip here would free one another app set.
fn teardown(hwnd: HWND) {
    OVERLAY_HWND.store(0, Ordering::SeqCst);
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_ID);
    }
    // Cursor hiding is best-effort: a failed call leaves nothing more to do.
    let _ = unsafe { MagShowSystemCursor(true) };
    let _ = unsafe { MagUninitialize() };
    unsafe {
        let _ = DestroyWindow(hwnd);
        if let Ok(hinstance) = GetModuleHandleW(None) {
            let _ = UnregisterClassW(CLASS_NAME, Some(hinstance.into()));
        }
    }
    TEARDOWN_DONE.store(true, Ordering::SeqCst);
}

/// The overlay window as the magnifier sees it (ADR 0004). Owns window placement and system
/// cursor hiding. Dropping it restores the machine and destroys the window.
struct WinHost {
    hwnd: HWND,
    /// What `apply_layout` last made the window.
    applied: Layout,
}

impl PointerSource for WinHost {
    fn position(&self) -> Option<ScreenPoint> {
        CursorPointer.position()
    }
}

impl OverlayHost for WinHost {
    fn apply_layout(&mut self, layout: &Layout) -> (u32, u32) {
        let was_hidden = self.applied == Layout::Hidden;
        let size = match layout {
            Layout::Hidden => {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
                (0, 0)
            }
            Layout::Fullscreen { monitor } => {
                let rect = RECT {
                    left: monitor.left,
                    top: monitor.top,
                    right: monitor.left + monitor.width as i32,
                    bottom: monitor.top + monitor.height as i32,
                };
                move_window(self.hwnd, rect);
                rect_dims(rect)
            }
            // Overlay docking (ADR 0007): the panel sits on top of the desktop at the monitor
            // edge. The work area is not changed and the cursor is not clipped.
            Layout::Docked { monitor, edge, thickness } => {
                let rect = to_rect(geometry::docked_rect(monitor, *edge, *thickness));
                move_window(self.hwnd, rect);
                rect_dims(rect)
            }
        };

        if was_hidden && *layout != Layout::Hidden {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOW);
            }
        }
        // Hidden in fullscreen, shown otherwise; while docked the magnifier then hides it inside
        // the panel through `set_system_cursor`. No reference count, so a repeated call is
        // harmless.
        let show_cursor = !matches!(layout, Layout::Fullscreen { .. });
        let _ = unsafe { MagShowSystemCursor(show_cursor) };

        self.applied = layout.clone();
        size
    }

    fn set_system_cursor(&mut self, visible: bool) {
        // Best-effort, like every other `MagShowSystemCursor` call: teardown shows it again.
        let _ = unsafe { MagShowSystemCursor(visible) };
    }

    fn raw_handles(&self) -> (RawDisplayHandle, RawWindowHandle) {
        let display = RawDisplayHandle::Windows(WindowsDisplayHandle::new());
        let window = RawWindowHandle::Win32(Win32WindowHandle::new(
            NonZeroIsize::new(self.hwnd.0 as isize).expect("overlay window handle is non-null"),
        ));
        (display, window)
    }
}

impl Drop for WinHost {
    fn drop(&mut self) {
        teardown(self.hwnd);
    }
}

/// Creates the overlay window and blocks on its message loop.
fn run_overlay(
    frame_state: FrameState,
    app_state: SharedState,
    outputs: Vec<OutputInfo>,
    desired_output: Arc<AtomicU32>,
    events: Receiver<CoreEvent>,
) {
    // Use primary monitor (first in list) as the starting monitor.
    let primary = outputs.first().cloned().unwrap_or(OutputInfo {
        idx: 0,
        left: 0,
        top: 0,
        width: 1920,
        height: 1080,
    });

    let hwnd = unsafe {
        let hinstance: HINSTANCE = GetModuleHandleW(None).unwrap().into();
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            lpszClassName: CLASS_NAME,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TRANSPARENT,
            CLASS_NAME,
            w!("clear-view"),
            WS_POPUP, // starts hidden
            primary.left,
            primary.top,
            primary.width as i32,
            primary.height as i32,
            None,
            None,
            Some(hinstance),
            None,
        )
        .expect("CreateWindowExW failed");

        SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).ok();
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA).ok();
        hwnd
    };
    OVERLAY_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    // From here on, any exit from this function (including a panic) restores the machine when
    // `host` is dropped.
    let mut host = WinHost { hwnd, applied: Layout::Hidden };

    // wgpu init (blocking): the window starts primary-monitor-sized.
    // SAFETY: `magnifier` is declared after `host`, so it (and its wgpu surface) is dropped
    // first; the window is destroyed in `host`'s `Drop`.
    let mut magnifier = unsafe {
        Magnifier::new(&host, primary, outputs, app_state, frame_state, desired_output, events)
    };

    // Cursor hiding is best-effort: a failed Mag* call leaves nothing to act on.
    let _ = unsafe { MagInitialize() };

    unsafe { SetTimer(Some(hwnd), TIMER_ID, TICK_MS, None) };

    // The loop runs the tick itself rather than dispatching WM_TIMER to `wnd_proc`, so the host
    // and magnifier are plain locals and `wnd_proc` never touches them.
    let mut msg = MSG::default();
    loop {
        let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if ret.0 == 0 {
            break; // WM_QUIT
        }
        if ret.0 == -1 {
            eprintln!("[render] GetMessageW failed: {}", windows::core::Error::from_thread());
            break;
        }
        if msg.message == WM_TIMER && msg.hwnd == hwnd && msg.wParam.0 == TIMER_ID {
            magnifier.tick(&mut host, Instant::now());
            continue;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    // `magnifier` then `host` drop here; `host` runs `teardown`.
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // Close is a request from another thread. The window is destroyed in `teardown`,
        // after the message loop ends, so nothing is torn down while a tick is running.
        WM_CLOSE => unsafe {
            PostQuitMessage(0);
            LRESULT(0)
        },
        WM_DESTROY => unsafe {
            PostQuitMessage(0);
            LRESULT(0)
        },
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        // Sent when the window lands on a monitor with other scaling (fullscreen, or a docked
        // panel that follows the target, roadmap 5.2). The suggested rect in lParam is the old
        // size scaled for the new DPI; the magnifier places the window in physical pixels and
        // already gave it the right rect, so the suggestion is ignored. The window is resized
        // only if this handler calls `SetWindowPos` with it (Microsoft docs, WM_DPICHANGED), so
        // `WM_GETDPISCALEDSIZE`, which only changes that suggestion, needs no handler (5.3).
        WM_DPICHANGED => LRESULT(0),
        // The message loop runs the tick; nothing to do if a timer message is dispatched.
        WM_TIMER => LRESULT(0),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn to_rect(r: ScreenRect) -> RECT {
    RECT {
        left: r.left,
        top: r.top,
        right: r.left + r.width as i32,
        bottom: r.top + r.height as i32,
    }
}

fn rect_dims(r: RECT) -> (u32, u32) {
    (
        (r.right - r.left).max(16) as u32,
        (r.bottom - r.top).max(16) as u32,
    )
}

fn move_window(hwnd: HWND, r: RECT) {
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
        .ok();
    }
}
