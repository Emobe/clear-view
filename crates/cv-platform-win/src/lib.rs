//! Windows backend (ADR 0004): DXGI capture, the pointer, DPI awareness, global hotkeys and the
//! overlay window (its thread and loop, AppBar docking, cursor clip and hiding, clean exit).
//! Empty on other targets, so a `--workspace` build elsewhere still resolves.
#![cfg(windows)]

mod appbar;
mod capture;
mod dpi;
mod hotkey;
mod overlay;
mod pointer;
mod proto;

pub use capture::{Capturer, enumerate_outputs};
pub use dpi::set_dpi_awareness;
pub use hotkey::{TOGGLE_LABEL, spawn_hotkey_thread};
pub use overlay::{OverlayHandle, spawn_overlay};
