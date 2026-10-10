//! The once-a-second `[perf]` line (roadmap 5.5a), printed only when `CV_PERF=1`. The counters
//! are cv-magnifier's (`cv_magnifier::perf`); this adds the process CPU time, which needs
//! `GetProcessTimes`, and prints.
//!
//! CPU is the whole process as a percentage of one core, so it can pass 100 (as in the 3.1
//! prototype's tables). GPU use is not measured here; `scripts/perf-sample.ps1` reads it from
//! the system's GPU Engine counters.

use std::time::{Duration, Instant};

use cv_magnifier::perf::{self, Snapshot};
use windows::Win32::{
    Foundation::FILETIME,
    System::Threading::{GetCurrentProcess, GetProcessTimes},
};

/// How often a line is printed.
const INTERVAL: Duration = Duration::from_secs(1);

pub(crate) struct PerfReporter {
    last: Instant,
    last_cpu: u64,
}

impl PerfReporter {
    /// `None` unless `CV_PERF=1`.
    pub(crate) fn new(now: Instant) -> Option<Self> {
        if !perf::enabled() {
            return None;
        }
        eprintln!("[perf] on (CV_PERF=1): one line a second; cpu is % of one core, rates per second");
        // Start the interval clean: discard what was counted before the loop began.
        perf::take();
        Some(Self { last: now, last_cpu: process_cpu_100ns().unwrap_or(0) })
    }

    /// Prints a line if a second has passed since the last one.
    pub(crate) fn poll(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.last);
        if elapsed < INTERVAL {
            return;
        }
        let cpu = process_cpu_100ns();
        let cpu_pct = cpu.map(|c| cpu_percent(c.saturating_sub(self.last_cpu), elapsed));
        eprintln!("{}", format_line(&perf::take(), elapsed, cpu_pct));
        self.last = now;
        if let Some(c) = cpu {
            self.last_cpu = c;
        }
    }
}

/// Kernel plus user time of the whole process, in 100 ns units.
fn process_cpu_100ns() -> Option<u64> {
    let (mut created, mut exited, mut kernel, mut user) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe {
        GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user)
            .ok()?;
    }
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    Some(ticks(kernel) + ticks(user))
}

/// CPU time used in `cpu_100ns` over `elapsed`, as a percentage of one core.
fn cpu_percent(cpu_100ns: u64, elapsed: Duration) -> f64 {
    let secs = elapsed.as_secs_f64();
    if secs <= 0.0 {
        return 0.0;
    }
    cpu_100ns as f64 / 1e7 / secs * 100.0
}

fn format_line(s: &Snapshot, elapsed: Duration, cpu_pct: Option<f64>) -> String {
    let secs = elapsed.as_secs_f64().max(f64::EPSILON);
    let rate = |n: u64| n as f64 / secs;
    let cpu = cpu_pct.map_or("?".to_string(), |c| format!("{c:.0}%"));
    format!(
        "[perf] cpu {cpu} | capture image {:.0}/s ptr-only {:.0}/s copy avg {:.1} ms max {:.1} | \
         tick {:.0}/s avg {:.1} ms max {:.1} | upload {:.0}/s avg {:.1} ms | render {:.0}/s",
        rate(s.image_frames.count),
        rate(s.pointer_frames.count),
        combined_avg_ms(s),
        s.image_frames.max_ms().max(s.pointer_frames.max_ms()),
        rate(s.ticks.count),
        s.ticks.avg_ms(),
        s.ticks.max_ms(),
        rate(s.uploads.count),
        s.uploads.avg_ms(),
        rate(s.renders),
    )
}

/// Mean copy time over image and pointer-only frames together.
fn combined_avg_ms(s: &Snapshot) -> f64 {
    let count = s.image_frames.count + s.pointer_frames.count;
    if count == 0 {
        return 0.0;
    }
    (s.image_frames.total + s.pointer_frames.total).as_secs_f64() * 1000.0 / count as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_magnifier::perf::Timed;

    #[test]
    fn cpu_percent_is_a_share_of_one_core() {
        // 0.5 s of CPU in 1 s is 50%; 1.5 s of CPU in 1 s (several threads) is 150%.
        assert!((cpu_percent(5_000_000, Duration::from_secs(1)) - 50.0).abs() < 1e-9);
        assert!((cpu_percent(15_000_000, Duration::from_secs(1)) - 150.0).abs() < 1e-9);
        assert_eq!(cpu_percent(1, Duration::ZERO), 0.0);
    }

    #[test]
    fn the_line_gives_rates_per_second_and_times_in_ms() {
        let ms = Duration::from_millis;
        let s = Snapshot {
            image_frames: Timed { count: 30, total: ms(120), max: ms(6) },
            pointer_frames: Timed { count: 10, total: ms(40), max: ms(9) },
            ticks: Timed { count: 60, total: ms(120), max: ms(4) },
            uploads: Timed { count: 30, total: ms(60), max: ms(3) },
            renders: 60,
        };
        // Over 2 s: 15/s images, 5/s pointer-only, copy (120 + 40) / 40 = 4.0 ms, max 9.
        let line = format_line(&s, Duration::from_secs(2), Some(37.4));
        assert_eq!(
            line,
            "[perf] cpu 37% | capture image 15/s ptr-only 5/s copy avg 4.0 ms max 9.0 | \
             tick 30/s avg 2.0 ms max 4.0 | upload 15/s avg 2.0 ms | render 30/s"
        );
    }

    #[test]
    fn an_unreadable_cpu_time_prints_a_question_mark() {
        let line = format_line(&Snapshot::default(), Duration::from_secs(1), None);
        assert!(line.starts_with("[perf] cpu ? |"));
    }
}
