//! Focus events in the core (ADR 0015, roadmap 9.4a). Pure logic with no Windows types: the
//! neutral description of the focused control, the password rule, the merge of the UI Automation
//! and MSAA readings, and the filter that decides when to read and what to publish. The focus
//! thread (9.4b) maps each source's answer into [`Control`], feeds the filter, and publishes the
//! [`CoreEvent`] it returns.

use std::time::{Duration, Instant};

use crate::ScreenRect;
use crate::events::{AppId, CoreEvent};

/// A value is cut at this many characters before it leaves the focus thread (and again in
/// [`merge`]). Starting value (ADR 0015).
pub const VALUE_MAX: usize = 256;
/// A read is due once no foreground or focus event has arrived for this long. Starting value.
pub const SETTLE: Duration = Duration::from_millis(50);
/// A read is due this long after the first event of a burst, however long the burst goes on.
/// Starting value.
pub const SETTLE_MAX: Duration = Duration::from_millis(150);
/// Two rectangles are the same control when every edge is within this many pixels. Starting value.
pub const RECT_TOLERANCE_PX: i64 = 2;

/// The most `FindAll` calls one dialog walk may make. Starting value (ADR 0016); the focus thread
/// enforces it.
pub const DIALOG_CALLS: usize = 8;
/// How deep a dialog walk may go below the dialog. Starting value (ADR 0016).
pub const DIALOG_DEPTH: usize = 3;
/// The most nodes one dialog walk may collect. Starting value (ADR 0016).
pub const DIALOG_NODES: usize = 64;
/// The most pieces of text a dialog gives. Starting value (ADR 0016).
pub const DIALOG_TEXTS: usize = 8;
/// The most milliseconds one dialog read may take; past it the walk stops as incomplete and says
/// nothing. Starting value (ADR 0018); the focus thread enforces it.
pub const DIALOG_MS: u64 = 250;

/// The foreground window.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub title: String,
    /// The name of the dialog inside the window that holds the focus (a property page, a XAML
    /// prompt; ADR 0018). Empty when there is none.
    pub dialog_name: String,
    /// The text of a dialog (ADR 0016), chosen by [`dialog_text`]: the inner dialog's when there
    /// is one, else the window's. Empty when it is not a dialog or says nothing. The same on every
    /// read in the same dialog, so the duplicate rule of [`FocusFilter`] holds.
    pub dialog_text: Vec<String>,
}

impl WindowInfo {
    /// A window with a title, no inner dialog and no dialog text; the fields are public to fill in.
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), dialog_name: String::new(), dialog_text: Vec::new() }
    }
}

/// One element found in a dialog, in terms both sources can be mapped to (ADR 0016). The focus
/// thread passes the nodes in tree order, depth first: a container is followed by its children.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogNode {
    pub name: String,
    pub role: Role,
    /// The element can take keyboard focus. Its name is never returned by [`dialog_text`].
    pub focusable: bool,
    pub offscreen: bool,
    /// 0 for a child of the dialog itself.
    pub depth: usize,
}

impl DialogNode {
    /// A node that is not focusable and not offscreen; the fields are public to fill in.
    pub fn new(name: impl Into<String>, role: Role, depth: usize) -> Self {
        Self { name: name.into(), role, focusable: false, offscreen: false, depth }
    }
}

// ── The dialog text ──────────────────────────────────────────────────────────────────────────

/// Picks the text of a dialog from what the focus thread found in it (ADR 0016).
///
/// 1. A walk that did not finish (`complete` false) gives nothing: a dialog too big for the budget
///    is a window full of controls, not a message.
/// 2. Only a `Text` node that is neither focusable nor offscreen gives text. A focusable node's
///    name is never returned; it is used only for the comparison in 3.
/// 3. Label rule: a text whose name equals the next focusable sibling's name is that control's
///    label and is dropped. Names are compared without one `&`, a trailing `:`, extra
///    whitespace and case.
/// 4. Group rule: a text right after a `Group` sibling is the group's description and is dropped.
/// 5. A text equal to the title or to an earlier text is dropped, and so is an empty one.
/// 6. At most [`DIALOG_TEXTS`] pieces, each cut at [`VALUE_MAX`] characters.
///
/// A sibling is a node at the same depth with no shallower node between; descendants are skipped.
pub fn dialog_text(nodes: &[DialogNode], complete: bool, title: &str) -> Vec<String> {
    if !complete {
        return Vec::new();
    }
    let title = flat_key(title);
    let mut seen: Vec<String> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        if out.len() >= DIALOG_TEXTS {
            break;
        }
        if node.role != Role::Text || node.focusable || node.offscreen {
            continue;
        }
        let text = node.name.split_whitespace().collect::<Vec<_>>().join(" ");
        let key = flat_key(&text);
        if key.is_empty() || key == title || seen.contains(&key) {
            continue;
        }
        let label = label_key(&text);
        let labels_next = siblings_after(nodes, i)
            .find(|n| n.focusable)
            .is_some_and(|n| label_key(&n.name) == label);
        let after_group = siblings_before(nodes, i).next().is_some_and(|n| n.role == Role::Group);
        if labels_next || after_group {
            continue;
        }
        seen.push(key);
        out.push(text.chars().take(VALUE_MAX).collect());
    }
    out
}

/// The nodes after `i` at the same depth, nearest first, up to the end of the parent.
fn siblings_after(nodes: &[DialogNode], i: usize) -> impl Iterator<Item = &DialogNode> {
    let depth = nodes[i].depth;
    nodes[i + 1..].iter().take_while(move |n| n.depth >= depth).filter(move |n| n.depth == depth)
}

/// The nodes before `i` at the same depth, nearest first, back to the start of the parent.
fn siblings_before(nodes: &[DialogNode], i: usize) -> impl Iterator<Item = &DialogNode> {
    let depth = nodes[i].depth;
    nodes[..i].iter().rev().take_while(move |n| n.depth >= depth).filter(move |n| n.depth == depth)
}

/// Whitespace flattened, one `&` marker removed, case ignored.
fn flat_key(text: &str) -> String {
    clean_name(text).split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// [`flat_key`] without a trailing colon, for the label rule ("File name:" labels "File name").
fn label_key(text: &str) -> String {
    flat_key(text).trim_end_matches(':').trim_end().to_string()
}

/// What a control is, in terms both UI Automation and MSAA can be mapped to. The platform owns the
/// mapping from its own ids (ADR 0015). New variants may be added.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Role {
    /// Neither source gave a role.
    #[default]
    Unknown,
    /// A container with no meaning of its own. Classic Win32 controls often read as this from
    /// UI Automation.
    Pane,
    /// A control the source has no role for.
    Custom,
    Window,
    Dialog,
    Button,
    SplitButton,
    CheckBox,
    RadioButton,
    /// A text box.
    Edit,
    ComboBox,
    List,
    ListItem,
    Menu,
    MenuBar,
    MenuItem,
    Tab,
    TabItem,
    Tree,
    TreeItem,
    Table,
    Header,
    HeaderItem,
    Slider,
    Spinner,
    ScrollBar,
    Thumb,
    ProgressBar,
    /// A label.
    Text,
    Link,
    Image,
    Document,
    Group,
    ToolBar,
    StatusBar,
    TitleBar,
    ToolTip,
    Separator,
    Calendar,
    AppBar,
}

impl Role {
    /// A role that says nothing about the control: a source that gives one of these may be
    /// overruled by one that gives something better.
    pub fn is_empty(self) -> bool {
        matches!(self, Self::Unknown | Self::Pane | Self::Custom)
    }
}

/// A two-state control that can also be in between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checked {
    On,
    Off,
    Mixed,
}

/// Whether the control is a password field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Password {
    Yes,
    No,
    /// No source said. Treated as a password: nothing about the value is read or kept.
    #[default]
    CantTell,
}

/// The state of a control. `None` means the source did not say.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct States {
    pub enabled: Option<bool>,
    pub checked: Option<Checked>,
    pub expanded: Option<bool>,
    pub selected: Option<bool>,
    pub password: Password,
}

/// A control's value (a text box's text, a slider's number).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Value {
    #[default]
    None,
    /// At most [`VALUE_MAX`] characters.
    Text(String),
    /// A password field, or one where the sources can't tell. Nothing about the value, including
    /// its length, was read.
    Withheld,
}

/// Which source a control's fields came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusSource {
    Uia,
    Msaa,
    /// Both described the same control and were merged.
    Both,
}

impl FocusSource {
    /// Short name for logs.
    pub fn name(self) -> &'static str {
        match self {
            Self::Uia => "uia",
            Self::Msaa => "msaa",
            Self::Both => "uia+msaa",
        }
    }
}

/// The control that has focus, as one source or both described it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub name: String,
    pub role: Role,
    pub states: States,
    pub value: Value,
    /// Physical virtual-screen pixels, for 9.9.
    pub rect: Option<ScreenRect>,
    pub source: FocusSource,
}

impl Control {
    /// A control with no states, no value and no rectangle; the fields are public to fill in.
    pub fn new(name: impl Into<String>, role: Role, source: FocusSource) -> Self {
        Self {
            name: name.into(),
            role,
            states: States::default(),
            value: Value::None,
            rect: None,
            source,
        }
    }

    /// No name and a role that says nothing.
    fn is_empty(&self) -> bool {
        self.name.trim().is_empty() && self.role.is_empty()
    }
}

// ── The password rule ────────────────────────────────────────────────────────────────────────

/// May a source's value be read? Only when that source says the field is not a password
/// (`Some(false)`) and the other source does not say it is (`Some(true)`). "Can't tell" (`None`)
/// counts as a password.
pub fn value_allowed(own: Option<bool>, other: Option<bool>) -> bool {
    own == Some(false) && other != Some(true)
}

// ── The merge ────────────────────────────────────────────────────────────────────────────────

/// Merges a UI Automation reading and an MSAA reading of the focus into one control (ADR 0015).
///
/// - Both describe the same control (rectangles within [`RECT_TOLERANCE_PX`], or equal names when
///   a rectangle is missing): field by field, UIA first, MSAA's role when UIA's says nothing.
/// - Different controls: UIA alone, unless it is empty; then MSAA alone.
/// - One failed: the other alone. Both failed: `None`.
///
/// Whatever the combination, a control that is not known to be a non-password has its value
/// withheld, and a text value is cut at [`VALUE_MAX`].
pub fn merge(uia: Option<Control>, msaa: Option<Control>) -> Option<Control> {
    let (uia, msaa) = match (uia, msaa) {
        (None, None) => return None,
        (Some(u), None) => return Some(finish(u, FocusSource::Uia)),
        (None, Some(m)) => return Some(finish(m, FocusSource::Msaa)),
        (Some(u), Some(m)) => (u, m),
    };
    if !same_control(&uia, &msaa) {
        return Some(if uia.is_empty() {
            finish(msaa, FocusSource::Msaa)
        } else {
            finish(uia, FocusSource::Uia)
        });
    }
    let name = if uia.name.trim().is_empty() {
        msaa.name.clone()
    } else {
        clean_name(&uia.name)
    };
    let role = if uia.role.is_empty() && !msaa.role.is_empty() { msaa.role } else { uia.role };
    let states = States {
        enabled: uia.states.enabled.or(msaa.states.enabled),
        checked: uia.states.checked.or(msaa.states.checked),
        expanded: uia.states.expanded.or(msaa.states.expanded),
        selected: uia.states.selected.or(msaa.states.selected),
        password: combine_password(uia.states.password, msaa.states.password),
    };
    let value = if uia.value == Value::Withheld || msaa.value == Value::Withheld {
        Value::Withheld
    } else if uia.value != Value::None {
        uia.value
    } else {
        msaa.value
    };
    let combined = Control { name, role, states, value, rect: uia.rect.or(msaa.rect), source: FocusSource::Both };
    Some(finish(combined, FocusSource::Both))
}

/// `Yes` if either says yes; `No` only if one says no and the other does not say yes.
fn combine_password(a: Password, b: Password) -> Password {
    match (a, b) {
        (Password::Yes, _) | (_, Password::Yes) => Password::Yes,
        (Password::No, _) | (_, Password::No) => Password::No,
        _ => Password::CantTell,
    }
}

/// The last step for every result: the source, the value cap and the password rule.
fn finish(mut control: Control, source: FocusSource) -> Control {
    control.source = source;
    if control.states.password != Password::No {
        control.value = Value::Withheld;
    } else if let Value::Text(text) = &mut control.value
        && text.chars().count() > VALUE_MAX
    {
        *text = text.chars().take(VALUE_MAX).collect();
    }
    control
}

/// Equal in everything the user would hear or see. Which source answered can flip between two
/// reads of the same control (UI Automation alone, then both), and that is not a change.
fn same_but_source(a: &Option<Control>, b: &Option<Control>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Control { source: b.source, ..a.clone() } == *b,
        (None, None) => true,
        _ => false,
    }
}

fn same_control(a: &Control, b: &Control) -> bool {
    match (a.rect, b.rect) {
        (Some(ra), Some(rb)) => {
            let edges = |r: ScreenRect| {
                let (left, top) = (i64::from(r.left), i64::from(r.top));
                [left, top, left + i64::from(r.width), top + i64::from(r.height)]
            };
            edges(ra)
                .iter()
                .zip(edges(rb))
                .all(|(x, y)| (x - y).abs() <= RECT_TOLERANCE_PX)
        }
        _ => {
            let (na, nb) = (comparable(&a.name), comparable(&b.name));
            !na.is_empty() && na == nb
        }
    }
}

fn comparable(name: &str) -> String {
    clean_name(name).trim().to_lowercase()
}

/// Removes the `&` that marks an accelerator key in a Win32 name (`&Save` is "Save"). `&&` is a
/// literal `&`.
pub fn clean_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            if chars.peek() == Some(&'&') {
                chars.next();
                out.push('&');
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ── The filter ───────────────────────────────────────────────────────────────────────────────

/// What one read found.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// The foreground program; `None` when the window has no program name (the shell's
    /// `ForegroundStaging` window, the desktop).
    pub app: Option<AppId>,
    pub window: WindowInfo,
    /// `None`: neither source could read the focused control.
    pub control: Option<Control>,
    /// The foreground window, opaque and compared only for equality (ADR 0018). A new key sets
    /// `window_changed` even when its foreground WinEvent has not arrived yet.
    pub window_key: u64,
    /// The inner dialog that holds the focus, opaque (ADR 0018); `None` when there is none. A new
    /// key inside the same window sets `dialog_changed`.
    pub dialog_key: Option<u64>,
}

/// What was last published, for the duplicate rule and the key rules.
#[derive(Debug)]
struct Published {
    app: AppId,
    window: WindowInfo,
    control: Option<Control>,
    window_key: u64,
    dialog_key: Option<u64>,
}

/// Decides when the focus thread reads and what it publishes (ADR 0015). The thread calls
/// [`winevent`](Self::winevent) for every foreground or focus WinEvent, waits until
/// [`due`](Self::due), brackets the read with [`begin_read`](Self::begin_read) and
/// [`finish_read`](Self::finish_read), and publishes what that returns. The clock is passed in.
#[derive(Debug, Default)]
pub struct FocusFilter {
    /// The first and the newest event of the burst waiting for a read.
    burst: Option<(Instant, Instant)>,
    /// A foreground change has come in since the last published event.
    window_changed: bool,
    reading: bool,
    /// An event arrived while the read was running.
    newer_during_read: bool,
    /// The previous reading was dropped as stale.
    dropped_last: bool,
    last: Option<Published>,
}

impl FocusFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// A foreground or focus WinEvent arrived. `foreground_changed` is true for the foreground
    /// event.
    pub fn winevent(&mut self, now: Instant, foreground_changed: bool) {
        self.burst = Some(match self.burst {
            Some((first, _)) => (first, now),
            None => (now, now),
        });
        self.window_changed |= foreground_changed;
        if self.reading {
            self.newer_during_read = true;
        }
    }

    /// When a read becomes due, or `None` with no event waiting.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.burst.map(|(first, last)| (last + SETTLE).min(first + SETTLE_MAX))
    }

    /// Whether to read now.
    pub fn due(&self, now: Instant) -> bool {
        self.next_deadline().is_some_and(|deadline| now >= deadline)
    }

    /// A read starts: it covers every event so far. Events that arrive before
    /// [`finish_read`](Self::finish_read) make the reading stale.
    pub fn begin_read(&mut self) {
        self.burst = None;
        self.reading = true;
        self.newer_during_read = false;
    }

    /// The read is over. Returns the event to publish, if any.
    pub fn finish_read(&mut self, now: Instant, reading: Reading) -> Option<CoreEvent> {
        self.reading = false;
        // A reading that started before a newer event is dropped and the newer burst gets its
        // own read; one drop at most in a row, so a stream of events can't starve the reads.
        if std::mem::take(&mut self.newer_during_read) && !self.dropped_last {
            self.dropped_last = true;
            return None;
        }
        self.dropped_last = false;

        let Reading { app, window, control, window_key, dialog_key } = reading;
        // No program name: nothing to say, and no key changes. The window change stays pending.
        let app = app?;
        // A foreground WinEvent, or a read that already sees a new foreground window (ADR 0018).
        let window_changed =
            self.window_changed || self.last.as_ref().is_some_and(|l| l.window_key != window_key);
        // Neither source could read the control: say so once, when the window becomes the
        // foreground, not for every later event in it.
        if control.is_none() && !window_changed {
            return None;
        }
        // Focus entered another inner dialog in the same window. Leaving one for the window's own
        // controls is no dialog change.
        let dialog_changed = !window_changed
            && dialog_key.is_some()
            && self.last.as_ref().is_none_or(|l| l.dialog_key != dialog_key);
        if !window_changed
            && let Some(last) = &self.last
            && last.app == app
            && last.window == window
            && last.dialog_key == dialog_key
            && same_but_source(&last.control, &control)
        {
            return None;
        }
        self.window_changed = false;
        self.last = Some(Published {
            app: app.clone(),
            window: window.clone(),
            control: control.clone(),
            window_key,
            dialog_key,
        });
        Some(CoreEvent::FocusChanged { at: now, app, window, window_changed, dialog_changed, control })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctl(name: &str, role: Role, source: FocusSource) -> Control {
        let mut c = Control::new(name, role, source);
        c.states.password = Password::No;
        c
    }

    fn uia(name: &str, role: Role) -> Control {
        ctl(name, role, FocusSource::Uia)
    }

    fn msaa(name: &str, role: Role) -> Control {
        ctl(name, role, FocusSource::Msaa)
    }

    fn rect(left: i32, top: i32, width: u32, height: u32) -> Option<ScreenRect> {
        Some(ScreenRect { left, top, width, height })
    }

    // ── value_allowed ──

    #[test]
    fn a_value_is_allowed_only_when_its_own_source_says_not_password() {
        assert!(value_allowed(Some(false), None));
        assert!(value_allowed(Some(false), Some(false)));
    }

    #[test]
    fn either_source_saying_password_withholds() {
        assert!(!value_allowed(Some(true), Some(false)));
        assert!(!value_allowed(Some(false), Some(true)));
        assert!(!value_allowed(Some(true), None));
    }

    #[test]
    fn cant_tell_counts_as_a_password() {
        assert!(!value_allowed(None, None));
        assert!(!value_allowed(None, Some(false)));
    }

    // ── clean_name ──

    #[test]
    fn the_accelerator_marker_is_removed() {
        assert_eq!(clean_name("&Save"), "Save");
        assert_eq!(clean_name("Save &as..."), "Save as...");
        assert_eq!(clean_name("Save"), "Save");
        assert_eq!(clean_name("Trailing &"), "Trailing ");
    }

    #[test]
    fn a_doubled_ampersand_is_a_literal_one() {
        assert_eq!(clean_name("R&&D"), "R&D");
        assert_eq!(clean_name("&&&Go"), "&Go");
    }

    // ── merge: one or no source ──

    #[test]
    fn both_failed_gives_nothing() {
        assert_eq!(merge(None, None), None);
    }

    #[test]
    fn one_source_alone_is_used_and_marked() {
        let got = merge(Some(uia("OK", Role::Button)), None).unwrap();
        assert_eq!((got.name.as_str(), got.role, got.source), ("OK", Role::Button, FocusSource::Uia));
        let got = merge(None, Some(msaa("OK", Role::Button))).unwrap();
        assert_eq!(got.source, FocusSource::Msaa);
    }

    #[test]
    fn a_source_used_alone_keeps_its_accelerator() {
        // Only the merge of two readings of one control cleans the name (ADR 0015).
        let got = merge(Some(uia("&Save", Role::Button)), None).unwrap();
        assert_eq!(got.name, "&Save");
    }

    // ── merge: the same control ──

    #[test]
    fn matching_rectangles_make_one_control() {
        let mut u = uia("Name", Role::Edit);
        u.rect = rect(100, 100, 200, 30);
        let mut m = msaa("different words", Role::Edit);
        m.rect = rect(102, 98, 198, 32);
        let got = merge(Some(u), Some(m)).unwrap();
        assert_eq!(got.source, FocusSource::Both);
        assert_eq!(got.name, "Name");
    }

    #[test]
    fn rectangles_more_than_the_tolerance_apart_are_different_controls() {
        let mut u = uia("List item", Role::ListItem);
        u.rect = rect(100, 100, 200, 30);
        let mut m = msaa("Window title", Role::Window);
        m.rect = rect(100, 100, 200, 33);
        let got = merge(Some(u), Some(m)).unwrap();
        assert_eq!((got.source, got.name.as_str()), (FocusSource::Uia, "List item"));
    }

    #[test]
    fn without_a_rectangle_equal_cleaned_names_make_one_control() {
        let got = merge(Some(uia("&Save", Role::Pane)), Some(msaa("save", Role::Button))).unwrap();
        assert_eq!(got.source, FocusSource::Both);
        assert_eq!(got.name, "Save");
    }

    #[test]
    fn without_a_rectangle_empty_names_are_not_the_same_control() {
        let got = merge(Some(uia("", Role::Button)), Some(msaa("", Role::Button))).unwrap();
        assert_eq!(got.source, FocusSource::Uia);
    }

    #[test]
    fn a_missing_rectangle_on_one_side_falls_back_to_names() {
        let mut u = uia("Open", Role::Button);
        u.rect = rect(0, 0, 10, 10);
        let got = merge(Some(u), Some(msaa("Open", Role::Button))).unwrap();
        assert_eq!(got.source, FocusSource::Both);
        assert_eq!(got.rect, rect(0, 0, 10, 10));
    }

    #[test]
    fn save_as_takes_msaas_role_and_cleans_uias_name() {
        // 9.2: UIA says Pane for every classic Win32 control and keeps the accelerator.
        let mut u = uia("&Save", Role::Pane);
        u.rect = rect(10, 10, 80, 24);
        let mut m = msaa("Save", Role::Button);
        m.rect = rect(10, 10, 80, 24);
        let got = merge(Some(u), Some(m)).unwrap();
        assert_eq!((got.name.as_str(), got.role), ("Save", Role::Button));
    }

    #[test]
    fn uias_role_stands_unless_it_says_nothing() {
        let mut u = uia("Mute", Role::CheckBox);
        u.rect = rect(0, 0, 10, 10);
        let mut m = msaa("Mute", Role::Button);
        m.rect = rect(0, 0, 10, 10);
        assert_eq!(merge(Some(u), Some(m)).unwrap().role, Role::CheckBox);

        let mut u = uia("Mute", Role::Pane);
        u.rect = rect(0, 0, 10, 10);
        let mut m = msaa("Mute", Role::Custom);
        m.rect = rect(0, 0, 10, 10);
        assert_eq!(merge(Some(u), Some(m)).unwrap().role, Role::Pane);
    }

    #[test]
    fn an_empty_uia_name_takes_msaas() {
        let mut u = uia("", Role::Edit);
        u.rect = rect(0, 0, 10, 10);
        let mut m = msaa("File name", Role::Edit);
        m.rect = rect(0, 0, 10, 10);
        assert_eq!(merge(Some(u), Some(m)).unwrap().name, "File name");
    }

    #[test]
    fn states_come_from_uia_where_it_has_them_else_msaa() {
        let mut u = uia("Mute", Role::CheckBox);
        u.rect = rect(0, 0, 10, 10);
        u.states.enabled = Some(true);
        u.states.checked = Some(Checked::Off);
        let mut m = msaa("Mute", Role::CheckBox);
        m.rect = rect(0, 0, 10, 10);
        m.states.enabled = Some(false);
        m.states.checked = Some(Checked::On);
        m.states.expanded = Some(true);
        m.states.selected = Some(true);
        let s = merge(Some(u), Some(m)).unwrap().states;
        assert_eq!(s.enabled, Some(true));
        assert_eq!(s.checked, Some(Checked::Off));
        assert_eq!(s.expanded, Some(true));
        assert_eq!(s.selected, Some(true));
    }

    #[test]
    fn msaa_alone_can_say_checked_but_never_unchecked() {
        // MSAA's checked appears only when ticked (Finding 25(j)): the platform leaves it None.
        let mut u = uia("Mute", Role::CheckBox);
        u.rect = rect(0, 0, 10, 10);
        let mut m = msaa("Mute", Role::CheckBox);
        m.rect = rect(0, 0, 10, 10);
        assert_eq!(merge(Some(u.clone()), Some(m.clone())).unwrap().states.checked, None);
        m.states.checked = Some(Checked::On);
        assert_eq!(merge(Some(u), Some(m)).unwrap().states.checked, Some(Checked::On));
    }

    #[test]
    fn a_value_comes_from_uia_else_msaa() {
        let mut u = uia("Volume", Role::Slider);
        u.rect = rect(0, 0, 10, 10);
        let mut m = msaa("Volume", Role::Slider);
        m.rect = rect(0, 0, 10, 10);
        m.value = Value::Text("40%".into());
        assert_eq!(merge(Some(u.clone()), Some(m.clone())).unwrap().value, Value::Text("40%".into()));
        u.value = Value::Text("40".into());
        assert_eq!(merge(Some(u), Some(m)).unwrap().value, Value::Text("40".into()));
    }

    #[test]
    fn a_value_withheld_by_either_side_is_withheld() {
        let mut u = uia("Volume", Role::Slider);
        u.rect = rect(0, 0, 10, 10);
        u.value = Value::Text("40".into());
        let mut m = msaa("Volume", Role::Slider);
        m.rect = rect(0, 0, 10, 10);
        m.value = Value::Withheld;
        assert_eq!(merge(Some(u), Some(m)).unwrap().value, Value::Withheld);
    }

    // ── merge: the password flag ──

    fn pair(a: Password, b: Password) -> Control {
        let mut u = uia("Field", Role::Edit);
        u.rect = rect(0, 0, 10, 10);
        u.states.password = a;
        u.value = Value::Text("secret".into());
        let mut m = msaa("Field", Role::Edit);
        m.rect = rect(0, 0, 10, 10);
        m.states.password = b;
        merge(Some(u), Some(m)).unwrap()
    }

    #[test]
    fn password_yes_if_either_says_yes() {
        assert_eq!(pair(Password::Yes, Password::No).states.password, Password::Yes);
        assert_eq!(pair(Password::No, Password::Yes).states.password, Password::Yes);
        assert_eq!(pair(Password::CantTell, Password::Yes).states.password, Password::Yes);
    }

    #[test]
    fn password_no_only_if_one_says_no_and_the_other_does_not_say_yes() {
        assert_eq!(pair(Password::No, Password::No).states.password, Password::No);
        assert_eq!(pair(Password::No, Password::CantTell).states.password, Password::No);
        assert_eq!(pair(Password::CantTell, Password::No).states.password, Password::No);
        assert_eq!(pair(Password::CantTell, Password::CantTell).states.password, Password::CantTell);
    }

    #[test]
    fn a_value_survives_only_when_the_merged_password_flag_is_no() {
        assert_eq!(pair(Password::No, Password::CantTell).value, Value::Text("secret".into()));
        assert_eq!(pair(Password::Yes, Password::No).value, Value::Withheld);
        assert_eq!(pair(Password::CantTell, Password::CantTell).value, Value::Withheld);
    }

    #[test]
    fn a_reading_used_alone_with_cant_tell_has_its_value_withheld() {
        let mut m = msaa("Field", Role::Edit);
        m.states.password = Password::CantTell;
        m.value = Value::Text("secret".into());
        assert_eq!(merge(None, Some(m)).unwrap().value, Value::Withheld);
        let mut u = uia("Field", Role::Edit);
        u.states.password = Password::Yes;
        u.value = Value::Text("secret".into());
        assert_eq!(merge(Some(u), None).unwrap().value, Value::Withheld);
    }

    // ── merge: different controls ──

    #[test]
    fn different_controls_give_uia_alone() {
        let mut u = uia("Inbox", Role::ListItem);
        u.rect = rect(0, 0, 100, 20);
        let mut m = msaa("Mail", Role::Window);
        m.rect = rect(0, 0, 800, 600);
        let got = merge(Some(u), Some(m)).unwrap();
        assert_eq!((got.name.as_str(), got.source), ("Inbox", FocusSource::Uia));
    }

    #[test]
    fn an_empty_uia_reading_gives_way_to_msaa_when_they_differ() {
        // Alt+Tab: UIA gives an unnamed Pane, MSAA names the switcher item.
        let mut u = uia("", Role::Pane);
        u.rect = rect(0, 0, 1000, 600);
        let mut m = msaa("Notepad", Role::ListItem);
        m.rect = rect(100, 100, 200, 150);
        let got = merge(Some(u), Some(m)).unwrap();
        assert_eq!((got.name.as_str(), got.source), ("Notepad", FocusSource::Msaa));
    }

    #[test]
    fn a_named_pane_is_not_empty() {
        let mut u = uia("Document 1", Role::Pane);
        u.rect = rect(0, 0, 1000, 600);
        let mut m = msaa("Text area", Role::Edit);
        m.rect = rect(0, 30, 1000, 570);
        assert_eq!(merge(Some(u), Some(m)).unwrap().source, FocusSource::Uia);
    }

    // ── merge: the value cap ──

    #[test]
    fn a_long_value_is_cut_at_the_cap() {
        let mut u = uia("Box", Role::Edit);
        u.value = Value::Text("é".repeat(VALUE_MAX + 50));
        let Value::Text(text) = merge(Some(u), None).unwrap().value else { panic!() };
        assert_eq!(text.chars().count(), VALUE_MAX);
    }

    #[test]
    fn a_value_at_the_cap_is_kept_whole() {
        let mut u = uia("Box", Role::Edit);
        u.value = Value::Text("x".repeat(VALUE_MAX));
        let Value::Text(text) = merge(Some(u), None).unwrap().value else { panic!() };
        assert_eq!(text.len(), VALUE_MAX);
    }

    // ── dialog_text (ADR 0016) ──

    fn text(name: &str, depth: usize) -> DialogNode {
        DialogNode::new(name, Role::Text, depth)
    }

    fn focusable(name: &str, role: Role, depth: usize) -> DialogNode {
        let mut n = DialogNode::new(name, role, depth);
        n.focusable = true;
        n
    }

    fn pieces(nodes: &[DialogNode], title: &str) -> Vec<String> {
        dialog_text(nodes, true, title)
    }

    #[test]
    fn a_message_box_gives_its_message() {
        let nodes = [
            text("Do you want to save changes to Untitled?", 0),
            focusable("Save", Role::Button, 0),
            focusable("Don't Save", Role::Button, 0),
            focusable("Cancel", Role::Button, 0),
        ];
        assert_eq!(pieces(&nodes, "Notepad"), ["Do you want to save changes to Untitled?"]);
    }

    #[test]
    fn a_walk_that_did_not_finish_gives_nothing() {
        let nodes = [text("Hello", 0), focusable("OK", Role::Button, 0)];
        assert!(dialog_text(&nodes, false, "").is_empty());
        assert_eq!(dialog_text(&nodes, true, ""), ["Hello"]);
    }

    #[test]
    fn a_focusable_nodes_name_never_comes_out() {
        // An edit whose name is its text (Finding 27(b)), and a "text" that can take focus.
        let nodes = [
            focusable("secret file name.txt", Role::Edit, 0),
            focusable("Click me", Role::Text, 0),
            focusable("OK", Role::Button, 0),
        ];
        assert!(pieces(&nodes, "").is_empty());
    }

    #[test]
    fn a_password_node_with_its_name_emptied_says_nothing() {
        let nodes = [text("Enter your password", 0), focusable("", Role::Edit, 0), text("", 0)];
        assert_eq!(pieces(&nodes, ""), ["Enter your password"]);
    }

    #[test]
    fn only_text_roles_give_text() {
        let nodes = [
            DialogNode::new("A link", Role::Link, 0),
            DialogNode::new("An image", Role::Image, 0),
            DialogNode::new("A pane", Role::Pane, 0),
            DialogNode::new("A button", Role::Button, 0),
            text("A label", 0),
        ];
        assert_eq!(pieces(&nodes, ""), ["A label"]);
    }

    #[test]
    fn offscreen_text_is_skipped() {
        let mut hidden = text("Hidden", 0);
        hidden.offscreen = true;
        assert_eq!(pieces(&[hidden, text("Shown", 0)], ""), ["Shown"]);
    }

    #[test]
    fn a_label_is_dropped_when_the_next_focusable_sibling_has_its_name() {
        // Save As: "File name:" before the file name box, "Save as type:" before its combo.
        let nodes = [
            text("File &name:", 0),
            focusable("File name", Role::Edit, 0),
            text("Save as &type:", 0),
            focusable("Save as type:", Role::ComboBox, 0),
        ];
        assert!(pieces(&nodes, "Save As").is_empty());
    }

    #[test]
    fn the_label_rule_ignores_case_spacing_and_one_accelerator() {
        let nodes = [text("USER   name :", 0), focusable("&User name", Role::Edit, 0)];
        assert!(pieces(&nodes, "").is_empty());
    }

    #[test]
    fn a_text_that_does_not_name_the_next_control_is_kept() {
        let nodes = [text("Enter a name for the new folder", 0), focusable("Name", Role::Edit, 0)];
        assert_eq!(pieces(&nodes, ""), ["Enter a name for the new folder"]);
    }

    #[test]
    fn the_label_rule_looks_past_other_nodes_and_descendants_to_the_next_focusable_sibling() {
        let nodes = [
            text("Name", 0),
            DialogNode::new("", Role::Pane, 0),
            text("Inner note", 1),
            focusable("Inner button", Role::Button, 1),
            text("A second line", 0),
            focusable("Name", Role::Edit, 0),
        ];
        // "Name" labels the edit three siblings on; the inner button is not a sibling.
        assert_eq!(pieces(&nodes, ""), ["Inner note", "A second line"]);
    }

    #[test]
    fn the_label_rule_does_not_cross_into_another_parent() {
        let nodes = [
            DialogNode::new("", Role::Pane, 0),
            text("Name", 1),
            DialogNode::new("", Role::Pane, 0),
            focusable("Name", Role::Edit, 1),
        ];
        assert_eq!(pieces(&nodes, ""), ["Name"]);
    }

    #[test]
    fn a_text_right_after_a_group_is_its_description() {
        let nodes = [
            DialogNode::new("Options", Role::Group, 0),
            text("Choose how the file is saved", 0),
            text("Another message", 0),
        ];
        assert_eq!(pieces(&nodes, ""), ["Another message"]);
    }

    #[test]
    fn the_group_rule_needs_the_group_to_be_the_previous_sibling() {
        let nodes = [
            DialogNode::new("Options", Role::Group, 0),
            text("Inside the group", 1),
            text("After the group", 0),
        ];
        // The first text is the group's child (no previous sibling); the second follows the group
        // at depth 0, so it is the group's description.
        assert_eq!(pieces(&nodes, ""), ["Inside the group"]);
    }

    #[test]
    fn a_text_equal_to_the_title_or_an_earlier_text_is_dropped() {
        let nodes = [
            text("Save As", 0),
            text("Replace file?", 0),
            text("replace  FILE?", 0),
            text("It already exists.", 0),
        ];
        assert_eq!(pieces(&nodes, "Save as"), ["Replace file?", "It already exists."]);
    }

    #[test]
    fn empty_and_blank_texts_are_dropped() {
        let nodes = [text("", 0), text("  \n ", 0), text("Real", 0)];
        assert_eq!(pieces(&nodes, ""), ["Real"]);
    }

    #[test]
    fn a_text_is_flattened() {
        assert_eq!(pieces(&[text("Line one\r\n  line   two", 0)], ""), ["Line one line two"]);
    }

    #[test]
    fn at_most_dialog_texts_pieces_each_cut_at_value_max() {
        let nodes: Vec<_> = (0..DIALOG_TEXTS + 4).map(|i| text(&format!("Message {i}"), 0)).collect();
        let got = pieces(&nodes, "");
        assert_eq!(got.len(), DIALOG_TEXTS);
        assert_eq!(got[0], "Message 0");
        let long = pieces(&[text(&"é".repeat(VALUE_MAX + 20), 0)], "");
        assert_eq!(long[0].chars().count(), VALUE_MAX);
    }

    #[test]
    fn a_save_as_shape_says_nothing() {
        let nodes = [
            DialogNode::new("Navigation pane", Role::Pane, 0),
            focusable("Items view", Role::List, 0),
            text("File name:", 0),
            focusable("File name", Role::Edit, 0),
            text("Save as type:", 0),
            focusable("Save as type:", Role::ComboBox, 0),
            focusable("Save", Role::Button, 0),
            focusable("Cancel", Role::Button, 0),
        ];
        assert!(pieces(&nodes, "Save As").is_empty());
    }

    #[test]
    fn no_nodes_give_no_text() {
        assert!(pieces(&[], "Title").is_empty());
    }

    #[test]
    fn a_new_window_has_no_dialog_text() {
        assert!(WindowInfo::new("Notepad").dialog_text.is_empty());
    }

    // ── FocusFilter ──

    fn base() -> Instant {
        Instant::now()
    }

    fn ms(base: Instant, n: u64) -> Instant {
        base + Duration::from_millis(n)
    }

    fn app(exe: &str) -> AppId {
        AppId { pid: 7, exe: exe.into() }
    }

    fn reading(exe: &str, title: &str, control: Option<Control>) -> Reading {
        Reading { app: Some(app(exe)), window: WindowInfo::new(title), control, ..nobody(0) }
    }

    /// A reading with no program name (the shell's staging window) in the window `window_key`.
    fn nobody(window_key: u64) -> Reading {
        Reading { app: None, window: WindowInfo::new(""), control: None, window_key, dialog_key: None }
    }

    /// A reading with both keys, in the window `title` of `notepad.exe`.
    fn keyed(title: &str, control: Control, window_key: u64, dialog_key: Option<u64>) -> Reading {
        Reading { window_key, dialog_key, ..reading("notepad.exe", title, Some(control)) }
    }

    fn flags(e: &CoreEvent) -> (bool, bool) {
        let CoreEvent::FocusChanged { window_changed, dialog_changed, .. } = e else {
            panic!("not a focus event")
        };
        (*window_changed, *dialog_changed)
    }

    fn read(f: &mut FocusFilter, at: Instant, r: Reading) -> Option<CoreEvent> {
        f.begin_read();
        f.finish_read(at, r)
    }

    fn parts(e: &CoreEvent) -> (bool, Option<&Control>) {
        let CoreEvent::FocusChanged { window_changed, control, .. } = e else {
            panic!("not a focus event")
        };
        (*window_changed, control.as_ref())
    }

    #[test]
    fn nothing_is_due_with_no_event() {
        let f = FocusFilter::new();
        assert_eq!(f.next_deadline(), None);
        assert!(!f.due(base()));
    }

    #[test]
    fn a_read_is_due_once_the_events_have_settled() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, false);
        assert_eq!(f.next_deadline(), Some(ms(t, 50)));
        assert!(!f.due(ms(t, 49)));
        assert!(f.due(ms(t, 50)));
    }

    #[test]
    fn each_new_event_moves_the_deadline() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, false);
        f.winevent(ms(t, 30), false);
        assert_eq!(f.next_deadline(), Some(ms(t, 80)));
    }

    #[test]
    fn a_long_burst_is_read_at_the_maximum() {
        let t = base();
        let mut f = FocusFilter::new();
        for n in 0..8 {
            f.winevent(ms(t, n * 20), false);
        }
        // The newest event is at 140; settle alone would say 190.
        assert_eq!(f.next_deadline(), Some(ms(t, 150)));
        assert!(f.due(ms(t, 150)));
    }

    #[test]
    fn an_alt_tab_burst_gives_one_read_and_one_event() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        for n in 1..10 {
            f.winevent(ms(t, n * 16), false);
        }
        assert!(!f.due(ms(t, 100)));
        assert!(f.due(ms(t, 150)));
        let c = msaa("Notepad", Role::ListItem);
        let e = read(&mut f, ms(t, 150), reading("explorer.exe", "Task Switching", Some(c))).unwrap();
        assert!(parts(&e).0);
        assert_eq!(f.next_deadline(), None);
        assert!(!f.due(ms(t, 400)));
    }

    #[test]
    fn the_event_carries_app_window_control_and_time() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        let c = uia("OK", Role::Button);
        let e = read(&mut f, ms(t, 60), reading("notepad.exe", "Untitled", Some(c.clone()))).unwrap();
        let CoreEvent::FocusChanged { at, app: a, window, window_changed, dialog_changed, control } = e
        else {
            panic!()
        };
        assert_eq!(at, ms(t, 60));
        assert_eq!(a, app("notepad.exe"));
        assert_eq!(window, WindowInfo::new("Untitled"));
        assert!(window_changed);
        assert!(!dialog_changed);
        assert_eq!(control, Some(c));
    }

    #[test]
    fn a_reading_started_before_a_newer_event_is_dropped() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, false);
        f.begin_read();
        f.winevent(ms(t, 70), false);
        let c = uia("A", Role::Button);
        assert_eq!(f.finish_read(ms(t, 80), reading("a.exe", "A", Some(c))), None);
        // The newer event is waiting for its own read.
        assert_eq!(f.next_deadline(), Some(ms(t, 120)));
    }

    #[test]
    fn a_stream_of_events_cannot_starve_the_reads() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, false);
        f.begin_read();
        f.winevent(ms(t, 10), false);
        assert_eq!(f.finish_read(ms(t, 20), reading("a.exe", "A", Some(uia("A", Role::Button)))), None);
        // A second read is also overtaken by an event, but it is not dropped.
        f.begin_read();
        f.winevent(ms(t, 30), false);
        let e = f.finish_read(ms(t, 40), reading("a.exe", "A", Some(uia("B", Role::Button))));
        assert!(e.is_some());
        // The next overtaken reading is dropped again.
        f.begin_read();
        f.winevent(ms(t, 50), false);
        assert_eq!(f.finish_read(ms(t, 60), reading("a.exe", "A", Some(uia("C", Role::Button)))), None);
    }

    #[test]
    fn a_window_change_survives_a_dropped_reading() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        f.begin_read();
        f.winevent(ms(t, 10), false);
        assert_eq!(f.finish_read(ms(t, 20), reading("a.exe", "A", Some(uia("A", Role::Button)))), None);
        let e = read(&mut f, ms(t, 80), reading("a.exe", "A", Some(uia("A", Role::Button)))).unwrap();
        assert!(parts(&e).0);
    }

    #[test]
    fn a_window_without_a_program_name_says_nothing() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        let r = nobody(0);
        assert_eq!(read(&mut f, ms(t, 60), r), None);
    }

    #[test]
    fn the_window_change_is_kept_for_the_next_window_with_a_name() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        let r = nobody(0);
        assert_eq!(read(&mut f, ms(t, 60), r), None);
        f.winevent(ms(t, 100), false);
        let e = read(&mut f, ms(t, 160), reading("a.exe", "A", Some(uia("A", Role::Button)))).unwrap();
        assert!(parts(&e).0);
    }

    #[test]
    fn an_unreadable_window_gives_one_event_when_it_becomes_the_foreground() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        let e = read(&mut f, ms(t, 60), reading("Taskmgr.exe", "Task Manager", None)).unwrap();
        assert_eq!(parts(&e), (true, None));
        // Task Manager raised 19 events: the rest give nothing.
        for n in 0..19u64 {
            let at = ms(t, 100 + n * 100);
            f.winevent(at, false);
            assert_eq!(read(&mut f, ms(at, 60), reading("Taskmgr.exe", "Task Manager", None)), None);
        }
    }

    #[test]
    fn no_control_and_no_window_change_says_nothing() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, false);
        assert_eq!(read(&mut f, ms(t, 60), reading("a.exe", "A", None)), None);
    }

    #[test]
    fn the_same_thing_twice_is_published_once() {
        let t = base();
        let mut f = FocusFilter::new();
        let c = uia("OK", Role::Button);
        f.winevent(t, true);
        assert!(read(&mut f, ms(t, 60), reading("a.exe", "A", Some(c.clone()))).is_some());
        f.winevent(ms(t, 200), false);
        assert_eq!(read(&mut f, ms(t, 260), reading("a.exe", "A", Some(c))), None);
    }

    #[test]
    fn the_same_control_from_another_source_is_published_once() {
        let t = base();
        let mut f = FocusFilter::new();
        let both = Control { source: FocusSource::Both, ..uia("Address", Role::Edit) };
        f.winevent(t, true);
        assert!(read(&mut f, ms(t, 60), reading("a.exe", "A", Some(both))).is_some());
        f.winevent(ms(t, 200), false);
        let alone = uia("Address", Role::Edit);
        assert_eq!(read(&mut f, ms(t, 260), reading("a.exe", "A", Some(alone))), None);
    }

    #[test]
    fn a_change_in_control_title_or_program_is_published() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        assert!(read(&mut f, ms(t, 60), reading("a.exe", "A", Some(uia("OK", Role::Button)))).is_some());
        let cases = [
            reading("a.exe", "A", Some(uia("Cancel", Role::Button))),
            reading("a.exe", "A2", Some(uia("Cancel", Role::Button))),
            reading("b.exe", "A2", Some(uia("Cancel", Role::Button))),
        ];
        for (i, r) in cases.into_iter().enumerate() {
            let at = ms(t, 200 + i as u64 * 200);
            f.winevent(at, false);
            let e = read(&mut f, ms(at, 60), r).unwrap();
            assert!(!parts(&e).0);
        }
    }

    #[test]
    fn focus_that_leaves_a_control_and_comes_back_is_not_a_duplicate() {
        let t = base();
        let mut f = FocusFilter::new();
        let a = uia("A", Role::Edit);
        let b = uia("B", Role::Edit);
        for (i, c) in [a.clone(), b, a].into_iter().enumerate() {
            let at = ms(t, i as u64 * 200);
            f.winevent(at, i == 0);
            assert!(read(&mut f, ms(at, 60), reading("a.exe", "A", Some(c))).is_some());
        }
    }

    #[test]
    fn coming_back_to_a_window_after_another_is_published_again() {
        let t = base();
        let mut f = FocusFilter::new();
        let c = uia("A", Role::Edit);
        f.winevent(t, true);
        assert!(read(&mut f, ms(t, 60), reading("a.exe", "A", Some(c.clone()))).is_some());
        // Switch to a window with no program name and back to the same control.
        f.winevent(ms(t, 200), true);
        let none = nobody(0);
        assert_eq!(read(&mut f, ms(t, 260), none), None);
        f.winevent(ms(t, 400), true);
        let e = read(&mut f, ms(t, 460), reading("a.exe", "A", Some(c))).unwrap();
        assert!(parts(&e).0);
    }

    #[test]
    fn a_repeat_with_the_same_dialog_text_is_a_duplicate_and_a_change_is_new() {
        let t = base();
        let mut f = FocusFilter::new();
        let with_text = |text: &str| {
            let mut r = reading("notepad.exe", "Notepad", Some(uia("OK", Role::Button)));
            r.window.dialog_text = vec![text.into()];
            r
        };
        f.winevent(t, true);
        let first = read(&mut f, ms(t, 60), with_text("Save changes?")).unwrap();
        assert!(parts(&first).0);
        f.winevent(ms(t, 200), false);
        assert_eq!(read(&mut f, ms(t, 260), with_text("Save changes?")), None);
        f.winevent(ms(t, 400), false);
        assert!(read(&mut f, ms(t, 460), with_text("Replace file?")).is_some());
    }

    // ── Window and dialog keys (ADR 0018) ──

    #[test]
    fn a_new_window_key_is_a_window_change_before_its_winevent() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        let e = read(&mut f, ms(t, 60), keyed("Explorer", uia("Files", Role::List), 1, None)).unwrap();
        assert!(flags(&e).0);
        // The dialog is already the foreground when its button's event is read.
        f.winevent(ms(t, 200), false);
        let e = read(&mut f, ms(t, 260), keyed("Replace or Skip Files", uia("Skip", Role::Button), 2, None));
        assert_eq!(flags(&e.unwrap()), (true, false));
    }

    #[test]
    fn the_same_window_key_with_no_winevent_is_no_window_change() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        assert!(read(&mut f, ms(t, 60), keyed("A", uia("A", Role::Edit), 1, None)).is_some());
        f.winevent(ms(t, 200), false);
        let e = read(&mut f, ms(t, 260), keyed("A", uia("B", Role::Edit), 1, None)).unwrap();
        assert_eq!(flags(&e), (false, false));
    }

    #[test]
    fn a_winevent_still_changes_the_window_with_the_same_key() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        assert!(read(&mut f, ms(t, 60), keyed("A", uia("A", Role::Edit), 1, None)).is_some());
        f.winevent(ms(t, 200), true);
        let e = read(&mut f, ms(t, 260), keyed("A", uia("A", Role::Edit), 1, None)).unwrap();
        assert!(flags(&e).0);
    }

    #[test]
    fn the_first_reading_changes_the_window_only_by_its_winevent() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, false);
        let e = read(&mut f, ms(t, 60), keyed("A", uia("A", Role::Edit), 5, None)).unwrap();
        assert!(!flags(&e).0);
    }

    #[test]
    fn a_reading_with_no_program_changes_no_key() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        assert!(read(&mut f, ms(t, 60), keyed("A", uia("A", Role::Edit), 1, None)).is_some());
        f.winevent(ms(t, 200), false);
        assert_eq!(read(&mut f, ms(t, 260), nobody(9)), None);
        // Back in window 1 with a new control: the key is still 1, so the window did not change.
        f.winevent(ms(t, 400), false);
        let e = read(&mut f, ms(t, 460), keyed("A", uia("B", Role::Edit), 1, None)).unwrap();
        assert!(!flags(&e).0);
    }

    #[test]
    fn entering_an_inner_dialog_is_a_dialog_change() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        assert!(read(&mut f, ms(t, 60), keyed("Notepad", uia("Text", Role::Document), 1, None)).is_some());
        f.winevent(ms(t, 200), false);
        let e = read(&mut f, ms(t, 260), keyed("Notepad", uia("Save", Role::Button), 1, Some(7))).unwrap();
        assert_eq!(flags(&e), (false, true));
        // Another control in the same dialog is no change.
        f.winevent(ms(t, 400), false);
        let r = keyed("Notepad", uia("Don't save", Role::Button), 1, Some(7));
        let e = read(&mut f, ms(t, 460), r).unwrap();
        assert_eq!(flags(&e), (false, false));
    }

    #[test]
    fn moving_to_another_page_is_a_dialog_change() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        let r = keyed("a Properties", uia("General", Role::TabItem), 1, Some(10));
        let e = read(&mut f, ms(t, 60), r).unwrap();
        // The window changed, so the page is spoken as part of it, not as a dialog change.
        assert_eq!(flags(&e), (true, false));
        f.winevent(ms(t, 200), false);
        let r = keyed("a Properties", uia("Security", Role::TabItem), 1, Some(11));
        let e = read(&mut f, ms(t, 260), r).unwrap();
        assert_eq!(flags(&e), (false, true));
    }

    #[test]
    fn leaving_an_inner_dialog_is_no_dialog_change() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        let r = keyed("a Properties", uia("Name", Role::Edit), 1, Some(10));
        assert!(read(&mut f, ms(t, 60), r).is_some());
        f.winevent(ms(t, 200), false);
        let e = read(&mut f, ms(t, 260), keyed("a Properties", uia("OK", Role::Button), 1, None)).unwrap();
        assert_eq!(flags(&e), (false, false));
    }

    #[test]
    fn a_new_dialog_key_alone_is_not_a_duplicate() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        assert!(read(&mut f, ms(t, 60), keyed("A", uia("OK", Role::Button), 1, Some(3))).is_some());
        f.winevent(ms(t, 200), false);
        assert_eq!(read(&mut f, ms(t, 260), keyed("A", uia("OK", Role::Button), 1, Some(3))), None);
        f.winevent(ms(t, 400), false);
        let e = read(&mut f, ms(t, 460), keyed("A", uia("OK", Role::Button), 1, Some(4))).unwrap();
        assert_eq!(flags(&e), (false, true));
        // Out of the dialog, the same control again is not a duplicate either.
        f.winevent(ms(t, 600), false);
        assert!(read(&mut f, ms(t, 660), keyed("A", uia("OK", Role::Button), 1, None)).is_some());
    }

    #[test]
    fn a_new_window_key_with_no_control_is_said_once() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        assert!(read(&mut f, ms(t, 60), keyed("A", uia("A", Role::Edit), 1, None)).is_some());
        f.winevent(ms(t, 200), false);
        let blank = Reading { control: None, ..keyed("Task Manager", uia("x", Role::Edit), 2, None) };
        assert!(flags(&read(&mut f, ms(t, 260), blank.clone()).unwrap()).0);
        f.winevent(ms(t, 400), false);
        assert_eq!(read(&mut f, ms(t, 460), blank), None);
    }

    #[test]
    fn a_new_dialog_with_no_control_says_nothing() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        assert!(read(&mut f, ms(t, 60), keyed("A", uia("A", Role::Edit), 1, None)).is_some());
        f.winevent(ms(t, 200), false);
        let blank = Reading { control: None, ..keyed("A", uia("x", Role::Edit), 1, Some(5)) };
        assert_eq!(read(&mut f, ms(t, 260), blank), None);
    }

    #[test]
    fn a_new_window_has_no_dialog_name() {
        assert!(WindowInfo::new("Notepad").dialog_name.is_empty());
    }

    // ── CoreEvent ──

    #[test]
    fn at_and_describe_cover_the_focus_event() {
        let t = base();
        let mut f = FocusFilter::new();
        f.winevent(t, true);
        let mut c = uia("Secret name", Role::Edit);
        c.states.password = Password::Yes;
        c.value = Value::Text("hunter2".into());
        let c = merge(Some(c), None);
        let e = read(&mut f, ms(t, 60), reading("brave.exe", "Login", c)).unwrap();
        assert_eq!(e.at(), ms(t, 60));
        let line = e.describe();
        assert_eq!(line, "focus Edit (uia) value withheld in brave.exe, new window");
        assert!(!line.contains("Secret") && !line.contains("hunter2"));

        let none = CoreEvent::FocusChanged {
            at: t,
            app: app("Taskmgr.exe"),
            window: WindowInfo::new("Task Manager"),
            window_changed: false,
            dialog_changed: false,
            control: None,
        };
        assert_eq!(none.describe(), "focus no control in Taskmgr.exe");

        let page = CoreEvent::FocusChanged {
            at: t,
            app: app("explorer.exe"),
            window: WindowInfo::new("a Properties"),
            window_changed: false,
            dialog_changed: true,
            control: None,
        };
        assert_eq!(page.describe(), "focus no control in explorer.exe, new dialog");
    }
}
