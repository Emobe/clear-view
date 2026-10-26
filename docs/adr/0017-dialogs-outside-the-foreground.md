# 0017: Dialogs outside the foreground window

Status: Accepted (you, 2026-10-26; set by the agent on your instruction)
Date: 2026-10-26

## Context

Roadmap 9.6c (speech stage 1, PRODUCT.md "Speech (v1.1)"). 9.6b2 is approved and speaks a dialog's title and text when the dialog becomes the foreground window. Your hand run (2026-10-26, `focus-log.txt`) found three common cases it misses and one cost (STATUS Findings 31 to 34). The roadmap item says this ADR supersedes ADR 0015 and 0016 where they disagree and splits 9.6c into sub-items.

### Rules this must keep (ROADMAP "Rules for speech steps", PRODUCT.md)

- With speech switched off, the magnifier behaves exactly as before.
- Password fields are never spoken. "Can't tell" counts as a password field.
- No UI Automation or MSAA call on the render thread. A hung app must not freeze the magnifier or stop speech from being silenced.
- The text that gets spoken is pure logic in cv-core with unit tests.

### Earlier decisions this touches

- **ADR 0015** (Accepted): one read per settled burst; "the foreground window" is the window of the event (`WindowInfo`), and `window_changed` is "the foreground window differs from the last FocusChanged's". A UIA timeout marks the foreground window hung until the foreground changes. Plain-data fields may be added to `WindowInfo` and `Control` without an ADR; OS handles may not go into an event (ADR 0004).
- **ADR 0016** (Accepted): the dialog check runs "only when the foreground window differs from the window the last dialog check ran for", keyed per window handle; the walk never enters a `Tab`; the text is spoken only when `window_changed`. Its "Dialogs inside a window (D3), later" section allowed `NormalizeElementBuildCache` with an `IsDialog` walker under 0016 and said anything beyond one more call per read needs a new ADR.
- **ADR 0013** (Accepted): one event gives one batch; `Kind::Focus` cuts off what was speaking.

### What the code does today

- `read_focus` (crates/cv-platform-win/src/focus/mod.rs) takes `caret::foreground()` (`GetForegroundWindow`), reads its title, program and dialog text, and keys the dialog cache and the hung mark on that window's handle.
- `FocusFilter::winevent(now, foreground_changed)` (crates/cv-core/src/focus.rs) sets `window_changed` only from an `EVENT_SYSTEM_FOREGROUND` WinEvent. A reading in a different window with no foreground WinEvent is published with `window_changed` false, so `focus_utterances` speaks neither title nor dialog text.
- `dialog::read_dialog` (focus/dialog.rs) starts its clock before `ElementFromHandleBuildCache`, so the ms in the `[focus] dialog` line covers the class check, the UIA root, the whole UIA walk and the MSAA walk. Its `calls` count is the count of the source that won (msaa), not of both.
- The walk (focus/walk.rs) enters `Pane`, `Group`, `Custom`, `Unknown` and `Window` nodes. A nested dialog (a property page, a `#32770` inside a `#32770`) is entered like any pane.

### What has been measured

From `focus-log.txt` (9.6b2, your run):

- **Explorer's "Replace or Skip Files"** (Finding 31): the buttons came as focus events in explorer.exe (framework `DirectUI`) with no `new window` and no dialog line. No foreground WinEvent was logged as a window change. The window showed as a new window, titled "Replace or Skip Files", only after you switched away and back. Inferred, not logged: the focus was in a top-level window that was not `GetForegroundWindow`.
- **Notepad's "save changes" prompt** (Finding 32): `Button "Cancel"`, merged uia+msaa, UIA framework `Win32`, no window change. Whether it is a XAML `ContentDialog` inside Notepad's window or a separate window is not known.
- **A file's Properties** (Finding 33): not in the log. You heard the title, then a control. No Properties log was kept, so what the sheet and its pages report is not known.
- **Save As from the elevated Notepad** (Finding 34): `[focus] dialog "#32770" via msaa: 1 calls, 8 nodes`, 266, 414 and 303 ms, text empty every time. The UIA read of the control before it took 53 to 101 ms. Because of the clock noted above, how much of that is the UIA walk and how much the MSAA walk is not known.

From the 9.2 probe logs (`probe.txt`, `probe-desktop.txt`, `probe-taskbar.txt`; the probe prints a `root` line when the event window's root is not the foreground window): 212 of 332, 113 of 262 and 13 of 68 events came from a window whose root was not the foreground. Most were in-between states and background windows: the `ForegroundStaging` window, the Alt+Tab host (`XamlExplorerHostIslandWindow`), the taskbar (`Shell_TrayWnd`), menus (`#32768`), Terminal and Brave windows in the background. **The event's own window is too noisy to decide "the window the user is in".**

### External facts

- **GetGUIThreadInfo** with thread id 0 "returns information for the foreground thread", including `hwndFocus`. It "succeeds even if the active window is not owned by the calling process" and is "useful for retrieving out-of-context information about a thread". It "may not return valid window handles … when a window is losing activation" ([docs](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getguithreadinfo)). It is a local call, no message to the target. The caret thread already uses it (`gui` source, ADR 0008).
- **GetAncestor**: `GA_PARENT` "does not include the owner"; `GA_ROOT` "retrieves the root window by walking the chain of parent windows"; `GA_ROOTOWNER` also walks owners ([docs](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getancestor)). `GA_ROOTOWNER` of an owned dialog is its owner (the Explorer window), so it would hide the dialog.
- **Property sheets.** "A property sheet and the pages it contains are actually dialog boxes." The sheet "manages the pages"; "each page in a property sheet is an application-defined modeless dialog box", a child window of the sheet. "A property page can contain a nested dialog box." The tabs belong to the sheet ([About Property Sheets](https://learn.microsoft.com/en-us/windows/win32/controls/property-sheets)). So in a file's Properties the focused control's parent chain is control, page (`#32770`), sheet (`#32770`, the top-level window), while focus on the tab control has only the sheet above it.
- **IsDialog in XAML.** "`Flyout` and `ContentDialog` elements default to `true` for `AutomationProperties.IsDialog`"; for a dialog a screen reader "typically speaks the title of the dialog, the focused control in the dialog, and then the content of the dialog" ([AutomationProperties.IsDialog](https://learn.microsoft.com/en-us/uwp/api/windows.ui.xaml.automation.automationproperties.isdialog)). A `ContentDialog` is drawn inside its window, with no HWND of its own.
- **NormalizeElementBuildCache** navigates "up the ancestor chain in the tree until an element that satisfies the view condition … is reached. If the root element is reached, the root element is returned" ([docs](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationtreewalker-normalizeelementbuildcache)). One call from us; how many provider calls UIA makes inside it is not documented and not measured.
- **Cache scope.** A cache request's scope may be the element, its children or its descendants. `TreeScope_Parent` and `TreeScope_Ancestors` are not supported ([Caching UI Automation Properties](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-cachingforclients), [FindAllBuildCache](https://learn.microsoft.com/en-us/windows/desktop/api/uiautomationclient/nf-uiautomationclient-iuiautomationelement-findallbuildcache)). Ancestors cannot be fetched with the focused element in one call.
- **NVDA.** On a focus change NVDA fires "focusEntered" for every ancestor below the point where the old and new focus chains differ (`api.getFocusDifferenceLevel()`, source/eventHandler.py, [nvaccess/nvda](https://github.com/nvaccess/nvda)), and a dialog ancestor is spoken with its text. `getDialogText` (source/NVDAObjects/behaviors.py) enters `PROPERTYPAGE` among its containers and does not recurse into a child that is itself a dialog: "Stops double reporting when focus is in a property page in a dialog".

## Options

### Which window the title and the dialog check key on

W1. **The foreground window (today).** Cost: misses Finding 31.
W2. **The focus window's root, from local calls.** `GetGUIThreadInfo(0).hwndFocus`, then `GetAncestor(GA_ROOT)`. It is used in place of the foreground window only when it differs from it **and** has a title or a dialog class (`map::is_dialog_class`); otherwise the foreground window stays, so an untitled popup (a combo box's drop-down list, a menu) is part of the window it came from. `window_changed` becomes: a foreground WinEvent arrived, **or** this window differs from the last published event's. Cost: two or three local calls per read. Whether `hwndFocus` names the dialog in the "Replace or Skip" case is the open question; the probe in 9.6c1 answers it.
W3. **The event's window root** (`GetAncestor(event.hwnd, GA_ROOT)`). Cost: the 9.2 logs show it is not the foreground window for a third to two thirds of events, mostly noise.
W4. **UIA: walk up from the focused element to its top-level element.** Cost: one cross-process call per level on every read; ancestors can't be cached (above).

### How a dialog inside a window is found

I1. **None (today).** Cost: misses Findings 32 and 33.
I2. **The Win32 parent chain, then UIA only where XAML may hold one.** First, `GetAncestor(GA_PARENT)` from the focus window up to (not including) the read window, taking the nearest window with a dialog class: a property page, a dialog nested in a dialog. Local calls only. Then, only when that found nothing and the focus is in XAML content (the focused element's cached `FrameworkId` is `XAML` or `WinUI`, or the focus window's class is a XAML host class from 9.6c1's probe), one `NormalizeElementBuildCache` from the focused element with an `IsDialog == true` walker, which finds a `ContentDialog` or `Flyout`. Cost: one more cross-process call per read in XAML apps only; Win32 apps pay nothing.
I3. **ADR 0016's D3 as written:** `NormalizeElementBuildCache` on every read in every app. Cost: one more call per focus change everywhere, including Chromium's deep trees, where the call's internal cost is unknown; misses nothing I2 finds except a non-XAML element marked `IsDialog`.
I4. **NVDA's ancestor chain:** read the focus's ancestors on every read and speak those that are new. Cost: one cross-process call per level per read; the most general, the most expensive, and it reaches into 9.7 (groups, positions).

### What a property sheet gives

P1. **Each page is a dialog inside the sheet** (I2's Win32 chain finds it). When focus enters a page, its name (the tab label) and its text are spoken; focus on the tabs or on OK and Cancel says only the control. The walk does not enter a node that is itself a dialog, so the sheet's own text never repeats the page's. Cost: a "General" page holds many static texts ("Type of file:", "Text Document (.txt)", "Location:" …); the existing caps (`DIALOG_TEXTS` 8, `DIALOG_SPOKEN_MAX` 300) cut it, so it is long but bounded. NVDA reads the same.
P2. **The sheet says only the selected tab's name.** Cost: needs the tab control's selection (a pattern read); the page's text is never heard.

### The MSAA walk's 0.3 to 0.4 s

T1. **Leave it.** Once per dialog; Save As gained nothing from it.
T2. **Measure, then bound it.** Split the `[focus] dialog` line into UIA root, UIA walk and MSAA walk (ms and calls each). Add a time budget `DIALOG_MS` for the whole dialog read, checked between calls; when it is spent the walk stops and counts as incomplete (says nothing, ADR 0016 rule 1). For a `#32770`, stop the UIA walk after its first level when no node there has a role other than `Pane` (the refused Win32 proxy) and go to MSAA at once. Cost: a slow dialog with a real message may say nothing; the starting value is a guess until the split is measured.
T3. **Publish the control first and the text after it.** Cost: changes your order (title, text, control) and splits ADR 0013's batch.

## Decision

Recommendation: **W2, I2, P1, T2**, built after a probe (9.6c1) that checks W2 and I2 against the three cases. It supersedes in part:

- ADR 0015: "the foreground window" as the window of a `FocusChanged` (it becomes the read window below); `window_changed` as only a foreground change; the hung mark keyed on the foreground window.
- ADR 0016: the dialog check keyed only on the foreground window; "Dialogs inside a window (D3), later" (replaced by I2, which is narrower than D3: no call on every read in every app); the walk entering a nested dialog.

Everything else in both stands: one read per settled burst, the merge, the password rules, the walk's budget and skip rules, the MSAA fallback for `#32770`, cv-core choosing the text.

### The read window (W2, cv-platform-win)

- Each read: the foreground window as today, then `GetGUIThreadInfo(0)`. If `hwndFocus` is set, its `GA_ROOT` root is the candidate. The **read window** is the candidate when it differs from the foreground window and has a title (`GetWindowTextW` non-empty) or a dialog class; else the foreground window.
- The read window gives the title, the program (its pid) and the dialog check. The own-process rule applies to both the foreground window and the read window: if either is ours, nothing is read.
- The hung mark is keyed on the read window and cleared when the read window changes.

### Dialogs inside the read window (I2)

- **Win32:** from the focus window up through `GA_PARENT` (at most 16 steps) to the read window, the nearest window whose class is a dialog class is the **inner dialog**. Local calls only.
- **XAML:** only when the Win32 chain found none and the focus is in XAML content (the gate in I2; the class list comes from 9.6c1), one `NormalizeElementBuildCache` from the focused element (the one the control read already holds) with a walker on `IsDialog == true` and a cache request of Name and the runtime id. A result that is the root, or the read window's own element, is no inner dialog. A timeout marks the read window hung, like any other call.
- The **dialog to walk** is the inner dialog if there is one, else the read window when ADR 0016's check says it is a dialog. Its text is found by ADR 0016's walk, with one more rule: a node that is itself a dialog (Win32 dialog class or cached `IsDialog`, MSAA role dialog or property page) is kept as a node but not entered. The walk's cache request gains `ClassName` and `IsDialog`.
- The text is cached per (read window, dialog) pair, a few pairs at most, so a later read in the same dialog carries the same text and returning to a page does not walk it again.

### What the core sees (cv-core)

- `Reading` gains `window_key: u64` and `dialog_key: Option<u64>`: opaque numbers the platform gives (the handle value, or a hash of the UIA runtime id), compared only for equality. They are not in the event, which still carries no handle (ADR 0004).
- `WindowInfo` gains `dialog_name: String`: the inner dialog's name (a page's tab label, a `ContentDialog`'s title), empty when the dialog is the window itself or there is none. `dialog_text` is now the text of the dialog to walk.
- `FocusChanged` gains `dialog_changed: bool`.
- `FocusFilter`: `window_changed` is a pending foreground WinEvent **or** a `window_key` different from the last published one. `dialog_changed` is true when the window did not change and `dialog_key` is `Some` and differs from the last published one. Leaving an inner dialog for the window's own controls changes nothing that is spoken. The duplicate rule also compares both keys. Unit-tested with an injected clock, as now.

### Wording (cv-core `wording`)

- `window_changed`: the title, then `dialog_name` when it is not empty and not the title, then the dialog text, then the control.
- `dialog_changed` only: `dialog_name`, then the dialog text, then the control.
- Neither: the control, as now. All `Kind::Focus`, one batch, every verbosity level. A name equal to the control's spoken name is said once, by the control.
- When joining the dialog text, a piece that ends in `:` is joined to the next one by a space, not ". " ("Type of file: Text Document"), so a page's label and value read as one phrase. This changes ADR 0016's join for that case only.

### Cost (T2)

- The `[focus] dialog` line gives ms and calls for the UIA root, the UIA walk and the MSAA walk separately, and the inner-dialog lookup gets its own ms (Win32 chain, Normalize).
- `DIALOG_MS`, a cv-core constant, starting at 250 ms for the whole dialog read; past it the walk stops as incomplete. 9.6c3 sets the final value from the split it measures and says so in its PR.
- A `#32770` whose first UIA level holds only `Pane` nodes skips the rest of the UIA walk and goes to MSAA.

### STATUS Finding 29 ("Desktop 1")

The control read's cache request gains `ClassName`. A UIA reading of a `Pane` whose class is `#32769` (the desktop) is treated as no UIA reading, by a pure rule in map.rs with a test, so MSAA alone or nothing is used. ADR 0015 allows this without an ADR; it is built in 9.6c3 because it changes the same cache request.

### Speech off

- `--no-default-features`: no focus thread, nothing changes.
- `tts` on, speech off: the focus thread reads nothing (ADR 0015), so no `GetGUIThreadInfo`, chain walk or Normalize call either. The caret thread and the magnifier are untouched.

### Roadmap

9.6c is split (pending this ADR):

- **9.6c1** Probe (dev only, not shipped): `focus_probe` prints per event a `gui` line (`GetGUIThreadInfo(0)`: `hwndFocus` and `hwndActive`, and `hwndFocus`'s root with class, title and program when it is not the foreground window), the dialog-class windows between the focus window and its root, and the focused element's `FrameworkId` and nearest `IsDialog` ancestor by `NormalizeElementBuildCache` (name, type, framework, ms). No app change. You run it on Explorer's "Replace or Skip Files", Notepad's "save changes" prompt, a file's Properties (open, Tab into the page, Ctrl+Tab to another page), Save As, one Settings dialog and Alt+Tab; the step records what each gave in STATUS and docs/prototypes/focus-sources.md. **Stop condition:** if "Replace or Skip"'s `hwndFocus` root is the foreground window, or neither the Win32 chain nor Normalize finds Notepad's prompt, stop and raise this ADR again before 9.6c2.
- **9.6c2** cv-core: `Reading::window_key` and `dialog_key`, `WindowInfo::dialog_name`, `FocusChanged::dialog_changed`, the filter rules, the wording (both cases and the `:` join), `DIALOG_MS`. Unit tests for each rule. No Windows code.
- **9.6c3** cv-platform-win: the read window (W2), the inner dialog (Win32 chain, gated Normalize), the walk's nested-dialog rule, the per-(window, dialog) cache, the hung mark on the read window, the timing split and `DIALOG_MS`, the `#32770` early switch to MSAA, the Finding 29 rule, the `CV_FOCUS_LOG` lines. Verify: Explorer's "Replace or Skip Files", Notepad's "save changes" prompt, a file's Properties (title, page name and text, then the control; Ctrl+Tab speaks the new page), Save As (should not get slower; ms from the log), Alt+Tab and a combo box drop-down (no extra titles), "Desktop 1" no longer spoken; a run with speech off and one with `--no-default-features`.

## Consequences

- Easier: the title and dialog text follow where the focus really is, with local Win32 calls in every app and one UIA call only in XAML content. Property pages and in-window dialogs reuse ADR 0016's walk and cv-core's text rules. The filter rules stay pure and tested.
- Harder: two more fields in the read, a second key in the filter, and a gate whose class list comes from a probe. The read window can now differ from the foreground window, which the caret thread does not know about (it keys on the foreground; nothing in the magnifier changes).
- Risks: `GetGUIThreadInfo` "may not return valid window handles … when a window is losing activation", so a read during a switch may fall back to the foreground window and say the title one read late. A titled popup that takes focus without activation (a program's own) would now be announced as a window. The XAML gate may miss a `ContentDialog` in a host class the probe did not see. `DIALOG_MS` may cut a slow dialog's real message. A long property page is heard up to 300 characters each time focus enters it.
- Not covered: browser web dialogs (`role="dialog"`), the UAC prompt (secure desktop, ADR 0016), text that changes while a dialog is open, groups and positions (9.7).
- Revisit: after 9.6c1 (the stop condition), after 9.6c3's Verify (`DIALOG_MS`, the gate, whether `dialog_changed` is too chatty on property pages), at 9.7 if group names need the ancestor chain (option I4), at 9.8 ("read the whole window or dialog").
