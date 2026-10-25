# 0015: Focus events

Status: Accepted (you, 2026-10-25; set by the agent on your instruction)
Date: 2026-10-25

## Context

Roadmap 9.3, the first ADR of speech stage 1 (Phase 9, PRODUCT.md "Speech (v1.1)"). It decides focus events in the core: which sources are read and in what order for each app type, what an event carries, how duplicate and noisy events are filtered, and how focus reading shares UI Automation with the caret thread. What gets spoken for a focused control is 9.5. Window and dialog speech is 9.6. Positions such as "3 of 7" are 9.7. Whether the magnifier follows focus is 9.9.

### Rules this must keep (ROADMAP "Rules for speech steps", PRODUCT.md)

- With speech switched off, the magnifier behaves exactly as before.
- Password fields are never spoken: no characters, no words, no length. "Can't tell" counts as a password field.
- No UI Automation or MSAA call runs on the render thread. A hung app must not freeze the magnifier or stop speech from being silenced.
- The text that gets spoken is pure logic in cv-core with unit tests.
- Only cv-platform-win and cv-tts depend on `windows`.

### Earlier decisions this touches

- **ADR 0004** (Accepted), core event rules: one `#[non_exhaustive]` enum in cv-core; plain data in physical virtual-screen pixels plus a timestamp, no OS handles or COM pointers; each event names its source; fan-out delivery, and a consumer never blocks a source.
- **ADR 0008** (Accepted): the caret thread in cv-platform-win is MTA COM, owns no window, adds no UIA event handlers, holds `CUIAutomation8` with both timeouts lowered to 500 ms, and wakes on out-of-context WinEvents (foreground, focus, caret show, hide and move). Its option 1 describes it as holding "every core UIA and MSAA object". When it was written there was no other core reader. It left the reader's UIA to the reader's ADR.
- **ADR 0013** (Accepted): the speech thread makes no UIA or MSAA call. "Events carry the text they need (9.3 decides the fields). The speech thread never asks UIA for more." Kinds `Focus` and `Command` are foreground: each new one cuts off everything. A batch cuts off what its first item's rule says, then the rest queue behind it, for example a dialog's title and then its text. Core events older than a silence are dropped. Its Revisit list names 9.3: "which fields events carry, and whether focus has its own thread".
- STATUS "Speech audit (8.1)", open question for 9.3: whether focus uses the caret thread's UIA instance or a second one. Conflict 3: the old cv-tts focus handler sent focus on a private channel, not through `CoreEvent` (cv-tts no longer has it: 8.7b deleted the UIA parts).

### What the code does today

- `CoreEvent` has `CaretMoved` and `CaretLost` (crates/cv-core/src/events.rs). The render thread's `Tracker::handle` (cv-core tracking.rs:181) matches both variants with no catch-all, so a new variant needs a new arm there.
- The speech thread subscribes with `EventHub::subscribe_with_wake` and turns each event into utterances with `speech::utterances_for`, which returns nothing for every event (cv-core speech.rs:258).
- The caret thread (crates/cv-platform-win/src/caret/mod.rs) hooks `EVENT_SYSTEM_FOREGROUND`, `EVENT_OBJECT_FOCUS`, `EVENT_OBJECT_SHOW` to `HIDE` and `EVENT_OBJECT_LOCATIONCHANGE` with `WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS`. The callback only sets wake flags, and the loop reads. It reads nothing when the foreground window is our own process, because reading our panel sends `WM_GETOBJECT` to the main thread, which at exit is waiting for the caret thread. `exe_name` and `foreground` in read.rs give the foreground program.
- The 9.1 focus probe (crates/cv-platform-win/examples/focus_probe.rs) reads, for each foreground or focus WinEvent: uia `GetFocusedElement` and then about 25 separate `Current*` property and pattern calls; msaa `AccessibleObjectFromEvent` for the event's window, object and child. It shows a value only when its own source says "not a password" and the other doesn't say "password" (`value_visible`, unit-tested).
- app spawns the speech thread only with the `tts` feature. A changed `tts_*` field or the speech on/off hotkey calls `SpeechHandle::wake()` (crates/app/src/main.rs).

### What 9.2 measured (docs/prototypes/focus-sources.md, STATUS Finding 25)

| Place | uia | msaa |
|---|---|---|
| XAML: Start, Search, Settings, taskbar, tray | right | right, or partly |
| Windows Terminal | right (Text pattern holds the whole buffer) | partly |
| Brave page | right (Document, Value pattern is the page address) | partly |
| Explorer list, desktop icons | right | right |
| Save As (classic Win32, from an elevated Notepad) | partly: type `Pane` for every control, `&Save` keeps its accelerator | right roles and clean names |
| Alt+Tab | partly: unnamed `Pane` | names the switcher and the item |
| Notepad text area | partly: `Pane`, name is the start of the document | partly: `client`, same name |
| Task Manager | fails: `0x80070005 Access is denied` (19 events) | fails: `0x80004005` |

- **Neither source is enough alone.** UIA is better in XAML, Terminal and Brave. MSAA is better in classic Win32 dialogs and on Alt+Tab.
- **Different objects.** UIA reads the current focus, MSAA reads the event's object, and in a burst they describe different controls (Finding 25(b)).
- **Bursts.** One Alt+Tab raises 8 to 10 events in about 150 ms. Serial reads ran up to 364 ms behind their events (Finding 25(c), probe.txt Save As).
- **Errors.** MSAA failed on 61 events, mostly transient objects (`0x80004005`) and 3 stale Chromium child ids (`0x80070057`). UIA failed only in Task Manager.
- **Cost.** UIA median 2 to 28 ms per read (a game about 120 ms). Outliers: about 500 ms for the first read into a new window, and 9.1 s once with a hung Steam. MSAA median 1 to 5 ms, max 336 ms.
- **Noise.** Our overlay raises a foreground event (Finding 25(f)). The shell's `ForegroundStaging` window and the desktop `#32769` come with no program name and carry nothing.
- **Values that no password flag protects.** Terminal's Text pattern returns the whole buffer and Brave's Value pattern the page address, both flagged `not-password` (Finding 25(h)).
- **Elevation.** An elevated Notepad (high integrity, as Task Manager) answered fully from both sources while the probe ran at medium integrity. Why Task Manager is refused is unknown.
- **Save As, read in probe.txt for this ADR:** UIA gave class `Edit`, framework `Win32`, type `Pane` and no patterns for the file name box, while MSAA gave `editable text` with an unreadable state (`?`, so the value was withheld as "can't tell"). Every Save As in the runs belonged to the elevated Notepad. User Interface Privilege Isolation stops a lower-integrity process from sending most window messages to a higher one ([UIPI](https://en.wikipedia.org/wiki/User_Interface_Privilege_Isolation); [UIAccess policy](https://technet.microsoft.com/en-us/library/jj852244.aspx)). So the `Pane` answers may come from UIA's Win32 proxy being refused those messages, not from the dialog. Not measured: a Save As from a non-elevated Notepad.
- Not measured: LibreOffice, Word, the Explorer address bar and rename, a real password field in 9.2 (9.1 saw a Reddit one: UIA `password`, MSAA `protected`).

### External facts

- **Caching.** Reading properties one at a time "requires a cross-process call for each property". A cache request lets a client "retrieve all of the desired properties for all of the desired elements with a single method call". A control pattern's properties, such as the Value pattern's Value, can be cached without the pattern ([Caching UI Automation Properties and Control Patterns](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-cachingforclients)). `GetFocusedElementBuildCache` "retrieves the UI Automation element that has the input focus, prefetches the requested properties and control patterns" ([docs](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation-getfocusedelementbuildcache)). `BuildUpdatedCache` gets a new snapshot for an element (same page). From the probe's call count and the 500 ms timeouts, my reading is that the 9.1 s Steam read was about 20 timed-out calls in a row. That is inferred, not measured.
- **AccessibleObjectFromEvent.** "Clients call this function within an event hook function", with the callback's own hwnd, object and child ids. It returns the object, or the parent of an element that is not an accessible object. Errors are expected "because of a user action" ([docs](https://learn.microsoft.com/en-us/windows/win32/api/oleacc/nf-oleacc-accessibleobjectfromevent)). The probe called it from the loop, after the callback, and it still resolved nearly always (Finding 25(d)).
- **UIA threading.** Clients call UIA from an MTA thread that owns no window. "A UI Automation client should not use multiple threads to add or remove event handlers" ([Understanding Threading Issues](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-threading), cited in ADR 0008). The warning is about handlers. Nothing found says a process may not hold two `CUIAutomation8` objects on two threads.
- **UIA timeouts.** Connection 2 s and transaction 20 s by default, settable through `IUIAutomation2` (ADR 0008). `IUIAutomation6` (Windows 10 1809) adds `CoalesceEvents`, which filters duplicate UIA *events* for handler clients, and `ConnectionRecoveryBehavior`, which "adjusts provider request timeouts when the provider is non-responsive", off by default ([IUIAutomation6](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomation6), [ConnectionRecoveryBehavior](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation6-put_connectionrecoverybehavior)). Neither applies to WinEvent-driven reads. How much recovery adjusts the timeouts is not documented there.

## Options

### Who reads focus, and how it shares UIA with the caret thread

1. **The caret thread reads focus too.** It already wakes on foreground and focus events and holds a UIA object. Cost: a focus read (2 to 28 ms, about 500 ms into a new window, up to the timeout in a hung app) runs before or after each caret read on the same thread, so caret tracking in the magnifier waits behind speech work. With speech off the reads can be skipped, so the speech-off rule holds. With speech on, the magnifier's caret latency changes. One hook set, one UIA object, one foreground cache. Caret and focus events come out in one order.
2. **A focus thread in cv-platform-win**, with the caret thread's shape: MTA, no window, its own `CUIAutomation8` with the same 500 ms timeouts, its own out-of-context hooks for foreground and focus, and no UIA event handlers. Cost: one more thread and shutdown path. A second UIA object in the process, which Microsoft's handler warning doesn't cover because neither thread adds handlers. Foreground and program-name code is shared from caret/read.rs, not copied. Caret and focus events from two threads have no common order, only their `at`. Gains: the caret thread and the magnifier stay exactly as they are, with speech on or off. A hung app's focus read never delays caret tracking. The thread exists only with `tts`.
3. **Option 2 with a UIA `FocusChanged` handler** in place of the focus WinEvent for the UIA half (the old cv-tts design). Cost: handler callbacks arrive on UIA's own threads and must be handed back to the focus thread. MSAA still needs WinEvents for the Win32 dialogs and Alt+Tab, so there are two event paths to merge. It is the first UIA handler in the process, so "one thread adds handlers" becomes a rule to keep. Nothing measured it: 9.1 and 9.2 used WinEvents with `GetFocusedElement`. Gains: the element arrives with its cache already filled.

### What an event means (which object)

A. **The control that has focus when it is read.** UIA reads it with `GetFocusedElementBuildCache`. MSAA reads the newest WinEvent's object, which after the filter's settle time below is normally the same control. The two readings are merged only when they are the same control. Cost: when the newest event's object isn't the focus, MSAA's reading is dropped. In a burst, intermediate controls are never described, which is the point for speech.
B. **The event's own object, from both sources.** MSAA from the event; UIA through `ElementFromIAccessible` on that same object. Cost: one more cross-process step per event. Whether the element it returns keeps the app's native UIA names and patterns (XAML, Chromium) or only the MSAA view is not documented in what I found and not measured. Every intermediate object in a burst is read, or must be filtered out after reading.
C. **UIA only, MSAA only when UIA fails.** Cost: Save As controls are `Pane` and Alt+Tab is unnamed. The 9.2 table shows MSAA is the better source in both.

### Fallback order per app type

i. **One field-by-field merge for every app, as pure logic in cv-core.** The platform maps each source's raw answer into the same neutral reading (name, role, states, value, rectangle, password flag). A cv-core function merges the two with fixed rules (Decision). Per-app differences are found at run time from what each source answers, the way ADR 0008's fixed caret chain works. Cost: the rules are a guess from 9.2's 14 places, and a wrong rule shows in every app at once. Gains: unit-tested without Windows (ROADMAP rule), and the same rules can merge AT-SPI later.
ii. **A per-app table keyed on window class or UIA framework id** (`Win32` dialogs use MSAA, XAML and Chromium use UIA). Cost: a hand-kept list, and an app missing from it gets the wrong source. The Save As case shows the class alone may not decide it: the same Win32 dialog may answer UIA well at medium integrity (not measured).

### Where filtering lives

a. **In the focus thread's loop.** Cost: the rules can be tested only by running apps.
b. **A pure `FocusFilter` in cv-core** that takes the WinEvent times and readings and says when to read and what to publish. The thread feeds it. Cost: a small state machine with an injected clock, like `SpeechQueue`. Gains: 9.4's "unit tests for the filtering logic".

## Decision

Recommendation: **option 2 (a focus thread), A (the control that has focus when read), i (one merge in cv-core), b (a pure filter in cv-core).**

It extends ADR 0004 with a new `CoreEvent` variant and fills in the fields ADR 0013 left to 9.3. It supersedes, in part, one phrase of ADR 0008: its option 1 calls the caret thread the holder of "every core UIA and MSAA object". The caret thread keeps every *caret* read, and the focus thread holds its own UIA object for focus reads. Changing 0008's Status line is yours. Nothing else in 0008 or 0013 changes.

How it shares UIA with the caret thread: it doesn't share an instance. Each core thread has its own `CUIAutomation8` with 500 ms timeouts, and neither adds UIA event handlers. If a later step needs a UIA handler (10.1 may), handlers go on one thread only, and that step's ADR says which.

### The focus thread (cv-platform-win)

- `platform::spawn_focus_source(hub, state) -> FocusHandle`, with `wake()` and `shutdown()` like `CaretHandle` (`PostThreadMessageW`, 1 s shutdown wait). app spawns it only with the `tts` feature. It is shut down after the overlay and caret threads and before the speech thread.
- MTA COM first, `CUIAutomation8` with `ConnectionTimeout` and `TransactionTimeout` at 500 ms (one shared constant with the caret thread). No window, no UIA handlers. No COM object leaves the thread.
- Out-of-context hooks for `EVENT_SYSTEM_FOREGROUND` and `EVENT_OBJECT_FOCUS`, with `WINEVENT_SKIPOWNPROCESS`. The callback stores the event's time, hwnd, object and child and returns. It does no read (as in the caret thread; 9.1 showed `AccessibleObjectFromEvent` still resolves from the loop).
- **Reads only while wanted.** On every wake it reads `tts_enabled` from `AppState`. While it is false, the thread drops the WinEvents and reads nothing. app calls `FocusHandle::wake()` wherever it calls `SpeechHandle::wake()`. When speech turns on, the thread reads once, as if the foreground had changed. 9.9 adds its own setting to "wanted" if it is built.
- **Never our own process.** No read when the foreground window belongs to clear-view (the same `WM_GETOBJECT` exit deadlock as the caret thread, Finding 25(f), and Finding 26: the panel is one control to both sources). Panel events are dropped, not spoken.
- **One read** (when the filter says one is due):
  1. The foreground window: hwnd, pid, program name (`exe_name`) and title. If the foreground changed since the last read, `window_changed` is true.
  2. UIA: `GetFocusedElementBuildCache` with one cache request: Name, ControlType, IsPassword, IsEnabled, BoundingRectangle, ProcessId, FrameworkId, and the Toggle, ExpandCollapse and SelectionItem state properties. One cross-process call. **No value property is in this request**, so no value is fetched before the password decision.
  3. MSAA: `AccessibleObjectFromEvent` for the newest stored WinEvent, then name, role, state and location.
  4. Password: cv-core `focus::value_allowed(uia_password, msaa_protected)`. It is the probe's rule, moved to cv-core with its tests: a source's value may be read only when that source says "not a password" and the other doesn't say "password". "Can't tell" is a password.
  5. Values, only if allowed: UIA through `BuildUpdatedCache` with the Value pattern's Value and the RangeValue pattern's Value, Minimum and Maximum, and MSAA `accValue`. Never the Text pattern: that is the whole Terminal buffer (Finding 25(h)), and reading text is Phases 10 and 12. A value is cut at `VALUE_MAX` (256 characters) before it leaves the thread.
  6. Both readings go to `focus::merge`, and the result to the filter.
- **Roles.** Each source's role is mapped to a neutral cv-core `Role` by a pure function in cv-platform-win, unit-tested there: UIA control type ids 50000 to 50040, MSAA `ROLE_SYSTEM_*` values. The ids are Windows values, so the mapping stays in the backend.
- **Hung windows.** A UIA call that fails with a timeout marks the foreground window hung. Both sources are skipped for that window until the foreground changes. The filter publishes that window once with no control, so 9.6 can still say the title, which Windows suffixes "(Not Responding)" (seen with Steam). With the cache request a hung app costs about two timed-out calls (about 1 s), not 9 s, and only this thread waits.
- `[focus]` stderr lines: one per published event, giving the program, role, source and whether a value was withheld, never the name or the value (a Notepad text area's name is the start of the document). `CV_FOCUS_LOG=1` adds names, like `CV_TRACK_LOG`, still without values.

### What the core reports (cv-core)

Sketch for 9.4. Names may change in the step, the content may not.

```rust
#[non_exhaustive]
pub enum CoreEvent {
    CaretMoved { .. },  // unchanged
    CaretLost { .. },   // unchanged
    /// Focus moved to a new control, or the foreground window changed. Sent only after the
    /// filter: never for our own process, never twice for the same thing.
    FocusChanged {
        at: Instant,
        app: AppId,
        window: WindowInfo,
        /// The foreground window differs from the last FocusChanged's.
        window_changed: bool,
        /// None: neither source could read the focused control (Task Manager, a hung window).
        control: Option<Control>,
    },
}

#[non_exhaustive]
pub struct WindowInfo { pub title: String }

#[non_exhaustive]
pub struct Control {
    pub name: String,
    pub role: Role,                 // neutral; Role::Unknown when neither source gives one
    pub states: States,
    pub value: Value,
    pub rect: Option<ScreenRect>,   // physical virtual-screen px, for 9.9
    pub source: FocusSource,        // Uia, Msaa or Both: which source the fields came from
}

#[non_exhaustive]
pub struct States {
    pub enabled: Option<bool>,
    pub checked: Option<Checked>,   // On, Off, Mixed
    pub expanded: Option<bool>,
    pub selected: Option<bool>,
    pub password: Password,         // Yes, No, CantTell
}

pub enum Value {
    None,
    Text(String),                   // at most VALUE_MAX characters
    /// A password field, or one where the sources can't tell. Nothing about the value,
    /// including its length, was read.
    Withheld,
}
```

- **One event for window and control.** 9.6 says the title and then the control. As two events, the second `Focus` utterance would cut off the first (ADR 0013, foreground kinds). As one event, `utterances_for` returns them as one batch.
- `CoreEvent::at` and `describe` cover the new variant. `describe` gives no name or value.
- Positions ("3 of 7"), a dialog's text and anything 9.6 and 9.7 need are added to these `#[non_exhaustive]` structs by those steps, as plain data, without a new ADR. Anything that would carry an OS handle or a COM pointer needs one.

### The merge (cv-core `focus::merge`, pure, unit-tested)

Input: a UIA reading and an MSAA reading, each `Option`, in the neutral form. Output: `Option<Control>`.

1. **Same control** if both have a rectangle and the rectangles match within 2 px on every edge, or, when either rectangle is missing, if the names match after cleaning. Then field by field:
   - name: UIA's, with a single `&` accelerator marker removed (`&Save` gives "Save"); MSAA's if UIA's is empty.
   - role: UIA's, unless it is `Pane`, `Custom` or `Unknown` and MSAA's is something else. Then MSAA's (Save As).
   - states: each field from UIA where UIA has it, else from MSAA (MSAA `checked` appears only when ticked, Finding 25(j), so MSAA alone can say Checked::On but never Checked::Off).
   - value: UIA's Value or RangeValue, else MSAA's (the Settings slider). `Withheld` if either side withheld it.
   - password: `Yes` if either says yes; `No` only if one says no and the other doesn't say yes; else `CantTell`.
   - source: `Both`.
2. **Not the same control:** UIA's reading alone (it is the focus now), unless UIA's reading is empty: no name and a role of `Pane`, `Custom` or `Unknown`. Then MSAA's alone (Alt+Tab, Brave's container pane).
3. **One source failed:** the other alone. **Both failed:** `None`.
4. A reading used alone keeps its own password flag, and "can't tell" stays `CantTell`, so its value is `Withheld`.

### The filter (cv-core `focus::FocusFilter`, pure, unit-tested)

Inputs: `winevent(now, foreground_changed)`, `reading(started, now, app, window, control)`, `tick(now)`. Outputs: "read now", "publish this event", or nothing. It has no Windows types.

- **Settle.** A read is due once no foreground or focus WinEvent has arrived for `SETTLE` (50 ms), or `SETTLE_MAX` (150 ms) after the first event of a burst, whichever comes first. An Alt+Tab burst (8 to 10 events in about 150 ms) gives one read.
- **Newest only.** One read covers every event since the last read, and MSAA uses the newest event's object.
- **Stale readings.** A reading that started before a newer WinEvent arrived is dropped and a new read is due. This drops at most one reading in a row, so a stream of events can't starve the reads.
- **Nothing to say.** No event when the foreground has no program name (`ForegroundStaging`, the `#32769` desktop), or when both sources gave nothing and the window didn't change.
- **Duplicates.** No event when app, window, `window_changed` false and the control are equal to the last published event. Focus that leaves a control and comes back to it is not a duplicate, because something else was published in between.
- **Unreadable windows.** A window whose control can't be read (Task Manager, a hung window) gives one event when it becomes the foreground, with `control: None`, not one per WinEvent (Task Manager raised 19).
- `SETTLE`, `SETTLE_MAX`, the 2 px match and `VALUE_MAX` are starting values. A step may tune them with a measurement and say so in its PR.

### Speech off

- `--no-default-features`: the focus thread isn't built and nothing changes.
- `tts` on and speech off: the thread exists, takes WinEvents and reads nothing. The caret thread is untouched in every case.
- The render thread receives `FocusChanged` on its hub receiver. `Tracker::handle` gets an arm that ignores it until 9.9, with a test that a `FocusChanged` changes neither the mode nor the target.
- `utterances_for` still returns nothing for it until 9.5.

### Roadmap

9.4 is split (pending this ADR):

- **9.4a** cv-core `focus`: `FocusChanged`, `WindowInfo`, `Control`, `Role`, `States`, `Value`, `Password`, `FocusSource`; `value_allowed` (moved from the probe with its tests); `merge`; `FocusFilter` with an injected clock; `CoreEvent::at` and `describe`; the `Tracker` arm that ignores focus. Unit tests for each rule above. No Windows code. Nothing publishes the event yet.
- **9.4b** cv-platform-win focus thread per this ADR: hooks, wanted check, the cached UIA read, MSAA from the newest event, values only after `value_allowed`, `VALUE_MAX`, role mapping with tests, hung-window rule, `wake` and `shutdown`, `[focus]` lines and `CV_FOCUS_LOG`. app spawns it with `tts` and calls `wake()` next to `SpeechHandle::wake()`. Verify: with speech on and `CV_FOCUS_LOG=1`, the 9.2 places give one `[focus]` line per real focus change (one per Alt+Tab); a Save As from a **non-elevated** Notepad, recorded (the open question above); Task Manager gives one line with no control; a Brave login form's password field shows "withheld" and the log has no characters from it; the settings panel gives no line; a run with speech off and one with `--no-default-features` change nothing in the magnifier.

9.5 then turns `FocusChanged` into utterances.

## Consequences

- Easier: speech work on focus never touches the caret thread or the magnifier. 9.5 to 9.7 are pure functions over one plain event and change no thread. Every per-app rule is in two tested places: the merge and the role mapping. A Linux backend fills the same neutral readings from AT-SPI. A password value is never read into our process, not just never spoken.
- Harder: a fourth long-lived core thread with COM, hooks and a shutdown path, and a second UIA object in the process. Foreground and program-name code is shared between caret/ and the focus thread, so a change to it touches both. Every focus announcement waits at least `SETTLE` (50 ms) after the last event.
- Risks: the merge rules come from one machine's runs. The rectangle match may fail where UIA and MSAA disagree about a control's bounds (not compared in 9.2). If UIA's `Pane` answers in Save As really come from elevation, the role rule is only a fallback there. A cache request may behave differently from `Current*` reads in some provider (not measured).
- Revisit: after 9.4b's Verify run (rectangle match, Save As at medium integrity, settle time); at 9.9 (what "wanted" means, and whether the magnifier uses `rect`); at 10.1, if typing echo needs UIA text events, which would be the process's first UIA handler; at 6.3 or 13.2, when UIAccess changes what elevated windows answer; if Task Manager's refusal is understood.
