//! The capture loop (ADR 0004). The backend supplies a `CaptureSource`; the loop here publishes
//! its frames to the shared slot and follows the monitor the render thread asks for.

use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use cv_core::{Frame, FrameState};

/// How long one `next_frame` call waits for a new frame before the loop checks for a monitor
/// switch again.
const FRAME_TIMEOUT_MS: u32 = 100;

/// How long the thread waits between attempts to make or rebuild a source that failed, for
/// example while the lock screen or a UAC prompt is up (STATUS Finding 16).
const RECONNECT_RETRY: Duration = Duration::from_millis(250);

/// Why a capture call failed. Each variant keeps the platform's message for the log.
#[derive(Debug, Clone, PartialEq)]
pub enum CaptureError {
    /// The duplication is no longer valid (desktop switch, mode change, full-screen app).
    AccessLost(String),
    /// The graphics device was removed or reset.
    DeviceLost(String),
    Other(String),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccessLost(m) => write!(f, "access lost: {m}"),
            Self::DeviceLost(m) => write!(f, "device lost: {m}"),
            Self::Other(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// One monitor's desktop image, as CPU frames (ADR 0001).
pub trait CaptureSource {
    /// Waits up to `timeout_ms` for a new frame. `Ok(None)` means no new frame in time.
    fn next_frame(&mut self, timeout_ms: u32) -> Result<Option<Frame>, CaptureError>;
    /// Captures output `idx` from now on. On error the source keeps its current output.
    fn switch_output(&mut self, idx: u32) -> Result<(), CaptureError>;
    /// Rebuilds the source for its current output after an error.
    fn reconnect(&mut self) -> Result<(), CaptureError>;
}

/// Starts the capture thread. `factory` runs on that thread with the output to start on, so
/// the source and any OS objects in it never cross threads. The thread is meant to be left
/// running and never ends on its own: a factory or reconnect that fails is retried every
/// `RECONNECT_RETRY` until capture is available again.
pub fn spawn_capture<S, F>(
    factory: F,
    frame_state: FrameState,
    desired_output: Arc<AtomicU32>,
) -> JoinHandle<()>
where
    S: CaptureSource,
    F: FnMut(u32) -> Result<S, CaptureError> + Send + 'static,
{
    spawn_capture_with(
        factory,
        frame_state,
        desired_output,
        Arc::new(AtomicBool::new(false)),
        RECONNECT_RETRY,
    )
}

/// `spawn_capture` with the stop flag and retry delay the tests need to end the thread.
fn spawn_capture_with<S, F>(
    mut factory: F,
    frame_state: FrameState,
    desired_output: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    retry: Duration,
) -> JoinHandle<()>
where
    S: CaptureSource,
    F: FnMut(u32) -> Result<S, CaptureError> + Send + 'static,
{
    std::thread::spawn(move || {
        if let Some((mut source, start)) = make_source(&mut factory, &desired_output, &stop, retry)
        {
            run_capture(&mut source, start, &frame_state, &desired_output, &stop, retry);
        }
    })
}

/// Calls `factory` for the wanted output until it succeeds. `None` only if `stop` is set.
fn make_source<S, F>(
    factory: &mut F,
    desired_output: &AtomicU32,
    stop: &AtomicBool,
    retry: Duration,
) -> Option<(S, u32)>
where
    F: FnMut(u32) -> Result<S, CaptureError>,
{
    let mut failed = 0u32;
    while !stop.load(Ordering::Relaxed) {
        let start = desired_output.load(Ordering::Relaxed);
        match factory(start) {
            Ok(source) => {
                if failed > 0 {
                    eprintln!("[capture] started after {} attempts", failed + 1);
                }
                return Some((source, start));
            }
            Err(e) => {
                if failed == 0 {
                    eprintln!("[capture] init failed: {e} — retrying every {} ms", retry.as_millis());
                }
                failed += 1;
                std::thread::sleep(retry);
            }
        }
    }
    None
}

/// The loop itself. A failed frame call puts it in an outage: each pass then tries `reconnect`
/// and sleeps `retry` if that fails, without reading frames, until a reconnect succeeds. A
/// monitor switch asked for during the outage is made on the first pass after it. Returns only
/// when `stop` is set.
fn run_capture<S: CaptureSource>(
    source: &mut S,
    mut current: u32,
    frame_state: &FrameState,
    desired_output: &AtomicU32,
    stop: &AtomicBool,
    retry: Duration,
) {
    // Failed reconnects in the current outage; `None` while capture works.
    let mut failed_reconnects: Option<u32> = None;

    while !stop.load(Ordering::Relaxed) {
        if let Some(failed) = failed_reconnects {
            match source.reconnect() {
                Ok(()) => {
                    if failed > 0 {
                        eprintln!("[capture] reconnected after {} attempts", failed + 1);
                    }
                    failed_reconnects = None;
                }
                Err(e) => {
                    // Logged once per outage, not on every retry.
                    if failed == 0 {
                        eprintln!(
                            "[capture] reconnect failed: {e} — retrying every {} ms",
                            retry.as_millis()
                        );
                    }
                    failed_reconnects = Some(failed + 1);
                    std::thread::sleep(retry);
                    continue;
                }
            }
        }

        // Switch output if the render thread requested a different monitor.
        let wanted = desired_output.load(Ordering::Relaxed);
        if wanted != current {
            match source.switch_output(wanted) {
                Ok(()) => current = wanted,
                Err(e) => eprintln!("[capture] switch_output({wanted}) failed: {e}"),
            }
        }

        match source.next_frame(FRAME_TIMEOUT_MS) {
            Ok(Some(frame)) => {
                *frame_state.lock().unwrap() = Some(Arc::new(frame));
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("[capture] {e} — reconnecting");
                failed_reconnects = Some(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        sync::{Mutex, mpsc},
    };

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Call {
        Next,
        Switch(u32),
        Reconnect,
    }

    /// A scripted source. When `frames` runs out, `next_frame` sets `stop`, which ends the loop.
    /// `switches` and `reconnects` succeed once their scripts run out.
    struct Fake {
        frames: VecDeque<Result<Option<Frame>, CaptureError>>,
        switches: VecDeque<bool>,
        reconnects: VecDeque<bool>,
        /// Runs on every `reconnect` call, before its result is decided.
        on_reconnect: Option<Box<dyn FnMut()>>,
        stop: Arc<AtomicBool>,
        calls: Arc<Mutex<Vec<Call>>>,
    }

    impl Fake {
        fn new(frames: Vec<Result<Option<Frame>, CaptureError>>) -> Self {
            Self {
                frames: frames.into(),
                switches: VecDeque::new(),
                reconnects: VecDeque::new(),
                on_reconnect: None,
                stop: Arc::new(AtomicBool::new(false)),
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl CaptureSource for Fake {
        fn next_frame(&mut self, timeout_ms: u32) -> Result<Option<Frame>, CaptureError> {
            assert_eq!(timeout_ms, FRAME_TIMEOUT_MS);
            self.calls.lock().unwrap().push(Call::Next);
            self.frames.pop_front().unwrap_or_else(|| {
                self.stop.store(true, Ordering::Relaxed);
                Ok(None)
            })
        }

        fn switch_output(&mut self, idx: u32) -> Result<(), CaptureError> {
            self.calls.lock().unwrap().push(Call::Switch(idx));
            if self.switches.pop_front().unwrap_or(true) {
                Ok(())
            } else {
                Err(CaptureError::Other("switch refused".into()))
            }
        }

        fn reconnect(&mut self) -> Result<(), CaptureError> {
            self.calls.lock().unwrap().push(Call::Reconnect);
            if let Some(hook) = &mut self.on_reconnect {
                hook();
            }
            if self.reconnects.pop_front().unwrap_or(true) {
                Ok(())
            } else {
                Err(CaptureError::DeviceLost("reconnect refused".into()))
            }
        }
    }

    /// A frame told apart from others by its width.
    fn frame(marker: u32) -> Result<Option<Frame>, CaptureError> {
        Ok(Some(Frame { width: marker, height: 1, data: Vec::new() }))
    }

    fn slot() -> FrameState {
        Arc::new(Mutex::new(None))
    }

    fn slot_marker(slot: &FrameState) -> Option<u32> {
        slot.lock().unwrap().as_ref().map(|f| f.width)
    }

    /// Runs the loop with no retry delay until the fake's script ends.
    fn run(fake: &mut Fake, current: u32, slot: &FrameState, desired_output: &AtomicU32) {
        let stop = fake.stop.clone();
        run_capture(fake, current, slot, desired_output, &stop, Duration::ZERO);
    }

    fn lost() -> Result<Option<Frame>, CaptureError> {
        Err(CaptureError::AccessLost("gone".into()))
    }

    #[test]
    fn frames_reach_the_slot_newest_last() {
        let mut fake = Fake::new(vec![frame(1), frame(2)]);
        let s = slot();
        run(&mut fake, 0, &s, &AtomicU32::new(0));
        assert_eq!(slot_marker(&s), Some(2));
        assert_eq!(fake.calls(), [Call::Next, Call::Next, Call::Next]);
    }

    #[test]
    fn timeout_leaves_the_slot_alone() {
        let mut fake = Fake::new(vec![Ok(None)]);
        let s = slot();
        *s.lock().unwrap() = Some(Arc::new(Frame { width: 7, height: 1, data: Vec::new() }));
        run(&mut fake, 0, &s, &AtomicU32::new(0));
        assert_eq!(slot_marker(&s), Some(7));
    }

    #[test]
    fn requested_output_is_switched_to_once() {
        let mut fake = Fake::new(vec![Ok(None), Ok(None)]);
        run(&mut fake, 0, &slot(), &AtomicU32::new(1));
        assert_eq!(fake.calls(), [Call::Switch(1), Call::Next, Call::Next, Call::Next]);
    }

    #[test]
    fn same_output_is_not_switched() {
        let mut fake = Fake::new(vec![Ok(None)]);
        run(&mut fake, 2, &slot(), &AtomicU32::new(2));
        assert!(!fake.calls().iter().any(|c| matches!(c, Call::Switch(_))));
    }

    #[test]
    fn failed_switch_is_retried_on_the_next_pass() {
        let mut fake = Fake::new(vec![Ok(None), Ok(None)]);
        fake.switches = [false, true].into();
        run(&mut fake, 0, &slot(), &AtomicU32::new(1));
        assert_eq!(
            fake.calls(),
            [Call::Switch(1), Call::Next, Call::Switch(1), Call::Next, Call::Next]
        );
    }

    #[test]
    fn error_reconnects_and_carries_on() {
        let mut fake = Fake::new(vec![lost(), frame(3)]);
        let s = slot();
        run(&mut fake, 0, &s, &AtomicU32::new(0));
        assert_eq!(slot_marker(&s), Some(3));
        assert_eq!(fake.calls(), [Call::Next, Call::Reconnect, Call::Next, Call::Next]);
    }

    #[test]
    fn failed_reconnect_is_retried_then_capture_resumes() {
        let mut fake = Fake::new(vec![lost(), frame(4)]);
        fake.reconnects = [false, false, true].into();
        let s = slot();
        run(&mut fake, 0, &s, &AtomicU32::new(0));
        assert_eq!(slot_marker(&s), Some(4));
        // No frame is asked for while the reconnect keeps failing.
        assert_eq!(
            fake.calls(),
            [
                Call::Next,
                Call::Reconnect,
                Call::Reconnect,
                Call::Reconnect,
                Call::Next,
                Call::Next
            ]
        );
    }

    #[test]
    fn switch_asked_for_during_an_outage_is_made_once_capture_is_back() {
        let desired = Arc::new(AtomicU32::new(0));
        let mut fake = Fake::new(vec![lost(), Ok(None)]);
        fake.reconnects = [false, true].into();
        let hook_desired = desired.clone();
        fake.on_reconnect = Some(Box::new(move || hook_desired.store(1, Ordering::Relaxed)));
        run(&mut fake, 0, &slot(), &desired);
        assert_eq!(
            fake.calls(),
            [
                Call::Next,
                Call::Reconnect,
                Call::Reconnect,
                Call::Switch(1),
                Call::Next,
                Call::Next
            ]
        );
    }

    #[test]
    fn spawned_thread_starts_on_the_requested_output_and_publishes() {
        let (tx, rx) = mpsc::channel();
        let s = slot();
        let stop = Arc::new(AtomicBool::new(false));
        let fake_stop = stop.clone();
        let handle = spawn_capture_with(
            move |idx| {
                tx.send(idx).unwrap();
                let mut fake = Fake::new(vec![frame(5)]);
                fake.stop = fake_stop.clone();
                Ok(fake)
            },
            s.clone(),
            Arc::new(AtomicU32::new(2)),
            stop,
            Duration::ZERO,
        );
        handle.join().unwrap();
        assert_eq!(rx.recv().unwrap(), 2);
        assert_eq!(slot_marker(&s), Some(5));
    }

    #[test]
    fn failed_factory_is_retried_then_frames_publish() {
        let s = slot();
        let stop = Arc::new(AtomicBool::new(false));
        let fake_stop = stop.clone();
        let attempts = Arc::new(AtomicU32::new(0));
        let counted = attempts.clone();
        let handle = spawn_capture_with(
            move |_| {
                if counted.fetch_add(1, Ordering::Relaxed) < 2 {
                    return Err(CaptureError::Other("locked".into()));
                }
                let mut fake = Fake::new(vec![frame(6)]);
                fake.stop = fake_stop.clone();
                Ok(fake)
            },
            s.clone(),
            Arc::new(AtomicU32::new(0)),
            stop,
            Duration::ZERO,
        );
        handle.join().unwrap();
        assert_eq!(attempts.load(Ordering::Relaxed), 3);
        assert_eq!(slot_marker(&s), Some(6));
    }

    #[test]
    fn spawned_thread_that_never_gets_a_source_ends_when_stopped() {
        let s = slot();
        let stop = Arc::new(AtomicBool::new(false));
        let factory_stop = stop.clone();
        let attempts = Arc::new(AtomicU32::new(0));
        let counted = attempts.clone();
        let handle = spawn_capture_with(
            move |_| {
                if counted.fetch_add(1, Ordering::Relaxed) == 4 {
                    factory_stop.store(true, Ordering::Relaxed);
                }
                Err::<Fake, _>(CaptureError::Other("no device".into()))
            },
            s.clone(),
            Arc::new(AtomicU32::new(0)),
            stop,
            Duration::ZERO,
        );
        handle.join().unwrap();
        assert_eq!(attempts.load(Ordering::Relaxed), 5);
        assert_eq!(slot_marker(&s), None);
    }

    #[test]
    fn errors_print_their_kind_and_message() {
        assert_eq!(CaptureError::AccessLost("x".into()).to_string(), "access lost: x");
        assert_eq!(CaptureError::DeviceLost("y".into()).to_string(), "device lost: y");
        assert_eq!(CaptureError::Other("z".into()).to_string(), "z");
    }
}
