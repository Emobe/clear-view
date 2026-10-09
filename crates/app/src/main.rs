mod app;
mod settings;

use std::sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, AtomicU32, Ordering}};

// The platform backend (ADR 0004). One per target; only Windows exists before v1.
#[cfg(windows)]
use cv_platform_win as platform;
#[cfg(not(windows))]
compile_error!("clear-view has a platform backend for Windows only");

/// Stops the render thread when dropped, so a panic in `main` still restores the work area,
/// system cursor and cursor clip.
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

    // Renderer thread: overlay window, reads frame_state + app state.
    // The guard stops it and restores the machine on return or panic; Ctrl+C and closing the
    // console are handled inside the backend.
    let mut overlay = OverlayGuard(Some(platform::spawn_overlay(
        frame_state.clone(),
        shared.clone(),
        outputs,
        desired_output.clone(),
    )));

    // Hotkey thread: Ctrl+Alt+Shift+Z toggles enabled (shows/hides overlay), +Up and +Down zoom.
    // The panel does not poll, so the thread wakes it after a change or a failed registration.
    let repaint: app::RepaintSlot = Arc::new(OnceLock::new());
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
        platform::spawn_hotkey_thread(
            // After the push, so a panel that was already drawn repaints with the failures.
            move |lines| {
                failures.lock().unwrap().extend(lines);
                wake_failures();
            },
            move |action| {
                state.write().apply(action);
                wake();
            },
        );
    }

    // TTS thread: SAPI speech, MTA COM init. Only with `--features tts`.
    #[cfg(feature = "tts")]
    let tts_shutdown = Arc::new(AtomicBool::new(false));
    #[cfg(feature = "tts")]
    let tts_handle = cv_tts::spawn_tts_thread(tts_shutdown.clone(), shared.clone());

    // Settings saver thread: writes settings.json when AppState changes
    let settings_shutdown = Arc::new(AtomicBool::new(false));
    let settings_handle = settings::spawn_saver(shared.clone(), settings_shutdown.clone());

    // egui settings panel on main thread
    let state_for_egui = shared.clone();
    let result = eframe::run_native(
        "clear-view settings",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([320.0, 340.0])
                .with_resizable(false)
                .with_always_on_top(),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(app::ClearViewApp::new(cc, state_for_egui, repaint, hotkey_failures)))),
    );

    // Before the other threads stop, so the work area, cursor and clip are back first.
    overlay.shutdown();

    #[cfg(feature = "tts")]
    {
        tts_shutdown.store(true, Ordering::Relaxed);
        tts_handle.join().ok();
    }

    // The saver does a final write after it sees the flag, so the last change is not lost.
    settings_shutdown.store(true, Ordering::Relaxed);
    settings_handle.join().ok();

    result
}
