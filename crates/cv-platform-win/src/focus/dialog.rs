//! A dialog's text (ADR 0016, roadmap 9.6b2). When a window comes to the front, this decides
//! whether it is a dialog and, if so, walks its tree once with a budget and maps what it finds into
//! plain `DialogNode`s. The choice of text is `cv_core::focus::dialog_text`; the walk's rules are
//! in walk.rs and map.rs. Everything here runs on the focus thread.
//!
//! - A window is a dialog by class (`GetClassNameW`, a local call), else by UI Automation
//!   `IsDialog`. Asked once per foreground window; the caller keeps the answer.
//! - The UI Automation walk reads children level by level with one cache request (name, type,
//!   focusable, offscreen, password) and never reads a value or a pattern.
//! - For a `#32770` where UI Automation finds no text (its Win32 proxy answers `Pane` for
//!   everything in an elevated app's dialog), MSAA's children replace the nodes.
//! - A password node's name is emptied here, before the node leaves this file: "can't tell" counts
//!   as a password.

use std::time::Instant;

use cv_core::{DialogNode, Role, focus::{DIALOG_NODES, dialog_text}};
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
    walk::{Hung, Source, Walk, needs_msaa, walk},
};
use crate::caret::UIA_TIMEOUT_MS;

/// What reading a window as a dialog gave.
pub(super) struct Outcome {
    /// Empty when the window is not a dialog, says nothing, or could not be read.
    pub text: Vec<String>,
    /// A call timed out: the window counts as hung until the foreground changes.
    pub hung: bool,
}

/// The window's class name, at most 255 characters.
fn window_class(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..len])
}

/// Checks whether `hwnd` is a dialog and, if it is, returns the text to speak for it.
pub(super) fn read_dialog(uia: Option<(&IUIAutomation, &Requests)>, hwnd: HWND, title: &str) -> Outcome {
    let started = Instant::now();
    let class = window_class(hwnd);
    let by_class = map::is_dialog_class(&class);

    let mut walked: Option<(Walk, &str)> = None;
    if let Some((uia, requests)) = uia {
        match uia_root(uia, requests, hwnd, by_class) {
            Ok(Some(root)) => match walk(&mut UiaSource { requests }, &root) {
                Ok(w) => walked = Some((w, "uia")),
                Err(Hung) => return hung(&class, started),
            },
            Ok(None) => {}
            Err(e) => {
                let elapsed = started.elapsed().as_millis() as u64;
                if map::is_hung(e.code().0, elapsed, UIA_TIMEOUT_MS) {
                    return hung(&class, started);
                }
            }
        }
    }
    // Without UI Automation a standard dialog still has MSAA.
    let is_dialog = walked.is_some() || by_class;
    if !is_dialog {
        return Outcome { text: Vec::new(), hung: false };
    }

    let uia_nodes = walked.as_ref().map_or(&[][..], |(w, _)| &w.nodes[..]);
    if needs_msaa(&class, uia_nodes)
        && let Some(root) = msaa_root(hwnd)
    {
        let msaa_started = Instant::now();
        let result = walk(&mut MsaaSource, &root);
        // MSAA has no timeout of ours to hit: a walk this slow is a program that is not answering.
        if result.is_err() || map::is_hung(0, msaa_started.elapsed().as_millis() as u64, UIA_TIMEOUT_MS) {
            return hung(&class, started);
        }
        walked = result.ok().map(|w| (w, "msaa"));
    }

    let Some((found, source)) = walked else {
        return Outcome { text: Vec::new(), hung: false };
    };
    let text = dialog_text(&found.nodes, found.complete, title);
    if log_names() {
        eprintln!(
            "[focus] dialog {class:?} via {source}: {} calls, {} nodes, {} ms, {}, text {text:?}",
            found.calls,
            found.nodes.len(),
            started.elapsed().as_millis(),
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

/// The window's element, when the window is a dialog: by class, else by `IsDialog`. `Ok(None)` is
/// a window that is not a dialog. An error is the call's own, so the caller can tell a timeout.
fn uia_root(
    uia: &IUIAutomation,
    requests: &Requests,
    hwnd: HWND,
    by_class: bool,
) -> windows::core::Result<Option<IUIAutomationElement>> {
    let element = unsafe { uia.ElementFromHandleBuildCache(hwnd, &requests.dialog_check) }?;
    if by_class {
        return Ok(Some(element));
    }
    // `IsDialog` is on the version 9 interface (Windows 10); a failed cast or read is "no".
    let is_dialog = element
        .cast::<IUIAutomationElement9>()
        .ok()
        .and_then(|e| unsafe { e.CachedIsDialog() }.ok())
        .is_some_and(|b| b.as_bool());
    Ok(is_dialog.then_some(element))
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
                out.push((uia_node(&element), Some(element)));
            }
        }
        Ok(out)
    }
}

/// One cached element as a node. The name is not even read for a password or when "can't tell";
/// an unreadable focusable flag counts as focusable, so a name is never spoken by mistake.
fn uia_node(element: &IUIAutomationElement) -> DialogNode {
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
    node
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

struct MsaaSource;

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
        for variant in children.iter().take(obtained.max(0) as usize) {
            if let Some(child) = msaa_child(parent, variant) {
                out.push(child);
            }
        }
        Ok(out)
    }
}

/// One child as a node, with its object when it has one (a child id is a simple element that
/// cannot have children). `None` when it is invisible or unavailable.
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
    let role = unsafe { acc.get_accRole(&id) }
        .ok()
        .and_then(|v| variant_i4(&v))
        .map(|r| map::msaa_role(r as u32))
        .unwrap_or_default();
    let name = if map::msaa_name_withheld(state) {
        String::new()
    } else {
        unsafe { acc.get_accName(&id) }.map(|n| n.to_string()).unwrap_or_default()
    };
    let mut node = DialogNode::new(name, role, 0);
    node.focusable = state.is_none_or(map::msaa_focusable);
    node.offscreen = state.is_some_and(map::msaa_offscreen);
    Some((node, object.then_some(acc)))
}
