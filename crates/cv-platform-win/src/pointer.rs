use cv_core::{PointerSource, ScreenPoint};
use windows::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetCursorPos};

/// The system pointer, read with `GetCursorPos`. Physical pixels because the process is
/// Per-Monitor v2 DPI aware.
pub struct CursorPointer;

impl PointerSource for CursorPointer {
    /// `None` when `GetCursorPos` fails, which it does when another desktop has input
    /// (lock screen, UAC prompt).
    fn position(&self) -> Option<ScreenPoint> {
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p) }.ok()?;
        Some(ScreenPoint { x: p.x, y: p.y })
    }
}
