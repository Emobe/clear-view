mod app;
mod hotkey;
mod settings;

use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU32, Ordering}};

use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

fn main() -> eframe::Result {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    // Load before any thread starts so the TTS thread's initial volume and rate come from the file.
    let shared = cv_core::shared_from(settings::load());
    let frame_state: cv_core::FrameState = Arc::new(Mutex::new(None));

    // Enumerate monitors once at startup.
    let outputs = cv_capture::enumerate_outputs();

    // Shared signal: render thread writes desired output index; capture thread reads it.
    let desired_output = Arc::new(AtomicU32::new(0));

    // Capture thread: DXGI → CPU Vec<u8> → frame_state
    {
        let frame_state    = frame_state.clone();
        let desired_output = desired_output.clone();
        std::thread::spawn(move || {
            let mut capturer = match cv_capture::Capturer::new() {
                Ok(c) => c,
                Err(e) => { eprintln!("[capture] init failed: {e}"); return; }
            };
            loop {
                // Switch output if the render thread requested a different monitor.
                let wanted = desired_output.load(Ordering::Relaxed);
                if wanted != capturer.output_idx {
                    if let Err(e) = capturer.switch_output(wanted) {
                        eprintln!("[capture] switch_output({wanted}) failed: {e}");
                    }
                }

                match capturer.next_frame(100) {
                    Ok(Some(frame)) => {
                        *frame_state.lock().unwrap() = Some(Arc::new(frame));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("[capture] {e} — reconnecting");
                        if capturer.reconnect().is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }

    // Renderer thread: overlay window, reads frame_state + app state
    {
        let frame_state    = frame_state.clone();
        let app_state      = shared.clone();
        let desired_output = desired_output.clone();
        std::thread::spawn(move || {
            cv_render::run_overlay(frame_state, app_state, outputs, desired_output);
        });
    }

    // Hotkey thread: Ctrl+Alt+Shift+Z toggles enabled, shows/hides overlay
    {
        let state = shared.clone();
        std::thread::spawn(move || {
            hotkey::hotkey_loop(state);
        });
    }

    // TTS thread: SAPI speech, MTA COM init
    let tts_shutdown = Arc::new(AtomicBool::new(false));
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
                .with_inner_size([320.0, 460.0])
                .with_resizable(false)
                .with_always_on_top(),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(app::ClearViewApp::new(cc, state_for_egui)))),
    );

    tts_shutdown.store(true, Ordering::Relaxed);
    tts_handle.join().ok();

    // The saver does a final write after it sees the flag, so the last change is not lost.
    settings_shutdown.store(true, Ordering::Relaxed);
    settings_handle.join().ok();

    result
}
