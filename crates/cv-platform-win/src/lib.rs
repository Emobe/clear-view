//! Windows backend (ADR 0004): DXGI capture, DPI awareness and global hotkeys.
//! Empty on other targets, so a `--workspace` build elsewhere still resolves.
#![cfg(windows)]

mod capture;
mod dpi;
mod hotkey;

pub use capture::{Capturer, enumerate_outputs};
pub use dpi::set_dpi_awareness;
pub use hotkey::{TOGGLE_LABEL, spawn_hotkey_thread};
