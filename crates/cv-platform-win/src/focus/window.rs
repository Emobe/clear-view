//! The window read (ADR 0019, roadmap 9.8b2): the answer to Ctrl+Alt+Shift+W. It reads the
//! controls of the foreground window, or of the dialog inside it that holds the focus, and gives
//! back a `CoreEvent::WindowRead` for the speech thread to speak. Runs on the focus thread, when
//! `tts_enabled` is true, never on the render thread.
//!
//! - The window and the inner dialog are found as for a focus read (ADR 0015, 0018): the
//!   foreground window, its title, and `inner::find` for the dialog that holds the focus. The dialog
//!   text is the one a focus read already cached for that pair, and is used only if the read
//!   fails.
//! - The tree is walked by `dialog::read_tree` with the window budgets. No value and no text
//!   pattern is read for any node.
//! - Our own window, a window with no program name and no foreground window answer `app: None`
//!   ("No window"). A window marked hung, or a call that times out, answers `Unreadable` with no
//!   nodes.

use std::time::Instant;

use cv_core::{AppId, CoreEvent, ReadOutcome, WindowInfo};
use windows::Win32::UI::Accessibility::IUIAutomation;

use super::{
    cache::DialogCache,
    dialog::{self, Target},
    inner,
    map::{self, Plan},
    read::{self, Requests},
};
use crate::caret::{UIA_TIMEOUT_MS, exe_name, foreground};

/// Reads the foreground window for a key pressed at `at`. Always answers: a key press is never
/// silent.
pub(super) fn read(
    uia: Option<(&IUIAutomation, &Requests)>,
    own_pid: u32,
    hung: &mut Option<isize>,
    cache: &DialogCache,
    at: Instant,
) -> CoreEvent {
    let no_window = || CoreEvent::WindowRead {
        at,
        app: None,
        window: WindowInfo::new(""),
        nodes: Vec::new(),
        outcome: ReadOutcome::Complete,
    };
    let Some(fg) = foreground() else {
        return no_window();
    };
    let key = fg.hwnd.0 as isize;
    // The foreground window changed: the old hung mark no longer applies.
    if hung.is_some_and(|h| h != key) {
        *hung = None;
    }
    let plan = map::read_plan(true, fg.pid, own_pid, key, *hung);
    if matches!(plan, Plan::Own | Plan::Off) {
        return no_window();
    }
    let exe = exe_name(fg.pid);
    // The shell's staging window and the desktop have no program name: nothing to read.
    if exe == "?" {
        return no_window();
    }
    let app = Some(AppId { pid: fg.pid, exe });
    let window_key = key as usize as u64;
    let mut window = WindowInfo::new(read::window_title(fg.hwnd));
    let started = Instant::now();
    let answer = |window: WindowInfo, nodes, outcome| CoreEvent::WindowRead {
        at,
        app: app.clone(),
        window,
        nodes,
        outcome,
    };

    // The text a focus read cached for the window itself: what an `Unreadable` answer still says.
    // It is replaced below by the inner dialog's when there is one.
    window.dialog_text = cache.get(window_key, None).map(<[String]>::to_vec).unwrap_or_default();
    if plan == Plan::Hung {
        return answer(window, Vec::new(), ReadOutcome::Unreadable);
    }

    // The focused element tells whether the focus is in XAML content, where an inner dialog is
    // found by UI Automation (ADR 0018). A timeout here is a hung window like any other.
    let mut focused = None;
    if let Some((uia, requests)) = uia {
        match read::uia_read(uia, requests) {
            Ok(reading) => focused = Some(reading),
            Err(e) => {
                if map::is_hung(e.code().0, started.elapsed().as_millis() as u64, UIA_TIMEOUT_MS) {
                    eprintln!("[focus] window read: uia timed out in {}; skipped until the foreground changes", fg.pid);
                    *hung = Some(key);
                    return answer(window, Vec::new(), ReadOutcome::Unreadable);
                }
            }
        }
    }
    let lookup = inner::find(uia, fg.hwnd, focused.as_ref());
    if lookup.hung {
        eprintln!("[focus] window read: dialog lookup timed out in {}; skipped until the foreground changes", fg.pid);
        *hung = Some(key);
        return answer(window, Vec::new(), ReadOutcome::Unreadable);
    }
    let dialog_key = lookup.inner.as_ref().map(|inner| inner.key);
    if let Some(inner) = &lookup.inner {
        window.dialog_name = inner.name.clone();
    }
    window.dialog_text = cache.get(window_key, dialog_key).map(<[String]>::to_vec).unwrap_or_default();

    let fg_target = Target::Window(fg.hwnd);
    let target = lookup.inner.as_ref().map_or(&fg_target, |inner| &inner.target);
    let tree = dialog::read_tree(uia, target, started);
    if tree.hung {
        *hung = Some(key);
    }
    answer(window, tree.nodes, tree.outcome)
}
