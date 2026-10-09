//! Caret probe (roadmap 4.1). A dev tool for measuring caret sources; not shipped.
//!
//! Logs, once a second, the caret rectangle from each source Windows offers for the foreground
//! window: `GetGUIThreadInfo` (gui), the MSAA caret object (msaa) and UI Automation text patterns
//! (uia). Rectangles are physical virtual-screen pixels, like every coordinate in the app. The
//! sources are read 10 times a second and each one's rectangle is outlined on screen: red gui,
//! green msaa, blue uia, each a little further out so overlapping outlines stay visible. 4.2 runs it across the app list and records
//! which sources are right in docs/prototypes/caret-sources.md.
//!
//! Run: `cargo run -p cv-platform-win --example caret_probe` (add `> probe.txt` to keep the
//! output). Ctrl+C stops it and the outlines go with the process; it changes nothing
//! system-wide, so there is nothing to restore.

fn main() {
    #[cfg(windows)]
    probe::run();
}

#[cfg(windows)]
mod probe {
    use super::outline;
    use std::{
        ffi::c_void,
        mem::size_of,
        ptr, thread,
        time::{Duration, Instant},
    };

    use cv_core::ScreenRect;
    use windows::{
        Win32::{
            Foundation::{CloseHandle, HWND, POINT},
            Graphics::Gdi::ClientToScreen,
            System::{
                Com::{
                    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                    SAFEARRAY,
                },
                Ole::{SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound},
                Threading::{
                    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                    QueryFullProcessImageNameW,
                },
                Variant::{VARIANT, VT_I4},
            },
            UI::{
                Accessibility::{
                    AccessibleObjectFromWindow, CUIAutomation8, IAccessible, IUIAutomation,
                    IUIAutomationElement, IUIAutomationTextPattern, IUIAutomationTextPattern2,
                    IUIAutomationTextRange, TextUnit_Character, UIA_TextPattern2Id,
                    UIA_TextPatternId,
                },
                HiDpi::{
                    AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
                    DPI_AWARENESS_PER_MONITOR_AWARE, DPI_AWARENESS_SYSTEM_AWARE,
                    DPI_AWARENESS_UNAWARE, GetAwarenessFromDpiAwarenessContext, GetDpiForWindow,
                    GetWindowDpiAwarenessContext,
                },
                WindowsAndMessaging::{
                    CHILDID_SELF, GUI_CARETBLINKING, GUITHREADINFO, GetClassNameW, GetCursorPos,
                    GetForegroundWindow, GetGUIThreadInfo, GetWindowTextW,
                    GetWindowThreadProcessId, OBJID_CARET,
                },
            },
        },
        core::{BOOL, Error, Interface, PWSTR, Result},
    };

    /// How often the sources are read and the outlines redrawn.
    const SAMPLE_EVERY: Duration = Duration::from_millis(100);
    /// How often a block is printed, so the log stays readable.
    const PRINT_EVERY: Duration = Duration::from_secs(1);

    pub fn run() {
        // Physical pixels everywhere, as in the app (a `[dpi]` line here means they aren't).
        cv_platform_win::set_dpi_awareness();

        // ADR 0003's rules for UIA: MTA first, on a thread that owns no window.
        if let Err(e) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
            eprintln!("[probe] CoInitializeEx failed: {}", describe(&e));
        }
        let uia: Option<IUIAutomation> =
            match unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) } {
                Ok(uia) => Some(uia),
                Err(e) => {
                    eprintln!("[probe] CUIAutomation8 unavailable: {}", describe(&e));
                    None
                }
            };

        // The outline window lives on its own thread: a UIA thread must not own windows
        // (ADR 0003).
        outline::spawn();

        println!(
            "caret probe: one block a second, rects are x,y wxh in physical screen pixels. \
             Outlines: red gui, green msaa, blue uia. Ctrl+C to stop."
        );
        let start = Instant::now();
        let mut last_print: Option<Instant> = None;
        loop {
            let now = Instant::now();
            let print = last_print.is_none_or(|t| now.duration_since(t) >= PRINT_EVERY);
            if print {
                last_print = Some(now);
            }
            sample(uia.as_ref(), start, print);
            thread::sleep(SAMPLE_EVERY.saturating_sub(now.elapsed()));
        }
    }

    /// Reads every source and updates the outlines. With `print`, also logs a block: the
    /// foreground app and pointer, then one line per source with its cost.
    fn sample(uia: Option<&IUIAutomation>, start: Instant, print: bool) {
        let fg = unsafe { GetForegroundWindow() };
        let mut pid = 0;
        let tid = unsafe { GetWindowThreadProcessId(fg, Some(&mut pid)) };
        if print {
            let mut pt = POINT::default();
            let pointer = match unsafe { GetCursorPos(&mut pt) } {
                Ok(()) => format!("{},{}", pt.x, pt.y),
                Err(_) => "?".into(),
            };
            println!(
                "[{:>4}s] fg {} \"{}\" pid {}   pointer {}",
                start.elapsed().as_secs(),
                process_name(pid),
                window_text(fg),
                pid,
                pointer
            );
        }

        let t = Instant::now();
        let info = gui_thread_info(tid);
        let gui = gui_reading(&info);
        let gui_ms = ms_since(t);

        // The caret object belongs to the window with keyboard focus.
        let focus = match &info {
            Ok(info) if !info.hwndFocus.is_invalid() => info.hwndFocus,
            _ => fg,
        };
        let t = Instant::now();
        let msaa = msaa_reading(focus);
        let msaa_ms = ms_since(t);

        let t = Instant::now();
        let uia = uia_reading(uia);
        let uia_ms = ms_since(t);

        outline::show([gui.rect, msaa.rect, uia.rect]);

        if print {
            print_source("gui", &gui, gui_ms);
            print_source("msaa", &msaa, msaa_ms);
            print_source("uia", &uia, uia_ms);
        }
    }

    fn ms_since(started: Instant) -> f64 {
        started.elapsed().as_secs_f64() * 1000.0
    }

    /// What one source reported: the log text and the rectangle to outline, if any.
    struct Reading {
        text: String,
        rect: Option<ScreenRect>,
    }

    impl Reading {
        fn found(rect: ScreenRect, details: String) -> Self {
            Self { text: format!("{}  {details}", fmt_rect(rect)), rect: Some(rect) }
        }

        fn missing(text: impl Into<String>) -> Self {
            Self { text: text.into(), rect: None }
        }
    }

    fn print_source(name: &str, reading: &Reading, ms: f64) {
        println!("  {name:<5} {}   {ms:.1} ms", reading.text);
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

    /// `rcCaret` is in client coordinates of `hwndCaret`, logical for that window's DPI mode
    /// (Microsoft docs), so the window's DPI and awareness are printed with it.
    fn gui_reading(info: &Result<GUITHREADINFO>) -> Reading {
        let info = match info {
            Ok(info) => info,
            Err(e) => return Reading::missing(describe(e)),
        };
        let hwnd = info.hwndCaret;
        if hwnd.is_invalid() {
            return Reading::missing("none (no caret window)");
        }
        let rc = info.rcCaret;
        let mut origin = POINT { x: rc.left, y: rc.top };
        if !unsafe { ClientToScreen(hwnd, &mut origin) }.as_bool() {
            return Reading::missing(format!("error ClientToScreen failed  hwnd {}", hex(hwnd)));
        }
        let rect = ScreenRect {
            left: origin.x,
            top: origin.y,
            width: (rc.right - rc.left).max(0) as u32,
            height: (rc.bottom - rc.top).max(0) as u32,
        };
        let blinking = if info.flags.0 & GUI_CARETBLINKING.0 != 0 {
            "blinking"
        } else {
            "not blinking"
        };
        Reading::found(
            rect,
            format!("hwnd {} {}  {}  {}", hex(hwnd), class_name(hwnd), blinking, dpi_info(hwnd)),
        )
    }

    // ── MSAA caret object ────────────────────────────────────────────────────────

    fn msaa_reading(target: HWND) -> Reading {
        if target.is_invalid() {
            return Reading::missing("none (no focus window)");
        }
        let acc = match caret_object(target) {
            Ok(acc) => acc,
            Err(e) => return Reading::missing(format!("{}  hwnd {}", describe(&e), hex(target))),
        };
        let mut child = VARIANT::default();
        // SAFETY: a VT_I4 VARIANT holding CHILDID_SELF; the union field written matches `vt`.
        unsafe {
            let v = &mut *child.Anonymous.Anonymous;
            v.vt = VT_I4;
            v.Anonymous.lVal = CHILDID_SELF as i32;
        }
        let (mut left, mut top, mut width, mut height) = (0, 0, 0, 0);
        match unsafe { acc.accLocation(&mut left, &mut top, &mut width, &mut height, &child) } {
            Ok(()) if width == 0 && height == 0 => Reading::missing(format!(
                "none (empty location {left},{top})  hwnd {}",
                hex(target)
            )),
            Ok(()) => {
                let rect = ScreenRect {
                    left,
                    top,
                    width: width.max(0) as u32,
                    height: height.max(0) as u32,
                };
                Reading::found(rect, format!("hwnd {}", hex(target)))
            }
            Err(e) => Reading::missing(format!("{}  hwnd {}", describe(&e), hex(target))),
        }
    }

    /// The docs say to pass NULL for the caret object, but that only finds a caret on the
    /// calling thread; out of process, clients pass the focus window.
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

    fn uia_reading(uia: Option<&IUIAutomation>) -> Reading {
        let Some(uia) = uia else {
            return Reading::missing("none (UI Automation unavailable)");
        };
        let element = match unsafe { uia.GetFocusedElement() } {
            Ok(element) => element,
            Err(e) => return Reading::missing(format!("{} (GetFocusedElement)", describe(&e))),
        };
        let about = element_info(&element);
        let (range, how) = match caret_range(&element) {
            Ok(found) => found,
            Err(msg) => return Reading::missing(format!("{msg}  {about}")),
        };
        match range_rect(&range) {
            Ok(Some((rect, count, expanded))) => {
                let mut notes = String::new();
                if expanded {
                    notes.push_str(" (expanded to char)");
                }
                if count > 1 {
                    notes.push_str(&format!(" ({count} rects, first shown)"));
                }
                Reading::found(rect, format!("{how}{notes}  {about}"))
            }
            Ok(None) => Reading::missing(format!(
                "none (no rectangle: off-screen or hidden)  {how}  {about}"
            )),
            Err(e) => Reading::missing(format!(
                "{} (GetBoundingRectangles)  {how}  {about}",
                describe(&e)
            )),
        }
    }

    /// TextPattern2's caret range first; TextPattern's first selection range otherwise.
    fn caret_range(
        element: &IUIAutomationElement,
    ) -> std::result::Result<(IUIAutomationTextRange, String), String> {
        if let Ok(pattern) =
            unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id) }
        {
            let mut active = BOOL(0);
            return match unsafe { pattern.GetCaretRange(&mut active) } {
                Ok(range) => {
                    let state = if active.as_bool() { "active" } else { "inactive" };
                    Ok((range, format!("TextPattern2.GetCaretRange {state}")))
                }
                Err(e) => Err(format!("{} (TextPattern2.GetCaretRange)", describe(&e))),
            };
        }
        if let Ok(pattern) =
            unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
        {
            let ranges = unsafe { pattern.GetSelection() }
                .map_err(|e| format!("{} (TextPattern.GetSelection)", describe(&e)))?;
            let count = unsafe { ranges.Length() }.unwrap_or(0);
            if count == 0 {
                return Err("none (TextPattern, no selection range)".into());
            }
            let range = unsafe { ranges.GetElement(0) }
                .map_err(|e| format!("{} (TextPattern selection range)", describe(&e)))?;
            return Ok((range, "TextPattern.GetSelection[0]".into()));
        }
        Err("none (focused element has no text pattern)".into())
    }

    /// The first bounding rectangle of `range`, how many there were, and whether the range had
    /// to be expanded: a degenerate range has none (Microsoft docs), so measure the character at
    /// the caret instead.
    fn range_rect(range: &IUIAutomationTextRange) -> Result<Option<(ScreenRect, usize, bool)>> {
        let rects = bounding_rects(range)?;
        if let Some(first) = rects.first() {
            return Ok(Some((*first, rects.len(), false)));
        }
        let wide = unsafe { range.Clone() }?;
        unsafe { wide.ExpandToEnclosingUnit(TextUnit_Character) }?;
        let rects = bounding_rects(&wide)?;
        Ok(rects.first().map(|first| (*first, rects.len(), true)))
    }

    /// `GetBoundingRectangles` gives doubles, four per line of text: left, top, width, height.
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

    /// The UIA framework and class of the focused element, which tell app types apart for 4.3.
    fn element_info(element: &IUIAutomationElement) -> String {
        let framework = unsafe { element.CurrentFrameworkId() }
            .map(|s| s.to_string())
            .unwrap_or_else(|_| "?".into());
        let class = unsafe { element.CurrentClassName() }
            .map(|s| s.to_string())
            .unwrap_or_else(|_| "?".into());
        format!("fw {framework:?} class {class:?}")
    }

    // ── Helpers ──────────────────────────────────────────────────────────────────

    fn fmt_rect(r: ScreenRect) -> String {
        format!("{},{} {}x{}", r.left, r.top, r.width, r.height)
    }

    fn hex(hwnd: HWND) -> String {
        format!("{:#x}", hwnd.0 as usize)
    }

    fn describe(e: &Error) -> String {
        format!("error {:#010x} {}", e.code().0, e.message())
    }

    fn dpi_info(hwnd: HWND) -> String {
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let context = unsafe { GetWindowDpiAwarenessContext(hwnd) };
        let awareness = if unsafe {
            AreDpiAwarenessContextsEqual(context, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
        }
        .as_bool()
        {
            "per-monitor-v2"
        } else {
            let awareness = unsafe { GetAwarenessFromDpiAwarenessContext(context) };
            if awareness == DPI_AWARENESS_PER_MONITOR_AWARE {
                "per-monitor"
            } else if awareness == DPI_AWARENESS_SYSTEM_AWARE {
                "system"
            } else if awareness == DPI_AWARENESS_UNAWARE {
                "unaware"
            } else {
                "unknown"
            }
        };
        format!("dpi {dpi} {awareness}")
    }

    fn process_name(pid: u32) -> String {
        let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
        else {
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

    fn window_text(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        let len = unsafe { GetWindowTextW(hwnd, &mut buf) }.max(0) as usize;
        let text = String::from_utf16_lossy(&buf[..len]);
        // Long titles push the useful columns off screen.
        text.chars().take(60).collect()
    }

    fn class_name(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        let len = unsafe { GetClassNameW(hwnd, &mut buf) }.max(0) as usize;
        String::from_utf16_lossy(&buf[..len])
    }
}

/// On-screen outlines of the latest readings: one click-through, topmost window over the whole
/// virtual screen, black made transparent with a colour key, never activated. It runs its own
/// thread and message loop; the probe thread hands it rectangles and posts a redraw when they
/// change.
#[cfg(windows)]
mod outline {
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
                CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetSystemMetrics,
                HTTRANSPARENT, LWA_COLORKEY, MSG, PostMessageW, RegisterClassExW,
                SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
                SW_SHOWNOACTIVATE, SetLayeredWindowAttributes, ShowWindow, TranslateMessage,
                WM_APP, WM_NCHITTEST, WM_PAINT, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
                WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
            },
        },
        core::w,
    };

    /// gui, msaa, uia, in the order `show` takes them. COLORREF is 0x00BBGGRR.
    const COLOURS: [COLORREF; 3] =
        [COLORREF(0x0000_00FF), COLORREF(0x0000_FF00), COLORREF(0x00FF_0000)];
    /// Gap between a rectangle and its outline, larger per source so coinciding rectangles still
    /// show all three outlines.
    const GAPS: [i32; 3] = [2, 5, 8];
    const THICKNESS: i32 = 2;
    const WM_REDRAW: u32 = WM_APP + 1;

    static RECTS: Mutex<[Option<ScreenRect>; 3]> = Mutex::new([None; 3]);
    /// The outline window as an integer, so the probe thread can post to it. 0 until created.
    static WINDOW: AtomicIsize = AtomicIsize::new(0);

    pub fn spawn() {
        thread::spawn(run);
    }

    /// Replaces the outlined rectangles (gui, msaa, uia) and asks the window to redraw, unless
    /// nothing moved: a redraw clears the whole virtual screen.
    pub fn show(rects: [Option<ScreenRect>; 3]) {
        let Ok(mut current) = RECTS.lock() else {
            return;
        };
        if *current == rects {
            return;
        }
        *current = rects;
        drop(current);
        let raw = WINDOW.load(Ordering::SeqCst);
        if raw != 0 {
            let hwnd = HWND(raw as *mut c_void);
            let _ = unsafe { PostMessageW(Some(hwnd), WM_REDRAW, WPARAM(0), LPARAM(0)) };
        }
    }

    fn run() {
        let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
            eprintln!("[probe] no module handle; outlines off");
            return;
        };
        let hinstance: HINSTANCE = module.into();
        let class = w!("clear_view_caret_probe_outline");
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            lpszClassName: class,
            ..Default::default()
        };
        unsafe { RegisterClassExW(&wc) };

        let (left, top, width, height) = virtual_screen();
        let created = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE
                    | WS_EX_TOOLWINDOW,
                class,
                w!("caret probe outlines"),
                WS_POPUP,
                left,
                top,
                width,
                height,
                None,
                None,
                Some(hinstance),
                None,
            )
        };
        let hwnd = match created {
            Ok(hwnd) => hwnd,
            Err(e) => {
                eprintln!("[probe] outline window failed: {e}; outlines off");
                return;
            }
        };
        // Black pixels are see-through; everything drawn in colour shows.
        let _ = unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_COLORKEY) };
        WINDOW.store(hwnd.0 as isize, Ordering::SeqCst);
        let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };

        let mut msg = MSG::default();
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    fn virtual_screen() -> (i32, i32, i32, i32) {
        unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_REDRAW => {
                let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
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

    fn paint(hwnd: HWND) {
        let rects = RECTS.lock().map(|r| *r).unwrap_or([None; 3]);
        let (origin_x, origin_y, _, _) = virtual_screen();
        let mut ps = PAINTSTRUCT::default();
        let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
        // Clear to the colour key, then draw each outline `THICKNESS` frames thick.
        unsafe { FillRect(hdc, &ps.rcPaint, HBRUSH(GetStockObject(BLACK_BRUSH).0)) };
        for ((rect, colour), gap) in rects.iter().zip(COLOURS).zip(GAPS) {
            let Some(r) = rect else {
                continue;
            };
            let brush = unsafe { CreateSolidBrush(colour) };
            for step in 0..THICKNESS {
                let pad = gap + step;
                let frame = RECT {
                    left: r.left - origin_x - pad,
                    top: r.top - origin_y - pad,
                    right: r.left - origin_x + r.width as i32 + pad,
                    bottom: r.top - origin_y + r.height as i32 + pad,
                };
                unsafe { FrameRect(hdc, &frame, brush) };
            }
            let _ = unsafe { DeleteObject(HGDIOBJ(brush.0)) };
        }
        let _ = unsafe { EndPaint(hwnd, &ps) };
    }
}
