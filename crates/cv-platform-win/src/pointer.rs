use std::{mem::size_of, sync::Arc};

use cv_core::{PointerLook, PointerSource, ScreenPoint};
use windows::{
    Win32::{
        Foundation::POINT,
        UI::WindowsAndMessaging::{
            CURSOR_SHOWING, CURSOR_SUPPRESSED, CURSORINFO, GetCursorInfo, GetCursorPos, HCURSOR,
            IDC_APPSTARTING, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_HELP, IDC_IBEAM, IDC_NO,
            IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, IDC_UPARROW,
            IDC_WAIT, LoadCursorW,
        },
    },
    core::PCWSTR,
};

use crate::pointer_shape::read_shape;

/// What the last `[pointer]` line said, so the next line prints only on a change.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Announced {
    Failed,
    Hidden,
    NoHandle,
    Handle(isize),
}

/// The system pointer: its position with `GetCursorPos` (physical pixels, because the process
/// is Per-Monitor v2 DPI aware) and its shape with `GetCursorInfo` + `GetIconInfo` (ADR 0010).
/// Lives on the render thread.
pub struct CursorPointer {
    /// The shared standard cursors by handle, to name them in the log.
    names: Vec<(isize, &'static str)>,
    /// The last handle converted, what it came to and the log text for it. A failed conversion
    /// is kept too, so it neither retries nor logs every tick.
    cached: Option<(isize, PointerLook, String)>,
    announced: Option<Announced>,
}

impl CursorPointer {
    pub fn new() -> Self {
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
        Self { names, cached: None, announced: None }
    }

    /// Prints `text` as a `[pointer]` line unless it is the line already printed for `what`.
    fn announce(&mut self, what: Announced, text: impl FnOnce() -> String) {
        if self.announced != Some(what) {
            eprintln!("[pointer] {}", text());
            self.announced = Some(what);
        }
    }

    fn label(&self, handle: isize) -> String {
        match self.names.iter().find(|(h, _)| *h == handle) {
            Some((_, name)) => (*name).to_string(),
            None => format!("custom {handle:#x}"),
        }
    }

    /// Converts a cursor the first time its handle is seen.
    fn convert(&self, handle: isize) -> (PointerLook, String) {
        let label = self.label(handle);
        match read_shape(HCURSOR(handle as *mut _)) {
            Ok((shape, kind)) => {
                let drawn = shape.mask.iter().filter(|b| **b != 0).count();
                let text = format!(
                    "{label} ({handle:#x}): {kind} {}x{}, hotspot ({}, {}), drawn {drawn} of {}",
                    shape.width,
                    shape.height,
                    shape.hotspot.0,
                    shape.hotspot.1,
                    shape.mask.len()
                );
                (PointerLook::Shape(Arc::new(shape)), text)
            }
            Err(reason) => (PointerLook::Unknown, format!("unknown: {label} ({handle:#x}): {reason}")),
        }
    }
}

impl Default for CursorPointer {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether `GetCursorInfo` flags say a pointer is on screen: `CURSOR_SHOWING` set and
/// `CURSOR_SUPPRESSED` (touch or pen input) clear. Flags 0 is hidden.
fn is_showing(flags: u32) -> bool {
    flags & CURSOR_SHOWING.0 != 0 && flags & CURSOR_SUPPRESSED.0 == 0
}

impl PointerSource for CursorPointer {
    /// `None` when `GetCursorPos` fails, which it does when another desktop has input
    /// (lock screen, UAC prompt).
    fn position(&self) -> Option<ScreenPoint> {
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p) }.ok()?;
        Some(ScreenPoint { x: p.x, y: p.y })
    }

    /// Reads the shape through `GetCursorInfo`, which still answers while
    /// `MagShowSystemCursor(false)` hides the cursor (roadmap 6.2b). The bitmaps are read only
    /// when `hCursor` changes; until then the same `Arc` comes back.
    fn look(&mut self) -> PointerLook {
        let mut info = CURSORINFO { cbSize: size_of::<CURSORINFO>() as u32, ..Default::default() };
        if let Err(e) = unsafe { GetCursorInfo(&mut info) } {
            self.announce(Announced::Failed, || format!("unknown: GetCursorInfo failed: {e}"));
            return PointerLook::Unknown;
        }
        if !is_showing(info.flags.0) {
            self.announce(Announced::Hidden, || "hidden".to_string());
            return PointerLook::Hidden;
        }
        let handle = info.hCursor.0 as isize;
        if handle == 0 {
            self.announce(Announced::NoHandle, || {
                "unknown: the system shows a pointer with no handle".to_string()
            });
            return PointerLook::Unknown;
        }

        if !matches!(&self.cached, Some((h, _, _)) if *h == handle) {
            let (look, text) = self.convert(handle);
            self.cached = Some((handle, look, text));
        }
        let Some((_, look, text)) = &self.cached else {
            return PointerLook::Unknown;
        };
        let look = look.clone();
        if self.announced != Some(Announced::Handle(handle)) {
            let text = text.clone();
            self.announce(Announced::Handle(handle), || text);
        }
        look
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_showing_flag_means_a_pointer_is_on_screen() {
        assert!(!is_showing(0));
        assert!(is_showing(CURSOR_SHOWING.0));
        assert!(!is_showing(CURSOR_SUPPRESSED.0));
        assert!(!is_showing(CURSOR_SHOWING.0 | CURSOR_SUPPRESSED.0));
    }
}
