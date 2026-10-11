//! Opt-in performance counters (roadmap 5.5a). Off unless the environment variable `CV_PERF` is
//! `1` when the first counter is touched; then the backend prints a `[perf]` line once a second
//! from `take`. Off, every call is one flag read and no clock read.
//!
//! The counters are statics so no signature on the seam changes. Capture and the render thread
//! record into them; the backend's loop reads and resets them.

use std::{
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
    time::{Duration, Instant},
};

/// True when `CV_PERF=1`. Read once.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("CV_PERF").is_ok_and(|v| v == "1"))
}

/// Count and time of one kind of event over an interval.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Timed {
    pub count: u64,
    pub total: Duration,
    pub max: Duration,
}

impl Timed {
    /// Mean in milliseconds; 0 with no events.
    pub fn avg_ms(&self) -> f64 {
        if self.count == 0 { 0.0 } else { self.total.as_secs_f64() * 1000.0 / self.count as f64 }
    }

    pub fn max_ms(&self) -> f64 {
        self.max.as_secs_f64() * 1000.0
    }
}

/// What happened since the last `take`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Snapshot {
    /// Capture frames that carried a desktop image (`LastPresentTime` non-zero), with the time
    /// to copy each into a `Frame`.
    pub image_frames: Timed,
    /// Capture frames with only a pointer update. They are copied in full all the same
    /// (STATUS Finding 4).
    pub pointer_frames: Timed,
    /// Every call of `Magnifier::tick`, enabled or not.
    pub ticks: Timed,
    /// Frame uploads to the GPU.
    pub uploads: Timed,
    /// Completed renders (presents).
    pub renders: u64,
}

/// A counter of events and their summed and largest duration, in nanoseconds.
struct Stat {
    count: AtomicU64,
    total_ns: AtomicU64,
    max_ns: AtomicU64,
}

impl Stat {
    const fn new() -> Self {
        Self { count: AtomicU64::new(0), total_ns: AtomicU64::new(0), max_ns: AtomicU64::new(0) }
    }

    fn add(&self, d: Duration) {
        let ns = u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
        self.count.fetch_add(1, Relaxed);
        self.total_ns.fetch_add(ns, Relaxed);
        self.max_ns.fetch_max(ns, Relaxed);
    }

    fn take(&self) -> Timed {
        Timed {
            count: self.count.swap(0, Relaxed),
            total: Duration::from_nanos(self.total_ns.swap(0, Relaxed)),
            max: Duration::from_nanos(self.max_ns.swap(0, Relaxed)),
        }
    }
}

static IMAGE_FRAMES: Stat = Stat::new();
static POINTER_FRAMES: Stat = Stat::new();
static TICKS: Stat = Stat::new();
static UPLOADS: Stat = Stat::new();
static RENDERS: AtomicU64 = AtomicU64::new(0);

/// The start of something to time: `None` when counters are off.
pub fn start() -> Option<Instant> {
    enabled().then(Instant::now)
}

/// A capture frame copied into a `Frame`; `image` is false for a pointer-only update.
pub fn captured(start: Option<Instant>, image: bool) {
    if let Some(t) = start {
        let stat = if image { &IMAGE_FRAMES } else { &POINTER_FRAMES };
        stat.add(t.elapsed());
    }
}

/// One `Magnifier::tick` finished.
pub fn ticked(start: Option<Instant>) {
    if let Some(t) = start {
        TICKS.add(t.elapsed());
    }
}

/// One frame uploaded to the GPU.
pub fn uploaded(start: Option<Instant>) {
    if let Some(t) = start {
        UPLOADS.add(t.elapsed());
    }
}

/// One render presented.
pub fn rendered() {
    if enabled() {
        RENDERS.fetch_add(1, Relaxed);
    }
}

/// Returns what was counted since the last call and starts over.
pub fn take() -> Snapshot {
    Snapshot {
        image_frames: IMAGE_FRAMES.take(),
        pointer_frames: POINTER_FRAMES.take(),
        ticks: TICKS.take(),
        uploads: UPLOADS.take(),
        renders: RENDERS.swap(0, Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_counts_sums_and_keeps_the_largest() {
        let s = Stat::new();
        s.add(Duration::from_millis(2));
        s.add(Duration::from_millis(6));
        s.add(Duration::from_millis(4));
        let t = s.take();
        assert_eq!(t.count, 3);
        assert_eq!(t.total, Duration::from_millis(12));
        assert_eq!(t.max, Duration::from_millis(6));
        assert!((t.avg_ms() - 4.0).abs() < 1e-9);
        assert!((t.max_ms() - 6.0).abs() < 1e-9);
    }

    #[test]
    fn take_resets_the_stat() {
        let s = Stat::new();
        s.add(Duration::from_millis(5));
        s.take();
        assert_eq!(s.take(), Timed::default());
    }

    #[test]
    fn an_empty_interval_averages_zero() {
        assert_eq!(Timed::default().avg_ms(), 0.0);
        assert_eq!(Timed::default().max_ms(), 0.0);
    }

    #[test]
    fn counters_do_nothing_without_a_start() {
        // `start` is None when `CV_PERF` is unset, and the recorders then skip the stat. With it
        // set, other tests' ticks would count into the statics, so there is nothing to check.
        if enabled() {
            return;
        }
        captured(None, true);
        ticked(None);
        uploaded(None);
        assert_eq!(IMAGE_FRAMES.take(), Timed::default());
        assert_eq!(TICKS.take(), Timed::default());
        assert_eq!(UPLOADS.take(), Timed::default());
    }
}
