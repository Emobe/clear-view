use std::ffi::c_void;
use std::mem::size_of;

use cv_core::Action;
use cv_core::hotkeys::{Binding, DEFAULT_BINDINGS, Key, KeyId, SilenceKey};
use windows::core::{HRESULT, w};
use windows::Win32::Foundation::{ERROR_HOTKEY_ALREADY_REGISTERED, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::{
    Input::{
        GetRawInputData, GetRegisteredRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RAWKEYBOARD,
        RID_INPUT, RIDEV_INPUTSINK, RIM_TYPEKEYBOARD, RegisterRawInputDevices,
        KeyboardAndMouse::{
            HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey,
            VK_CONTROL, VK_DOWN, VK_MENU, VK_NEXT, VK_PRIOR, VK_UP,
        },
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

// RAWKEYBOARD.Flags and MakeCode values; the windows crate does not define them.
const RI_KEY_BREAK: u16 = 1;
const RI_KEY_E0: u16 = 2;
const KEYBOARD_OVERRUN_MAKE_CODE: u16 = 0xFF;

/// What one raw keyboard report means to the silence rule: which key, and down or up. Raw input
/// reports both Ctrl keys as `VK_CONTROL`; the E0 prefix marks the right one. Overrun reports
/// and unmapped keys (virtual key 0xFF) are `Other`.
fn key_event(vkey: u16, make_code: u16, flags: u16) -> (KeyId, bool) {
    let down = flags & RI_KEY_BREAK == 0;
    if make_code == KEYBOARD_OVERRUN_MAKE_CODE || vkey >= 0xFF {
        return (KeyId::Other, down);
    }
    let id = match vkey {
        v if v == VK_CONTROL.0 && flags & RI_KEY_E0 != 0 => KeyId::RightCtrl,
        v if v == VK_CONTROL.0 => KeyId::LeftCtrl,
        _ => KeyId::Other,
    };
    (id, down)
}

/// The raw input window has no behaviour of its own: the loop reads its `WM_INPUT` messages.
unsafe extern "system" fn keys_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Creates the message-only window that is the raw input target, and registers the keyboard
/// collection for it with `RIDEV_INPUTSINK` so it gets keys while another window has the focus
/// (ADR 0014). Observe only: no `RIDEV_NOLEGACY`, so every key still reaches its app.
///
/// # Safety
/// Call on the thread that will run the message loop.
unsafe fn create_raw_input_window() -> windows::core::Result<HWND> {
    unsafe {
        let hinstance: HINSTANCE = GetModuleHandleW(None)?.into();
        let class = w!("clear-view-keys");
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(keys_wnd_proc),
            hInstance: hinstance,
            lpszClassName: class,
            ..Default::default()
        };
        // Zero means it failed; CreateWindowExW then reports the error.
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("clear-view keys"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinstance),
            None,
        )?;
        if let Err(e) = register_keyboard(hwnd) {
            let _ = DestroyWindow(hwnd);
            return Err(e);
        }
        Ok(hwnd)
    }
}

const HID_USAGE_PAGE_GENERIC: u16 = 0x01;
const HID_USAGE_GENERIC_KEYBOARD: u16 = 0x06;

/// Registers the keyboard collection for raw input to `hwnd`, in the background too.
fn register_keyboard(hwnd: HWND) -> windows::core::Result<()> {
    let keyboard = RAWINPUTDEVICE {
        usUsagePage: HID_USAGE_PAGE_GENERIC,
        usUsage: HID_USAGE_GENERIC_KEYBOARD,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    };
    unsafe { RegisterRawInputDevices(&[keyboard], size_of::<RAWINPUTDEVICE>() as u32) }
}

/// True while the process's keyboard raw input registration still targets `hwnd` with
/// `RIDEV_INPUTSINK`. Registration is one per process and device type, so any other code in the
/// process (winit, which eframe runs on, registers the keyboard when its event loop is created)
/// replaces ours without a sign.
fn keyboard_still_ours(hwnd: HWND) -> bool {
    let size = size_of::<RAWINPUTDEVICE>() as u32;
    let mut count = 0u32;
    // The first call only reports how many devices are registered.
    unsafe { GetRegisteredRawInputDevices(None, &mut count, size) };
    if count == 0 {
        return false;
    }
    let mut devices = vec![RAWINPUTDEVICE::default(); count as usize];
    let got = unsafe { GetRegisteredRawInputDevices(Some(devices.as_mut_ptr()), &mut count, size) };
    if got == u32::MAX {
        return false;
    }
    devices[..(got as usize).min(devices.len())].iter().any(|d| {
        d.usUsagePage == HID_USAGE_PAGE_GENERIC
            && d.usUsage == HID_USAGE_GENERIC_KEYBOARD
            && d.hwndTarget == hwnd
            && d.dwFlags.0 & RIDEV_INPUTSINK.0 != 0
    })
}

/// How often the hotkey thread checks that the raw input registration is still ours.
const RAW_INPUT_CHECK_MS: u32 = 500;
const RAW_INPUT_TIMER: usize = 1;

/// Reads the keyboard report behind a `WM_INPUT` message, or `None` for any other device or a
/// failed read.
///
/// # Safety
/// `lparam` must be the `lParam` of a `WM_INPUT` message being handled now.
unsafe fn read_keyboard(lparam: LPARAM) -> Option<RAWKEYBOARD> {
    unsafe {
        let mut raw = RAWINPUT::default();
        let mut size = size_of::<RAWINPUT>() as u32;
        let copied = GetRawInputData(
            HRAWINPUT(lparam.0 as *mut c_void),
            RID_INPUT,
            Some(&mut raw as *mut RAWINPUT as *mut c_void),
            &mut size,
            size_of::<RAWINPUTHEADER>() as u32,
        );
        if copied == u32::MAX || raw.header.dwType != RIM_TYPEKEYBOARD.0 {
            return None;
        }
        Some(raw.data.keyboard)
    }
}

const MODS: HOT_KEY_MODIFIERS =
    HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0 | MOD_SHIFT.0 | MOD_NOREPEAT.0);

/// Starts the hotkey thread. `RegisterHotKey` with no window posts `WM_HOTKEY` to the thread
/// that registered it, so registering and the message loop share this one thread.
/// The thread is detached and ends with the process.
///
/// `on_failures` runs once, only if a binding failed, with one line per failed binding for the
/// settings panel. `on_action` runs for each hotkey press. `speech_keys` also registers the
/// speech bindings and watches Ctrl for `Action::Silence` through raw input; app passes true
/// only with the `tts` feature (ADR 0014). Without it the thread makes no window and reads no keys.
pub fn spawn_hotkey_thread(
    speech_keys: bool,
    on_failures: impl FnOnce(Vec<String>) + Send + 'static,
    on_action: impl FnMut(Action) + Send + 'static,
) {
    std::thread::spawn(move || hotkey_loop(speech_keys, on_failures, on_action));
}

/// Registers the bindings once, then blocks in `GetMessageW` and reports each `WM_HOTKEY`, and
/// with `speech_keys` each lone Ctrl press from `WM_INPUT`. A binding that fails to register
/// has no hotkey for the session: it is logged and reported, with no fallback combination and
/// no retry (ADR 0005). The raw input window is treated the same way (ADR 0014).
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

        // `CV_KEYS_LOG=1` logs the raw Ctrl and Alt reports, to see how AltGr arrives (ADR 0014).
        let log_raw = speech_keys && std::env::var_os("CV_KEYS_LOG").is_some();
        let mut silence = SilenceKey::new();
        let mut raw_window = None;
        if speech_keys {
            match create_raw_input_window() {
                Ok(hwnd) => {
                    raw_window = Some(hwnd);
                    SetTimer(Some(hwnd), RAW_INPUT_TIMER, RAW_INPUT_CHECK_MS, None);
                }
                Err(e) => {
                    eprintln!("[hotkey] raw input for the silence key failed: {e}");
                    failures.push(format!(
                        "Silence key (Ctrl): could not be set up (error {:#010x})",
                        e.code().0
                    ));
                }
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
            if let Some(hwnd) = raw_window
                && msg.message == WM_TIMER
                && msg.hwnd == hwnd
                && msg.wParam.0 == RAW_INPUT_TIMER
            {
                if !keyboard_still_ours(hwnd) {
                    match register_keyboard(hwnd) {
                        Ok(()) => eprintln!("[hotkey] raw keyboard input had been taken over; restored"),
                        Err(e) => eprintln!("[hotkey] could not restore raw keyboard input: {e}"),
                    }
                }
                continue;
            }
            if msg.message == WM_INPUT && raw_window.is_some() {
                if let Some(kb) = read_keyboard(msg.lParam) {
                    if log_raw && (kb.VKey == VK_CONTROL.0 || kb.VKey == VK_MENU.0) {
                        eprintln!(
                            "[keys] raw vkey={:#04x} make={:#04x} flags={:#x}",
                            kb.VKey, kb.MakeCode, kb.Flags
                        );
                    }
                    let (id, down) = key_event(kb.VKey, kb.MakeCode, kb.Flags);
                    if silence.feed(id, down) {
                        on_action(Action::Silence);
                    }
                }
                // The system cleans up after a WM_INPUT only through DefWindowProc.
                DefWindowProcW(msg.hwnd, msg.message, msg.wParam, msg.lParam);
                continue;
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
    fn raw_reports_map_to_the_two_ctrl_keys() {
        let ctrl = VK_CONTROL.0;
        assert_eq!(key_event(ctrl, 0x1D, 0), (KeyId::LeftCtrl, true));
        assert_eq!(key_event(ctrl, 0x1D, RI_KEY_BREAK), (KeyId::LeftCtrl, false));
        assert_eq!(key_event(ctrl, 0x1D, RI_KEY_E0), (KeyId::RightCtrl, true));
        assert_eq!(key_event(ctrl, 0x1D, RI_KEY_E0 | RI_KEY_BREAK), (KeyId::RightCtrl, false));
    }

    #[test]
    fn other_keys_and_overruns_are_other() {
        assert_eq!(key_event(0x41, 0x1E, 0), (KeyId::Other, true));
        assert_eq!(key_event(VK_MENU.0, 0x38, RI_KEY_E0 | RI_KEY_BREAK), (KeyId::Other, false));
        assert_eq!(key_event(VK_CONTROL.0, KEYBOARD_OVERRUN_MAKE_CODE, 0), (KeyId::Other, true));
        assert_eq!(key_event(0xFF, 0, 0), (KeyId::Other, true));
    }

    #[test]
    fn a_held_ctrl_through_the_mapping_silences_once() {
        let mut rule = SilenceKey::new();
        let mut presses = 0;
        for flags in [0, 0, 0, RI_KEY_BREAK] {
            let (id, down) = key_event(VK_CONTROL.0, 0x1D, flags);
            presses += rule.feed(id, down) as u32;
        }
        assert_eq!(presses, 1);
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

    /// Starts the hotkey thread with speech keys, lets `disturb` run once it is set up, injects
    /// a Ctrl tap with `SendInput` and returns the first action reported within 2 s.
    fn inject_ctrl_tap(disturb: impl FnOnce()) -> Option<Action> {
        use std::sync::mpsc::channel;
        use std::time::Duration;
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
        };

        let (tx, rx) = channel();
        spawn_hotkey_thread(true, |lines| eprintln!("failures: {lines:?}"), move |a| {
            let _ = tx.send(a);
        });
        std::thread::sleep(Duration::from_millis(500)); // let the thread set up
        disturb();

        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: VK_CONTROL, dwFlags: flags, ..Default::default() },
            },
        };
        let events = [key(Default::default()), key(KEYEVENTF_KEYUP)];
        let sent = unsafe { SendInput(&events, size_of::<INPUT>() as i32) };
        assert_eq!(sent, 2, "SendInput");
        rx.recv_timeout(Duration::from_secs(2)).ok()
    }

    /// These two need an interactive desktop, so they are ignored. Run them one at a time, each
    /// starts a hotkey thread that lives to the end of the process:
    /// `cargo test -p cv-platform-win ctrl_tap_reaches -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn ctrl_tap_reaches_the_hotkey_thread_as_silence() {
        assert_eq!(inject_ctrl_tap(|| {}), Some(Action::Silence));
    }

    /// winit registers the keyboard for raw input when eframe starts, after the hotkey thread
    /// did. Another registrant is simulated here; the thread must take the keyboard back.
    #[test]
    #[ignore]
    fn ctrl_tap_still_silences_after_another_registrant_takes_the_keyboard() {
        let got = inject_ctrl_tap(|| {
            // Registering the keyboard for this new window replaces the thread's registration.
            let _other = unsafe { create_raw_input_window() }.expect("second window");
            std::thread::sleep(std::time::Duration::from_millis(1200));
        });
        assert_eq!(got, Some(Action::Silence));
    }
}
