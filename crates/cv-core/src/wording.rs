//! What is spoken for the control that has focus (roadmap 9.5, ADR 0015). Pure logic with no
//! Windows types: [`describe_control`] turns a [`Control`] into one phrase at a [`Verbosity`], and
//! [`focus_utterances`] wraps it for the speech queue.
//!
//! - The phrase is name, role, state, value, joined by commas (the voice pauses at each).
//! - A name that is really the control's text (Notepad's document, the Save As file name box) is
//!   not read: only the role is (STATUS Finding 27(b)).
//! - A value is spoken only when the control is known not to be a password. "Can't tell" and
//!   `Value::Withheld` say nothing, not even that a value exists.
//! - A control with no name, no value and a role that says nothing is not spoken (the Alt+Tab pane,
//!   Finding 27(c)). The window title is 9.6's job; positions and per-role wording are 9.7's.
//!
//! The numbers ([`NAME_MAX`], [`VALUE_SPOKEN_MAX`]) are starting values.

use serde::{Deserialize, Serialize};

use crate::focus::{Checked, Control, Password, Role, Value, clean_name};
use crate::speech::{Kind, Utterance};

/// A name longer than this is cut at a word (or, for a text box or document, taken to be the text
/// itself and not read).
pub const NAME_MAX: usize = 80;
/// A value is cut at a word near this many characters before it is spoken.
pub const VALUE_SPOKEN_MAX: usize = 100;

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

/// The utterance for a focus event, if there is anything to say. The foreground window changing
/// is not spoken here (9.6).
pub fn focus_utterances(control: Option<&Control>, verbosity: Verbosity) -> Vec<Utterance> {
    let Some(control) = control else {
        return Vec::new();
    };
    let text = describe_control(control, verbosity);
    if text.is_empty() { Vec::new() } else { vec![Utterance::new(Kind::Focus, text)] }
}

/// One phrase for a control, or an empty string when there is nothing worth saying.
pub fn describe_control(c: &Control, verbosity: Verbosity) -> String {
    let password = c.states.password == Password::Yes;
    let text_role = matches!(c.role, Role::Edit | Role::Document) || c.role.is_empty();

    let value = spoken_value(c);
    let (name, name_was_text) = spoken_name(&c.name, text_role);
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

    #[test]
    fn no_control_says_nothing() {
        assert!(focus_utterances(None, Verbosity::Normal).is_empty());
    }

    #[test]
    fn an_utterance_is_a_focus_kind() {
        let c = control("OK", Role::Button);
        assert_eq!(
            focus_utterances(Some(&c), Verbosity::Normal),
            vec![Utterance::new(Kind::Focus, "OK, button")]
        );
        assert!(focus_utterances(Some(&control("", Role::Pane)), Verbosity::Normal).is_empty());
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
