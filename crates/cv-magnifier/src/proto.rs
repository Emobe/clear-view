//! Roadmap 3.1 prototype: switches and counters for the docked-overlay measurements. Lives only
//! on the throwaway branch step/3.1-overlay-prototype and is never merged.

use std::sync::atomic::AtomicU32;

/// Presents since the backend last read the counter.
pub static PRESENTS: AtomicU32 = AtomicU32::new(0);
/// Ticks that skipped the present because nothing changed (`CV_PROTO_IDLE_SKIP=1`).
pub static SKIPPED: AtomicU32 = AtomicU32::new(0);

/// True when the environment variable `name` is set to `1`.
pub fn flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1")
}
