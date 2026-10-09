//! The capture loop (ADR 0004). The backend supplies a `CaptureSource`; the loop here publishes
//! its frames to the shared slot and follows the monitor the render thread asks for.

use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    thread::JoinHandle,
};

use cv_core::{Frame, FrameState};

/// How long one `next_frame` call waits for a new frame before the loop checks for a monitor
/// switch again.
const FRAME_TIMEOUT_MS: u32 = 100;

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
/// running; it ends only if the source cannot be created or a reconnect fails.
pub fn spawn_capture<S, F>(
    factory: F,
    frame_state: FrameState,
    desired_output: Arc<AtomicU32>,
) -> JoinHandle<()>
where
    S: CaptureSource,
    F: FnOnce(u32) -> Result<S, CaptureError> + Send + 'static,
{
    std::thread::spawn(move || {
        let start = desired_output.load(Ordering::Relaxed);
        let mut source = match factory(start) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[capture] init failed: {e}");
                return;
            }
        };
        run_capture(&mut source, start, &frame_state, &desired_output);
    })
}

/// The loop itself. Returns when a reconnect fails.
fn run_capture<S: CaptureSource>(
    source: &mut S,
    mut current: u32,
    frame_state: &FrameState,
    desired_output: &AtomicU32,
) {
    loop {
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
                if source.reconnect().is_err() {
                    break;
                }
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

    /// A scripted source. When `frames` runs out, `next_frame` fails; when `reconnects` runs
    /// out, `reconnect` fails, so every script ends the loop.
    struct Fake {
        frames: VecDeque<Result<Option<Frame>, CaptureError>>,
        switches: VecDeque<bool>,
        reconnects: VecDeque<bool>,
        calls: Arc<Mutex<Vec<Call>>>,
    }

    impl Fake {
        fn new(frames: Vec<Result<Option<Frame>, CaptureError>>) -> Self {
            Self {
                frames: frames.into(),
                switches: VecDeque::new(),
                reconnects: VecDeque::new(),
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
            self.frames
                .pop_front()
                .unwrap_or_else(|| Err(CaptureError::Other("script ended".into())))
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
            if self.reconnects.pop_front().unwrap_or(false) {
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

    #[test]
    fn frames_reach_the_slot_newest_last() {
        let mut fake = Fake::new(vec![frame(1), frame(2)]);
        let s = slot();
        run_capture(&mut fake, 0, &s, &AtomicU32::new(0));
        assert_eq!(slot_marker(&s), Some(2));
        assert_eq!(fake.calls(), [Call::Next, Call::Next, Call::Next, Call::Reconnect]);
    }

    #[test]
    fn timeout_leaves_the_slot_alone() {
        let mut fake = Fake::new(vec![Ok(None)]);
        let s = slot();
        *s.lock().unwrap() = Some(Arc::new(Frame { width: 7, height: 1, data: Vec::new() }));
        run_capture(&mut fake, 0, &s, &AtomicU32::new(0));
        assert_eq!(slot_marker(&s), Some(7));
    }

    #[test]
    fn requested_output_is_switched_to_once() {
        let mut fake = Fake::new(vec![Ok(None), Ok(None)]);
        run_capture(&mut fake, 0, &slot(), &AtomicU32::new(1));
        assert_eq!(
            fake.calls(),
            [Call::Switch(1), Call::Next, Call::Next, Call::Next, Call::Reconnect]
        );
    }

    #[test]
    fn same_output_is_not_switched() {
        let mut fake = Fake::new(vec![Ok(None)]);
        run_capture(&mut fake, 2, &slot(), &AtomicU32::new(2));
        assert!(!fake.calls().iter().any(|c| matches!(c, Call::Switch(_))));
    }

    #[test]
    fn failed_switch_is_retried_on_the_next_pass() {
        let mut fake = Fake::new(vec![Ok(None), Ok(None)]);
        fake.switches = [false, true].into();
        run_capture(&mut fake, 0, &slot(), &AtomicU32::new(1));
        assert_eq!(
            fake.calls(),
            [Call::Switch(1), Call::Next, Call::Switch(1), Call::Next, Call::Next, Call::Reconnect]
        );
    }

    #[test]
    fn error_reconnects_and_carries_on() {
        let mut fake = Fake::new(vec![Err(CaptureError::AccessLost("gone".into())), frame(3)]);
        fake.reconnects = [true].into();
        let s = slot();
        run_capture(&mut fake, 0, &s, &AtomicU32::new(0));
        assert_eq!(slot_marker(&s), Some(3));
        assert_eq!(
            fake.calls(),
            [Call::Next, Call::Reconnect, Call::Next, Call::Next, Call::Reconnect]
        );
    }

    #[test]
    fn failed_reconnect_ends_the_loop() {
        let mut fake = Fake::new(vec![Err(CaptureError::DeviceLost("gone".into())), frame(4)]);
        let s = slot();
        run_capture(&mut fake, 0, &s, &AtomicU32::new(0));
        assert_eq!(fake.calls(), [Call::Next, Call::Reconnect]);
        assert_eq!(slot_marker(&s), None);
    }

    #[test]
    fn spawned_thread_starts_on_the_requested_output_and_publishes() {
        let (tx, rx) = mpsc::channel();
        let s = slot();
        let handle = spawn_capture(
            move |idx| {
                tx.send(idx).unwrap();
                Ok(Fake::new(vec![frame(5)]))
            },
            s.clone(),
            Arc::new(AtomicU32::new(2)),
        );
        handle.join().unwrap();
        assert_eq!(rx.recv().unwrap(), 2);
        assert_eq!(slot_marker(&s), Some(5));
    }

    #[test]
    fn spawned_thread_ends_when_the_source_cannot_be_made() {
        let s = slot();
        let handle = spawn_capture(
            |_| Err::<Fake, _>(CaptureError::Other("no device".into())),
            s.clone(),
            Arc::new(AtomicU32::new(0)),
        );
        handle.join().unwrap();
        assert_eq!(slot_marker(&s), None);
    }

    #[test]
    fn errors_print_their_kind_and_message() {
        assert_eq!(CaptureError::AccessLost("x".into()).to_string(), "access lost: x");
        assert_eq!(CaptureError::DeviceLost("y".into()).to_string(), "device lost: y");
        assert_eq!(CaptureError::Other("z".into()).to_string(), "z");
    }
}
