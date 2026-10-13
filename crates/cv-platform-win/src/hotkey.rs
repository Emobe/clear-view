use cv_core::Action;
use cv_core::hotkeys::{Binding, DEFAULT_BINDINGS, Key};
use windows::core::HRESULT;
use windows::Win32::Foundation::ERROR_HOTKEY_ALREADY_REGISTERED;
use windows::Win32::UI::{
    Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, VK_DOWN,
        VK_NEXT, VK_PRIOR, VK_UP,
    },
    WindowsAndMessaging::*,
};

// Bindings are data in cv-core (ADR 0014, which extends 0005); every one uses Ctrl+Alt+Shift.
pub use cv_core::hotkeys::TOGGLE_LABEL;

/// The virtual-key code for a neutral key. Letters A to Z share their ASCII codes.
fn virtual_key(key: Key) -> u32 {
    match key {
        Key::Letter(c) => {
            debug_assert!(c.is_ascii_uppercase(), "binding letter {c:?} is not A-Z");
            c as u32
        }
        Key::Up => VK_UP.0 as u32,
        Key::Down => VK_DOWN.0 as u32,
        Key::PageUp => VK_PRIOR.0 as u32,
        Key::PageDown => VK_NEXT.0 as u32,
    }
}

/// The id a binding is registered under: its place in the table, from 1.
fn hotkey_id(index: usize) -> i32 {
    index as i32 + 1
}

/// The bindings to register, with their ids. Speech bindings only when `speech_keys` is set.
fn active_bindings(speech_keys: bool) -> impl Iterator<Item = (i32, &'static Binding)> {
    DEFAULT_BINDINGS
        .iter()
        .enumerate()
        .filter(move |(_, b)| speech_keys || !b.speech)
        .map(|(i, b)| (hotkey_id(i), b))
}

const MODS: HOT_KEY_MODIFIERS =
    HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0 | MOD_SHIFT.0 | MOD_NOREPEAT.0);

/// Starts the hotkey thread. `RegisterHotKey` with no window posts `WM_HOTKEY` to the thread
/// that registered it, so registering and the message loop share this one thread.
/// The thread is detached and ends with the process.
///
/// `on_failures` runs once, only if a binding failed, with one line per failed binding for the
/// settings panel. `on_action` runs for each hotkey press. `speech_keys` also registers the
/// speech bindings; app will pass true only with the `tts` feature, from roadmap 8.6b (ADR 0014).
pub fn spawn_hotkey_thread(
    speech_keys: bool,
    on_failures: impl FnOnce(Vec<String>) + Send + 'static,
    on_action: impl FnMut(Action) + Send + 'static,
) {
    std::thread::spawn(move || hotkey_loop(speech_keys, on_failures, on_action));
}

/// Registers the bindings once, then blocks in `GetMessageW` and reports each `WM_HOTKEY`.
/// A binding that fails to register has no hotkey for the session: it is logged and reported,
/// with no fallback combination and no retry (ADR 0005).
fn hotkey_loop(
    speech_keys: bool,
    on_failures: impl FnOnce(Vec<String>),
    mut on_action: impl FnMut(Action),
) {
    unsafe {
        let mut failures = Vec::new();
        for (id, b) in active_bindings(speech_keys) {
            if let Err(e) = RegisterHotKey(None, id, MODS, virtual_key(b.chord.key)) {
                eprintln!("[hotkey] RegisterHotKey failed for {}: {e}", b.label);
                let why = if e.code() == HRESULT::from_win32(ERROR_HOTKEY_ALREADY_REGISTERED.0) {
                    "in use by another program".to_string()
                } else {
                    format!("could not be registered (error {:#010x})", e.code().0)
                };
                failures.push(format!("{}: {why}", b.label));
            }
        }
        if !failures.is_empty() {
            on_failures(failures);
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
            let Some((_, b)) = active_bindings(speech_keys).find(|(id, _)| *id == msg.wParam.0 as i32)
            else {
                continue;
            };
            on_action(b.action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_map_to_their_ascii_codes() {
        assert_eq!(virtual_key(Key::Letter('Z')), 0x5A);
        assert_eq!(virtual_key(Key::Letter('S')), 0x53);
    }

    #[test]
    fn the_magnifier_ids_are_unchanged() {
        let ids: Vec<_> = active_bindings(false).map(|(id, b)| (id, b.action)).collect();
        assert_eq!(ids, [(1, Action::Toggle), (2, Action::ZoomIn), (3, Action::ZoomOut)]);
    }

    #[test]
    fn speech_bindings_are_registered_only_when_asked() {
        assert!(active_bindings(false).all(|(_, b)| !b.speech));
        assert!(active_bindings(true).any(|(_, b)| b.action == Action::ToggleSpeech));
    }
}
