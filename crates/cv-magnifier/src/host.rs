//! The overlay window as the magnifier sees it (ADR 0004). The magnifier says what the window
//! should be; the backend decides how (window placement and cursor hiding stay inside it).

use cv_core::{Edge, OutputInfo, PointerSource};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

/// What the overlay window should look like.
#[derive(Debug, Clone, PartialEq)]
pub enum Layout {
    Hidden,
    Fullscreen { monitor: OutputInfo },
    /// `thickness` is the panel size in physical pixels across `edge`.
    Docked { monitor: OutputInfo, edge: Edge, thickness: u32 },
}

/// The window the magnifier draws into. The backend owns it, its thread and its loop, and
/// calls the magnifier on each tick.
pub trait OverlayHost: PointerSource {
    /// Makes the window match `layout`: size, position, visibility and system cursor.
    /// Returns the client size in physical pixels, or (0, 0) for
    /// `Layout::Hidden`.
    ///
    /// Leaves the system cursor hidden for `Layout::Fullscreen` and shown for every other
    /// layout, whatever `set_system_cursor` asked for before.
    ///
    /// May send window messages synchronously, so the backend must not call into the
    /// magnifier from inside it.
    fn apply_layout(&mut self, layout: &Layout) -> (u32, u32);

    /// Shows or hides the system cursor while docked (ADR 0007: hidden while the pointer is
    /// inside the panel). The magnifier calls it only when the answer changes.
    fn set_system_cursor(&mut self, visible: bool);

    /// Handles for the wgpu surface. Valid until the host is dropped.
    fn raw_handles(&self) -> (RawDisplayHandle, RawWindowHandle);
}
