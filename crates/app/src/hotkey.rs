use std::sync::{Arc, Mutex};

use cv_core::SharedState;
use windows::core::HRESULT;
use windows::Win32::Foundation::ERROR_HOTKEY_ALREADY_REGISTERED;
use windows::Win32::UI::{
    Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, VK_DOWN,
        VK_UP, VK_Z,
    },
    WindowsAndMessaging::*,
};

/// One line per binding that failed to register, for the settings panel. Filled once at startup
/// by the hotkey thread; never saved.
pub type HotkeyFailures = Arc<Mutex<Vec<String>>>;

#[derive(Clone, Copy)]
enum Action {
    Toggle,
    ZoomIn,
    ZoomOut,
}

struct Binding {
    id: i32,
    vk: u32,
    label: &'static str,
    action: Action,
}

// The Win key is avoided: the shell owns those combinations (Win+= is Windows Magnifier).
// Bindings are constants for v1 (ADR 0005); every one uses Ctrl+Alt+Shift.
const BINDINGS: [Binding; 3] = [
    Binding { id: 1, vk: VK_Z.0 as u32,    label: "Ctrl+Alt+Shift+Z",    action: Action::Toggle },
    Binding { id: 2, vk: VK_UP.0 as u32,   label: "Ctrl+Alt+Shift+Up",   action: Action::ZoomIn },
    Binding { id: 3, vk: VK_DOWN.0 as u32, label: "Ctrl+Alt+Shift+Down", action: Action::ZoomOut },
];

pub const TOGGLE_LABEL: &str = BINDINGS[0].label;

const MODS: HOT_KEY_MODIFIERS =
    HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0 | MOD_SHIFT.0 | MOD_NOREPEAT.0);

/// Registers the bindings once, then blocks in `GetMessageW` and applies each `WM_HOTKEY`.
/// A binding that fails to register has no hotkey for the session: it is logged and added to
/// `failures`, with no fallback combination and no retry (ADR 0005).
/// `wake` runs after each change so the settings panel redraws.
pub fn hotkey_loop(state: SharedState, failures: HotkeyFailures, wake: impl Fn()) {
    unsafe {
        let mut failed = false;
        for b in &BINDINGS {
            if let Err(e) = RegisterHotKey(None, b.id, MODS, b.vk) {
                eprintln!("[hotkey] RegisterHotKey failed for {}: {e}", b.label);
                let why = if e.code() == HRESULT::from_win32(ERROR_HOTKEY_ALREADY_REGISTERED.0) {
                    "in use by another program".to_string()
                } else {
                    format!("could not be registered (error {:#010x})", e.code().0)
                };
                failures.lock().unwrap().push(format!("{}: {why}", b.label));
                failed = true;
            }
        }
        // After the pushes, so a panel that was already drawn repaints with the failures.
        if failed {
            wake();
        }

        let mut msg = MSG::default();
        loop {
            match GetMessageW(&mut msg, None, 0, 0).0 {
                0 => break,
                -1 => {
                    eprintln!("[hotkey] GetMessageW failed — hotkeys stopped");
                    break;
                }
                _ => {}
            }
            if msg.message != WM_HOTKEY {
                continue;
            }
            let Some(b) = BINDINGS.iter().find(|b| b.id == msg.wParam.0 as i32) else {
                continue;
            };
            {
                let mut s = state.write();
                match b.action {
                    Action::Toggle => s.enabled = !s.enabled,
                    Action::ZoomIn => s.step_zoom(1),
                    Action::ZoomOut => s.step_zoom(-1),
                }
            }
            wake();
        }
    }
}
