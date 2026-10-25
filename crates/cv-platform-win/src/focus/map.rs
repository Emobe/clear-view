//! The pure half of the focus thread (ADR 0015): the mapping from each source's own ids to the
//! neutral core types, and the decisions that need no Windows call. Unit-tested here because the
//! ids are Windows values, so the mapping stays in the backend.

use cv_core::{
    Checked, Password, Role, States,
    focus::VALUE_MAX,
};
use windows::Win32::UI::Accessibility::{
    ROLE_SYSTEM_APPLICATION, ROLE_SYSTEM_BUTTONDROPDOWN, ROLE_SYSTEM_BUTTONDROPDOWNGRID,
    ROLE_SYSTEM_BUTTONMENU, ROLE_SYSTEM_CELL, ROLE_SYSTEM_CHECKBUTTON, ROLE_SYSTEM_CLIENT,
    ROLE_SYSTEM_COLUMNHEADER, ROLE_SYSTEM_COMBOBOX, ROLE_SYSTEM_DIALOG, ROLE_SYSTEM_DOCUMENT,
    ROLE_SYSTEM_DROPLIST, ROLE_SYSTEM_GRAPHIC, ROLE_SYSTEM_GROUPING, ROLE_SYSTEM_HOTKEYFIELD,
    ROLE_SYSTEM_IPADDRESS, ROLE_SYSTEM_LINK, ROLE_SYSTEM_LIST, ROLE_SYSTEM_LISTITEM,
    ROLE_SYSTEM_MENUBAR, ROLE_SYSTEM_MENUITEM, ROLE_SYSTEM_MENUPOPUP, ROLE_SYSTEM_OUTLINE,
    ROLE_SYSTEM_OUTLINEITEM, ROLE_SYSTEM_PAGETAB, ROLE_SYSTEM_PAGETABLIST, ROLE_SYSTEM_PANE,
    ROLE_SYSTEM_PROGRESSBAR, ROLE_SYSTEM_PROPERTYPAGE, ROLE_SYSTEM_PUSHBUTTON,
    ROLE_SYSTEM_RADIOBUTTON, ROLE_SYSTEM_ROW, ROLE_SYSTEM_ROWHEADER, ROLE_SYSTEM_SCROLLBAR,
    ROLE_SYSTEM_SEPARATOR, ROLE_SYSTEM_SLIDER, ROLE_SYSTEM_SPINBUTTON, ROLE_SYSTEM_SPLITBUTTON,
    ROLE_SYSTEM_STATICTEXT, ROLE_SYSTEM_STATUSBAR, ROLE_SYSTEM_TABLE, ROLE_SYSTEM_TEXT,
    ROLE_SYSTEM_TITLEBAR, ROLE_SYSTEM_TOOLBAR, ROLE_SYSTEM_TOOLTIP, ROLE_SYSTEM_WINDOW,
};

// MSAA state bits (`STATE_SYSTEM_*`, oleacc.h).
const STATE_UNAVAILABLE: u32 = 0x1;
const STATE_SELECTED: u32 = 0x2;
const STATE_CHECKED: u32 = 0x10;
/// Also called `INDETERMINATE`.
const STATE_MIXED: u32 = 0x20;
const STATE_EXPANDED: u32 = 0x200;
const STATE_COLLAPSED: u32 = 0x400;
const STATE_PROTECTED: u32 = 0x2000_0000;

/// `STATE_SYSTEM_PROTECTED` is set: MSAA's way of saying "password".
pub(super) fn msaa_protected(state: u32) -> bool {
    state & STATE_PROTECTED != 0
}

/// A UI Automation control type id (50000 to 50040) as a neutral role.
pub(super) fn uia_role(control_type: i32) -> Role {
    match control_type {
        50000 => Role::Button,
        50001 => Role::Calendar,
        50002 => Role::CheckBox,
        50003 => Role::ComboBox,
        50004 => Role::Edit,
        50005 => Role::Link,
        50006 => Role::Image,
        50007 => Role::ListItem,
        50008 => Role::List,
        50009 => Role::Menu,
        50010 => Role::MenuBar,
        50011 => Role::MenuItem,
        50012 => Role::ProgressBar,
        50013 => Role::RadioButton,
        50014 => Role::ScrollBar,
        50015 => Role::Slider,
        50016 => Role::Spinner,
        50017 => Role::StatusBar,
        50018 => Role::Tab,
        50019 => Role::TabItem,
        50020 => Role::Text,
        50021 => Role::ToolBar,
        50022 => Role::ToolTip,
        50023 => Role::Tree,
        50024 => Role::TreeItem,
        50025 => Role::Custom,
        50026 => Role::Group,
        50027 => Role::Thumb,
        // A data grid is a table; its items are rows or cells.
        50028 => Role::Table,
        50029 => Role::ListItem,
        50030 => Role::Document,
        50031 => Role::SplitButton,
        50032 => Role::Window,
        50033 => Role::Pane,
        50034 => Role::Header,
        50035 => Role::HeaderItem,
        50036 => Role::Table,
        50037 => Role::TitleBar,
        50038 => Role::Separator,
        50039 => Role::Custom,
        50040 => Role::AppBar,
        _ => Role::Unknown,
    }
}

/// An MSAA `ROLE_SYSTEM_*` value as a neutral role. `0` and values outside the table are
/// `Unknown` or `Custom`.
pub(super) fn msaa_role(role: u32) -> Role {
    match role {
        0 => Role::Unknown,
        ROLE_SYSTEM_TITLEBAR => Role::TitleBar,
        ROLE_SYSTEM_MENUBAR => Role::MenuBar,
        ROLE_SYSTEM_SCROLLBAR => Role::ScrollBar,
        ROLE_SYSTEM_WINDOW | ROLE_SYSTEM_APPLICATION => Role::Window,
        // The client area of a window: the same "says nothing" role as a pane (Notepad's text
        // area reads as `client`).
        ROLE_SYSTEM_CLIENT | ROLE_SYSTEM_PANE | ROLE_SYSTEM_PROPERTYPAGE => Role::Pane,
        ROLE_SYSTEM_MENUPOPUP => Role::Menu,
        ROLE_SYSTEM_MENUITEM => Role::MenuItem,
        ROLE_SYSTEM_TOOLTIP => Role::ToolTip,
        ROLE_SYSTEM_DOCUMENT => Role::Document,
        ROLE_SYSTEM_DIALOG => Role::Dialog,
        ROLE_SYSTEM_GROUPING => Role::Group,
        ROLE_SYSTEM_SEPARATOR => Role::Separator,
        ROLE_SYSTEM_TOOLBAR => Role::ToolBar,
        ROLE_SYSTEM_STATUSBAR => Role::StatusBar,
        ROLE_SYSTEM_TABLE => Role::Table,
        ROLE_SYSTEM_COLUMNHEADER | ROLE_SYSTEM_ROWHEADER => Role::HeaderItem,
        ROLE_SYSTEM_ROW | ROLE_SYSTEM_CELL | ROLE_SYSTEM_LISTITEM => Role::ListItem,
        ROLE_SYSTEM_LINK => Role::Link,
        ROLE_SYSTEM_LIST => Role::List,
        ROLE_SYSTEM_OUTLINE => Role::Tree,
        ROLE_SYSTEM_OUTLINEITEM => Role::TreeItem,
        ROLE_SYSTEM_PAGETAB => Role::TabItem,
        ROLE_SYSTEM_PAGETABLIST => Role::Tab,
        ROLE_SYSTEM_GRAPHIC => Role::Image,
        ROLE_SYSTEM_STATICTEXT => Role::Text,
        ROLE_SYSTEM_TEXT | ROLE_SYSTEM_HOTKEYFIELD | ROLE_SYSTEM_IPADDRESS => Role::Edit,
        ROLE_SYSTEM_PUSHBUTTON => Role::Button,
        ROLE_SYSTEM_CHECKBUTTON => Role::CheckBox,
        ROLE_SYSTEM_RADIOBUTTON => Role::RadioButton,
        ROLE_SYSTEM_COMBOBOX | ROLE_SYSTEM_DROPLIST => Role::ComboBox,
        ROLE_SYSTEM_PROGRESSBAR => Role::ProgressBar,
        ROLE_SYSTEM_SLIDER => Role::Slider,
        ROLE_SYSTEM_SPINBUTTON => Role::Spinner,
        ROLE_SYSTEM_SPLITBUTTON
        | ROLE_SYSTEM_BUTTONDROPDOWN
        | ROLE_SYSTEM_BUTTONMENU
        | ROLE_SYSTEM_BUTTONDROPDOWNGRID => Role::SplitButton,
        _ => Role::Custom,
    }
}

/// An MSAA state mask as neutral states. `None` (the state could not be read) leaves everything
/// unsaid, with the password flag at "can't tell".
///
/// MSAA says `checked`, `selected` and `expanded` only when they hold, so those fields are
/// `Some` only then (Finding 25(j)): MSAA alone never says "unchecked". `enabled` is the absence
/// of `UNAVAILABLE`, which MSAA does state both ways.
pub(super) fn msaa_states(state: Option<u32>) -> States {
    let mut states = States::default();
    let Some(s) = state else {
        return states;
    };
    states.enabled = Some(s & STATE_UNAVAILABLE == 0);
    states.checked = if s & STATE_CHECKED != 0 {
        Some(Checked::On)
    } else if s & STATE_MIXED != 0 {
        Some(Checked::Mixed)
    } else {
        None
    };
    states.expanded = if s & STATE_EXPANDED != 0 {
        Some(true)
    } else if s & STATE_COLLAPSED != 0 {
        Some(false)
    } else {
        None
    };
    states.selected = (s & STATE_SELECTED != 0).then_some(true);
    states.password = if msaa_protected(s) { Password::Yes } else { Password::No };
    states
}

/// UI Automation's `IsPassword`: `None` when the property could not be read.
pub(super) fn uia_password(is_password: Option<bool>) -> Password {
    match is_password {
        Some(true) => Password::Yes,
        Some(false) => Password::No,
        None => Password::CantTell,
    }
}

/// `ToggleState`: 0 off, 1 on, 2 indeterminate.
pub(super) fn checked_from_toggle(state: i32) -> Option<Checked> {
    match state {
        0 => Some(Checked::Off),
        1 => Some(Checked::On),
        2 => Some(Checked::Mixed),
        _ => None,
    }
}

/// `ExpandCollapseState`: 0 collapsed, 1 expanded, 2 partly expanded, 3 leaf. A leaf has nothing
/// to expand, so it says nothing; a partly expanded node counts as expanded.
pub(super) fn expanded_from_state(state: i32) -> Option<bool> {
    match state {
        0 => Some(false),
        1 | 2 => Some(true),
        _ => None,
    }
}

/// A RangeValue number as text: a whole number without a decimal point, otherwise up to four
/// decimals with trailing zeros removed. `None` for NaN and infinity.
pub(super) fn number_text(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return Some(format!("{}", value as i64));
    }
    let text = format!("{value:.4}");
    Some(text.trim_end_matches('0').trim_end_matches('.').to_string())
}

/// Cuts a value at [`VALUE_MAX`] characters before it leaves the thread.
pub(super) fn cut_value(text: &str) -> String {
    text.chars().take(VALUE_MAX).collect()
}

/// Whether a UI Automation failure means the app is not answering. The error is
/// `UIA_E_TIMEOUT`, or the call took at least twice the lowered timeout (a connection timeout and
/// a transaction timeout back to back).
pub(super) fn is_hung(code: i32, elapsed_ms: u64, timeout_ms: u32) -> bool {
    code == windows::Win32::UI::Accessibility::UIA_E_TIMEOUT as i32
        || elapsed_ms >= 2 * u64::from(timeout_ms)
}

/// What one wake of the focus thread does with the foreground window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Plan {
    /// Speech is off: read nothing.
    Off,
    /// The foreground window is ours: no UI Automation or MSAA call (the settings panel would
    /// send `WM_GETOBJECT` to the main thread, which at exit waits for this one).
    Own,
    /// A call into this window timed out and the foreground has not changed since: skip both
    /// sources.
    Hung,
    Read,
}

pub(super) fn read_plan(
    wanted: bool,
    fg_pid: u32,
    own_pid: u32,
    fg_hwnd: isize,
    hung: Option<isize>,
) -> Plan {
    if !wanted {
        Plan::Off
    } else if fg_pid == own_pid {
        Plan::Own
    } else if hung == Some(fg_hwnd) {
        Plan::Hung
    } else {
        Plan::Read
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Accessibility::{
        UIA_AppBarControlTypeId, UIA_ButtonControlTypeId, UIA_EditControlTypeId,
        UIA_PaneControlTypeId,
    };

    #[test]
    fn uia_control_type_literals_match_the_windows_ids() {
        assert_eq!(UIA_ButtonControlTypeId.0, 50000);
        assert_eq!(UIA_EditControlTypeId.0, 50004);
        assert_eq!(UIA_PaneControlTypeId.0, 50033);
        assert_eq!(UIA_AppBarControlTypeId.0, 50040);
    }

    #[test]
    fn uia_types_map_to_roles() {
        assert_eq!(uia_role(50000), Role::Button);
        assert_eq!(uia_role(50004), Role::Edit);
        assert_eq!(uia_role(50033), Role::Pane);
        assert_eq!(uia_role(50025), Role::Custom);
        assert_eq!(uia_role(50028), Role::Table);
        assert_eq!(uia_role(50029), Role::ListItem);
        assert_eq!(uia_role(50040), Role::AppBar);
    }

    #[test]
    fn every_uia_type_in_the_documented_range_has_a_role() {
        for id in 50000..=50040 {
            assert_ne!(uia_role(id), Role::Unknown, "control type {id}");
        }
    }

    #[test]
    fn ids_outside_the_uia_range_are_unknown() {
        assert_eq!(uia_role(0), Role::Unknown);
        assert_eq!(uia_role(49999), Role::Unknown);
        assert_eq!(uia_role(50041), Role::Unknown);
        assert_eq!(uia_role(-1), Role::Unknown);
    }

    #[test]
    fn msaa_roles_map_to_roles() {
        assert_eq!(msaa_role(ROLE_SYSTEM_PUSHBUTTON), Role::Button);
        assert_eq!(msaa_role(ROLE_SYSTEM_CHECKBUTTON), Role::CheckBox);
        assert_eq!(msaa_role(ROLE_SYSTEM_TEXT), Role::Edit);
        assert_eq!(msaa_role(ROLE_SYSTEM_STATICTEXT), Role::Text);
        assert_eq!(msaa_role(ROLE_SYSTEM_LISTITEM), Role::ListItem);
        assert_eq!(msaa_role(ROLE_SYSTEM_DIALOG), Role::Dialog);
        assert_eq!(msaa_role(ROLE_SYSTEM_SPLITBUTTON), Role::SplitButton);
    }

    #[test]
    fn the_msaa_client_area_says_nothing_like_a_pane() {
        assert!(msaa_role(ROLE_SYSTEM_CLIENT).is_empty());
        assert!(msaa_role(ROLE_SYSTEM_PANE).is_empty());
    }

    #[test]
    fn msaa_role_zero_is_unknown_and_other_values_are_custom() {
        assert_eq!(msaa_role(0), Role::Unknown);
        assert_eq!(msaa_role(9999), Role::Custom);
    }

    #[test]
    fn msaa_states_say_checked_only_when_ticked() {
        let ticked = msaa_states(Some(STATE_CHECKED));
        assert_eq!(ticked.checked, Some(Checked::On));
        assert_eq!(msaa_states(Some(0)).checked, None);
        assert_eq!(msaa_states(Some(STATE_MIXED)).checked, Some(Checked::Mixed));
    }

    #[test]
    fn msaa_enabled_is_the_absence_of_unavailable() {
        assert_eq!(msaa_states(Some(0)).enabled, Some(true));
        assert_eq!(msaa_states(Some(STATE_UNAVAILABLE)).enabled, Some(false));
    }

    #[test]
    fn msaa_expanded_and_selected_are_said_only_when_stated() {
        let s = msaa_states(Some(STATE_EXPANDED | STATE_SELECTED));
        assert_eq!((s.expanded, s.selected), (Some(true), Some(true)));
        assert_eq!(msaa_states(Some(STATE_COLLAPSED)).expanded, Some(false));
        let none = msaa_states(Some(0));
        assert_eq!((none.expanded, none.selected), (None, None));
    }

    #[test]
    fn msaa_protected_is_a_password_a_readable_state_without_it_is_not() {
        assert_eq!(msaa_states(Some(STATE_PROTECTED)).password, Password::Yes);
        assert_eq!(msaa_states(Some(STATE_PROTECTED | STATE_CHECKED)).password, Password::Yes);
        assert_eq!(msaa_states(Some(0)).password, Password::No);
        assert!(msaa_protected(STATE_PROTECTED));
        assert!(!msaa_protected(STATE_CHECKED));
    }

    #[test]
    fn an_unreadable_msaa_state_can_not_tell() {
        let s = msaa_states(None);
        assert_eq!(s.password, Password::CantTell);
        assert_eq!((s.enabled, s.checked, s.expanded, s.selected), (None, None, None, None));
    }

    #[test]
    fn uia_password_property_maps_and_unreadable_can_not_tell() {
        assert_eq!(uia_password(Some(true)), Password::Yes);
        assert_eq!(uia_password(Some(false)), Password::No);
        assert_eq!(uia_password(None), Password::CantTell);
    }

    #[test]
    fn toggle_states_map_to_checked() {
        assert_eq!(checked_from_toggle(0), Some(Checked::Off));
        assert_eq!(checked_from_toggle(1), Some(Checked::On));
        assert_eq!(checked_from_toggle(2), Some(Checked::Mixed));
        assert_eq!(checked_from_toggle(9), None);
    }

    #[test]
    fn expand_collapse_states_map_to_expanded() {
        assert_eq!(expanded_from_state(0), Some(false));
        assert_eq!(expanded_from_state(1), Some(true));
        assert_eq!(expanded_from_state(2), Some(true));
        assert_eq!(expanded_from_state(3), None);
    }

    #[test]
    fn numbers_read_without_noise() {
        assert_eq!(number_text(40.0).as_deref(), Some("40"));
        assert_eq!(number_text(-3.0).as_deref(), Some("-3"));
        assert_eq!(number_text(0.5).as_deref(), Some("0.5"));
        assert_eq!(number_text(2.0 / 3.0).as_deref(), Some("0.6667"));
        assert_eq!(number_text(f64::NAN), None);
        assert_eq!(number_text(f64::INFINITY), None);
    }

    #[test]
    fn a_value_is_cut_at_the_cap_by_characters() {
        let long = "é".repeat(VALUE_MAX + 10);
        assert_eq!(cut_value(&long).chars().count(), VALUE_MAX);
        assert_eq!(cut_value("short"), "short");
    }

    #[test]
    fn a_timeout_or_a_very_slow_failure_is_a_hung_app() {
        let timeout = windows::Win32::UI::Accessibility::UIA_E_TIMEOUT as i32;
        assert!(is_hung(timeout, 3, 500));
        assert!(is_hung(-1, 1000, 500));
        assert!(!is_hung(-1, 999, 500));
        // Task Manager: access denied, and quick.
        assert!(!is_hung(0x8007_0005_u32 as i32, 4, 500));
    }

    #[test]
    fn the_read_plan_checks_speech_then_our_own_process_then_hung() {
        assert_eq!(read_plan(false, 1, 9, 5, None), Plan::Off);
        assert_eq!(read_plan(false, 9, 9, 5, Some(5)), Plan::Off);
        assert_eq!(read_plan(true, 9, 9, 5, None), Plan::Own);
        assert_eq!(read_plan(true, 1, 9, 5, Some(5)), Plan::Hung);
        assert_eq!(read_plan(true, 1, 9, 5, Some(6)), Plan::Read);
        assert_eq!(read_plan(true, 1, 9, 5, None), Plan::Read);
    }
}
