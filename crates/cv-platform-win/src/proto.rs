//! Roadmap 3.1 prototype: shared state and helpers for the docked-overlay measurements. Lives
//! only on the throwaway branch step/3.1-overlay-prototype and is never merged.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use windows::Win32::{
    Foundation::{FILETIME, RECT},
    System::Threading::{GetCurrentProcess, GetProcessTimes},
};

/// The overlay window's rect in virtual-screen pixels, written by the render thread and read
/// by the capture thread to classify dirty rects. Four separate atomics: a torn read during a
/// layout change costs one misclassified frame, which is fine for a measurement.
static VISIBLE: AtomicBool = AtomicBool::new(false);
static LEFT: AtomicI32 = AtomicI32::new(0);
static TOP: AtomicI32 = AtomicI32::new(0);
static RIGHT: AtomicI32 = AtomicI32::new(0);
static BOTTOM: AtomicI32 = AtomicI32::new(0);

pub fn set_overlay(rect: Option<RECT>) {
    if let Some(r) = rect {
        LEFT.store(r.left, Ordering::Relaxed);
        TOP.store(r.top, Ordering::Relaxed);
        RIGHT.store(r.right, Ordering::Relaxed);
        BOTTOM.store(r.bottom, Ordering::Relaxed);
    }
    VISIBLE.store(rect.is_some(), Ordering::Relaxed);
}

pub fn overlay() -> Option<RECT> {
    VISIBLE.load(Ordering::Relaxed).then(|| RECT {
        left: LEFT.load(Ordering::Relaxed),
        top: TOP.load(Ordering::Relaxed),
        right: RIGHT.load(Ordering::Relaxed),
        bottom: BOTTOM.load(Ordering::Relaxed),
    })
}

/// Kernel plus user CPU time of the whole process, in 100 ns units.
pub fn process_cpu_100ns() -> u64 {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let ok = unsafe {
        GetProcessTimes(GetCurrentProcess(), &mut creation, &mut exit, &mut kernel, &mut user)
    };
    if ok.is_err() {
        return 0;
    }
    let as_u64 = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    as_u64(kernel) + as_u64(user)
}

pub fn contains(outer: &RECT, inner: &RECT) -> bool {
    inner.left >= outer.left
        && inner.top >= outer.top
        && inner.right <= outer.right
        && inner.bottom <= outer.bottom
}

pub fn intersects(a: &RECT, b: &RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}
