//! A dialog's text (ADR 0016, 0018, roadmap 9.6b2 and 9.6c3). This decides whether a window is a
//! dialog and, if so, walks its tree once with a budget and maps what it finds into plain
//! `DialogNode`s. The choice of text is `cv_core::focus::dialog_text`; the walk's rules are in
//! walk.rs and map.rs. Everything here runs on the focus thread.
//!
//! - A window is a dialog by class (`GetClassNameW`, a local call), else by UI Automation
//!   `IsDialog`. Asked once per (foreground window, inner dialog) pair; the caller keeps the
//!   answer. An inner dialog found by inner.rs is walked from its window or element and needs no
//!   check.
//! - The UI Automation walk reads children level by level with one cache request (name, type,
//!   focusable, offscreen, password, class, `IsDialog`) and never reads a value or a pattern. A
//!   node that is itself a dialog is kept but not entered.
//! - For a `#32770` where UI Automation finds no text (its Win32 proxy answers `Pane` for
//!   everything in an elevated app's dialog), MSAA's children replace the nodes. A `#32770` whose
//!   first UI Automation level is only panes goes to MSAA at once.
//! - The whole read has `DIALOG_MS`; past it the walk stops as incomplete and says nothing.
//! - A password node's name is emptied here, before the node leaves this file: "can't tell" counts
//!   as a password.

use std::time::{Duration, Instant};

use cv_core::{
    DialogNode, Role,
    focus::{DIALOG_MS, DIALOG_NODES, dialog_text},
};
use windows::{
    Win32::{
        Foundation::HWND,
        System::Variant::{VARIANT, VT_DISPATCH, VT_I4},
        UI::{
            Accessibility::{
                AccessibleChildren, AccessibleObjectFromWindow, IAccessible, IUIAutomation,
                IUIAutomationElement, IUIAutomationElement9, TreeScope_Children,
            },
            WindowsAndMessaging::{CHILDID_SELF, GetClassNameW, OBJID_CLIENT},
        },
    },
    core::Interface,
};

use super::{
    log_names, map,
    read::{Requests, variant_i4},
    walk::{Hung, Limits, Source, Walk, needs_msaa, walk},
};
use crate::caret::UIA_TIMEOUT_MS;

/// What to read as a dialog.
pub(super) enum Target {
    /// A window: the foreground window, or an inner dialog that is a window of its own.
    Window(HWND),
    /// An inner dialog that is no window (a XAML prompt), already known to say `IsDialog`.
    Element(IUIAutomationElement),
}

/// What reading a window as a dialog gave.
pub(super) struct Outcome {
    /// Empty when the window is not a dialog, says nothing, or could not be read.
    pub text: Vec<String>,
    /// A call timed out: the window counts as hung until the foreground changes.
    pub hung: bool,
}

/// Where the time of one dialog read went, for the log (ADR 0018, T2).
#[derive(Default)]
struct Timing {
    uia_root: Duration,
    uia_walk: Duration,
    uia_calls: usize,
    msaa_walk: Duration,
    msaa_calls: usize,
}

/// The window's class name, at most 255 characters.
pub(super) fn window_class(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..len])
}

/// Checks whether `target` is a dialog and, if it is, returns the text to speak for it.
pub(super) fn read_dialog(uia: Option<(&IUIAutomation, &Requests)>, target: &Target, title: &str) -> Outcome {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(DIALOG_MS);
    let limits = |panes_stop| Limits { deadline: Some(deadline), panes_stop };
    let (class, hwnd) = match target {
        Target::Window(hwnd) => (window_class(*hwnd), Some(*hwnd)),
        Target::Element(_) => (String::new(), None),
    };
    let by_class = map::is_dialog_class(&class);
    let mut timing = Timing::default();

    let mut walked: Option<(Walk, &str)> = None;
    if let Some((uia, requests)) = uia {
        let root_started = Instant::now();
        let root = uia_root(uia, requests, target, by_class);
        timing.uia_root = root_started.elapsed();
        match root {
            Ok(Some(root)) => {
                let walk_started = Instant::now();
                let result = walk(&mut UiaSource { requests }, &root, &limits(class == "#32770"));
                timing.uia_walk = walk_started.elapsed();
                match result {
                    Ok(w) => {
                        timing.uia_calls = w.calls;
                        walked = Some((w, "uia"));
                    }
                    Err(Hung) => return hung(&class, started),
                }
            }
            Ok(None) => {}
            Err(e) => {
                let elapsed = started.elapsed().as_millis() as u64;
                if map::is_hung(e.code().0, elapsed, UIA_TIMEOUT_MS) {
                    return hung(&class, started);
                }
            }
        }
    }
    // Without UI Automation a standard dialog still has MSAA. An inner dialog or a window found
    // to be one by UI Automation is a dialog too.
    let is_dialog = walked.is_some() || by_class || matches!(target, Target::Element(_));
    if !is_dialog {
        return Outcome { text: Vec::new(), hung: false };
    }

    let uia_nodes = walked.as_ref().map_or(&[][..], |(w, _)| &w.nodes[..]);
    if needs_msaa(&class, uia_nodes)
        && let Some(hwnd) = hwnd
        && let Some(root) = msaa_root(hwnd)
    {
        let msaa_started = Instant::now();
        let mut source = MsaaSource { deadline, late: false };
        let result = walk(&mut source, &root, &limits(false));
        timing.msaa_walk = msaa_started.elapsed();
        // MSAA has no timeout of ours to hit: a walk this slow is a program that is not answering.
        if result.is_err() || map::is_hung(0, timing.msaa_walk.as_millis() as u64, UIA_TIMEOUT_MS) {
            return hung(&class, started);
        }
        if let Ok(w) = result {
            timing.msaa_calls = w.calls;
            walked = Some((w, "msaa"));
        }
    }

    let Some((found, source)) = walked else {
        return Outcome { text: Vec::new(), hung: false };
    };
    let text = dialog_text(&found.nodes, found.complete, title);
    if log_names() {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        eprintln!(
            "[focus] dialog {:?} via {source}: uia root {:.0} ms, uia walk {:.0} ms ({} calls{}), \
             msaa walk {:.0} ms ({} calls), {} nodes, {:.0} ms in all, {}, text {text:?}",
            if class.is_empty() { "(element)" } else { &class },
            ms(timing.uia_root),
            ms(timing.uia_walk),
            timing.uia_calls,
            if found.panes_only { ", panes only" } else { "" },
            ms(timing.msaa_walk),
            timing.msaa_calls,
            found.nodes.len(),
            ms(started.elapsed()),
            if found.complete { "complete" } else { "incomplete" },
        );
    }
    Outcome { text, hung: false }
}

fn hung(class: &str, started: Instant) -> Outcome {
    eprintln!(
        "[focus] dialog {class:?} timed out after {} ms; skipped until the foreground changes",
        started.elapsed().as_millis(),
    );
    Outcome { text: Vec::new(), hung: true }
}

// ── UI Automation ────────────────────────────────────────────────────────────

/// The target's element, when it is a dialog: an inner dialog's own; a window's by class, else by
/// `IsDialog`. `Ok(None)` is a window that is not a dialog. An error is the call's own, so the
/// caller can tell a timeout.
fn uia_root(
    uia: &IUIAutomation,
    requests: &Requests,
    target: &Target,
    by_class: bool,
) -> windows::core::Result<Option<IUIAutomationElement>> {
    let hwnd = match target {
        Target::Element(element) => return Ok(Some(element.clone())),
        Target::Window(hwnd) => *hwnd,
    };
    let element = unsafe { uia.ElementFromHandleBuildCache(hwnd, &requests.dialog_check) }?;
    if by_class {
        return Ok(Some(element));
    }
    Ok(cached_is_dialog(&element).is_some_and(|dialog| dialog).then_some(element))
}

/// `IsDialog` is on the version 9 interface (Windows 10); a failed cast or read is `None`.
fn cached_is_dialog(element: &IUIAutomationElement) -> Option<bool> {
    element
        .cast::<IUIAutomationElement9>()
        .ok()
        .and_then(|e| unsafe { e.CachedIsDialog() }.ok())
        .map(|b| b.as_bool())
}

struct UiaSource<'a> {
    requests: &'a Requests,
}

impl Source for UiaSource<'_> {
    type Handle = IUIAutomationElement;

    fn children(
        &mut self,
        parent: &IUIAutomationElement,
    ) -> Result<Vec<(DialogNode, Option<IUIAutomationElement>)>, Hung> {
        let started = Instant::now();
        let found = unsafe {
            parent.FindAllBuildCache(TreeScope_Children, &self.requests.any, &self.requests.dialog_node)
        };
        let array = match found {
            Ok(array) => array,
            // No match comes back as an empty result; a refusal is no children; a timeout ends the
            // walk.
            Err(e) if map::is_hung(e.code().0, started.elapsed().as_millis() as u64, UIA_TIMEOUT_MS) => {
                return Err(Hung);
            }
            Err(_) => return Ok(Vec::new()),
        };
        let len = unsafe { array.Length() }.unwrap_or(0).clamp(0, DIALOG_NODES as i32);
        let mut out = Vec::new();
        for i in 0..len {
            if let Ok(element) = unsafe { array.GetElement(i) } {
                let (node, nested) = uia_node(&element);
                // A dialog inside the dialog (a property page) is kept, not entered.
                out.push((node, (!nested).then_some(element)));
            }
        }
        Ok(out)
    }
}

/// One cached element as a node, and whether it is itself a dialog. The name is not even read for
/// a password or when "can't tell"; an unreadable focusable flag counts as focusable, so a name is
/// never spoken by mistake.
fn uia_node(element: &IUIAutomationElement) -> (DialogNode, bool) {
    let password = unsafe { element.CachedIsPassword() }.ok().map(|b| b.as_bool());
    let name = if map::uia_name_withheld(password) {
        String::new()
    } else {
        unsafe { element.CachedName() }.map(|n| n.to_string()).unwrap_or_default()
    };
    let role = unsafe { element.CachedControlType() }.map(|t| map::uia_role(t.0)).unwrap_or(Role::Unknown);
    let mut node = DialogNode::new(name, role, 0);
    node.focusable = unsafe { element.CachedIsKeyboardFocusable() }.map(|b| b.as_bool()).unwrap_or(true);
    node.offscreen = unsafe { element.CachedIsOffscreen() }.map(|b| b.as_bool()).unwrap_or(false);
    let class = unsafe { element.CachedClassName() }.map(|c| c.to_string()).unwrap_or_default();
    (node, map::is_nested_dialog(&class, cached_is_dialog(element)))
}

// ── MSAA ─────────────────────────────────────────────────────────────────────

/// The window's client object. `None` when the window gives none.
fn msaa_root(hwnd: HWND) -> Option<IAccessible> {
    let mut raw = std::ptr::null_mut();
    unsafe { AccessibleObjectFromWindow(hwnd, OBJID_CLIENT.0 as u32, &IAccessible::IID, &mut raw) }
        .ok()?;
    // SAFETY: on success `raw` is an owned IAccessible pointer.
    (!raw.is_null()).then(|| unsafe { IAccessible::from_raw(raw) })
}

struct MsaaSource {
    /// `DIALOG_MS` after the dialog read started; the children after it are not read.
    deadline: Instant,
    late: bool,
}

impl Source for MsaaSource {
    type Handle = IAccessible;

    fn children(&mut self, parent: &IAccessible) -> Result<Vec<(DialogNode, Option<IAccessible>)>, Hung> {
        let count = unsafe { parent.accChildCount() }.unwrap_or(0).clamp(0, DIALOG_NODES as i32);
        if count == 0 {
            return Ok(Vec::new());
        }
        let mut children: Vec<VARIANT> = (0..count).map(|_| VARIANT::default()).collect();
        let mut obtained = 0;
        if unsafe { AccessibleChildren(parent, 0, &mut children, &mut obtained) }.is_err() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        // Dropping the VARIANTs releases the IDispatch pointers (Microsoft: the caller releases).
        // Each child costs a few cross-process calls (Finding 34), so the time is checked per
        // child.
        for variant in children.iter().take(obtained.max(0) as usize) {
            if Instant::now() >= self.deadline {
                self.late = true;
                break;
            }
            if let Some(child) = msaa_child(parent, variant) {
                out.push(child);
            }
        }
        Ok(out)
    }

    fn out_of_time(&self) -> bool {
        self.late
    }
}

/// One child as a node, with its object when it has one and is not itself a dialog (a child id is
/// a simple element that cannot have children). `None` when it is invisible or unavailable.
fn msaa_child(parent: &IAccessible, variant: &VARIANT) -> Option<(DialogNode, Option<IAccessible>)> {
    // SAFETY: the union field read is the one `vt` says is set.
    let inner = unsafe { &*variant.Anonymous.Anonymous };
    let (acc, id, object) = if inner.vt == VT_DISPATCH {
        let dispatch = unsafe { (*inner.Anonymous.pdispVal).as_ref() }?;
        (dispatch.cast::<IAccessible>().ok()?, VARIANT::from(CHILDID_SELF as i32), true)
    } else if inner.vt == VT_I4 {
        (parent.clone(), VARIANT::from(unsafe { inner.Anonymous.lVal }), false)
    } else {
        return None;
    };

    let state = unsafe { acc.get_accState(&id) }.ok().and_then(|v| variant_i4(&v)).map(|s| s as u32);
    if map::msaa_dialog_skipped(state) {
        return None;
    }
    let raw_role = unsafe { acc.get_accRole(&id) }.ok().and_then(|v| variant_i4(&v)).map(|r| r as u32);
    let role = raw_role.map(map::msaa_role).unwrap_or_default();
    let name = if map::msaa_name_withheld(state) {
        String::new()
    } else {
        unsafe { acc.get_accName(&id) }.map(|n| n.to_string()).unwrap_or_default()
    };
    let mut node = DialogNode::new(name, role, 0);
    node.focusable = state.is_none_or(map::msaa_focusable);
    node.offscreen = state.is_some_and(map::msaa_offscreen);
    let nested = raw_role.is_some_and(map::msaa_nested_dialog);
    Some((node, (object && !nested).then_some(acc)))
}
