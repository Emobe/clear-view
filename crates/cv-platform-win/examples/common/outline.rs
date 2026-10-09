//! On-screen outlines for the dev examples (caret_probe, caret_events): one small click-through,
//! topmost, never-activated window per slot, sized to its outline and moved with it, black made
//! transparent with a colour key. Slots: 0 red (gui), 1 green (msaa), 2 blue (uia), 3 yellow
//! (focus rectangle). Nothing covers the whole monitor: a topmost window that does makes the
//! shell treat the monitor as running a full-screen app and drop the taskbar below other
//! windows. The windows run on their own thread and message loop (a UIA thread must not own
//! windows, ADR 0003); callers hand over rectangles and a relayout is posted when they change.
//!
//! Included with `#[path]` by each example; Cargo does not build `examples/common/` on its own.

use std::{
    ffi::c_void,
    mem::size_of,
    sync::{
        Mutex,
        atomic::{AtomicIsize, Ordering},
    },
    thread,
};

use cv_core::ScreenRect;
use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BLACK_BRUSH, BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect,
            FrameRect, GetStockObject, HBRUSH, HGDIOBJ, InvalidateRect, PAINTSTRUCT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW,
            HTTRANSPARENT, HWND_TOPMOST, LWA_COLORKEY, MSG, PostMessageW, RegisterClassExW,
            SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetWindowPos,
            ShowWindow, TranslateMessage, WM_APP, WM_NCHITTEST, WM_PAINT, WNDCLASSEXW,
            WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
            WS_POPUP,
        },
    },
    core::w,
};

pub const SLOTS: usize = 4;

/// Red, green, blue, yellow, in slot order. COLORREF is 0x00BBGGRR.
const COLOURS: [COLORREF; SLOTS] = [
    COLORREF(0x0000_00FF),
    COLORREF(0x0000_FF00),
    COLORREF(0x00FF_0000),
    COLORREF(0x0000_FFFF),
];
/// Gap between a rectangle and its outline, larger per slot so coinciding rectangles still
/// show every outline.
const GAPS: [i32; SLOTS] = [2, 5, 8, 11];
const THICKNESS: i32 = 2;
const WM_RELAYOUT: u32 = WM_APP + 1;

static RECTS: Mutex<[Option<ScreenRect>; SLOTS]> = Mutex::new([None; SLOTS]);
/// The outline windows as integers, so other threads can post to them. 0 until created.
static WINDOWS: [AtomicIsize; SLOTS] = [const { AtomicIsize::new(0) }; SLOTS];

pub fn spawn() {
    thread::spawn(run);
}

/// Replaces the outlined rectangles, one per slot, and asks the outline thread to move the
/// windows, unless nothing moved.
pub fn show(rects: [Option<ScreenRect>; SLOTS]) {
    let Ok(mut current) = RECTS.lock() else {
        return;
    };
    if *current == rects {
        return;
    }
    *current = rects;
    drop(current);
    // All the windows belong to the outline thread, so any one of them can take the message.
    if let Some(hwnd) = window(0) {
        let _ = unsafe { PostMessageW(Some(hwnd), WM_RELAYOUT, WPARAM(0), LPARAM(0)) };
    }
}

fn window(index: usize) -> Option<HWND> {
    let raw = WINDOWS[index].load(Ordering::SeqCst);
    (raw != 0).then_some(HWND(raw as *mut c_void))
}

fn run() {
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        eprintln!("[outline] no module handle; outlines off");
        return;
    };
    let hinstance: HINSTANCE = module.into();
    let class = w!("clear_view_dev_outline");
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance,
        lpszClassName: class,
        ..Default::default()
    };
    unsafe { RegisterClassExW(&wc) };

    // Created hidden; `relayout` shows, sizes and places each one.
    for slot in &WINDOWS {
        let created = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE
                    | WS_EX_TOOLWINDOW,
                class,
                w!("clear-view dev outline"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(hinstance),
                None,
            )
        };
        let hwnd = match created {
            Ok(hwnd) => hwnd,
            Err(e) => {
                eprintln!("[outline] window failed: {e}; outlines off");
                return;
            }
        };
        // Black pixels are see-through; the outline drawn in colour shows.
        let _ = unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_COLORKEY) };
        slot.store(hwnd.0 as isize, Ordering::SeqCst);
    }

    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_RELAYOUT => {
            relayout();
            LRESULT(0)
        }
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Hides each window with no rectangle and fits the others around theirs: the window is the
/// rectangle grown by its gap plus the outline thickness.
fn relayout() {
    let rects = RECTS.lock().map(|r| *r).unwrap_or([None; SLOTS]);
    for (index, (rect, gap)) in rects.iter().zip(GAPS).enumerate() {
        let Some(hwnd) = window(index) else {
            continue;
        };
        let Some(r) = rect else {
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
            continue;
        };
        let pad = gap + THICKNESS;
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                r.left - pad,
                r.top - pad,
                r.width as i32 + 2 * pad,
                r.height as i32 + 2 * pad,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}

/// Clears to the colour key and draws the window's outline `THICKNESS` frames thick along its
/// edges.
fn paint(hwnd: HWND) {
    let index = WINDOWS.iter().position(|w| w.load(Ordering::SeqCst) == hwnd.0 as isize);
    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    unsafe { FillRect(hdc, &ps.rcPaint, HBRUSH(GetStockObject(BLACK_BRUSH).0)) };
    let mut client = RECT::default();
    if let Some(index) = index
        && unsafe { GetClientRect(hwnd, &mut client) }.is_ok()
    {
        let brush = unsafe { CreateSolidBrush(COLOURS[index]) };
        for step in 0..THICKNESS {
            let frame = RECT {
                left: client.left + step,
                top: client.top + step,
                right: client.right - step,
                bottom: client.bottom - step,
            };
            unsafe { FrameRect(hdc, &frame, brush) };
        }
        let _ = unsafe { DeleteObject(HGDIOBJ(brush.0)) };
    }
    let _ = unsafe { EndPaint(hwnd, &ps) };
}
