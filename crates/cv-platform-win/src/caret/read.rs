//! One read of the caret source chain for the foreground window (ADR 0008): msaa, then uia,
//! then gui, then the focus rectangle. The first non-empty rectangle wins. Every rectangle
//! leaves here in physical virtual-screen pixels. The source code started as the 4.1 probe's
//! (examples/caret_probe.rs).

use std::{ffi::c_void, mem::size_of, ptr};

use cv_core::{CaretSource, ScreenRect};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HWND, POINT, RECT},
        Graphics::Gdi::ClientToScreen,
        System::{
            Com::SAFEARRAY,
            Ole::{SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound},
            Threading::{
                OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                QueryFullProcessImageNameW,
            },
            Variant::{VARIANT, VT_I4},
        },
        UI::{
            Accessibility::{
                AccessibleObjectFromWindow, IAccessible, IUIAutomation, IUIAutomationElement,
                IUIAutomationTextPattern, IUIAutomationTextPattern2, IUIAutomationTextRange,
                TextUnit_Character, UIA_TextPattern2Id, UIA_TextPatternId,
            },
            HiDpi::{
                GetWindowDpiAwarenessContext, LogicalToPhysicalPointForPerMonitorDPI,
                SetThreadDpiAwarenessContext,
            },
            WindowsAndMessaging::{
                CHILDID_SELF, GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetWindowRect,
                GetWindowThreadProcessId, OBJID_CARET,
            },
        },
    },
    core::{BOOL, Error, Interface, PWSTR, Result},
};

use super::tracker::Found;

/// The foreground window and the process and thread that own it.
pub(super) struct Foreground {
    pub hwnd: HWND,
    pub pid: u32,
    pub tid: u32,
}

/// `None` while no window is in the foreground, for example during a switch.
pub(super) fn foreground() -> Option<Foreground> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    let mut pid = 0;
    let tid = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    (tid != 0).then_some(Foreground { hwnd, pid, tid })
}

/// Walks the chain for `fg`. `uia` is `None` when UI Automation could not be created; uia and
/// the element half of the focus rectangle are then skipped.
pub(super) fn read_chain(uia: Option<&IUIAutomation>, fg: &Foreground) -> Option<Found> {
    let info = gui_thread_info(fg.tid).ok();
    // The caret object belongs to the window with keyboard focus.
    let focus = info
        .as_ref()
        .map(|i| i.hwndFocus)
        .filter(|h| !h.is_invalid())
        .unwrap_or(fg.hwnd);

    if let Some(rect) = msaa_rect(focus) {
        return Some((rect, CaretSource::Msaa));
    }

    let mut text_element = None;
    if let Some(uia) = uia {
        match uia_caret(uia) {
            Uia::Rect(rect) => return Some((rect, CaretSource::Uia)),
            Uia::NoRect(element) => text_element = Some(element),
            Uia::NoText => {}
        }
    }

    if let Some(rect) = info.as_ref().and_then(gui_rect) {
        return Some((rect, CaretSource::Gui));
    }

    // The focus rectangle stands in only for a control that has a caret but gave no position
    // for it: a text element with no caret rectangle (the Explorer XAML boxes), or a caret
    // window with an empty caret rectangle. Any other focused control is focus tracking, which
    // is not in v1.
    if let Some(element) = text_element
        && let Ok(rc) = unsafe { element.CurrentBoundingRectangle() }
        && let Some(rect) = from_rect(rc)
    {
        return Some((rect, CaretSource::FocusRect));
    }
    if let Some(info) = &info
        && !info.hwndCaret.is_invalid()
        && !info.hwndFocus.is_invalid()
    {
        let mut rc = RECT::default();
        // Physical pixels: this process is Per-Monitor v2.
        if unsafe { GetWindowRect(info.hwndFocus, &mut rc) }.is_ok()
            && let Some(rect) = from_rect(rc)
        {
            return Some((rect, CaretSource::FocusRect));
        }
    }
    None
}

/// A screen `RECT` as a `ScreenRect`; `None` when it has no area at all.
fn from_rect(rc: RECT) -> Option<ScreenRect> {
    let width = (rc.right - rc.left).max(0) as u32;
    let height = (rc.bottom - rc.top).max(0) as u32;
    (width > 0 || height > 0).then_some(ScreenRect { left: rc.left, top: rc.top, width, height })
}

// ── GetGUIThreadInfo ─────────────────────────────────────────────────────────

fn gui_thread_info(tid: u32) -> Result<GUITHREADINFO> {
    let mut info = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(tid, &mut info) }?;
    Ok(info)
}

/// `rcCaret` is in client coordinates of `hwndCaret`, logical for that window's DPI awareness
/// (Microsoft docs). `ClientToScreen` runs in the window's own DPI context, so it gives that
/// window's logical screen coordinates, and `LogicalToPhysicalPointForPerMonitorDPI` turns both
/// corners into physical pixels. For a Per-Monitor aware window both steps change nothing.
fn gui_rect(info: &GUITHREADINFO) -> Option<ScreenRect> {
    let hwnd = info.hwndCaret;
    if hwnd.is_invalid() {
        return None;
    }
    let rc = info.rcCaret;
    let mut corners = [POINT { x: rc.left, y: rc.top }, POINT { x: rc.right, y: rc.bottom }];

    let previous = unsafe { SetThreadDpiAwarenessContext(GetWindowDpiAwarenessContext(hwnd)) };
    let mapped = corners
        .iter_mut()
        .all(|p| unsafe { ClientToScreen(hwnd, p) }.as_bool());
    if !previous.is_invalid() {
        unsafe { SetThreadDpiAwarenessContext(previous) };
    }
    if !mapped {
        return None;
    }
    for p in &mut corners {
        // On failure the point stays logical, which is right at 100% and for aware windows.
        let _ = unsafe { LogicalToPhysicalPointForPerMonitorDPI(Some(hwnd), p) };
    }
    from_rect(RECT {
        left: corners[0].x,
        top: corners[0].y,
        right: corners[1].x,
        bottom: corners[1].y,
    })
}

// ── MSAA caret object ────────────────────────────────────────────────────────

fn msaa_rect(target: HWND) -> Option<ScreenRect> {
    if target.is_invalid() {
        return None;
    }
    let acc = caret_object(target).ok()?;
    let mut child = VARIANT::default();
    // SAFETY: a VT_I4 VARIANT holding CHILDID_SELF; the union field written matches `vt`.
    unsafe {
        let v = &mut *child.Anonymous.Anonymous;
        v.vt = VT_I4;
        v.Anonymous.lVal = CHILDID_SELF as i32;
    }
    let (mut left, mut top, mut width, mut height) = (0, 0, 0, 0);
    unsafe { acc.accLocation(&mut left, &mut top, &mut width, &mut height, &child) }.ok()?;
    from_rect(RECT { left, top, right: left + width, bottom: top + height })
}

/// The docs say to pass NULL for the caret object, but that only finds a caret on the calling
/// thread; out of process, clients pass the focus window.
fn caret_object(hwnd: HWND) -> Result<IAccessible> {
    let mut raw: *mut c_void = ptr::null_mut();
    unsafe { AccessibleObjectFromWindow(hwnd, OBJID_CARET.0 as u32, &IAccessible::IID, &mut raw) }?;
    if raw.is_null() {
        return Err(Error::empty());
    }
    // SAFETY: on success `raw` is an owned IAccessible pointer.
    Ok(unsafe { IAccessible::from_raw(raw) })
}

// ── UI Automation ────────────────────────────────────────────────────────────

enum Uia {
    /// The caret's rectangle.
    Rect(ScreenRect),
    /// The focused element has a text pattern but gave no caret rectangle.
    NoRect(IUIAutomationElement),
    /// No focused element, or it has no text pattern.
    NoText,
}

fn uia_caret(uia: &IUIAutomation) -> Uia {
    let Ok(element) = (unsafe { uia.GetFocusedElement() }) else {
        return Uia::NoText;
    };
    match caret_range(&element) {
        None => Uia::NoText,
        Some(Err(())) => Uia::NoRect(element),
        Some(Ok(range)) => match range_rect(&range) {
            Some(rect) => Uia::Rect(rect),
            None => Uia::NoRect(element),
        },
    }
}

/// TextPattern2's caret range first; TextPattern's first selection range otherwise. `None`
/// when the element has neither pattern, `Some(Err)` when it has one but gave no range.
fn caret_range(element: &IUIAutomationElement) -> Option<std::result::Result<IUIAutomationTextRange, ()>> {
    if let Ok(pattern) =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id) }
    {
        let mut active = BOOL(0);
        return Some(unsafe { pattern.GetCaretRange(&mut active) }.map_err(|_| ()));
    }
    if let Ok(pattern) =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
    {
        let first = unsafe { pattern.GetSelection() }
            .ok()
            .filter(|ranges| unsafe { ranges.Length() }.unwrap_or(0) > 0)
            .and_then(|ranges| unsafe { ranges.GetElement(0) }.ok());
        return Some(first.ok_or(()));
    }
    None
}

/// The first bounding rectangle of `range`. A degenerate range has none (Microsoft docs), so
/// the character at the caret is measured instead.
fn range_rect(range: &IUIAutomationTextRange) -> Option<ScreenRect> {
    if let Some(first) = bounding_rects(range).ok()?.first() {
        return Some(*first);
    }
    let wide = unsafe { range.Clone() }.ok()?;
    unsafe { wide.ExpandToEnclosingUnit(TextUnit_Character) }.ok()?;
    bounding_rects(&wide).ok()?.first().copied()
}

/// `GetBoundingRectangles` gives doubles, four per line of text: left, top, width, height.
/// Empty rectangles are dropped.
fn bounding_rects(range: &IUIAutomationTextRange) -> Result<Vec<ScreenRect>> {
    let array = unsafe { range.GetBoundingRectangles() }?;
    if array.is_null() {
        return Ok(Vec::new());
    }
    let values = read_doubles(array);
    let _ = unsafe { SafeArrayDestroy(array) };
    Ok(values?
        .as_chunks::<4>()
        .0
        .iter()
        .map(|r| ScreenRect {
            left: r[0].round() as i32,
            top: r[1].round() as i32,
            width: r[2].max(0.0).round() as u32,
            height: r[3].max(0.0).round() as u32,
        })
        .filter(|r| r.width > 0 || r.height > 0)
        .collect())
}

fn read_doubles(array: *const SAFEARRAY) -> Result<Vec<f64>> {
    let lower = unsafe { SafeArrayGetLBound(array, 1) }?;
    let upper = unsafe { SafeArrayGetUBound(array, 1) }?;
    let mut values = Vec::new();
    for i in lower..=upper {
        let mut value = 0f64;
        unsafe { SafeArrayGetElement(array, &i, &mut value as *mut f64 as *mut c_void) }?;
        values.push(value);
    }
    Ok(values)
}

// ── Process name ─────────────────────────────────────────────────────────────

/// The executable's file name, for example "notepad.exe"; "?" when the process can't be
/// queried.
pub(super) fn exe_name(pid: u32) -> String {
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return "?".into();
    };
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let queried = unsafe {
        QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
    };
    let _ = unsafe { CloseHandle(process) };
    match queried {
        Ok(()) => {
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            path.rsplit('\\').next().unwrap_or(&path).to_string()
        }
        Err(_) => "?".into(),
    }
}
