//! One read of the focused control from each source (ADR 0015). UI Automation answers for the
//! control that has focus now, MSAA for the control the newest WinEvent named. Each is mapped
//! into the neutral `Control` by the functions in map.rs. The 9.1 probe
//! (examples/focus_probe.rs) is where the calls started.
//!
//! The password decision comes before any value: the cache request that finds the element holds
//! no value property, and a value is fetched (UI Automation) or read (MSAA) only after
//! `cv_core::focus::value_allowed` says so.

use cv_core::{Control, FocusSource, ScreenRect, Value};
use windows::{
    Win32::{
        Foundation::HWND,
        System::Variant::{VARIANT, VT_I4},
        UI::{
            Accessibility::{
                AccessibleObjectFromEvent, IAccessible, IUIAutomation, IUIAutomationCacheRequest,
                IUIAutomationCondition, IUIAutomationElement, IUIAutomationExpandCollapsePattern,
                IUIAutomationRangeValuePattern, IUIAutomationSelectionItemPattern,
                IUIAutomationTogglePattern, IUIAutomationValuePattern, UIA_BoundingRectanglePropertyId,
                UIA_ControlTypePropertyId, UIA_ExpandCollapseExpandCollapseStatePropertyId,
                UIA_ExpandCollapsePatternId, UIA_FrameworkIdPropertyId, UIA_IsDialogPropertyId,
                UIA_IsEnabledPropertyId, UIA_IsKeyboardFocusablePropertyId, UIA_IsOffscreenPropertyId,
                UIA_IsPasswordPropertyId, UIA_NamePropertyId, UIA_PATTERN_ID, UIA_PROPERTY_ID,
                UIA_RangeValuePatternId, UIA_RangeValueValuePropertyId,
                UIA_SelectionItemIsSelectedPropertyId, UIA_SelectionItemPatternId,
                UIA_ToggleToggleStatePropertyId, UIA_TogglePatternId, UIA_ValuePatternId,
                UIA_ValueValuePropertyId,
            },
            WindowsAndMessaging::GetWindowTextW,
        },
    },
    core::Result,
};

use super::map;

/// What a foreground or focus WinEvent named, kept from the callback for the MSAA read.
#[derive(Clone, Copy)]
pub(super) struct Stored {
    pub hwnd: HWND,
    pub id_object: i32,
    pub id_child: i32,
}

/// The two cache requests: the one that finds the focused element (no value in it) and the one
/// that fetches a value once the password decision allows it.
pub(super) struct Requests {
    state: IUIAutomationCacheRequest,
    value: IUIAutomationCacheRequest,
    /// A dialog walk's node: name, type, focusable, offscreen, password. No value, no pattern
    /// (ADR 0016).
    pub dialog_node: IUIAutomationCacheRequest,
    /// The window element's `IsDialog`, for a window whose class is not a dialog class.
    pub dialog_check: IUIAutomationCacheRequest,
    /// Matches every child.
    pub any: IUIAutomationCondition,
}

impl Requests {
    pub fn new(uia: &IUIAutomation) -> Result<Self> {
        let state = request(
            uia,
            &[
                UIA_NamePropertyId,
                UIA_ControlTypePropertyId,
                UIA_IsPasswordPropertyId,
                UIA_IsEnabledPropertyId,
                UIA_BoundingRectanglePropertyId,
                UIA_FrameworkIdPropertyId,
                UIA_ToggleToggleStatePropertyId,
                UIA_ExpandCollapseExpandCollapseStatePropertyId,
                UIA_SelectionItemIsSelectedPropertyId,
            ],
            &[UIA_TogglePatternId, UIA_ExpandCollapsePatternId, UIA_SelectionItemPatternId],
        )?;
        let value = request(
            uia,
            &[UIA_ValueValuePropertyId, UIA_RangeValueValuePropertyId],
            &[UIA_ValuePatternId, UIA_RangeValuePatternId],
        )?;
        let dialog_node = request(
            uia,
            &[
                UIA_NamePropertyId,
                UIA_ControlTypePropertyId,
                UIA_IsKeyboardFocusablePropertyId,
                UIA_IsOffscreenPropertyId,
                UIA_IsPasswordPropertyId,
            ],
            &[],
        )?;
        let dialog_check = request(uia, &[UIA_IsDialogPropertyId], &[])?;
        let any = unsafe { uia.CreateTrueCondition() }?;
        Ok(Self { state, value, dialog_node, dialog_check, any })
    }
}

fn request(
    uia: &IUIAutomation,
    properties: &[UIA_PROPERTY_ID],
    patterns: &[UIA_PATTERN_ID],
) -> Result<IUIAutomationCacheRequest> {
    let request = unsafe { uia.CreateCacheRequest() }?;
    for property in properties {
        unsafe { request.AddProperty(*property) }?;
    }
    for pattern in patterns {
        unsafe { request.AddPattern(*pattern) }?;
    }
    Ok(request)
}

/// The window's title, at most 256 characters.
pub(super) fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..len])
}

/// A screen `RECT` as a `ScreenRect`; `None` when it has no area at all.
fn screen_rect(left: i32, top: i32, right: i32, bottom: i32) -> Option<ScreenRect> {
    let width = (right - left).max(0) as u32;
    let height = (bottom - top).max(0) as u32;
    (width > 0 || height > 0).then_some(ScreenRect { left, top, width, height })
}

// ── UI Automation ────────────────────────────────────────────────────────────

pub(super) struct UiaReading {
    /// The cached fields; the value is not read yet.
    pub control: Control,
    /// `IsPassword`; `None` when the property could not be read.
    pub password: Option<bool>,
    /// For the log.
    pub framework: String,
    element: IUIAutomationElement,
}

/// The focused element, with one cross-process call. An error is the call's own, so the caller
/// can tell a timeout from a refusal.
pub(super) fn uia_read(uia: &IUIAutomation, requests: &Requests) -> Result<UiaReading> {
    let element = unsafe { uia.GetFocusedElementBuildCache(&requests.state) }?;

    let password = unsafe { element.CachedIsPassword() }.ok().map(|b| b.as_bool());
    let name = unsafe { element.CachedName() }.map(|n| n.to_string()).unwrap_or_default();
    let role = unsafe { element.CachedControlType() }
        .map(|t| map::uia_role(t.0))
        .unwrap_or_default();
    let framework = unsafe { element.CachedFrameworkId() }.map(|f| f.to_string()).unwrap_or_default();

    let mut control = Control::new(name, role, FocusSource::Uia);
    control.states.password = map::uia_password(password);
    control.states.enabled = unsafe { element.CachedIsEnabled() }.ok().map(|b| b.as_bool());
    control.rect = unsafe { element.CachedBoundingRectangle() }
        .ok()
        .and_then(|rc| screen_rect(rc.left, rc.top, rc.right, rc.bottom));
    if let Ok(p) =
        unsafe { element.GetCachedPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) }
        && let Ok(state) = unsafe { p.CachedToggleState() }
    {
        control.states.checked = map::checked_from_toggle(state.0);
    }
    if let Ok(p) = unsafe {
        element.GetCachedPatternAs::<IUIAutomationExpandCollapsePattern>(UIA_ExpandCollapsePatternId)
    } && let Ok(state) = unsafe { p.CachedExpandCollapseState() }
    {
        control.states.expanded = map::expanded_from_state(state.0);
    }
    if let Ok(p) = unsafe {
        element.GetCachedPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)
    } && let Ok(selected) = unsafe { p.CachedIsSelected() }
    {
        control.states.selected = Some(selected.as_bool());
    }
    Ok(UiaReading { control, password, framework, element })
}

/// The Value pattern's string, else the RangeValue pattern's number, from a fresh cache. Never
/// the Text pattern (Finding 25(h)). Only called when `value_allowed` allowed it. An error is
/// the call's own; `Ok(None)` is a control with no value.
pub(super) fn uia_value(reading: &UiaReading, requests: &Requests) -> Result<Option<String>> {
    let element = unsafe { reading.element.BuildUpdatedCache(&requests.value) }?;
    if let Ok(p) =
        unsafe { element.GetCachedPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
    {
        let text = unsafe { p.CachedValue() }?.to_string();
        return Ok((!text.is_empty()).then(|| map::cut_value(&text)));
    }
    if let Ok(p) =
        unsafe { element.GetCachedPatternAs::<IUIAutomationRangeValuePattern>(UIA_RangeValuePatternId) }
    {
        let number = unsafe { p.CachedValue() }?;
        return Ok(map::number_text(number));
    }
    Ok(None)
}

// ── MSAA ─────────────────────────────────────────────────────────────────────

pub(super) struct MsaaReading {
    pub control: Control,
    /// The `PROTECTED` state bit; `None` when the state could not be read.
    pub protected: Option<bool>,
    acc: IAccessible,
    child: VARIANT,
}

/// The object the WinEvent named. `None` when it is gone or gives nothing: the events of a
/// burst are mostly transient objects (Finding 25(c)).
pub(super) fn msaa_read(event: &Stored) -> Option<MsaaReading> {
    let mut acc: Option<IAccessible> = None;
    let mut child = VARIANT::default();
    unsafe {
        AccessibleObjectFromEvent(
            event.hwnd,
            event.id_object as u32,
            event.id_child as u32,
            &mut acc,
            &mut child,
        )
    }
    .ok()?;
    let acc = acc?;

    let state = unsafe { acc.get_accState(&child) }
        .ok()
        .and_then(|v| variant_i4(&v))
        .map(|s| s as u32);
    let role = unsafe { acc.get_accRole(&child) }
        .ok()
        .and_then(|v| variant_i4(&v))
        .map(|r| map::msaa_role(r as u32))
        .unwrap_or_default();
    let name = unsafe { acc.get_accName(&child) }.map(|n| n.to_string()).unwrap_or_default();

    let mut control = Control::new(name, role, FocusSource::Msaa);
    control.states = map::msaa_states(state);
    let (mut left, mut top, mut width, mut height) = (0, 0, 0, 0);
    if unsafe { acc.accLocation(&mut left, &mut top, &mut width, &mut height, &child) }.is_ok() {
        control.rect = screen_rect(left, top, left + width, top + height);
    }
    let protected = state.map(map::msaa_protected);
    Some(MsaaReading { control, protected, acc, child })
}

/// `accValue`, cut. Only called when `value_allowed` allowed it.
pub(super) fn msaa_value(reading: &MsaaReading) -> Option<String> {
    let text = unsafe { reading.acc.get_accValue(&reading.child) }.ok()?.to_string();
    (!text.is_empty()).then(|| map::cut_value(&text))
}

pub(super) fn variant_i4(v: &VARIANT) -> Option<i32> {
    // SAFETY: the union field read is the one `vt` says is set.
    unsafe {
        let inner = &*v.Anonymous.Anonymous;
        if inner.vt == VT_I4 { Some(inner.Anonymous.lVal) } else { None }
    }
}

/// A value is `Text` when there is one, `None` when the control has none. (`Withheld` is set by
/// the caller when the password rule says so.)
pub(super) fn value_of(text: Option<String>) -> Value {
    match text {
        Some(t) => Value::Text(t),
        None => Value::None,
    }
}
