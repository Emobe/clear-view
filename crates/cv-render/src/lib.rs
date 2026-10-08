mod appbar;
mod gfx;

use std::{
    cell::RefCell,
    ffi::c_void,
    mem::size_of,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering},
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
                ClipCursor, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                GetCursorPos, GetMessageW, HTTRANSPARENT, IDC_ARROW, KillTimer, LWA_ALPHA,
                LoadCursorW, MSG, PostMessageW, PostQuitMessage, RegisterClassExW,
                RegisterWindowMessageW, SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER,
                SetLayeredWindowAttributes, SetTimer, SetWindowDisplayAffinity, SetWindowPos,
                ShowWindow, SystemParametersInfoW, TranslateMessage, UnregisterClassW,
                WDA_EXCLUDEFROMCAPTURE, WM_CLOSE, WM_DESTROY, WM_NCHITTEST, WM_TIMER,
                WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOPMOST,
                WS_EX_TRANSPARENT, WS_POPUP, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
            },
        },
    },
    core::{BOOL, PCWSTR, w},
};

use cv_core::{
    ColorFilter, DisplayMode, Edge, Frame, FrameState, Interpolation, OutputInfo, SharedState,
};
use cv_core::geometry::{self, panel_pct_to_px, window_dims};
use gfx::WgpuState;

struct WindowData {
    wgpu: WgpuState,
    frame_state: FrameState,
    app_state: SharedState,
    smooth_x: f32,
    smooth_y: f32,
    last_tick: Instant,
    /// All monitors enumerated at startup (virtual-screen coords).
    outputs: Vec<OutputInfo>,
    /// Index into `outputs` for the currently-active monitor.
    active_output_idx: u32,
    /// Top-left of the active monitor in virtual screen coordinates.
    monitor_left: i32,
    monitor_top: i32,
    /// Width/height of the active monitor.
    screen_w: i32,
    screen_h: i32,
    /// Shared with the capture thread — signals which output to duplicate.
    desired_output: Arc<AtomicU32>,
    callback_msg: u32,
    // Change-detection: what is currently applied to the window.
    cur_enabled: bool,
    cur_mode: DisplayMode,
    cur_panel_size: u32,
    appbar_active: bool,
    // GPU write caching — skip redundant uploads/uniform writes.
    last_frame: Option<Arc<Frame>>,
    last_crop: [f32; 4],
    last_color_mode: u32,
    last_interp_mode: u32,
    last_cursor_x: u32,
    last_cursor_y: u32,
}

thread_local! {
    static WIN_DATA: RefCell<Option<WindowData>> = const { RefCell::new(None) };
}

const CLASS_NAME: PCWSTR = w!("clear_view_overlay");

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
    /// If the thread does not finish in time, restores the cursor and clip from the calling
    /// thread (both are system-wide). The AppBar can only be removed by the render thread.
    pub fn shutdown(self) {
        request_close();
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        while !self.thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if self.thread.is_finished() {
            let _ = self.thread.join();
        } else {
            eprintln!("[render] thread did not stop in time; restoring cursor and clip only");
            let _ = unsafe { MagShowSystemCursor(true) };
            update_clip_cursor(false);
        }
    }
}

/// Starts the render thread: it creates the overlay window and runs its message loop.
/// It also installs a console handler so Ctrl+C and closing the console window restore the
/// machine before the process ends.
pub fn spawn_overlay(
    frame_state: FrameState,
    app_state: SharedState,
    outputs: Vec<OutputInfo>,
    desired_output: Arc<AtomicU32>,
) -> OverlayHandle {
    // Best-effort: with no console attached there is nothing to hook.
    let _ = unsafe { SetConsoleCtrlHandler(Some(console_ctrl_handler), true) };
    let thread = std::thread::spawn(move || {
        run_overlay(frame_state, app_state, outputs, desired_output);
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

/// Restores everything the overlay changed: the AppBar work area, the system cursor and the
/// cursor clip, then destroys the window. Runs from `TeardownGuard`, so it also runs when the
/// render thread panics. It does not rely on `WIN_DATA` being in a usable state.
fn teardown(hwnd: HWND) {
    OVERLAY_HWND.store(0, Ordering::SeqCst);
    unsafe {
        let _ = KillTimer(Some(hwnd), 1);
    }
    // Drop the wgpu surface before the window it draws to goes away.
    let data = WIN_DATA.with(|d| d.try_borrow_mut().ok().and_then(|mut b| b.take()));
    // With no data (early panic, or a borrow in flight) assume the AppBar may be registered;
    // ABM_REMOVE always succeeds.
    let appbar_active = data.as_ref().is_none_or(|w| w.appbar_active);
    drop(data);
    if appbar_active {
        appbar::unregister(hwnd);
    }
    // Cursor hiding and the clip are best-effort: a failed call leaves nothing more to do.
    let _ = unsafe { MagShowSystemCursor(true) };
    let _ = unsafe { MagUninitialize() };
    update_clip_cursor(false);
    unsafe {
        let _ = DestroyWindow(hwnd);
        if let Ok(hinstance) = GetModuleHandleW(None) {
            let _ = UnregisterClassW(CLASS_NAME, Some(hinstance.into()));
        }
    }
    TEARDOWN_DONE.store(true, Ordering::SeqCst);
}

struct TeardownGuard(HWND);

impl Drop for TeardownGuard {
    fn drop(&mut self) {
        teardown(self.0);
    }
}

/// Creates the overlay window and blocks on its message loop.
fn run_overlay(
    frame_state: FrameState,
    app_state: SharedState,
    outputs: Vec<OutputInfo>,
    desired_output: Arc<AtomicU32>,
) {
    // Use primary monitor (first in list) as the starting monitor.
    let primary = outputs.first().cloned().unwrap_or(OutputInfo {
        idx: 0,
        left: 0,
        top: 0,
        width: 1920,
        height: 1080,
    });
    let screen_w = primary.width as i32;
    let screen_h = primary.height as i32;

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
            screen_w,
            screen_h,
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
    // From here on, any exit from this function (including a panic) restores the machine.
    let _guard = TeardownGuard(hwnd);

    let callback_msg = unsafe { RegisterWindowMessageW(w!("ClearViewAppBar")) };

    // wgpu init (blocking) — window starts primary-monitor-sized.
    let wgpu = WgpuState::new(
        hwnd,
        screen_w as u32,
        screen_h as u32,
        primary.width,
        primary.height,
    );

    WIN_DATA.with(|d| {
        *d.borrow_mut() = Some(WindowData {
            wgpu,
            frame_state,
            app_state,
            smooth_x: screen_w as f32 / 2.0 + primary.left as f32,
            smooth_y: screen_h as f32 / 2.0 + primary.top as f32,
            last_tick: Instant::now(),
            active_output_idx: primary.idx,
            monitor_left: primary.left,
            monitor_top: primary.top,
            screen_w,
            screen_h,
            outputs,
            desired_output,
            callback_msg,
            cur_enabled: false,
            cur_mode: DisplayMode::Fullscreen,
            cur_panel_size: 300,
            appbar_active: false,
            last_frame: None,
            last_crop: [f32::NAN; 4],   // NAN != NAN → forces first write
            last_color_mode: u32::MAX,  // forces first write
            last_interp_mode: u32::MAX, // forces first write
            last_cursor_x: u32::MAX,    // forces first write
            last_cursor_y: u32::MAX,    // forces first write
        });
    });

    // Cursor hiding is best-effort: a failed Mag* call leaves nothing to act on.
    let _ = unsafe { MagInitialize() };

    unsafe { SetTimer(Some(hwnd), 1, 16, None) };

    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    // `_guard` runs `teardown` as the function returns.
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let callback_msg = WIN_DATA
        .with(|d| d.borrow().as_ref().map(|w| w.callback_msg))
        .unwrap_or(0);

    if msg != 0 && msg == callback_msg {
        handle_appbar_callback(hwnd, wparam);
        return LRESULT(0);
    }

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
        WM_TIMER => {
            on_timer(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn handle_appbar_callback(hwnd: HWND, wparam: WPARAM) {
    if wparam.0 != appbar::ABN_POSCHANGED {
        return;
    }

    let params = WIN_DATA.with(|d| {
        let b = d.borrow();
        let w = b.as_ref()?;
        if !w.appbar_active {
            return None;
        }
        match w.cur_mode {
            DisplayMode::Docked(e) => Some((e, w.cur_panel_size, w.screen_w, w.screen_h)),
            _ => None,
        }
    });
    let Some((edge, panel_pct, sw, sh)) = params else {
        return;
    };
    let thickness = panel_pct_to_px(panel_pct, match edge { Edge::Top | Edge::Bottom => sh, _ => sw });

    // Borrow is dropped — safe to call SetWindowPos.
    let rect = appbar::reposition(hwnd, edge, thickness, sw, sh);
    move_window(hwnd, rect);
    WIN_DATA.with(|d| {
        if let Some(w) = d.borrow_mut().as_mut() {
            let (rw, rh) = rect_dims(rect);
            w.wgpu.resize(rw, rh);
        }
    });
}

fn on_timer(hwnd: HWND) {
    // ── Snapshot desired state ───────────────────────────────────────────────
    struct Snap {
        enabled: bool,
        mode: DisplayMode,
        panel_size: u32,
        zoom: f32,
        smooth_speed: f32,
        color_filter: ColorFilter,
        interpolation: Interpolation,
        frame: Option<Arc<Frame>>,
        cur_enabled: bool,
        cur_mode: DisplayMode,
        cur_panel_size: u32,
        appbar_active: bool,
        screen_w: i32,
        screen_h: i32,
        monitor_left: i32,
        monitor_top: i32,
        callback_msg: u32,
    }

    let snap = WIN_DATA.with(|d| {
        let b = d.borrow();
        let w = b.as_ref()?;
        let s = w.app_state.read();
        let frame = w.frame_state.lock().ok()?.clone();
        Some(Snap {
            enabled: s.enabled,
            mode: s.display_mode,
            panel_size: s.panel_size,
            zoom: s.zoom,
            smooth_speed: s.smooth_speed,
            color_filter: s.color_filter,
            interpolation: s.interpolation,
            frame,
            cur_enabled: w.cur_enabled,
            cur_mode: w.cur_mode,
            cur_panel_size: w.cur_panel_size,
            appbar_active: w.appbar_active,
            screen_w: w.screen_w,
            screen_h: w.screen_h,
            monitor_left: w.monitor_left,
            monitor_top: w.monitor_top,
            callback_msg: w.callback_msg,
        })
    });
    let Some(snap) = snap else { return };

    let sw = snap.screen_w;
    let sh = snap.screen_h;

    // ── State transitions ────────────────────────────────────────────────────
    let enabled_changed = snap.enabled != snap.cur_enabled;
    let mode_changed = snap.mode != snap.cur_mode;
    let panel_size_changed = snap.panel_size != snap.cur_panel_size;

    let need_transition =
        enabled_changed || (snap.enabled && mode_changed) || (snap.enabled && panel_size_changed);

    let mut new_appbar_active = snap.appbar_active;
    let mut new_rect: Option<RECT> = None;

    if need_transition {
        if enabled_changed && !snap.enabled {
            // ── Disabling ─────────────────────────────────────────────────
            if snap.appbar_active {
                appbar::unregister(hwnd);
                new_appbar_active = false;
            }
            unsafe { let _ = ShowWindow(hwnd, SW_HIDE); }
        } else if enabled_changed && snap.enabled {
            // ── Enabling ──────────────────────────────────────────────────
            if snap.appbar_active {
                appbar::unregister(hwnd);
                new_appbar_active = false;
            }
            match snap.mode {
                DisplayMode::Fullscreen => {
                    new_rect = Some(RECT {
                        left: snap.monitor_left,
                        top: snap.monitor_top,
                        right: snap.monitor_left + sw,
                        bottom: snap.monitor_top + sh,
                    });
                }
                DisplayMode::Docked(e) => {
                    let rect = appbar::register(hwnd, e, panel_pct_to_px(snap.panel_size, match e { Edge::Top | Edge::Bottom => sh, _ => sw }), sw, sh, snap.callback_msg);
                    new_rect = Some(rect);
                    new_appbar_active = true;
                }
            }
        } else if snap.enabled && mode_changed {
            // ── Mode change while enabled ─────────────────────────────────
            if snap.appbar_active {
                appbar::unregister(hwnd);
                new_appbar_active = false;
            }
            match snap.mode {
                DisplayMode::Fullscreen => {
                    new_rect = Some(RECT {
                        left: snap.monitor_left,
                        top: snap.monitor_top,
                        right: snap.monitor_left + sw,
                        bottom: snap.monitor_top + sh,
                    });
                }
                DisplayMode::Docked(e) => {
                    let rect = appbar::register(hwnd, e, panel_pct_to_px(snap.panel_size, match e { Edge::Top | Edge::Bottom => sh, _ => sw }), sw, sh, snap.callback_msg);
                    new_rect = Some(rect);
                    new_appbar_active = true;
                }
            }
        } else if snap.enabled && panel_size_changed {
            // ── Panel size change while docked ────────────────────────────
            if let DisplayMode::Docked(e) = snap.mode {
                let rect = appbar::reposition(hwnd, e, panel_pct_to_px(snap.panel_size, match e { Edge::Top | Edge::Bottom => sh, _ => sw }), sw, sh);
                new_rect = Some(rect);
            }
        }

        // ── move_window outside any WIN_DATA borrow ───────────────────────
        if let Some(rect) = new_rect {
            move_window(hwnd, rect);
        }

        // ── ShowWindow (only on enable/disable) ───────────────────────────
        if enabled_changed && snap.enabled {
            unsafe { let _ = ShowWindow(hwnd, SW_SHOW); }
        }

        // ── Cursor visibility via Magnification API ───────────────────────
        let show_cursor = !(snap.enabled && snap.mode == DisplayMode::Fullscreen);
        let _ = unsafe { MagShowSystemCursor(show_cursor) };

        // ── Write back tracking state + resize wgpu surface ──────────────
        WIN_DATA.with(|d| {
            let mut b = d.borrow_mut();
            let w = b.as_mut().unwrap();
            w.cur_enabled    = snap.enabled;
            w.cur_mode       = snap.mode;
            w.cur_panel_size = snap.panel_size;
            w.appbar_active  = new_appbar_active;
            if let Some(rect) = new_rect {
                let (rw, rh) = rect_dims(rect);
                w.wgpu.resize(rw, rh);
            }
        });
    }

    // ── Render ───────────────────────────────────────────────────────────────
    if !snap.enabled {
        return;
    }

    let mut cursor = windows::Win32::Foundation::POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut cursor);
    }

    // ── Monitor follow: collect switch rect before taking the main borrow ───
    // We compute the move rect here (outside WIN_DATA borrow) so that
    // move_window → SetWindowPos → wnd_proc re-entry cannot deadlock.
    let monitor_move_rect: Option<RECT> = WIN_DATA.with(|d| {
        let mut b = d.borrow_mut();
        let w = b.as_mut().unwrap();

        let target = geometry::output_at(&w.outputs, cursor.x, cursor.y)?;
        if target.idx == w.active_output_idx {
            return None;
        }

        let new_idx = target.idx;
        let nl = target.left;
        let nt = target.top;
        let nw = target.width;
        let nh = target.height;

        w.desired_output.store(new_idx, Ordering::Relaxed);
        w.active_output_idx = new_idx;
        w.monitor_left = nl;
        w.monitor_top = nt;
        w.screen_w = nw as i32;
        w.screen_h = nh as i32;

        w.wgpu.recreate_frame_texture(nw, nh);
        w.last_frame = None; // force re-upload on next frame

        if snap.mode == DisplayMode::Fullscreen {
            let rect = RECT {
                left: nl,
                top: nt,
                right: nl + nw as i32,
                bottom: nt + nh as i32,
            };
            w.wgpu.resize(nw, nh);
            Some(rect)
        } else {
            None
        }
    });

    // SetWindowPos is called here — borrow fully released, no re-entrancy risk.
    if let Some(rect) = monitor_move_rect {
        move_window(hwnd, rect);
    }

    // ── Clip cursor every tick while docked ───────────────────────────────
    update_clip_cursor(matches!(snap.mode, DisplayMode::Docked(_)));

    // ── Main render: lerp, crop, upload, uniforms, present ──────────────────
    WIN_DATA.with(|d| {
        let mut b = d.borrow_mut();
        let w = b.as_mut().unwrap();

        // Frame-rate-independent lerp toward actual cursor.
        let now = Instant::now();
        let dt = now.duration_since(w.last_tick).as_secs_f32();
        w.last_tick = now;
        let alpha = geometry::smooth_alpha(snap.smooth_speed, dt);
        w.smooth_x = geometry::lerp_toward(w.smooth_x, cursor.x as f32, alpha);
        w.smooth_y = geometry::lerp_toward(w.smooth_y, cursor.y as f32, alpha);

        // Convert smoothed cursor to monitor-local coordinates (DXGI frame origin = 0,0).
        let cx = geometry::to_monitor_local(w.smooth_x, w.monitor_left);
        let cy = geometry::to_monitor_local(w.smooth_y, w.monitor_top);

        let cur_sw = w.screen_w;
        let cur_sh = w.screen_h;
        let (win_w, win_h) = window_dims(snap.mode, snap.panel_size, cur_sw, cur_sh);

        let fw = w.wgpu.tex_w as f32;
        let fh = w.wgpu.tex_h as f32;
        let crop_rect = geometry::compute_crop(cx, cy, win_w, win_h, snap.zoom, fw, fh);
        let crop = crop_rect.normalized();

        // Upload frame only when the capture thread has produced a new Arc<Frame>.
        if let Some(frame) = &snap.frame {
            let new_frame = w
                .last_frame
                .as_ref()
                .is_none_or(|last| !Arc::ptr_eq(last, frame));
            if new_frame {
                w.wgpu.upload_frame(&frame.data, frame.width, frame.height);
                w.last_frame = Some(Arc::clone(frame));
            }
        }

        // Cursor position in output window pixel space, accounting for zoom/crop.
        let (cursor_x, cursor_y) = geometry::cursor_in_output(cx, cy, &crop_rect, win_w, win_h);

        // Write uniforms only when crop, settings, or cursor have changed.
        let color_mode = snap.color_filter.as_u32();
        let interp_mode = snap.interpolation.as_u32();
        if crop != w.last_crop
            || color_mode != w.last_color_mode
            || interp_mode != w.last_interp_mode
            || cursor_x != w.last_cursor_x
            || cursor_y != w.last_cursor_y
        {
            w.wgpu
                .write_uniforms(crop, color_mode, interp_mode, cursor_x, cursor_y);
            w.last_crop = crop;
            w.last_color_mode = color_mode;
            w.last_interp_mode = interp_mode;
            w.last_cursor_x = cursor_x;
            w.last_cursor_y = cursor_y;
        }

        if !w.wgpu.render() {
            // Surface lost/outdated — reconfigure to recover.
            let (rw, rh) = window_dims(snap.mode, snap.panel_size, cur_sw, cur_sh);
            w.wgpu.resize(rw, rh);
        }
    });
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn rect_dims(r: RECT) -> (u32, u32) {
    (
        (r.right - r.left).max(16) as u32,
        (r.bottom - r.top).max(16) as u32,
    )
}

/// Clip the cursor to the current work area (docked) or release the clip (fullscreen/disabled).
/// Must be called after appbar registration/repositioning so the work area is already updated.
fn update_clip_cursor(clip: bool) {
    unsafe {
        if clip {
            let mut work = RECT::default();
            SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some(&mut work as *mut RECT as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
            .ok();
            let _ = ClipCursor(Some(&work));
        } else {
            let _ = ClipCursor(None);
        }
    }
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
    appbar::notify_moved(hwnd);
}
