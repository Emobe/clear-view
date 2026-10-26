//! Focus probe (roadmap 9.1). A dev tool for measuring focus sources; not shipped.
//!
//! On every focus change it logs what UI Automation (uia) and MSAA (msaa) say about the
//! focused control: name, role, value and state, with the foreground window's title and
//! program. 9.2 runs it across the app list and records which source is right in
//! docs/prototypes/focus-sources.md; 9.3 decides the focus events from that.
//!
//! - Trigger: out-of-context WinEvents for foreground and focus changes, like the caret thread
//!   (ADR 0008). The callback only queues the event; the loop does the reading. Every event is
//!   logged as it arrives, with no filtering, so 9.3 can see the duplicates and noise.
//! - uia reads the focused element (`GetFocusedElement`) at read time, which may be a little
//!   later than the event. msaa reads the event's own object (`AccessibleObjectFromEvent` with
//!   the event's window, object and child ids). The docs say to call that inside the hook
//!   callback; here it runs from the loop to keep the callback short, and the log shows whether
//!   it still resolves.
//! - Passwords: a value is printed only when its own source says the field is not a password and
//!   the other source does not say it is (uia `IsPassword`, msaa `PROTECTED`). "Can't tell"
//!   counts as a password, so the log never holds one.
//! - Each source's time in ms is logged, and how long the event waited before it was read.
//! - Roadmap 9.6c1 (ADR 0017) adds three lines per event, for dialogs outside the foreground
//!   window: `gui` (`GetGUIThreadInfo(0)`: `hwndActive`, `hwndFocus`, and the focus window's
//!   root with the ADR's "read window" verdict), `chain` (the `GA_PARENT` windows from the focus
//!   window up to that root, the nearest dialog class marked) and `xaml` (the focused element's
//!   framework and the nearest `IsDialog` ancestor by `NormalizeElementBuildCache`). They print
//!   classes, titles and element names only, never a value.
//!
//! Run: `cargo run -p cv-platform-win --example focus_probe` (add `> probe.txt` to keep the
//! output). Ctrl+C stops it; it changes nothing system-wide, so there is nothing to restore.

fn main() {
    #[cfg(windows)]
    probe::run();
}

#[cfg(windows)]
mod probe {
    use std::{
        cell::RefCell,
        time::{Duration, Instant},
    };

    use windows::{
        Win32::{
            Foundation::{CloseHandle, HWND},
            System::{
                Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
                Threading::{
                    INFINITE, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                    QueryFullProcessImageNameW,
                },
                Variant::{VARIANT, VT_I4},
            },
            UI::{
                Accessibility::{
                    AccessibleObjectFromEvent, CUIAutomation8, ExpandCollapseState_Collapsed,
                    ExpandCollapseState_Expanded, ExpandCollapseState_LeafNode,
                    ExpandCollapseState_PartiallyExpanded, GetRoleTextW, GetStateTextW,
                    HWINEVENTHOOK, IAccessible, IUIAutomation, IUIAutomation2, IUIAutomationElement,
                    IUIAutomationElement9, IUIAutomationExpandCollapsePattern,
                    IUIAutomationSelectionItemPattern, IUIAutomationTextPattern,
                    IUIAutomationTogglePattern, IUIAutomationValuePattern, SetWinEventHook,
                    ToggleState_Indeterminate, ToggleState_Off, ToggleState_On,
                    UIA_ControlTypePropertyId, UIA_ExpandCollapsePatternId,
                    UIA_FrameworkIdPropertyId, UIA_InvokePatternId, UIA_IsDialogPropertyId,
                    UIA_NamePropertyId, UIA_NativeWindowHandlePropertyId, UIA_PATTERN_ID,
                    UIA_RangeValuePatternId, UIA_ScrollPatternId, UIA_SelectionItemPatternId,
                    UIA_SelectionPatternId, UIA_TextPatternId, UIA_TogglePatternId,
                    UIA_ValuePatternId, UIA_WindowPatternId,
                },
                WindowsAndMessaging::{
                    CHILDID_SELF, DispatchMessageW, EVENT_OBJECT_FOCUS, EVENT_SYSTEM_FOREGROUND,
                    GA_PARENT, GA_ROOT, GUITHREADINFO, GetAncestor, GetClassNameW,
                    GetForegroundWindow, GetGUIThreadInfo, GetWindowTextW,
                    GetWindowThreadProcessId, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx,
                    PM_REMOVE, PeekMessageW, QS_ALLINPUT, TranslateMessage, WINEVENT_OUTOFCONTEXT,
                    WINEVENT_SKIPOWNPROCESS,
                },
            },
        },
        core::{BSTR, Error, PWSTR, Result},
    };

    /// `STATE_SYSTEM_PROTECTED`, the MSAA way of saying "password".
    const STATE_PROTECTED: u32 = 0x2000_0000;
    /// Longest text printed for a name, value or title; the rest is cut.
    const MAX_TEXT: usize = 80;
    /// UI Automation's timeouts, as on the caret thread, so one hung app cannot stall the probe.
    const UIA_TIMEOUT_MS: u32 = 500;
    /// The most `GA_PARENT` steps the `chain` line takes from the focus window (ADR 0017, I2).
    const CHAIN_MAX: usize = 16;

    /// What the hook callback saw, queued for the loop.
    struct Event {
        event: u32,
        hwnd: HWND,
        id_object: i32,
        id_child: i32,
        at: Instant,
    }

    thread_local! {
        /// Filled by `on_win_event`, drained by the loop. Out-of-context events arrive on the
        /// thread that set the hook, inside its message retrieval, so the two never overlap.
        static QUEUE: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
    }

    pub fn run() {
        // MTA first, on a thread that owns no window (ADR 0003's rules, kept by ADR 0008).
        if let Err(e) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
            eprintln!("[probe] CoInitializeEx failed: {}", describe(&e));
        }
        let uia = create_uia();

        // Kept for the life of the process; Ctrl+C ends it and the hooks go with it.
        let mut hooks = Vec::new();
        for event in [EVENT_SYSTEM_FOREGROUND, EVENT_OBJECT_FOCUS] {
            let hook = unsafe {
                SetWinEventHook(
                    event,
                    event,
                    None,
                    Some(on_win_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                )
            };
            if hook.is_invalid() {
                eprintln!("[probe] SetWinEventHook failed for event {event:#x}");
            } else {
                hooks.push(hook);
            }
        }

        println!(
            "focus probe: one block per foreground or focus event, no filtering. Values of \
             password fields, and of fields where neither source can say, are withheld. \
             Ctrl+C to stop."
        );
        let start = Instant::now();
        let mut msg = MSG::default();
        loop {
            let _ = unsafe {
                MsgWaitForMultipleObjectsEx(None, INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
            };
            // WinEvent callbacks run inside PeekMessageW.
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            for event in QUEUE.take() {
                report(uia.as_ref(), start, &event);
            }
        }
    }

    fn create_uia() -> Option<IUIAutomation> {
        use windows::core::Interface;
        let uia: IUIAutomation =
            match unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) } {
                Ok(uia) => uia,
                Err(e) => {
                    eprintln!("[probe] UI Automation unavailable: {}", describe(&e));
                    return None;
                }
            };
        match uia.cast::<IUIAutomation2>() {
            Ok(uia2) => unsafe {
                let _ = uia2.SetConnectionTimeout(UIA_TIMEOUT_MS);
                let _ = uia2.SetTransactionTimeout(UIA_TIMEOUT_MS);
            },
            Err(e) => eprintln!(
                "[probe] IUIAutomation2 unavailable ({}); UIA timeouts stay 2 s and 20 s",
                describe(&e)
            ),
        }
        Some(uia)
    }

    /// Only records what happened: out-of-context callbacks must return quickly (Microsoft
    /// docs), and the loop does the reading.
    unsafe extern "system" fn on_win_event(
        _hook: HWINEVENTHOOK,
        event: u32,
        hwnd: HWND,
        id_object: i32,
        id_child: i32,
        _thread: u32,
        _time: u32,
    ) {
        QUEUE.with(|queue| {
            queue.borrow_mut().push(Event { event, hwnd, id_object, id_child, at: Instant::now() })
        });
    }

    // ── One event ────────────────────────────────────────────────────────────────

    fn report(uia: Option<&IUIAutomation>, start: Instant, event: &Event) {
        let waited = event.at.elapsed();
        println!(
            "[{:>7.2}s] {} hwnd {} object {} child {}   read {:.1} ms after the event",
            event.at.duration_since(start).as_secs_f64(),
            event_name(event.event),
            hex(event.hwnd),
            event.id_object,
            event.id_child,
            waited.as_secs_f64() * 1000.0
        );

        let fg = unsafe { GetForegroundWindow() };
        println!("  fg    {}", window_line(fg));
        if !event.hwnd.is_invalid() {
            let root = unsafe { GetAncestor(event.hwnd, GA_ROOT) };
            if root != fg {
                println!("  root  {}", window_line(root));
            }
        }
        let read_window = gui_report(fg);

        // Read everything about both sources first: each one's password answer gates the
        // other's value.
        let t = Instant::now();
        let uia_focus = uia_snapshot(uia);
        let mut uia_time = t.elapsed();
        let t = Instant::now();
        let msaa_focus = msaa_snapshot(event);
        let mut msaa_time = t.elapsed();

        let uia_password = uia_focus.as_ref().ok().and_then(|u| u.password);
        let msaa_protected = msaa_focus.as_ref().ok().and_then(|m| m.protected);

        match &uia_focus {
            Ok(u) => {
                let t = Instant::now();
                let value = if cv_core::focus::value_allowed(uia_password, msaa_protected) {
                    uia_value(&u.element)
                } else {
                    withheld(uia_password, msaa_protected).into()
                };
                uia_time += t.elapsed();
                println!("  uia   {}\n        value {value}   {}", u.text, ms(uia_time));
                if let Some(uia) = uia {
                    println!("  xaml  {}", xaml_line(uia, &u.element, read_window));
                }
            }
            Err(msg) => println!("  uia   {msg}   {}", ms(uia_time)),
        }
        match &msaa_focus {
            Ok(m) => {
                let t = Instant::now();
                let value = if cv_core::focus::value_allowed(msaa_protected, uia_password) {
                    msaa_value(m)
                } else {
                    withheld(msaa_protected, uia_password).into()
                };
                msaa_time += t.elapsed();
                println!("  msaa  {}\n        value {value}   {}", m.text, ms(msaa_time));
            }
            Err(msg) => println!("  msaa  {msg}   {}", ms(msaa_time)),
        }
    }

    fn event_name(event: u32) -> &'static str {
        match event {
            EVENT_SYSTEM_FOREGROUND => "EVENT_SYSTEM_FOREGROUND",
            EVENT_OBJECT_FOCUS => "EVENT_OBJECT_FOCUS",
            _ => "event ?",
        }
    }

    fn ms(d: Duration) -> String {
        format!("{:.1} ms", d.as_secs_f64() * 1000.0)
    }

    // ── The password rule ────────────────────────────────────────────────────────

    // May a source's value be printed? `cv_core::focus::value_allowed` (ADR 0015).

    /// What to print in place of a value that may not be shown.
    fn withheld(own: Option<bool>, other: Option<bool>) -> &'static str {
        if own == Some(true) || other == Some(true) {
            "<withheld: password>"
        } else {
            "<withheld: can't tell>"
        }
    }

    // ── UI Automation ────────────────────────────────────────────────────────────

    struct UiaFocus {
        element: IUIAutomationElement,
        /// `IsPassword`; `None` when the property could not be read.
        password: Option<bool>,
        text: String,
    }

    fn uia_snapshot(uia: Option<&IUIAutomation>) -> std::result::Result<UiaFocus, String> {
        let Some(uia) = uia else {
            return Err("none (UI Automation unavailable)".into());
        };
        let element = unsafe { uia.GetFocusedElement() }
            .map_err(|e| format!("{} (GetFocusedElement)", describe(&e)))?;

        let password = unsafe { element.CurrentIsPassword() }.ok().map(|b| b.as_bool());
        let control_type = unsafe { element.CurrentControlType() }.map(|t| t.0);
        let type_text = match control_type {
            Ok(id) => format!("{} {id}", control_type_name(id)),
            Err(e) => describe(&e),
        };

        let mut text = format!(
            "name {:?}  type {type_text} ({})  class {:?}  id {:?}  fw {:?}  pid {}",
            bstr(unsafe { element.CurrentName() }),
            bstr(unsafe { element.CurrentLocalizedControlType() }),
            bstr(unsafe { element.CurrentClassName() }),
            bstr(unsafe { element.CurrentAutomationId() }),
            bstr(unsafe { element.CurrentFrameworkId() }),
            unsafe { element.CurrentProcessId() }.unwrap_or(-1),
        );
        text.push_str(&format!("\n        state {}", uia_state(&element, password)));
        text.push_str(&format!("\n        patterns {}", uia_patterns(&element)));
        Ok(UiaFocus { element, password, text })
    }

    /// Enabled, offscreen, keyboard focus, password, and the state each pattern gives.
    fn uia_state(element: &IUIAutomationElement, password: Option<bool>) -> String {
        let flag = |r: Result<windows::core::BOOL>, yes: &str, no: &str| match r {
            Ok(b) if b.as_bool() => yes.to_string(),
            Ok(_) => no.to_string(),
            Err(_) => format!("{yes}?"),
        };
        let mut parts = vec![
            flag(unsafe { element.CurrentIsEnabled() }, "enabled", "disabled"),
            flag(unsafe { element.CurrentIsOffscreen() }, "offscreen", "onscreen"),
            flag(unsafe { element.CurrentHasKeyboardFocus() }, "keyboard-focus", "no-keyboard-focus"),
            match password {
                Some(true) => "password".into(),
                Some(false) => "not-password".into(),
                None => "password?".into(),
            },
        ];
        if let Ok(p) = unsafe {
            element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
        } && let Ok(ro) = unsafe { p.CurrentIsReadOnly() }
        {
            parts.push(if ro.as_bool() { "read-only" } else { "editable" }.into());
        }
        if let Ok(p) =
            unsafe { element.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) }
            && let Ok(state) = unsafe { p.CurrentToggleState() }
        {
            let name = match state {
                s if s == ToggleState_Off => "off",
                s if s == ToggleState_On => "on",
                s if s == ToggleState_Indeterminate => "indeterminate",
                _ => "?",
            };
            parts.push(format!("toggle {name}"));
        }
        if let Ok(p) = unsafe {
            element.GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(
                UIA_ExpandCollapsePatternId,
            )
        } && let Ok(state) = unsafe { p.CurrentExpandCollapseState() }
        {
            let name = match state {
                s if s == ExpandCollapseState_Collapsed => "collapsed",
                s if s == ExpandCollapseState_Expanded => "expanded",
                s if s == ExpandCollapseState_PartiallyExpanded => "partly-expanded",
                s if s == ExpandCollapseState_LeafNode => "leaf",
                _ => "?",
            };
            parts.push(name.into());
        }
        if let Ok(p) = unsafe {
            element.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                UIA_SelectionItemPatternId,
            )
        } && let Ok(selected) = unsafe { p.CurrentIsSelected() }
        {
            parts.push(if selected.as_bool() { "selected" } else { "not-selected" }.into());
        }
        parts.join(", ")
    }

    /// The patterns the element offers, from the ones 9.5 to 9.8 will care about.
    fn uia_patterns(element: &IUIAutomationElement) -> String {
        const PATTERNS: [(&str, UIA_PATTERN_ID); 9] = [
            ("Invoke", UIA_InvokePatternId),
            ("Value", UIA_ValuePatternId),
            ("RangeValue", UIA_RangeValuePatternId),
            ("Toggle", UIA_TogglePatternId),
            ("ExpandCollapse", UIA_ExpandCollapsePatternId),
            ("Selection", UIA_SelectionPatternId),
            ("SelectionItem", UIA_SelectionItemPatternId),
            ("Text", UIA_TextPatternId),
            ("Scroll", UIA_ScrollPatternId),
        ];
        let mut names: Vec<&str> = PATTERNS
            .iter()
            .filter(|(_, id)| unsafe { element.GetCurrentPattern(*id) }.is_ok())
            .map(|(name, _)| *name)
            .collect();
        if unsafe { element.GetCurrentPattern(UIA_WindowPatternId) }.is_ok() {
            names.push("Window");
        }
        if names.is_empty() { "none".into() } else { names.join(",") }
    }

    /// The Value pattern's value, else the start of the Text pattern's document.
    /// Only called when `value_allowed` allowed it.
    fn uia_value(element: &IUIAutomationElement) -> String {
        if let Ok(p) =
            unsafe { element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
        {
            return match unsafe { p.CurrentValue() } {
                Ok(v) => format!("{:?} (ValuePattern)", clip(&v.to_string())),
                Err(e) => format!("{} (ValuePattern.CurrentValue)", describe(&e)),
            };
        }
        if let Ok(p) =
            unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
        {
            let text = unsafe { p.DocumentRange() }
                .and_then(|range| unsafe { range.GetText(MAX_TEXT as i32 + 1) });
            return match text {
                Ok(t) => format!("{:?} (TextPattern document)", clip(&t.to_string())),
                Err(e) => format!("{} (TextPattern document)", describe(&e)),
            };
        }
        "none (no Value or Text pattern)".into()
    }

    /// UI Automation control type ids 50000 to 50040, by name.
    fn control_type_name(id: i32) -> &'static str {
        const NAMES: [&str; 41] = [
            "Button", "Calendar", "CheckBox", "ComboBox", "Edit", "Hyperlink", "Image",
            "ListItem", "List", "Menu", "MenuBar", "MenuItem", "ProgressBar", "RadioButton",
            "ScrollBar", "Slider", "Spinner", "StatusBar", "Tab", "TabItem", "Text", "ToolBar",
            "ToolTip", "Tree", "TreeItem", "Custom", "Group", "Thumb", "DataGrid", "DataItem",
            "Document", "SplitButton", "Window", "Pane", "Header", "HeaderItem", "Table",
            "TitleBar", "Separator", "SemanticZoom", "AppBar",
        ];
        usize::try_from(id - 50000).ok().and_then(|i| NAMES.get(i)).copied().unwrap_or("?")
    }

    // ── MSAA ─────────────────────────────────────────────────────────────────────

    struct MsaaFocus {
        acc: IAccessible,
        child: VARIANT,
        /// The `PROTECTED` state bit; `None` when the state could not be read.
        protected: Option<bool>,
        text: String,
    }

    fn msaa_snapshot(event: &Event) -> std::result::Result<MsaaFocus, String> {
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
        .map_err(|e| format!("{} (AccessibleObjectFromEvent)", describe(&e)))?;
        let acc = acc.ok_or("none (AccessibleObjectFromEvent gave no object)")?;

        let state = unsafe { acc.get_accState(&child) }.ok().and_then(|v| variant_i4(&v));
        let protected = state.map(|s| s as u32 & STATE_PROTECTED != 0);
        let role = match unsafe { acc.get_accRole(&child) } {
            Ok(v) => match variant_i4(&v) {
                Some(role) => format!("{} {role}", role_text(role as u32)),
                None => "non-standard role".into(),
            },
            Err(e) => describe(&e),
        };
        let state_text = match state {
            Some(s) => state_names(s as u32),
            None => "?".into(),
        };
        let text = format!(
            "name {:?}  role {role}  child {}\n        state {state_text}",
            bstr(unsafe { acc.get_accName(&child) }),
            variant_i4(&child).map_or("?".to_string(), |c| {
                if c == CHILDID_SELF as i32 { "self".into() } else { c.to_string() }
            }),
        );
        Ok(MsaaFocus { acc, child, protected, text })
    }

    fn msaa_value(msaa: &MsaaFocus) -> String {
        match unsafe { msaa.acc.get_accValue(&msaa.child) } {
            Ok(v) => format!("{:?}", clip(&v.to_string())),
            Err(e) => describe(&e),
        }
    }

    fn variant_i4(v: &VARIANT) -> Option<i32> {
        // SAFETY: the union field read is the one `vt` says is set.
        unsafe {
            let inner = &*v.Anonymous.Anonymous;
            if inner.vt == VT_I4 { Some(inner.Anonymous.lVal) } else { None }
        }
    }

    fn role_text(role: u32) -> String {
        let mut buf = [0u16; 128];
        let len = unsafe { GetRoleTextW(role, Some(&mut buf)) } as usize;
        String::from_utf16_lossy(&buf[..len.min(buf.len())])
    }

    /// The names of the set bits of an MSAA state mask, comma separated.
    fn state_names(state: u32) -> String {
        if state == 0 {
            return "normal".into();
        }
        let mut names = Vec::new();
        for bit in (0..32).map(|i| 1u32 << i).filter(|bit| state & bit != 0) {
            let mut buf = [0u16; 64];
            let len = unsafe { GetStateTextW(bit, Some(&mut buf)) } as usize;
            names.push(String::from_utf16_lossy(&buf[..len.min(buf.len())]));
        }
        names.join(",")
    }

    // ── Dialogs outside the foreground window (roadmap 9.6c1, ADR 0017) ──────────

    /// The dialog classes of ADR 0016 (`focus/map.rs` `is_dialog_class`; an example cannot reach
    /// that module, so the list is repeated here).
    fn is_dialog_class(class: &str) -> bool {
        matches!(class, "#32770" | "NUIDialog" | "Shell_Dialog" | "Shell_SystemDialog")
    }

    /// The part of a window the verdicts look at.
    struct Link {
        class: String,
        title: String,
    }

    /// ADR 0017 W2: would the focus window's root replace the foreground window?
    #[derive(Debug, PartialEq)]
    enum Verdict {
        SameAsForeground,
        ReadWindow,
        ForegroundStays,
    }

    impl Verdict {
        fn text(&self) -> &'static str {
            match self {
                Verdict::SameAsForeground => "focus root IS the foreground window",
                Verdict::ReadWindow => {
                    "focus root DIFFERS from the foreground and has a title or a dialog class: \
                     it would be the read window"
                }
                Verdict::ForegroundStays => {
                    "focus root differs from the foreground but has no title and no dialog \
                     class: the foreground window stays"
                }
            }
        }
    }

    fn read_window_verdict(root_is_foreground: bool, root: &Link) -> Verdict {
        if root_is_foreground {
            Verdict::SameAsForeground
        } else if !root.title.is_empty() || is_dialog_class(&root.class) {
            Verdict::ReadWindow
        } else {
            Verdict::ForegroundStays
        }
    }

    /// The nearest window with a dialog class on the way up (ADR 0017 I2). `chain` starts at the
    /// focus window and ends at its root when `reached_root` is true; the root is the read
    /// window, never an inner dialog, so it is left out. Gives the index into `chain`.
    fn inner_dialog(chain: &[Link], reached_root: bool) -> Option<usize> {
        let below_root = if reached_root { chain.len().saturating_sub(1) } else { chain.len() };
        chain[..below_root].iter().position(|link| is_dialog_class(&link.class))
    }

    /// The `GA_PARENT` windows from `focus` up to `root`, at most `CHAIN_MAX` steps; the flag says
    /// whether the root was reached.
    fn parent_chain(focus: HWND, root: HWND) -> (Vec<HWND>, bool) {
        let mut chain = vec![focus];
        let mut current = focus;
        while current != root && chain.len() <= CHAIN_MAX {
            current = unsafe { GetAncestor(current, GA_PARENT) };
            if current.is_invalid() {
                return (chain, false);
            }
            chain.push(current);
        }
        let reached = current == root;
        (chain, reached)
    }

    /// Prints the `gui` and `chain` lines. Returns the read window ADR 0017 W2 would pick: the
    /// focus window's root when the verdict says so, else the foreground window.
    fn gui_report(fg: HWND) -> HWND {
        let t = Instant::now();
        let mut info =
            GUITHREADINFO { cbSize: size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        let got = unsafe { GetGUIThreadInfo(0, &mut info) };
        let took = ms(t.elapsed());
        if let Err(e) = got {
            println!("  gui   {} (GetGUIThreadInfo)   {took}", describe(&e));
            return fg;
        }
        println!(
            "  gui   active {}  focus {}   {took}",
            hex_or_none(info.hwndActive),
            hex_or_none(info.hwndFocus)
        );
        if info.hwndFocus.is_invalid() {
            println!("        no focus window: the foreground window stays");
            return fg;
        }
        let root = unsafe { GetAncestor(info.hwndFocus, GA_ROOT) };
        if root.is_invalid() {
            println!("        focus window has no root: the foreground window stays");
            return fg;
        }
        let link = |hwnd| Link { class: class_name(hwnd), title: window_text(hwnd) };
        let verdict = read_window_verdict(root == fg, &link(root));
        if verdict == Verdict::SameAsForeground {
            println!("        focus root {}: {}", hex(root), verdict.text());
        } else {
            println!("        focus root {}\n        {}", window_line(root), verdict.text());
        }

        let (windows, reached) = parent_chain(info.hwndFocus, root);
        let links: Vec<Link> = windows.iter().map(|hwnd| link(*hwnd)).collect();
        let inner = inner_dialog(&links, reached);
        println!(
            "  chain {} window(s) from the focus window{}",
            links.len(),
            if reached { " to its root" } else { " (cut: the root was not reached)" }
        );
        for (i, (hwnd, link)) in windows.iter().zip(&links).enumerate() {
            let mark = if inner == Some(i) {
                "   <- inner dialog"
            } else if reached && i + 1 == links.len() {
                "   <- root"
            } else if is_dialog_class(&link.class) {
                "   (dialog class)"
            } else {
                ""
            };
            println!("        {i}: {} class {:?} \"{}\"{mark}", hex(*hwnd), link.class, link.title);
        }
        println!(
            "        inner dialog: {}",
            inner.map_or("none".to_string(), |i| format!("#{i}"))
        );

        if verdict == Verdict::ReadWindow { root } else { fg }
    }

    /// The `xaml` line: the focused element's framework, then what the nearest `IsDialog`
    /// ancestor is (ADR 0017 I2). One `NormalizeElementBuildCache` call, timed alone.
    fn xaml_line(uia: &IUIAutomation, focused: &IUIAutomationElement, read_window: HWND) -> String {
        let framework = bstr(unsafe { focused.CurrentFrameworkId() });
        let (found, took) = normalize_dialog(uia, focused);
        let found = match found {
            Ok(element) => describe_normalized(&element, read_window),
            Err(e) => format!("normalize failed: {}", describe(&e)),
        };
        format!("focused fw {framework:?}\n        normalize {found}   {}", ms(took))
    }

    /// `NormalizeElementBuildCache` from `focused` with a walker on `IsDialog == true`, and the
    /// time of that one call (zero when the setup failed). Normalize returns the root when no
    /// ancestor matches, so the caller must read `IsDialog` on the result.
    fn normalize_dialog(
        uia: &IUIAutomation,
        focused: &IUIAutomationElement,
    ) -> (Result<IUIAutomationElement>, Duration) {
        let setup = || -> Result<_> {
            let condition =
                unsafe { uia.CreatePropertyCondition(UIA_IsDialogPropertyId, &true.into()) }?;
            let walker = unsafe { uia.CreateTreeWalker(&condition) }?;
            let request = unsafe { uia.CreateCacheRequest() }?;
            for property in [
                UIA_NamePropertyId,
                UIA_ControlTypePropertyId,
                UIA_FrameworkIdPropertyId,
                UIA_IsDialogPropertyId,
                UIA_NativeWindowHandlePropertyId,
            ] {
                unsafe { request.AddProperty(property) }?;
            }
            Ok((walker, request))
        };
        let (walker, request) = match setup() {
            Ok(pair) => pair,
            Err(e) => return (Err(e), Duration::ZERO),
        };
        let t = Instant::now();
        let found = unsafe { walker.NormalizeElementBuildCache(focused, &request) };
        (found, t.elapsed())
    }

    fn describe_normalized(element: &IUIAutomationElement, read_window: HWND) -> String {
        use windows::core::Interface;
        let is_dialog = element
            .cast::<IUIAutomationElement9>()
            .ok()
            .and_then(|e| unsafe { e.CachedIsDialog() }.ok())
            .map(|b| b.as_bool());
        let verdict = match is_dialog {
            Some(true) => "an IsDialog ancestor",
            Some(false) => "no IsDialog ancestor (the root came back)",
            None => "IsDialog unreadable",
        };
        let control_type = match unsafe { element.CachedControlType() } {
            Ok(id) => format!("{} {}", control_type_name(id.0), id.0),
            Err(e) => describe(&e),
        };
        let hwnd = unsafe { element.CachedNativeWindowHandle() }.unwrap_or_default();
        let hwnd_text = if hwnd.is_invalid() {
            "no window".to_string()
        } else if hwnd == read_window {
            format!("{} = the read window", hex(hwnd))
        } else {
            hex(hwnd)
        };
        format!(
            "{verdict}: name {:?}  type {control_type}  fw {:?}  hwnd {hwnd_text}",
            bstr(unsafe { element.CachedName() }),
            bstr(unsafe { element.CachedFrameworkId() }),
        )
    }

    fn hex_or_none(hwnd: HWND) -> String {
        if hwnd.is_invalid() { "none".into() } else { hex(hwnd) }
    }

    // ── Windows and text ─────────────────────────────────────────────────────────

    fn window_line(hwnd: HWND) -> String {
        if hwnd.is_invalid() {
            return "none".into();
        }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        format!(
            "{} {} \"{}\" class {:?}",
            hex(hwnd),
            process_name(pid),
            window_text(hwnd),
            class_name(hwnd)
        )
    }

    fn window_text(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        let len = unsafe { GetWindowTextW(hwnd, &mut buf) }.max(0) as usize;
        clip(&String::from_utf16_lossy(&buf[..len]))
    }

    fn class_name(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        let len = unsafe { GetClassNameW(hwnd, &mut buf) }.max(0) as usize;
        String::from_utf16_lossy(&buf[..len])
    }

    fn process_name(pid: u32) -> String {
        let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
        else {
            return "?".into();
        };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let queried = unsafe {
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
        };
        let _ = unsafe { CloseHandle(process) };
        match queried {
            Ok(()) => {
                let path = String::from_utf16_lossy(&buf[..len as usize]);
                path.rsplit('\\').next().unwrap_or(&path).to_string()
            }
            Err(_) => "?".into(),
        }
    }

    /// One line, at most `MAX_TEXT` characters: control characters become spaces and a cut is
    /// marked with `...`.
    fn clip(s: &str) -> String {
        let flat: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        if flat.chars().count() > MAX_TEXT {
            let mut cut: String = flat.chars().take(MAX_TEXT).collect();
            cut.push_str("...");
            cut
        } else {
            flat
        }
    }

    fn bstr(r: Result<BSTR>) -> String {
        match r {
            Ok(b) => clip(&b.to_string()),
            Err(e) => format!("?({:#x})", e.code().0),
        }
    }

    fn hex(hwnd: HWND) -> String {
        format!("{:#x}", hwnd.0 as usize)
    }

    fn describe(e: &Error) -> String {
        format!("error {:#010x} {}", e.code().0, e.message())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn either_source_saying_password_withholds_both_values() {
            assert_eq!(withheld(Some(true), None), "<withheld: password>");
            assert_eq!(withheld(Some(false), Some(true)), "<withheld: password>");
        }

        #[test]
        fn cant_tell_counts_as_a_password() {
            assert_eq!(withheld(None, None), "<withheld: can't tell>");
            assert_eq!(withheld(None, Some(false)), "<withheld: can't tell>");
        }

        #[test]
        fn control_types_are_named_and_unknown_ids_are_marked() {
            assert_eq!(control_type_name(50000), "Button");
            assert_eq!(control_type_name(50004), "Edit");
            assert_eq!(control_type_name(50040), "AppBar");
            assert_eq!(control_type_name(50041), "?");
            assert_eq!(control_type_name(0), "?");
            assert_eq!(control_type_name(-1), "?");
        }

        fn link(class: &str, title: &str) -> Link {
            Link { class: class.into(), title: title.into() }
        }

        #[test]
        fn the_dialog_classes_are_the_four_of_adr_0016() {
            for class in ["#32770", "NUIDialog", "Shell_Dialog", "Shell_SystemDialog"] {
                assert!(is_dialog_class(class), "{class}");
            }
            assert!(!is_dialog_class("Notepad"));
            assert!(!is_dialog_class("#32768"));
        }

        #[test]
        fn a_focus_root_that_is_the_foreground_changes_nothing() {
            let root = link("CabinetWClass", "Documents");
            assert_eq!(read_window_verdict(true, &root), Verdict::SameAsForeground);
        }

        #[test]
        fn another_root_is_the_read_window_only_with_a_title_or_a_dialog_class() {
            assert_eq!(
                read_window_verdict(false, &link("OperationStatusWindow", "Replace or Skip Files")),
                Verdict::ReadWindow
            );
            assert_eq!(read_window_verdict(false, &link("#32770", "")), Verdict::ReadWindow);
            assert_eq!(
                read_window_verdict(false, &link("ComboLBox", "")),
                Verdict::ForegroundStays
            );
        }

        #[test]
        fn the_inner_dialog_is_the_nearest_dialog_class_below_the_root() {
            // A control in a property page in a property sheet: the page is the inner dialog.
            let chain = [link("Button", "Read-only"), link("#32770", "General"), link("#32770", "Properties")];
            assert_eq!(inner_dialog(&chain, true), Some(1));
        }

        #[test]
        fn the_root_is_never_the_inner_dialog() {
            // Focus on the sheet's tab control: only the sheet (the root) is above it.
            let chain = [link("SysTabControl32", "Tab1"), link("#32770", "Properties")];
            assert_eq!(inner_dialog(&chain, true), None);
            // The focus window being the root itself.
            assert_eq!(inner_dialog(&[link("#32770", "Save as")], true), None);
        }

        #[test]
        fn the_focus_window_can_be_the_inner_dialog() {
            let chain = [link("#32770", "Page"), link("#32770", "Sheet")];
            assert_eq!(inner_dialog(&chain, true), Some(0));
        }

        #[test]
        fn a_cut_chain_counts_every_window_it_has() {
            let chain = [link("Edit", ""), link("#32770", "Page")];
            assert_eq!(inner_dialog(&chain, false), Some(1));
            assert_eq!(inner_dialog(&[], false), None);
        }

        #[test]
        fn text_is_one_line_and_cut_at_the_limit() {
            assert_eq!(clip("a\r\nb\tc"), "a  b c");
            let long = "x".repeat(MAX_TEXT + 5);
            assert_eq!(clip(&long), format!("{}...", "x".repeat(MAX_TEXT)));
            assert_eq!(clip(&"y".repeat(MAX_TEXT)), "y".repeat(MAX_TEXT));
        }
    }
}
