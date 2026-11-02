//! The dialog inside the foreground window that holds the focus (ADR 0018, I2, roadmap 9.6c3): a
//! property sheet's page, Notepad's "save changes" prompt, a XAML `ContentDialog`. The window of a
//! reading stays the foreground window; this finds the dialog within it, if any.
//!
//! - Win32 chain first: `GetGUIThreadInfo(0).hwndFocus`, used only when its root is the
//!   foreground window; then `GA_PARENT` upward (at most [`CHAIN_MAX`] steps) and the nearest
//!   window with a dialog class is the inner dialog. Local calls only.
//! - Then, only when the chain found none and the focused element's framework is XAML or WinUI,
//!   one `NormalizeElementBuildCache` from the focused element with an `IsDialog == true` walker.
//!   A failure that is not a timeout is "no inner dialog" (STATUS Finding 37); a timeout marks the
//!   foreground window hung, like any other call.
//! - The dialog's name is its window text (chain) or its UI Automation name (XAML); neither is a
//!   control's value, so the password rule has nothing to withhold.

use std::time::{Duration, Instant};

use windows::{
    Win32::{
        Foundation::HWND,
        System::{
            Com::SAFEARRAY,
            Ole::{SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound},
            Variant::{VARENUM, VARIANT, VT_ARRAY, VT_I4},
        },
        UI::{
            Accessibility::{IUIAutomation, IUIAutomationElement, IUIAutomationElement9, UIA_RuntimeIdPropertyId},
            WindowsAndMessaging::{
                GA_PARENT, GA_ROOT, GUITHREADINFO, GetAncestor, GetGUIThreadInfo,
            },
        },
    },
    core::Interface,
};

use super::{
    dialog::{Target, window_class},
    map,
    read::{Requests, UiaReading, window_title},
};
use crate::caret::UIA_TIMEOUT_MS;

/// The most `GA_PARENT` steps taken from the focus window. Starting value (ADR 0018).
const CHAIN_MAX: usize = 16;

/// A dialog inside the foreground window.
pub(super) struct Inner {
    /// Opaque, compared only for equality: the window handle, else a hash of the runtime id.
    pub key: u64,
    pub name: String,
    /// What the dialog walk starts from.
    pub target: Target,
    /// For the log: how it was found.
    pub via: &'static str,
}

/// What one lookup found and cost.
#[derive(Default)]
pub(super) struct Lookup {
    pub inner: Option<Inner>,
    /// The `IsDialog` lookup timed out: the foreground window counts as hung.
    pub hung: bool,
    pub chain: Duration,
    /// `None` when the lookup did not run.
    pub normalize: Option<Duration>,
}

/// Looks for the inner dialog of the foreground window `fg`. `focused` is the UI Automation
/// reading of the focused control, when there was one.
pub(super) fn find(
    uia: Option<(&IUIAutomation, &Requests)>,
    fg: HWND,
    focused: Option<&UiaReading>,
) -> Lookup {
    let started = Instant::now();
    let by_chain = chain(fg);
    let mut lookup = Lookup { chain: started.elapsed(), ..Lookup::default() };
    if let Some((hwnd, name)) = by_chain {
        lookup.inner = Some(Inner {
            key: hwnd.0 as usize as u64,
            name,
            target: Target::Window(hwnd),
            via: "chain",
        });
        return lookup;
    }
    // Only XAML content can hold a dialog that is no window.
    let (Some((_, requests)), Some(focused)) = (uia, focused) else {
        return lookup;
    };
    if !map::is_xaml_framework(&focused.framework) {
        return lookup;
    }
    let started = Instant::now();
    let found = normalize(requests, &focused.element, fg);
    lookup.normalize = Some(started.elapsed());
    match found {
        Ok(inner) => lookup.inner = inner,
        Err(Hung) => lookup.hung = true,
    }
    lookup
}

/// A call timed out.
struct Hung;

/// The nearest dialog-class window between the focus window and the foreground window, with its
/// name. `None` when the focus is not in the foreground window (a closing dialog, Alt+Tab's
/// "Task Switching") or the chain holds no dialog.
fn chain(fg: HWND) -> Option<(HWND, String)> {
    let mut info = GUITHREADINFO { cbSize: size_of::<GUITHREADINFO>() as u32, ..Default::default() };
    // Thread id 0: the foreground thread. It "may not return valid window handles" while a window
    // is losing activation (Microsoft), which then reads as no focus window.
    unsafe { GetGUIThreadInfo(0, &mut info) }.ok()?;
    let focus = info.hwndFocus;
    if focus.is_invalid() {
        return None;
    }
    let root = unsafe { GetAncestor(focus, GA_ROOT) };
    if !map::focus_in_foreground(root.0 as isize, fg.0 as isize) {
        return None;
    }
    let mut windows = Vec::new();
    let mut classes = Vec::new();
    let mut current = focus;
    while windows.len() < CHAIN_MAX && !current.is_invalid() && current != fg {
        windows.push(current);
        classes.push(window_class(current));
        current = unsafe { GetAncestor(current, GA_PARENT) };
    }
    let index = map::nearest_dialog(classes.iter().map(String::as_str))?;
    let hwnd = windows[index];
    Some((hwnd, window_title(hwnd)))
}

/// One `NormalizeElementBuildCache` from the focused element. `Ok(None)` is no inner dialog:
/// nothing matched (the call fails with a code that is not a timeout, Finding 37), the match is
/// not a dialog, or it is the foreground window itself.
fn normalize(requests: &Requests, focused: &IUIAutomationElement, fg: HWND) -> Result<Option<Inner>, Hung> {
    let Some((walker, request)) = &requests.dialog_ancestor else {
        return Ok(None);
    };
    let started = Instant::now();
    let element = match unsafe { walker.NormalizeElementBuildCache(focused, request) } {
        Ok(element) => element,
        Err(e) => {
            let elapsed = started.elapsed().as_millis() as u64;
            return if map::is_hung(e.code().0, elapsed, UIA_TIMEOUT_MS) { Err(Hung) } else { Ok(None) };
        }
    };
    // The docs say the root comes back when nothing matches; check the answer either way.
    let is_dialog = element
        .cast::<IUIAutomationElement9>()
        .ok()
        .and_then(|e| unsafe { e.CachedIsDialog() }.ok())
        .map(|b| b.as_bool());
    let hwnd = unsafe { element.CachedNativeWindowHandle() }.unwrap_or_default();
    if !map::normalized_is_inner(is_dialog, hwnd.0 as isize, fg.0 as isize) {
        return Ok(None);
    }
    let name = unsafe { element.CachedName() }.map(|n| n.to_string()).unwrap_or_default();
    let key = if hwnd.is_invalid() {
        runtime_id(&element).and_then(|id| map::runtime_id_key(&id)).unwrap_or_else(|| map::name_key(&name))
    } else {
        hwnd.0 as usize as u64
    };
    Ok(Some(Inner { key, name, target: Target::Element(element), via: "normalize" }))
}

/// The cached runtime id: a `SAFEARRAY` of `i32`. `None` when it is not there or not that shape.
fn runtime_id(element: &IUIAutomationElement) -> Option<Vec<i32>> {
    let variant = unsafe { element.GetCachedPropertyValue(UIA_RuntimeIdPropertyId) }.ok()?;
    int_array(&variant)
}

fn int_array(variant: &VARIANT) -> Option<Vec<i32>> {
    // SAFETY: the union field reads are the ones `vt` says are set.
    let (vt, array): (VARENUM, *mut SAFEARRAY) = unsafe {
        let inner = &*variant.Anonymous.Anonymous;
        (inner.vt, inner.Anonymous.parray)
    };
    if vt != VARENUM(VT_ARRAY.0 | VT_I4.0) || array.is_null() {
        return None;
    }
    let low = unsafe { SafeArrayGetLBound(array, 1) }.ok()?;
    let high = unsafe { SafeArrayGetUBound(array, 1) }.ok()?;
    let mut out = Vec::new();
    for i in low..=high {
        let mut value = 0i32;
        unsafe { SafeArrayGetElement(array, &i, (&mut value as *mut i32).cast()) }.ok()?;
        out.push(value);
    }
    Some(out)
}
