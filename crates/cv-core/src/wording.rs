//! What is spoken for the control that has focus (roadmap 9.5, ADR 0015). Pure logic with no
//! Windows types: [`describe_control`] turns a [`Control`] into one phrase at a [`Verbosity`],
//! [`describe_window`] does the same for a window title (roadmap 9.6a), and [`focus_utterances`]
//! wraps both for the speech queue.
//!
//! - The phrase is name, role, state, value, joined by commas (the voice pauses at each).
//! - A name that is really the control's text (Notepad's document, the Save As file name box) is
//!   not read: only the role is (STATUS Finding 27(b)).
//! - A value is spoken only when the control is known not to be a password. "Can't tell" and
//!   `Value::Withheld` say nothing, not even that a value exists.
//! - A control with no name, no value and a role that says nothing is not spoken (the Alt+Tab pane,
//!   Finding 27(c)). Positions and per-role wording are 9.7's.
//! - When the foreground window changed, the title is spoken first, at every verbosity level, then
//!   the dialog's text (roadmap 9.6b1, ADR 0016) as one utterance, then the control. A title or a
//!   piece of dialog text equal to the control's name is said once, by the control. A window with
//!   no title says nothing.
//!
//! The numbers ([`NAME_MAX`], [`VALUE_SPOKEN_MAX`], [`TITLE_MAX`], [`DIALOG_SPOKEN_MAX`]) are
//! starting values.

use serde::{Deserialize, Serialize};

use crate::focus::{Checked, Control, Password, Role, Value, WindowInfo, clean_name};
use crate::speech::{Kind, Utterance};

/// A name longer than this is cut at a word (or, for a text box or document, taken to be the text
/// itself and not read).
pub const NAME_MAX: usize = 80;
/// A value is cut at a word near this many characters before it is spoken.
pub const VALUE_SPOKEN_MAX: usize = 100;
/// A window title is cut at a word near this many characters (browser tabs run long).
pub const TITLE_MAX: usize = 80;
/// A dialog's text is cut at a word near this many characters (ADR 0016). Starting value.
pub const DIALOG_SPOKEN_MAX: usize = 300;

/// How much is said about a focused control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Verbosity {
    /// The name, whether it is checked or unavailable, and the value. The role only when there
    /// is no name.
    Brief,
    /// Adds the role (not for labels or list items that have a name) and expanded or collapsed.
    #[default]
    Normal,
    /// Adds every role and "selected".
    Full,
}

impl Verbosity {
    pub const ALL: [Verbosity; 3] = [Self::Brief, Self::Normal, Self::Full];

    /// The panel's label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Brief => "Brief",
            Self::Normal => "Normal",
            Self::Full => "Full",
        }
    }
}

/// The utterances for a focus event, in the order they are spoken: when the foreground window
/// changed, the window title, the inner dialog's name and the dialog's text; when only the inner
/// dialog changed, its name and the dialog's text; then the control. All are `Kind::Focus`, so the
/// queue speaks them as one batch and the control does not cut the title off (ADR 0013, ADR 0015,
/// ADR 0016, ADR 0018). Any may be missing; the list is empty when there is nothing to say.
pub fn focus_utterances(
    window: &WindowInfo,
    window_changed: bool,
    dialog_changed: bool,
    control: Option<&Control>,
    verbosity: Verbosity,
) -> Vec<Utterance> {
    let phrase = control.map(|c| describe_control(c, verbosity)).unwrap_or_default();
    let mut title = if window_changed { describe_window(&window.title) } else { String::new() };
    // The name the control speaks, if any. A name that is not spoken (a document's text) is none.
    let control_name = control
        .and_then(|c| spoken_name(&c.name, is_text_role(c.role)).0)
        .map(|name| name.to_lowercase());
    // A title that is the name the control speaks is said once, by the control, whose phrase has
    // the role ("Save As, dialog").
    if !title.is_empty() && control_name.as_deref() == Some(title.to_lowercase().as_str()) {
        title.clear();
    }
    let (dialog_name, dialog) = if window_changed || dialog_changed {
        (
            describe_dialog_name(&window.dialog_name, &window.title, control_name.as_deref()),
            describe_dialog_text(&window.dialog_text, control_name.as_deref()),
        )
    } else {
        (String::new(), String::new())
    };
    [title, dialog_name, dialog, phrase]
        .into_iter()
        .filter(|text| !text.is_empty())
        .map(|text| Utterance::new(Kind::Focus, text))
        .collect()
}

/// The window title as it is spoken, or an empty string for a window with no title. Said at every
/// verbosity level. A title is not cleaned of `&` as a control name is: it is not a Win32 label.
pub fn describe_window(title: &str) -> String {
    cut_at_word(&flatten(title), TITLE_MAX)
}

/// An inner dialog's name as it is spoken (ADR 0018), or an empty string: flattened and cut at a
/// word near [`TITLE_MAX`]. Dropped when the window title contains it, ignoring case (Notepad's
/// prompt is named "Notepad" inside "Untitled - Notepad"), and when it is the name the control
/// speaks (`control_name`, lowercase): the control says it. Said at every verbosity level.
pub fn describe_dialog_name(name: &str, title: &str, control_name: Option<&str>) -> String {
    let name = flatten(name);
    let lower = name.to_lowercase();
    if name.is_empty()
        || flatten(title).to_lowercase().contains(&lower)
        || control_name == Some(lower.as_str())
    {
        return String::new();
    }
    cut_at_word(&name, TITLE_MAX)
}

/// A dialog's text as one utterance, or an empty string. The pieces are joined with ". " (a plain
/// space after one that already ends in a stop), flattened, and cut at a word near
/// [`DIALOG_SPOKEN_MAX`]. A piece equal to the name the control speaks (`control_name`, lowercase)
/// is left out: the control says it. Said at every verbosity level, like the title (ADR 0016).
pub fn describe_dialog_text(pieces: &[String], control_name: Option<&str>) -> String {
    let mut out = String::new();
    for piece in pieces {
        let piece = flatten(piece);
        if piece.is_empty() || control_name == Some(piece.to_lowercase().as_str()) {
            continue;
        }
        if !out.is_empty() {
            out.push_str(if out.ends_with(['.', '!', '?', ':']) { " " } else { ". " });
        }
        out.push_str(&piece);
    }
    cut_at_word(&out, DIALOG_SPOKEN_MAX)
}

/// One phrase for a control, or an empty string when there is nothing worth saying.
pub fn describe_control(c: &Control, verbosity: Verbosity) -> String {
    let password = c.states.password == Password::Yes;
    let value = spoken_value(c);
    let (name, name_was_text) = spoken_name(&c.name, is_text_role(c.role));
    // A name equal to the value is said once.
    let value = match (&name, value) {
        (Some(n), Some(v)) if *n == v => None,
        (_, v) => v,
    };

    let role = role_word(c.role, password, verbosity, name.is_some())
        // A long name that was dropped as text still tells you where you are.
        .or(if name_was_text && c.role.is_empty() { Some("text") } else { None });

    if name.is_none() && role.is_none() && value.is_none() {
        return String::new();
    }

    let mut parts: Vec<&str> = Vec::new();
    if let Some(n) = &name {
        parts.push(n);
    }
    if let Some(r) = role {
        parts.push(r);
    }
    match c.states.checked {
        Some(Checked::On) => parts.push("checked"),
        Some(Checked::Off) => parts.push("not checked"),
        Some(Checked::Mixed) => parts.push("partially checked"),
        None => {}
    }
    if verbosity != Verbosity::Brief {
        match c.states.expanded {
            Some(true) => parts.push("expanded"),
            Some(false) => parts.push("collapsed"),
            None => {}
        }
    }
    if verbosity == Verbosity::Full && c.states.selected == Some(true) {
        parts.push("selected");
    }
    if c.states.enabled == Some(false) {
        parts.push("unavailable");
    }
    if let Some(v) = &value {
        parts.push(v);
    }
    parts.join(", ")
}

/// Roles whose name may be the control's own text, not a label (Finding 27(b)).
fn is_text_role(role: Role) -> bool {
    matches!(role, Role::Edit | Role::Document) || role.is_empty()
}

/// The name to say, and whether a long name was dropped because it is the control's text.
fn spoken_name(raw: &str, text_role: bool) -> (Option<String>, bool) {
    let cleaned = clean_name(raw);
    let mut lines = cleaned.lines().map(flatten).filter(|l| !l.is_empty());
    let Some(first) = lines.next() else {
        return (None, false);
    };
    let more_lines = lines.next().is_some();
    if text_role {
        if more_lines || first.chars().count() > NAME_MAX {
            return (None, true);
        }
        return (Some(first), false);
    }
    (Some(cut_at_word(&first, NAME_MAX)), false)
}

/// The value to say: only text, only when the control is known not to be a password.
fn spoken_value(c: &Control) -> Option<String> {
    if c.states.password != Password::No {
        return None;
    }
    let Value::Text(text) = &c.value else {
        return None;
    };
    let flat = flatten(text);
    if flat.is_empty() { None } else { Some(cut_at_word(&flat, VALUE_SPOKEN_MAX)) }
}

/// The role as a spoken word, if this level says it.
fn role_word(role: Role, password: bool, verbosity: Verbosity, named: bool) -> Option<&'static str> {
    // A password field is always marked; the word says nothing about what is typed in it.
    if role == Role::Edit && password {
        return Some("password edit");
    }
    let word = word_for(role)?;
    let say = match verbosity {
        Verbosity::Brief => !named,
        Verbosity::Normal => !named || !matches!(role, Role::Text | Role::ListItem),
        Verbosity::Full => true,
    };
    say.then_some(word)
}

/// `None` for the roles that say nothing.
fn word_for(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::Unknown | Role::Pane | Role::Custom => return None,
        Role::Window => "window",
        Role::Dialog => "dialog",
        Role::Button => "button",
        Role::SplitButton => "split button",
        Role::CheckBox => "check box",
        Role::RadioButton => "radio button",
        Role::Edit => "edit",
        Role::ComboBox => "combo box",
        Role::List => "list",
        Role::ListItem => "list item",
        Role::Menu => "menu",
        Role::MenuBar => "menu bar",
        Role::MenuItem => "menu item",
        Role::Tab => "tab control",
        Role::TabItem => "tab",
        Role::Tree => "tree",
        Role::TreeItem => "tree item",
        Role::Table => "table",
        Role::Header => "header",
        Role::HeaderItem => "column header",
        Role::Slider => "slider",
        Role::Spinner => "spin box",
        Role::ScrollBar => "scroll bar",
        Role::Thumb => "thumb",
        Role::ProgressBar => "progress bar",
        Role::Text => "text",
        Role::Link => "link",
        Role::Image => "image",
        Role::Document => "document",
        Role::Group => "group",
        Role::ToolBar => "tool bar",
        Role::StatusBar => "status bar",
        Role::TitleBar => "title bar",
        Role::ToolTip => "tool tip",
        Role::Separator => "separator",
        Role::Calendar => "calendar",
        Role::AppBar => "app bar",
    })
}

/// Whitespace runs (including line breaks) become single spaces.
fn flatten(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// At most `max` characters, ending at the last whole word when the cut falls inside one.
fn cut_at_word(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let end = s.char_indices().nth(max).map_or(s.len(), |(i, _)| i);
    let head = &s[..end];
    let cut = if s[end..].starts_with(char::is_whitespace) {
        head
    } else {
        head.rsplit_once(char::is_whitespace).map_or(head, |(words, _)| words)
    };
    cut.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::{FocusSource, States};

    fn control(name: &str, role: Role) -> Control {
        let mut c = Control::new(name, role, FocusSource::Uia);
        c.states.password = Password::No;
        c
    }

    fn say(c: &Control) -> String {
        describe_control(c, Verbosity::Normal)
    }

    fn at(c: &Control, v: Verbosity) -> String {
        describe_control(c, v)
    }

    #[test]
    fn a_button_is_name_then_role() {
        assert_eq!(say(&control("Save", Role::Button)), "Save, button");
    }

    #[test]
    fn the_accelerator_marker_is_dropped() {
        assert_eq!(say(&control("&Save", Role::Button)), "Save, button");
        assert_eq!(say(&control("Fish && Chips", Role::Button)), "Fish & Chips, button");
    }

    #[test]
    fn check_states_are_spoken_at_every_level() {
        for (state, word) in [
            (Checked::On, "checked"),
            (Checked::Off, "not checked"),
            (Checked::Mixed, "partially checked"),
        ] {
            let mut c = control("Remember me", Role::CheckBox);
            c.states.checked = Some(state);
            assert_eq!(at(&c, Verbosity::Normal), format!("Remember me, check box, {word}"));
            assert_eq!(at(&c, Verbosity::Brief), format!("Remember me, {word}"));
            assert_eq!(at(&c, Verbosity::Full), format!("Remember me, check box, {word}"));
        }
    }

    #[test]
    fn an_edit_says_its_value_after_its_role() {
        let mut c = control("Search", Role::Edit);
        c.value = Value::Text("hello world".into());
        assert_eq!(say(&c), "Search, edit, hello world");
        assert_eq!(at(&c, Verbosity::Brief), "Search, hello world");
    }

    #[test]
    fn the_levels_differ_on_the_same_control() {
        let mut c = control("Appearance", Role::TreeItem);
        c.states.expanded = Some(false);
        c.states.selected = Some(true);
        assert_eq!(at(&c, Verbosity::Brief), "Appearance");
        assert_eq!(at(&c, Verbosity::Normal), "Appearance, tree item, collapsed");
        assert_eq!(at(&c, Verbosity::Full), "Appearance, tree item, collapsed, selected");
    }

    #[test]
    fn labels_and_list_items_with_a_name_skip_the_role_at_normal() {
        assert_eq!(say(&control("Downloads", Role::ListItem)), "Downloads");
        assert_eq!(say(&control("Total", Role::Text)), "Total");
        assert_eq!(at(&control("Downloads", Role::ListItem), Verbosity::Full), "Downloads, list item");
    }

    #[test]
    fn a_control_with_no_name_gives_its_role_at_every_level() {
        for v in Verbosity::ALL {
            assert_eq!(at(&control("", Role::ListItem), v), "list item", "{v:?}");
            assert_eq!(at(&control("  ", Role::Button), v), "button", "{v:?}");
        }
    }

    #[test]
    fn unavailable_is_spoken_at_every_level() {
        let mut c = control("Save", Role::Button);
        c.states.enabled = Some(false);
        assert_eq!(at(&c, Verbosity::Brief), "Save, unavailable");
        assert_eq!(at(&c, Verbosity::Normal), "Save, button, unavailable");
        c.states.enabled = Some(true);
        assert_eq!(at(&c, Verbosity::Normal), "Save, button");
    }

    #[test]
    fn a_nameless_pane_is_not_spoken() {
        // The Alt+Tab switcher (Finding 27(c)).
        for role in [Role::Pane, Role::Custom, Role::Unknown] {
            for v in Verbosity::ALL {
                assert_eq!(at(&control("", role), v), "", "{role:?} {v:?}");
            }
        }
    }

    #[test]
    fn a_nameless_pane_with_only_a_state_is_not_spoken() {
        let mut c = control("", Role::Pane);
        c.states.checked = Some(Checked::On);
        assert_eq!(say(&c), "");
    }

    #[test]
    fn a_named_pane_says_its_name_and_no_role() {
        assert_eq!(say(&control("Taskbar", Role::Pane)), "Taskbar");
    }

    fn window(title: &str) -> WindowInfo {
        WindowInfo::new(title)
    }

    fn focus(title: &str, changed: bool, c: Option<&Control>, v: Verbosity) -> Vec<Utterance> {
        focus_utterances(&window(title), changed, false, c, v)
    }

    fn said(items: &[Utterance]) -> Vec<&str> {
        items.iter().map(|u| u.text.as_str()).collect()
    }

    #[test]
    fn no_control_and_no_window_change_says_nothing() {
        assert!(focus("Notepad", false, None, Verbosity::Normal).is_empty());
    }

    #[test]
    fn an_utterance_is_a_focus_kind() {
        let c = control("OK", Role::Button);
        assert_eq!(
            focus("Notepad", false, Some(&c), Verbosity::Normal),
            vec![Utterance::new(Kind::Focus, "OK, button")]
        );
        assert!(focus("Notepad", false, Some(&control("", Role::Pane)), Verbosity::Normal).is_empty());
    }

    // ── The window title (9.6a) ──

    #[test]
    fn a_new_window_says_its_title_then_the_control() {
        let c = control("Save", Role::Button);
        let items = focus("Untitled - Notepad", true, Some(&c), Verbosity::Normal);
        assert_eq!(said(&items), ["Untitled - Notepad", "Save, button"]);
        assert!(items.iter().all(|u| u.kind == Kind::Focus));
    }

    #[test]
    fn a_new_window_with_no_control_says_its_title_alone() {
        let items = focus("Task Manager", true, None, Verbosity::Normal);
        assert_eq!(said(&items), ["Task Manager"]);
    }

    #[test]
    fn the_title_is_not_repeated_when_the_window_did_not_change() {
        let c = control("Save", Role::Button);
        let items = focus("Untitled - Notepad", false, Some(&c), Verbosity::Normal);
        assert_eq!(said(&items), ["Save, button"]);
    }

    #[test]
    fn the_title_is_spoken_at_every_verbosity_level() {
        let c = control("Save", Role::Button);
        for v in Verbosity::ALL {
            assert_eq!(focus("Notepad", true, Some(&c), v)[0].text, "Notepad", "{v:?}");
            assert_eq!(said(&focus("Notepad", true, None, v)), ["Notepad"], "{v:?}");
        }
    }

    #[test]
    fn a_nameless_pane_after_a_title_leaves_the_title_alone() {
        // The Alt+Tab pane (Finding 27(c)).
        let pane = control("", Role::Pane);
        assert_eq!(said(&focus("Notepad", true, Some(&pane), Verbosity::Normal)), ["Notepad"]);
    }

    #[test]
    fn a_window_with_no_title_says_nothing_for_it() {
        let c = control("Save", Role::Button);
        for title in ["", "  ", "\n\t"] {
            assert_eq!(said(&focus(title, true, Some(&c), Verbosity::Normal)), ["Save, button"]);
            assert!(focus(title, true, None, Verbosity::Normal).is_empty(), "{title:?}");
        }
    }

    #[test]
    fn a_title_equal_to_the_control_name_is_said_once_by_the_control() {
        let c = control("Save As", Role::Dialog);
        let items = focus("Save As", true, Some(&c), Verbosity::Normal);
        assert_eq!(said(&items), ["Save As, dialog"]);
        // Accelerators, case and spacing do not make a difference.
        let c = control("&Save  as", Role::Dialog);
        assert_eq!(said(&focus("SAVE AS", true, Some(&c), Verbosity::Normal)), ["Save as, dialog"]);
    }

    #[test]
    fn a_title_that_only_overlaps_the_control_name_is_kept() {
        let c = control("Save", Role::Button);
        assert_eq!(said(&focus("Save As", true, Some(&c), Verbosity::Normal)), ["Save As", "Save, button"]);
    }

    #[test]
    fn a_title_equal_to_a_name_the_control_does_not_speak_is_kept() {
        // A pane whose long name is dropped as text: nothing but "text" is said, so the title stays.
        let name = "x ".repeat(100);
        let c = control(&name, Role::Pane);
        let items = focus(name.trim(), true, Some(&c), Verbosity::Normal);
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].text, "text");
        // And when the control says nothing at all, the title is not dropped for matching it.
        let c = control("", Role::Pane);
        assert_eq!(said(&focus("", true, Some(&c), Verbosity::Normal)), Vec::<&str>::new());
    }

    #[test]
    fn a_title_is_flattened_and_cut_at_a_word() {
        assert_eq!(describe_window("Report\n  final\tdraft"), "Report final draft");
        let title = "alpha beta ".repeat(20);
        let spoken = describe_window(&title);
        assert!(spoken.chars().count() <= TITLE_MAX, "{spoken}");
        assert!(spoken.ends_with("beta") || spoken.ends_with("alpha"), "{spoken}");
        assert!(title.starts_with(&spoken));
        assert_eq!(describe_window(&"a".repeat(300)).len(), TITLE_MAX);
    }

    #[test]
    fn a_title_keeps_its_ampersands() {
        assert_eq!(describe_window("Fish & Chips - Brave"), "Fish & Chips - Brave");
    }

    #[test]
    fn a_hung_window_title_is_spoken_whole() {
        let items = focus("Steam (Not Responding)", true, None, Verbosity::Normal);
        assert_eq!(said(&items), ["Steam (Not Responding)"]);
    }

    #[test]
    fn a_password_control_after_a_title_is_still_never_read() {
        let mut c = control("Password", Role::Edit);
        c.states.password = Password::Yes;
        c.value = Value::Text("hunter2".into());
        let items = focus("Sign in", true, Some(&c), Verbosity::Full);
        assert_eq!(said(&items), ["Sign in", "Password, password edit"]);
    }

    // ── The dialog text (9.6b1, ADR 0016) ──

    fn dialog(title: &str, pieces: &[&str]) -> WindowInfo {
        let mut w = WindowInfo::new(title);
        w.dialog_text = pieces.iter().map(|p| p.to_string()).collect();
        w
    }

    fn focus_in(
        w: &WindowInfo,
        changed: bool,
        c: Option<&Control>,
        v: Verbosity,
    ) -> Vec<Utterance> {
        focus_utterances(w, changed, false, c, v)
    }

    #[test]
    fn a_new_dialog_says_title_then_text_then_control() {
        let w = dialog("Notepad", &["Do you want to save changes to Untitled?"]);
        let c = control("Save", Role::Button);
        let items = focus_in(&w, true, Some(&c), Verbosity::Normal);
        assert_eq!(said(&items), ["Notepad", "Do you want to save changes to Untitled?", "Save, button"]);
        assert!(items.iter().all(|u| u.kind == Kind::Focus));
    }

    #[test]
    fn the_dialog_text_is_spoken_at_every_verbosity_level() {
        let w = dialog("Notepad", &["Save changes?"]);
        let c = control("Save", Role::Button);
        for v in Verbosity::ALL {
            assert_eq!(said(&focus_in(&w, true, Some(&c), v))[1], "Save changes?", "{v:?}");
        }
    }

    #[test]
    fn the_dialog_text_is_not_repeated_when_the_window_did_not_change() {
        let w = dialog("Notepad", &["Save changes?"]);
        let c = control("Cancel", Role::Button);
        assert_eq!(said(&focus_in(&w, false, Some(&c), Verbosity::Normal)), ["Cancel, button"]);
        assert!(focus_in(&w, false, None, Verbosity::Normal).is_empty());
    }

    #[test]
    fn the_dialog_text_is_spoken_alone_when_there_is_no_control() {
        let w = dialog("", &["Save changes?"]);
        assert_eq!(said(&focus_in(&w, true, None, Verbosity::Normal)), ["Save changes?"]);
    }

    #[test]
    fn the_dialog_text_is_one_utterance_joined_with_stops() {
        let w = dialog("", &["Replace file?", "It already exists", "Really"]);
        assert_eq!(
            said(&focus_in(&w, true, None, Verbosity::Normal)),
            ["Replace file? It already exists. Really"]
        );
    }

    #[test]
    fn a_piece_equal_to_the_control_name_is_said_once_by_the_control() {
        let w = dialog("Notepad", &["Save", "Save changes?"]);
        let c = control("&SAVE", Role::Button);
        assert_eq!(
            said(&focus_in(&w, true, Some(&c), Verbosity::Normal)),
            ["Notepad", "Save changes?", "SAVE, button"]
        );
        // A name the control does not speak (a document's text) does not remove a piece.
        let name = "x ".repeat(100);
        let c = control(&name, Role::Pane);
        let w = dialog("", &[name.trim()]);
        assert_eq!(focus_in(&w, true, Some(&c), Verbosity::Normal).len(), 2);
    }

    #[test]
    fn the_dialog_text_is_flattened_and_cut_at_a_word() {
        assert_eq!(describe_dialog_text(&["Line one\r\n line   two".into()], None), "Line one line two");
        let long = "alpha beta ".repeat(60);
        let spoken = describe_dialog_text(std::slice::from_ref(&long), None);
        assert!(spoken.chars().count() <= DIALOG_SPOKEN_MAX, "{}", spoken.len());
        assert!(spoken.ends_with("beta") || spoken.ends_with("alpha"), "{spoken}");
        assert!(long.starts_with(&spoken));
        assert!(spoken.chars().count() > DIALOG_SPOKEN_MAX - 12);
    }

    #[test]
    fn empty_dialog_text_says_nothing() {
        assert_eq!(describe_dialog_text(&[], None), "");
        assert_eq!(describe_dialog_text(&["  ".into(), "".into()], None), "");
        let w = dialog("Notepad", &[" "]);
        assert_eq!(said(&focus_in(&w, true, None, Verbosity::Normal)), ["Notepad"]);
    }

    #[test]
    fn a_password_control_after_dialog_text_is_still_never_read() {
        let mut c = control("Password", Role::Edit);
        c.states.password = Password::Yes;
        c.value = Value::Text("hunter2".into());
        let w = dialog("Sign in", &["Enter your password"]);
        let items = focus_in(&w, true, Some(&c), Verbosity::Full);
        assert_eq!(said(&items), ["Sign in", "Enter your password", "Password, password edit"]);
    }

    // ── The inner dialog (9.6c2, ADR 0018) ──

    fn inner(title: &str, name: &str, pieces: &[&str]) -> WindowInfo {
        let mut w = dialog(title, pieces);
        w.dialog_name = name.into();
        w
    }

    fn focus_flags(
        w: &WindowInfo,
        window_changed: bool,
        dialog_changed: bool,
        c: Option<&Control>,
    ) -> Vec<Utterance> {
        focus_utterances(w, window_changed, dialog_changed, c, Verbosity::Normal)
    }

    #[test]
    fn a_new_window_says_title_dialog_name_text_then_control() {
        let w = inner("CLAUDE.md Properties", "General", &["Type of file:", "Markdown Source File"]);
        let c = control("OK", Role::Button);
        let items = focus_flags(&w, true, false, Some(&c));
        assert_eq!(
            said(&items),
            ["CLAUDE.md Properties", "General", "Type of file: Markdown Source File", "OK, button"]
        );
        assert!(items.iter().all(|u| u.kind == Kind::Focus));
    }

    #[test]
    fn a_new_dialog_in_the_same_window_says_name_text_then_control() {
        let w = inner("CLAUDE.md Properties", "Security", &["Object name:", "D:\\CLAUDE.md"]);
        let c = control("Edit", Role::Button);
        assert_eq!(
            said(&focus_flags(&w, false, true, Some(&c))),
            ["Security", "Object name: D:\\CLAUDE.md", "Edit, button"]
        );
    }

    #[test]
    fn no_change_says_only_the_control() {
        let w = inner("CLAUDE.md Properties", "General", &["Type of file:"]);
        let c = control("OK", Role::Button);
        assert_eq!(said(&focus_flags(&w, false, false, Some(&c))), ["OK, button"]);
    }

    #[test]
    fn the_dialog_name_and_text_are_spoken_at_every_verbosity_level() {
        let w = inner("a Properties", "General", &["Location:"]);
        let c = control("OK", Role::Button);
        for v in Verbosity::ALL {
            let items = focus_utterances(&w, false, true, Some(&c), v);
            assert_eq!(said(&items)[..2], ["General", "Location:"], "{v:?}");
        }
    }

    #[test]
    fn a_dialog_name_the_title_contains_is_dropped() {
        let w = inner("Untitled - Notepad", "NOTEPAD", &["Do you want to save changes to Untitled?"]);
        let c = control("Save", Role::Button);
        assert_eq!(
            said(&focus_flags(&w, false, true, Some(&c))),
            ["Do you want to save changes to Untitled?", "Save, button"]
        );
        assert_eq!(describe_dialog_name("Notepad", "Untitled - Notepad", None), "");
        assert_eq!(describe_dialog_name("General", "CLAUDE.md Properties", None), "General");
    }

    #[test]
    fn a_dialog_name_equal_to_the_control_name_is_said_once_by_the_control() {
        let w = inner("a Properties", "Details", &[]);
        let c = control("Details", Role::TabItem);
        let items = focus_flags(&w, false, true, Some(&c));
        assert_eq!(items.len(), 1);
        assert!(items[0].text.starts_with("Details"), "{}", items[0].text);
    }

    #[test]
    fn an_empty_dialog_name_says_nothing() {
        assert_eq!(describe_dialog_name("", "a Properties", None), "");
        assert_eq!(describe_dialog_name(" \r\n ", "a Properties", None), "");
        let w = inner("a Properties", "", &[]);
        let c = control("OK", Role::Button);
        assert_eq!(said(&focus_flags(&w, false, true, Some(&c))), ["OK, button"]);
    }

    #[test]
    fn a_dialog_name_is_flattened_and_cut_at_a_word() {
        assert_eq!(describe_dialog_name("Line\r\none", "", None), "Line one");
        let long = "alpha beta ".repeat(20);
        let spoken = describe_dialog_name(&long, "", None);
        assert!(spoken.chars().count() <= TITLE_MAX, "{}", spoken.len());
        assert!(long.starts_with(&spoken));
    }

    #[test]
    fn a_piece_ending_in_a_colon_is_joined_to_the_next_by_a_space() {
        let pieces =
            ["Type of file:".to_string(), "Text Document".into(), "Opens with:".into(), "Notepad".into()];
        assert_eq!(describe_dialog_text(&pieces, None), "Type of file: Text Document. Opens with: Notepad");
    }

    #[test]
    fn a_password_control_in_an_inner_dialog_is_still_never_read() {
        let mut c = control("Password", Role::Edit);
        c.states.password = Password::Yes;
        c.value = Value::Text("hunter2".into());
        let w = inner("Settings", "Sign in", &["Enter your password"]);
        let items = focus_utterances(&w, false, true, Some(&c), Verbosity::Full);
        assert_eq!(said(&items), ["Sign in", "Enter your password", "Password, password edit"]);
        assert!(items.iter().all(|u| !u.text.contains("hunter2")));
    }

    // ── Names that are the control's text (Finding 27(b)) ──

    #[test]
    fn a_long_name_on_an_edit_is_the_text_and_not_read() {
        let doc = "word ".repeat(40);
        let c = control(&doc, Role::Edit);
        assert_eq!(say(&c), "edit");
        let c = control(&doc, Role::Document);
        assert_eq!(say(&c), "document");
    }

    #[test]
    fn a_multi_line_name_on_an_edit_is_the_text_and_not_read() {
        let c = control("first line\nsecond line", Role::Edit);
        assert_eq!(say(&c), "edit");
    }

    #[test]
    fn a_long_name_on_a_pane_says_only_text() {
        // Notepad's document read as a pane.
        let c = control(&"x ".repeat(100), Role::Pane);
        assert_eq!(say(&c), "text");
        assert_eq!(at(&c, Verbosity::Brief), "text");
    }

    #[test]
    fn a_short_name_on_an_edit_is_read() {
        assert_eq!(say(&control("File name:", Role::Edit)), "File name:, edit");
    }

    #[test]
    fn a_long_name_on_other_roles_is_cut_at_a_word() {
        let name = "alpha beta ".repeat(20);
        let said = say(&control(&name, Role::ListItem));
        assert!(said.chars().count() <= NAME_MAX, "{said}");
        assert!(said.ends_with("beta") || said.ends_with("alpha"), "{said}");
        assert!(name.starts_with(&said));
    }

    #[test]
    fn a_multi_line_name_on_a_button_uses_the_first_line() {
        assert_eq!(say(&control("Inbox\n3 unread", Role::Button)), "Inbox, button");
    }

    #[test]
    fn a_name_equal_to_the_value_is_said_once() {
        let mut c = control("hello", Role::Edit);
        c.value = Value::Text("hello".into());
        assert_eq!(say(&c), "hello, edit");
    }

    // ── Values and passwords ──

    #[test]
    fn a_long_value_is_cut_at_a_word() {
        let mut c = control("Address", Role::Edit);
        c.value = Value::Text("https://example.com/ ".repeat(20));
        let said = say(&c);
        let value = said.strip_prefix("Address, edit, ").unwrap();
        assert!(value.chars().count() <= VALUE_SPOKEN_MAX, "{value}");
        assert!(value.ends_with("example.com/"), "{value}");
    }

    #[test]
    fn a_value_with_no_break_is_cut_hard() {
        let mut c = control("Address", Role::Edit);
        c.value = Value::Text("a".repeat(300));
        let said = say(&c);
        assert_eq!(said.len(), "Address, edit, ".len() + VALUE_SPOKEN_MAX);
    }

    #[test]
    fn a_value_is_flattened_to_one_line() {
        let mut c = control("Notes", Role::Edit);
        c.value = Value::Text("one\n\ntwo\tthree".into());
        assert_eq!(say(&c), "Notes, edit, one two three");
    }

    #[test]
    fn an_empty_value_says_nothing() {
        let mut c = control("Search", Role::Edit);
        c.value = Value::Text("  ".into());
        assert_eq!(say(&c), "Search, edit");
    }

    #[test]
    fn a_password_field_is_marked_and_never_read() {
        let mut c = control("Password", Role::Edit);
        c.states.password = Password::Yes;
        c.value = Value::Text("hunter2".into());
        for v in Verbosity::ALL {
            let said = at(&c, v);
            assert_eq!(said, "Password, password edit", "{v:?}");
            assert!(!said.contains("hunter2"));
        }
        c.value = Value::Withheld;
        assert_eq!(say(&c), "Password, password edit");
    }

    #[test]
    fn a_value_is_never_read_when_the_sources_cannot_tell() {
        let mut c = control("File name:", Role::Edit);
        c.states = States::default();
        assert_eq!(c.states.password, Password::CantTell);
        c.value = Value::Text("secret.txt".into());
        assert_eq!(say(&c), "File name:, edit");
    }

    #[test]
    fn a_withheld_value_says_nothing_about_a_value() {
        let mut c = control("Save", Role::Button);
        c.value = Value::Withheld;
        assert_eq!(say(&c), "Save, button");
    }

    #[test]
    fn a_nameless_password_field_is_still_marked() {
        let mut c = control("", Role::Edit);
        c.states.password = Password::Yes;
        assert_eq!(say(&c), "password edit");
    }

    // ── Helpers ──

    #[test]
    fn cut_at_word_keeps_short_text_whole() {
        assert_eq!(cut_at_word("one two", 7), "one two");
        assert_eq!(cut_at_word("one two three", 7), "one two");
        assert_eq!(cut_at_word("one two three", 8), "one two");
        assert_eq!(cut_at_word("onetwothree", 5), "onetw");
    }

    #[test]
    fn cut_at_word_counts_characters_not_bytes() {
        assert_eq!(cut_at_word("ééééé éé", 5), "ééééé");
        assert_eq!(cut_at_word("ééééé ééé", 7), "ééééé");
    }

    #[test]
    fn every_role_has_a_word_or_is_one_that_says_nothing() {
        for role in [Role::Unknown, Role::Pane, Role::Custom] {
            assert!(word_for(role).is_none());
        }
        assert_eq!(word_for(Role::Button), Some("button"));
    }

    #[test]
    fn verbosity_defaults_to_normal_and_round_trips() {
        assert_eq!(Verbosity::default(), Verbosity::Normal);
        for v in Verbosity::ALL {
            let json = serde_json::to_string(&v).unwrap();
            assert_eq!(serde_json::from_str::<Verbosity>(&json).unwrap(), v);
        }
    }
}
