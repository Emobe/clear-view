mod app;
mod settings;

use std::sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, AtomicU32, Ordering}};

use cv_core::Action;

// The platform backend (ADR 0004). One per target; only Windows exists before v1.
#[cfg(windows)]
use cv_platform_win as platform;
#[cfg(not(windows))]
compile_error!("clear-view has a platform backend for Windows only");

/// Stops the render thread when dropped, so a panic in `main` still shows the system cursor and
/// removes the overlay window.
struct OverlayGuard(Option<platform::OverlayHandle>);

impl OverlayGuard {
    fn shutdown(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.shutdown();
        }
    }
}

impl Drop for OverlayGuard {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Stops the caret thread when dropped, so a panic in `main` still unhooks it.
struct CaretGuard(Option<platform::CaretHandle>);

impl CaretGuard {
    fn shutdown(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.shutdown();
        }
    }
}

impl Drop for CaretGuard {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Stops the focus thread when dropped, so a panic in `main` still unhooks it. Only with `tts`.
#[cfg(feature = "tts")]
struct FocusGuard(Option<platform::FocusHandle>);

#[cfg(feature = "tts")]
impl FocusGuard {
    fn shutdown(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.shutdown();
        }
    }
}

#[cfg(feature = "tts")]
impl Drop for FocusGuard {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn main() -> eframe::Result {
    // Before any window or thread: everything below works in physical pixels.
    platform::set_dpi_awareness();
    // Windows build and multiplane overlay support, for the docked overlay (ADR 0007).
    platform::log_system_info();

    // Load before any thread starts so the TTS thread's initial volume and rate come from the file.
    let shared = cv_core::shared_from(settings::load());
    let frame_state: cv_core::FrameState = Arc::new(Mutex::new(None));

    // Enumerate monitors once at startup.
    let outputs = platform::enumerate_outputs();

    // Shared signal: render thread writes desired output index; capture thread reads it.
    let desired_output = Arc::new(AtomicU32::new(0));

    // Capture thread: the backend's source → CPU frames → frame_state. Left running; the
    // source is created on that thread.
    let _capture = cv_magnifier::spawn_capture(
        platform::Capturer::new_for_output,
        frame_state.clone(),
        desired_output.clone(),
    );

    // Core events (ADR 0008). The caret thread publishes caret moves here and logs `[caret]`
    // lines; the render thread subscribes for the tracking policy.
    let events = Arc::new(cv_core::EventHub::new());

    // Speech thread (ADR 0013), only with `tts`. It makes the engines when speech is turned on
    // and does nothing while it is off.
    // The panel does not poll, so the thread wakes it when the voice list or state changes.
    let repaint: app::RepaintSlot = Arc::new(OnceLock::new());
    #[cfg(feature = "tts")]
    let speech = {
        let repaint = repaint.clone();
        cv_tts::spawn_speech(shared.clone(), &events, move || {
            if let Some(ctx) = repaint.get() {
                ctx.request_repaint();
            }
        })
    };

    // Renderer thread: overlay window, reads frame_state + app state and drains its core events.
    // The guard stops it and restores the machine on return or panic; Ctrl+C and closing the
    // console are handled inside the backend.
    let mut overlay = OverlayGuard(Some(platform::spawn_overlay(
        frame_state.clone(),
        shared.clone(),
        outputs,
        desired_output.clone(),
        events.subscribe(),
    )));

    let mut caret = CaretGuard(Some(platform::spawn_caret_source(events.clone())));

    // Focus thread (ADR 0015), only with `tts`. It takes WinEvents but reads nothing while speech
    // is off; the speech on/off hotkey and the panel wake it.
    #[cfg(feature = "tts")]
    let mut focus = FocusGuard(Some(platform::spawn_focus_source(events.clone(), shared.clone())));
    #[cfg(feature = "tts")]
    let focus_waker = focus.0.as_ref().map(|h| h.waker()).expect("the focus thread was just spawned");

    // Hotkey thread: Ctrl+Alt+Shift+Z toggles enabled (shows/hides overlay), +Up and +Down zoom.
    // The panel does not poll, so the thread wakes it after a change or a failed registration.
    let hotkey_failures: app::HotkeyFailures = Arc::new(Mutex::new(Vec::new()));
    {
        let wake = {
            let repaint = repaint.clone();
            move || {
                if let Some(ctx) = repaint.get() {
                    ctx.request_repaint();
                }
            }
        };
        let failures = hotkey_failures.clone();
        let wake_failures = wake.clone();
        let state = shared.clone();
        #[cfg(feature = "tts")]
        let speech = speech.clone();
        platform::spawn_hotkey_thread(
            // Speech keys (silence on a lone Ctrl, speech on/off) only with `tts` (ADR 0014).
            cfg!(feature = "tts"),
            // After the push, so a panel that was already drawn repaints with the failures.
            move |lines| {
                failures.lock().unwrap().extend(lines);
                wake_failures();
            },
            move |action| {
                match action {
                    Action::Silence => {
                        eprintln!("[keys] silence");
                        #[cfg(feature = "tts")]
                        speech.silence();
                        return;
                    }
                    Action::ToggleSpeech => eprintln!("[keys] speech on/off"),
                    _ => {}
                }
                state.write().apply(action);
                #[cfg(feature = "tts")]
                if action == Action::ToggleSpeech {
                    // After the state change, so the thread sees the new value.
                    speech.wake();
                    focus_waker.wake();
                }
                wake();
            },
        );
    }

    // Settings saver thread: writes settings.json when AppState changes
    let settings_shutdown = Arc::new(AtomicBool::new(false));
    let settings_handle = settings::spawn_saver(shared.clone(), settings_shutdown.clone());

    // egui settings panel on main thread
    let state_for_egui = shared.clone();
    #[cfg(feature = "tts")]
    let speech_for_panel = speech.clone();
    let result = eframe::run_native(
        "clear-view settings",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([340.0, 560.0])
                .with_min_inner_size([300.0, 240.0])
                .with_always_on_top(),
            ..Default::default()
        },
        Box::new(|cc| {
            Ok(Box::new(app::ClearViewApp::new(
                cc,
                state_for_egui,
                repaint,
                hotkey_failures,
                #[cfg(feature = "tts")]
                speech_for_panel,
                #[cfg(feature = "tts")]
                focus_waker,
            )))
        }),
    );

    // Before the other threads stop, so the system cursor is back first.
    overlay.shutdown();
    caret.shutdown();
    #[cfg(feature = "tts")]
    focus.shutdown();
    #[cfg(feature = "tts")]
    speech.shutdown();

    // The saver does a final write after it sees the flag, so the last change is not lost.
    settings_shutdown.store(true, Ordering::Relaxed);
    settings_handle.join().ok();

    result
}
