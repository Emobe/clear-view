//! The overlay window as the magnifier sees it (ADR 0004). The magnifier says what the window
//! should be; the backend decides how (AppBar, cursor clip and cursor hiding stay inside it).
//! Declared in 2.4; the backend implements it when the per-tick logic moves here in 2.5.

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
    /// Makes the window match `layout`: size, position, visibility, system cursor and any
    /// work-area reservation. Returns the client size in physical pixels.
    ///
    /// May send window messages synchronously, so the backend must not call into the
    /// magnifier from inside it.
    fn apply_layout(&mut self, layout: &Layout) -> (u32, u32);

    /// Handles for the wgpu surface. Valid until the host is dropped.
    fn raw_handles(&self) -> (RawDisplayHandle, RawWindowHandle);
}
