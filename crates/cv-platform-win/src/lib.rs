//! Windows backend (ADR 0004): DXGI capture, the pointer, DPI awareness, global hotkeys, the
//! startup system log, the overlay window (its thread and loop, fullscreen and docked
//! placement, cursor hiding, clean exit) and the caret thread (ADR 0008).
//! Empty on other targets, so a `--workspace` build elsewhere still resolves.
#![cfg(windows)]

mod capture;
mod caret;
mod dpi;
mod hotkey;
mod overlay;
mod pointer;
mod sysinfo;

pub use capture::{Capturer, enumerate_outputs};
pub use caret::{CaretHandle, spawn_caret_source};
pub use dpi::set_dpi_awareness;
pub use hotkey::{TOGGLE_LABEL, spawn_hotkey_thread};
pub use overlay::{OverlayHandle, spawn_overlay};
pub use sysinfo::log_system_info;
