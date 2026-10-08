use cv_core::SharedState;
use windows::Win32::UI::{
    Input::KeyboardAndMouse::{
        MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, VK_Z,
    },
    WindowsAndMessaging::*,
};

const HOTKEY_ID: i32 = 1;

/// Registers Ctrl+Alt+Shift+Z as a global toggle and pumps WM_HOTKEY messages.
/// The Win key is avoided: the shell owns those combinations (Win+= is Windows Magnifier).
pub fn hotkey_loop(state: SharedState) {
    unsafe {
        let mods = MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT;
        if RegisterHotKey(None, HOTKEY_ID, mods, VK_Z.0 as u32).is_err() {
            eprintln!("[hotkey] RegisterHotKey failed — Ctrl+Alt+Shift+Z unavailable");
        }

        let mut msg = MSG::default();
        loop {
            if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_HOTKEY && msg.wParam.0 as i32 == HOTKEY_ID {
                    let mut s = state.write();
                    s.enabled = !s.enabled;
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}
