//! Pointer probe (roadmap 6.2a, ADR 0010). A dev tool for measuring what Windows reports about
//! the mouse pointer while `MagShowSystemCursor(false)` hides it; not shipped.
//!
//! Three phases, each `--secs` long (default 20): the system cursor shown, hidden, shown again.
//! In every phase two sources are logged:
//!
//! - `GetCursorInfo` (user32), read every 50 ms on the main thread: `flags` (`CURSOR_SHOWING`,
//!   `CURSOR_SUPPRESSED` or 0 for hidden), the cursor handle (named when it is one of the
//!   standard shared cursors) and the screen position. When the handle changes, `GetIconInfo`
//!   and `GetDIBits` read its shape: size, hotspot, monochrome or colour, and how many pixels are
//!   drawn by each candidate rule (6.2c needs to know which rule colour cursors without an alpha
//!   channel follow).
//! - Desktop Duplication on one output (`--output`, default 0), on its own thread: every frame
//!   with a mouse update (visibility, position, and the new shape when there is one). No image is
//!   copied.
//!
//! Each phase ends with a summary line; all three are printed again together at the end. Visit the
//! cases in each phase: arrow, over text (Notepad), over a link (browser), at a window edge
//! (resize), and a busy cursor (start an app). Keep the pointer on the duplicated output.
//!
//! Run (with clear-view closed, it hides the cursor too):
//! `cargo run -p cv-platform-win --example pointer_probe > probe.txt`
//! Ctrl+C or closing the console shows the cursor again; Windows restores it if the process is
//! killed, as with the app (STATUS 1.1). It changes nothing else system-wide.

fn main() {
    #[cfg(windows)]
    probe::run();
}

#[cfg(windows)]
mod probe {
    use std::{
        collections::BTreeSet,
        ffi::c_void,
        mem::size_of,
        process, ptr,
        sync::{
            Mutex, OnceLock,
            atomic::{AtomicBool, AtomicU8, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };

    use windows::{
        Win32::{
            Foundation::{HMODULE, POINT},
            Graphics::{
                Direct3D::D3D_DRIVER_TYPE_HARDWARE,
                Direct3D11::{D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device},
                Dxgi::{
                    DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
                    DXGI_OUTDUPL_POINTER_SHAPE_INFO, DXGI_OUTDUPL_POINTER_SHAPE_TYPE_COLOR,
                    DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MASKED_COLOR,
                    DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MONOCHROME, IDXGIDevice, IDXGIOutput1,
                    IDXGIOutputDuplication,
                },
                Gdi::{
                    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject,
                    GetDC, GetDIBits, GetObjectW, HBITMAP, HDC, HGDIOBJ, ReleaseDC,
                },
            },
            System::Console::SetConsoleCtrlHandler,
            UI::{
                Magnification::{MagInitialize, MagShowSystemCursor, MagUninitialize},
                WindowsAndMessaging::{
                    CURSOR_SHOWING, CURSOR_SUPPRESSED, CURSORINFO, GetCursorInfo, GetCursorPos,
                    GetIconInfo, HCURSOR, HICON, ICONINFO, IDC_APPSTARTING, IDC_ARROW, IDC_CROSS,
                    IDC_HAND, IDC_HELP, IDC_IBEAM, IDC_NO, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS,
                    IDC_SIZENWSE, IDC_SIZEWE, IDC_UPARROW, IDC_WAIT, LoadCursorW,
                },
            },
        },
        core::{BOOL, Error, Interface, PCWSTR},
    };

    /// How often `GetCursorInfo` is read.
    const SAMPLE_EVERY: Duration = Duration::from_millis(50);
    /// How often the heartbeat line (current flags, cursor and position) is printed.
    const HEARTBEAT: Duration = Duration::from_secs(1);
    /// Position lines from Desktop Duplication are printed at most this often.
    const DXGI_POSITION_EVERY: Duration = Duration::from_millis(500);
    /// Cursor-change lines printed per phase; an animated cursor must not flood the log.
    const CHANGE_LINES_PER_PHASE: u32 = 40;

    const PHASE_NAMES: [&str; 3] = ["shown-1", "hidden", "shown-2"];

    static START: OnceLock<Instant> = OnceLock::new();
    /// Index into `PHASE_NAMES`; 3 once the run is over.
    static PHASE: AtomicU8 = AtomicU8::new(0);
    static STOP: AtomicBool = AtomicBool::new(false);
    static STATS: Mutex<[PhaseStats; 3]> =
        Mutex::new([PhaseStats::new(), PhaseStats::new(), PhaseStats::new()]);

    /// What one phase saw, printed as the phase summary.
    struct PhaseStats {
        flags: BTreeSet<u32>,
        cursors: BTreeSet<String>,
        handle_changes: u32,
        dxgi_frames: u32,
        dxgi_mouse_updates: u32,
        dxgi_visible: BTreeSet<bool>,
        dxgi_shapes: BTreeSet<String>,
    }

    impl PhaseStats {
        const fn new() -> Self {
            Self {
                flags: BTreeSet::new(),
                cursors: BTreeSet::new(),
                handle_changes: 0,
                dxgi_frames: 0,
                dxgi_mouse_updates: 0,
                dxgi_visible: BTreeSet::new(),
                dxgi_shapes: BTreeSet::new(),
            }
        }

        fn summary(&self) -> String {
            format!(
                "GetCursorInfo flags seen {:?}; cursor handle changes {}; cursors seen {:?}; \
                 DXGI frames {}, mouse updates {}, Visible seen {:?}, shapes received {:?}",
                self.flags.iter().map(|f| flags_text(*f)).collect::<Vec<_>>(),
                self.handle_changes,
                self.cursors,
                self.dxgi_frames,
                self.dxgi_mouse_updates,
                self.dxgi_visible,
                self.dxgi_shapes,
            )
        }
    }

    pub fn run() {
        // Physical pixels everywhere, as in the app.
        cv_platform_win::set_dpi_awareness();
        let (secs, output) = parse_args();
        START.get_or_init(Instant::now);

        // Cursor hiding is the only Magnification API use, as in the app. The guard shows the
        // cursor again on any exit from `run`, and the console handler on Ctrl+C or a console
        // close (which end the process without unwinding).
        let _ = unsafe { MagInitialize() };
        let _guard = CursorGuard;
        if let Err(e) = unsafe { SetConsoleCtrlHandler(Some(console_handler), true) } {
            eprintln!("[probe] SetConsoleCtrlHandler failed: {}", describe(&e));
        }

        let dxgi = thread::spawn(move || duplication_thread(output));

        println!(
            "pointer probe: three phases of {secs} s: cursor shown, hidden (MagShowSystemCursor(false)), \
             shown again. DXGI output {output}. Visit in each phase: arrow, text (Notepad), a link, \
             a window edge (resize), a busy cursor (start an app). Ctrl+C to stop; the cursor comes back."
        );

        let screen_dc = unsafe { GetDC(None) };
        let mut watch = CursorWatch::new();
        for phase in 0..3u8 {
            PHASE.store(phase, Ordering::SeqCst);
            let hide = phase == 1;
            let ok = unsafe { MagShowSystemCursor(!hide) }.as_bool();
            log(format!(
                "=== phase {}: system cursor {} (MagShowSystemCursor returned {ok}) ===",
                PHASE_NAMES[phase as usize],
                if hide { "HIDDEN" } else { "shown" },
            ));
            watch.start_phase();
            let end = Instant::now() + Duration::from_secs(secs);
            while Instant::now() < end {
                watch.sample(phase as usize, screen_dc);
                thread::sleep(SAMPLE_EVERY);
            }
            log(format!(
                "--- summary {}: {}",
                PHASE_NAMES[phase as usize],
                STATS.lock().unwrap()[phase as usize].summary()
            ));
        }

        PHASE.store(3, Ordering::SeqCst);
        STOP.store(true, Ordering::SeqCst);
        let _ = dxgi.join();
        unsafe { ReleaseDC(None, screen_dc) };

        println!("=== comparison ===");
        let stats = STATS.lock().unwrap();
        for (name, phase) in PHASE_NAMES.iter().zip(stats.iter()) {
            println!("{name}: {}", phase.summary());
        }
    }

    fn parse_args() -> (u64, u32) {
        let (mut secs, mut output) = (20u64, 0u32);
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let value = args.next().and_then(|v| v.parse::<u64>().ok());
            match (arg.as_str(), value) {
                ("--secs", Some(v)) if v > 0 => secs = v,
                ("--output", Some(v)) => output = v as u32,
                _ => {
                    eprintln!("usage: pointer_probe [--secs N] [--output N]");
                    process::exit(2);
                }
            }
        }
        (secs, output)
    }

    /// Shows the system cursor and releases the Magnification API when dropped.
    struct CursorGuard;

    impl Drop for CursorGuard {
        fn drop(&mut self) {
            let _ = unsafe { MagShowSystemCursor(true) };
            let _ = unsafe { MagUninitialize() };
        }
    }

    /// Ctrl+C, Ctrl+Break and console close: show the cursor, then let the default handler end
    /// the process (returning FALSE).
    unsafe extern "system" fn console_handler(_ctrl_type: u32) -> BOOL {
        let _ = unsafe { MagShowSystemCursor(true) };
        BOOL(0)
    }

    fn log(message: String) {
        let t = START.get().map_or(0.0, |s| s.elapsed().as_secs_f64());
        let phase = PHASE_NAMES.get(PHASE.load(Ordering::SeqCst) as usize).copied().unwrap_or("end");
        println!("[{t:7.2}s {phase:>7}] {message}");
    }

    fn describe(e: &Error) -> String {
        format!("error {:#010x} {}", e.code().0, e.message())
    }

    fn flags_text(flags: u32) -> String {
        let name = match flags {
            0 => "hidden",
            f if f == CURSOR_SHOWING.0 => "SHOWING",
            f if f == CURSOR_SUPPRESSED.0 => "SUPPRESSED",
            f if f == CURSOR_SHOWING.0 | CURSOR_SUPPRESSED.0 => "SHOWING|SUPPRESSED",
            _ => "unknown",
        };
        format!("{flags} ({name})")
    }

    // ---- GetCursorInfo ----------------------------------------------------------------------

    struct CursorWatch {
        /// The standard shared cursors, to name a handle.
        names: Vec<(isize, &'static str)>,
        last_flags: Option<u32>,
        last_handle: isize,
        last_heartbeat: Option<Instant>,
        failed: bool,
        change_lines: u32,
    }

    impl CursorWatch {
        fn new() -> Self {
            let standard: [(PCWSTR, &'static str); 14] = [
                (IDC_ARROW, "arrow"),
                (IDC_IBEAM, "ibeam"),
                (IDC_WAIT, "wait"),
                (IDC_CROSS, "cross"),
                (IDC_UPARROW, "uparrow"),
                (IDC_SIZENWSE, "size-nwse"),
                (IDC_SIZENESW, "size-nesw"),
                (IDC_SIZEWE, "size-we"),
                (IDC_SIZENS, "size-ns"),
                (IDC_SIZEALL, "size-all"),
                (IDC_NO, "no"),
                (IDC_HAND, "hand"),
                (IDC_APPSTARTING, "app-starting"),
                (IDC_HELP, "help"),
            ];
            let names = standard
                .iter()
                .filter_map(|(id, name)| {
                    unsafe { LoadCursorW(None, *id) }.ok().map(|h| (h.0 as isize, *name))
                })
                .collect();
            Self {
                names,
                last_flags: None,
                last_handle: 0,
                last_heartbeat: None,
                failed: false,
                change_lines: 0,
            }
        }

        /// Forget what the last phase saw, so the first sample of a phase prints its state.
        fn start_phase(&mut self) {
            self.last_flags = None;
            self.last_handle = -1;
            self.last_heartbeat = None;
            self.change_lines = 0;
        }

        fn label(&self, handle: isize) -> String {
            if handle == 0 {
                return "none".to_string();
            }
            match self.names.iter().find(|(h, _)| *h == handle) {
                Some((_, name)) => (*name).to_string(),
                None => format!("unlabelled {handle:#x}"),
            }
        }

        fn sample(&mut self, phase: usize, dc: HDC) {
            let mut info = CURSORINFO { cbSize: size_of::<CURSORINFO>() as u32, ..Default::default() };
            if let Err(e) = unsafe { GetCursorInfo(&mut info) } {
                if !self.failed {
                    log(format!("GetCursorInfo failed: {}", describe(&e)));
                }
                self.failed = true;
                return;
            }
            self.failed = false;

            let flags = info.flags.0;
            let handle = info.hCursor.0 as isize;
            let label = self.label(handle);
            let pos = (info.ptScreenPos.x, info.ptScreenPos.y);

            {
                let mut stats = STATS.lock().unwrap();
                let stats = &mut stats[phase];
                stats.flags.insert(flags);
                stats.cursors.insert(label.clone());
                if self.last_handle != handle && self.last_handle != -1 {
                    stats.handle_changes += 1;
                }
            }

            if self.last_flags != Some(flags) {
                log(format!("GetCursorInfo flags {} cursor {label} pos {pos:?}", flags_text(flags)));
                self.last_flags = Some(flags);
            }
            if self.last_handle != handle {
                if self.change_lines < CHANGE_LINES_PER_PHASE {
                    self.change_lines += 1;
                    let shape = if handle == 0 { "no handle".to_string() } else { describe_shape(info.hCursor, dc) };
                    log(format!("cursor now {label} ({handle:#x}): {shape}"));
                } else if self.change_lines == CHANGE_LINES_PER_PHASE {
                    self.change_lines += 1;
                    log("(more cursor changes this phase, not printed; the summary counts them)".to_string());
                }
                self.last_handle = handle;
            }
            let now = Instant::now();
            if self.last_heartbeat.is_none_or(|t| now.duration_since(t) >= HEARTBEAT) {
                self.last_heartbeat = Some(now);
                log(format!("now: flags {} cursor {label} pos {pos:?}", flags_text(flags)));
            }
        }
    }

    /// Owns the two bitmaps `GetIconInfo` hands out and deletes them: "the calling application
    /// must manage these bitmaps and delete them with DeleteObject".
    struct IconBitmaps(ICONINFO);

    impl Drop for IconBitmaps {
        fn drop(&mut self) {
            for bitmap in [self.0.hbmMask, self.0.hbmColor] {
                if !bitmap.is_invalid() {
                    let _ = unsafe { DeleteObject(HGDIOBJ(bitmap.0)) };
                }
            }
        }
    }

    /// One line on a cursor's shape from `GetIconInfo` and `GetDIBits`. Pixel counts use 32 bpp
    /// copies of the bitmaps: a 1 bpp bit set reads as white.
    fn describe_shape(cursor: HCURSOR, dc: HDC) -> String {
        let mut raw = ICONINFO::default();
        if let Err(e) = unsafe { GetIconInfo(HICON(cursor.0), &mut raw) } {
            return format!("GetIconInfo failed: {}", describe(&e));
        }
        let bitmaps = IconBitmaps(raw);
        let (hot_x, hot_y) = (raw.xHotspot, raw.yHotspot);
        let Some((mask_w, mask_h)) = bitmap_size(raw.hbmMask) else {
            return "GetObjectW failed on the mask bitmap".to_string();
        };
        let Some(mask) = bitmap_bgra(dc, raw.hbmMask, mask_w, mask_h) else {
            return "GetDIBits failed on the mask bitmap".to_string();
        };

        if raw.hbmColor.is_invalid() {
            // Monochrome: the AND mask on top, the XOR mask below, so the cursor is half as tall.
            let h = mask_h / 2;
            let (mut drawn, mut opaque, mut inverting) = (0u32, 0u32, 0u32);
            for y in 0..h {
                for x in 0..mask_w {
                    let and = mask[((y * mask_w + x) * 4) as usize] != 0;
                    let xor = mask[(((y + h) * mask_w + x) * 4) as usize] != 0;
                    match (and, xor) {
                        (false, _) => opaque += 1,
                        (true, true) => inverting += 1,
                        (true, false) => {}
                    }
                    if !(and && !xor) {
                        drawn += 1;
                    }
                }
            }
            drop(bitmaps);
            return format!(
                "monochrome {mask_w}x{h}, hotspot ({hot_x}, {hot_y}), drawn {drawn} \
                 (AND=0 {opaque}, inverting {inverting})"
            );
        }

        let Some((w, h)) = bitmap_size(raw.hbmColor) else {
            return "GetObjectW failed on the colour bitmap".to_string();
        };
        let Some(color) = bitmap_bgra(dc, raw.hbmColor, w, h) else {
            return "GetDIBits failed on the colour bitmap".to_string();
        };
        let alpha_set = color.iter().skip(3).step_by(4).filter(|a| **a > 0).count();
        // Mask bit 0 = the pixel is drawn (AND mask clear). Only compared when the sizes agree.
        let mask_clear = if mask_w == w && mask_h == h {
            mask.iter().step_by(4).filter(|b| **b == 0).count().to_string()
        } else {
            format!("n/a (mask {mask_w}x{mask_h})")
        };
        drop(bitmaps);
        format!(
            "colour {w}x{h}, hotspot ({hot_x}, {hot_y}), alpha>0 {alpha_set}, mask AND=0 {mask_clear}, \
             of {} pixels",
            w * h
        )
    }

    fn bitmap_size(bitmap: HBITMAP) -> Option<(u32, u32)> {
        let mut bm = BITMAP::default();
        let got = unsafe {
            GetObjectW(
                HGDIOBJ(bitmap.0),
                size_of::<BITMAP>() as i32,
                Some(ptr::from_mut(&mut bm).cast::<c_void>()),
            )
        };
        (got != 0 && bm.bmWidth > 0 && bm.bmHeight > 0).then_some((bm.bmWidth as u32, bm.bmHeight as u32))
    }

    /// The bitmap as top-down 32 bpp BGRA bytes, or `None` if `GetDIBits` copied nothing.
    fn bitmap_bgra(dc: HDC, bitmap: HBITMAP, w: u32, h: u32) -> Option<Vec<u8>> {
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bytes = vec![0u8; (w * h * 4) as usize];
        let lines = unsafe {
            GetDIBits(dc, bitmap, 0, h, Some(bytes.as_mut_ptr().cast::<c_void>()), &mut info, DIB_RGB_COLORS)
        };
        (lines != 0).then_some(bytes)
    }

    // ---- Desktop Duplication ----------------------------------------------------------------

    fn duplication_thread(output: u32) {
        let mut state = None;
        let mut last_visible: Option<bool> = None;
        let mut last_position: Option<Instant> = None;
        while !STOP.load(Ordering::SeqCst) {
            let Some((dup, left, top)) = state.as_ref() else {
                match create_duplication(output) {
                    Ok((dup, rect)) => {
                        log(format!(
                            "DXGI output {output} duplicated, desktop rect left {} top {} ({}x{})",
                            rect.0, rect.1, rect.2, rect.3
                        ));
                        state = Some((dup, rect.0, rect.1));
                    }
                    Err(e) => {
                        log(format!("DXGI duplication of output {output} failed: {}", describe(&e)));
                        thread::sleep(Duration::from_millis(500));
                    }
                }
                continue;
            };

            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource = None;
            match unsafe { dup.AcquireNextFrame(100, &mut info, &mut resource) } {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => continue,
                Err(e) => {
                    if e.code() == DXGI_ERROR_ACCESS_LOST {
                        log("DXGI access lost; recreating the duplication".to_string());
                    } else {
                        log(format!("DXGI AcquireNextFrame failed: {}", describe(&e)));
                    }
                    state = None;
                    thread::sleep(Duration::from_millis(250));
                    continue;
                }
            }
            drop(resource);

            let phase = PHASE.load(Ordering::SeqCst) as usize;
            let mouse_update = info.LastMouseUpdateTime != 0;
            let visible = info.PointerPosition.Visible.as_bool();
            let shape = (mouse_update && info.PointerShapeBufferSize > 0)
                .then(|| read_shape(dup, info.PointerShapeBufferSize));

            if let Some(stats) = STATS.lock().unwrap().get_mut(phase) {
                stats.dxgi_frames += 1;
                if mouse_update {
                    stats.dxgi_mouse_updates += 1;
                    stats.dxgi_visible.insert(visible);
                }
                if let Some(Ok(text)) = &shape {
                    stats.dxgi_shapes.insert(text.clone());
                }
            }

            if mouse_update {
                let now = Instant::now();
                let due = last_position.is_none_or(|t| now.duration_since(t) >= DXGI_POSITION_EVERY);
                if last_visible != Some(visible) || due {
                    last_visible = Some(visible);
                    last_position = Some(now);
                    let p = info.PointerPosition.Position;
                    let mut real = POINT::default();
                    let real = unsafe { GetCursorPos(&mut real) }.map_or((0, 0), |()| (real.x, real.y));
                    log(format!(
                        "DXGI mouse update: Visible {visible}, top-left of shape ({}, {}) on the output, \
                         ({}, {}) on the desktop; GetCursorPos {real:?}",
                        p.x,
                        p.y,
                        p.x + left,
                        p.y + top
                    ));
                }
            }
            match shape {
                Some(Ok(text)) => log(format!("DXGI pointer shape: {text}")),
                Some(Err(text)) => log(format!("DXGI pointer shape: {text}")),
                None => {}
            }

            let _ = unsafe { dup.ReleaseFrame() };
        }
    }

    /// Reads the new pointer shape; the text names its type, size, pitch and hotspot.
    fn read_shape(dup: &IDXGIOutputDuplication, size: u32) -> Result<String, String> {
        let mut buffer = vec![0u8; size as usize];
        let mut required = 0u32;
        let mut info = DXGI_OUTDUPL_POINTER_SHAPE_INFO::default();
        unsafe { dup.GetFramePointerShape(size, buffer.as_mut_ptr().cast::<c_void>(), &mut required, &mut info) }
            .map_err(|e| format!("GetFramePointerShape failed: {}", describe(&e)))?;
        let kind = match info.Type as i32 {
            t if t == DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MONOCHROME.0 => "monochrome",
            t if t == DXGI_OUTDUPL_POINTER_SHAPE_TYPE_COLOR.0 => "colour",
            t if t == DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MASKED_COLOR.0 => "masked-colour",
            _ => "unknown type",
        };
        // A monochrome shape's height is twice the cursor's (AND mask, then XOR mask).
        Ok(format!(
            "{kind} {}x{} (pitch {}, hotspot ({}, {}), {required} bytes)",
            info.Width, info.Height, info.Pitch, info.HotSpot.x, info.HotSpot.y
        ))
    }

    /// Duplicates output `output` of the default adapter; also gives its desktop rect as
    /// (left, top, width, height).
    fn create_duplication(output: u32) -> windows::core::Result<(IDXGIOutputDuplication, (i32, i32, i32, i32))> {
        let mut device: Option<ID3D11Device> = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                Default::default(),
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )?;
        }
        let device = device.expect("D3D11CreateDevice returned no device");
        let dxgi: IDXGIDevice = device.cast()?;
        let adapter = unsafe { dxgi.GetAdapter()? };
        let out = unsafe { adapter.EnumOutputs(output)? };
        let rect = unsafe { out.GetDesc()? }.DesktopCoordinates;
        let out1: IDXGIOutput1 = out.cast()?;
        let dup = unsafe { out1.DuplicateOutput(&device)? };
        Ok((dup, (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top)))
    }
}
