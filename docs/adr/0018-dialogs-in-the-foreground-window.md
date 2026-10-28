# 0018: Dialogs inside the foreground window

Status: Accepted (you, 2026-10-28; set by the agent on your instruction)
Date: 2026-10-27

## Context

Roadmap 9.6c (speech stage 1, PRODUCT.md "Speech (v1.1)"). ADR 0017 (Accepted) chose W2, I2, P1 and T2, to be built only after a probe (9.6c1) checked W2 and I2. You ran the probe on 2026-10-27 (`probe-9.6c1.txt`, not committed). Its first stop condition fired: "Replace or Skip"'s focus root was the foreground window. So 0017 is raised again, as its roadmap item says (STATUS Findings 35 to 38, docs/prototypes/focus-sources.md "Dialogs outside the foreground window").

### Rules this must keep (ROADMAP "Rules for speech steps", PRODUCT.md)

- With speech switched off, the magnifier behaves exactly as before.
- Password fields are never spoken. "Can't tell" counts as a password field.
- No UI Automation or MSAA call on the render thread. A hung app must not freeze the magnifier or stop speech from being silenced.
- The text that gets spoken is pure logic in cv-core with unit tests.

### Earlier decisions this touches

- **ADR 0015** (Accepted): the window of a `FocusChanged` is the foreground window; `window_changed` is set only by an `EVENT_SYSTEM_FOREGROUND` WinEvent; the hung mark is keyed on the foreground window.
- **ADR 0016** (Accepted): the dialog check (class list, else UIA `IsDialog`) runs once per foreground window; a bounded walk; the MSAA fallback for `#32770`; the text is spoken only when `window_changed`. Its "D3, later" section allowed one `NormalizeElementBuildCache` call per read.
- **ADR 0017** (Accepted): the read window (W2, the root of `GetGUIThreadInfo(0).hwndFocus`), the inner dialog (I2), property pages (P1), the cost split and budget (T2), the Finding 29 rule, and the cv-core additions (`window_key`, `dialog_key`, `dialog_name`, `dialog_changed`).

### What the code does today

- `read_focus` (crates/cv-platform-win/src/focus/mod.rs) reads `GetForegroundWindow` first. Title, program, dialog check, dialog cache and hung mark are all keyed on that window.
- `on_win_event` queues every `EVENT_SYSTEM_FOREGROUND` with `foreground: true`. `FocusFilter::winevent` (crates/cv-core/src/focus.rs) ORs that into `window_changed`, which stays set until an event is published. A reading made stale by a newer event is dropped while the flag stays pending. So a foreground WinEvent that reaches the thread always gives the next published event `window_changed`.
- `map::is_hung` (focus/map.rs) counts a UIA failure as a hang only for `UIA_E_TIMEOUT` or a call that took at least twice the timeout. Any other error is not a hang.
- `map::is_dialog_class` knows `#32770`, `NUIDialog`, `Shell_Dialog` and `Shell_SystemDialog`.

### What the 9.6c1 probe measured (one run, 224 events)

- **"Replace or Skip Files"** (Finding 35): the dialog is a top-level `OperationStatusWindow` in explorer.exe (not a dialog class, framework DirectUI). The first button event (64.81 s) was read 1 ms after it arrived. `GetForegroundWindow` was already the dialog, `GetGUIThreadInfo` had no focus window yet, and UIA's focused element was already the dialog's button. That event's own `hwnd`, however, was Explorer's file view, and its MSAA object was the ".claude" list item. `EVENT_SYSTEM_FOREGROUND` for the dialog was logged right after, read 53.6 ms later. From then on the focus root was the foreground window. Normalize found no `IsDialog` ancestor. The dialog's `DirectUIHWND` client gave MSAA role `dialogue` (67.25 s); its root window gave role `window`.
- **The 9.6b2 run** (Finding 31, `focus-log.txt` lines 112 to 119): `[keys] silence` (Ctrl, so likely a paste) and then three dialog buttons published with no `new window`. By the code above, that means no `EVENT_SYSTEM_FOREGROUND` reached the thread before them. Microsoft says the system sends that event on every foreground change, also between windows of one thread ([Event Constants](https://learn.microsoft.com/en-us/windows/win32/winauto/event-constants)). So in that run the dialog most likely did not become the foreground window. Microsoft also says an app "cannot force a window to the foreground while the user is working with another window. Instead, Windows flashes the taskbar button" ([SetForegroundWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow)). That fits your report that the dialog showed as a window only after you switched away and back (line 129). It is inferred, not logged: the log prints no foreground events.
- **Notepad's "save changes" prompt** (Finding 36): the focus root is the foreground window; the `GA_PARENT` chain holds no dialog class. `NormalizeElementBuildCache` with an `IsDialog == true` walker found a XAML `Window` named "Notepad", `IsDialog` true, no `hwnd`, from the Save button on (0.4 to 0.8 ms). It found nothing on the burst's first event, an unnamed `InputSiteWindowClass` pane. The focused element's `FrameworkId` was `XAML`.
- **A file's Properties** (Finding 36): the sheet (`#32770`, titled "… Properties") is the foreground. The `GA_PARENT` chain from the focus window names the page (`#32770`, window text "General", "Digital Signatures", "Security") as the nearest dialog class. That holds when focus is in a list on the page too. Normalize gives the same pages. On OK or Cancel there is no inner dialog, and Normalize returns the sheet.
- **Save As**: Normalize returns the dialog itself (the foreground window). No inner dialog.
- **Normalize failures** (Finding 37): 189 of 223 calls failed with `0x80004005`. Every success was inside a real dialog. The docs say the root is returned when nothing matches ([NormalizeElementBuildCache](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationtreewalker-normalizeelementbuildcache)). Here it was not.
- **W2's hazards** (Finding 38): while Properties was closing, the foreground was an untitled `StubWindow32` and `hwndFocus` was still the sheet's Cancel button. On Alt+Tab, the focus root was "Task Switching" one event before the foreground was.
- **Cost**: `GetGUIThreadInfo(0)` under 1 ms on every event. Normalize median 1.1 ms found, 3.6 ms failed, worst 48 ms.
- **Not seen**: a Settings `ContentDialog`, a message box or task dialog that does not take the foreground, any dialog's text read by these routes.

### External facts

- **GetGUIThreadInfo** with thread id 0 reports the *foreground* thread's `hwndFocus` and "may not return valid window handles … when a window is losing activation" ([docs](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getguithreadinfo)). A dialog on a thread that is not the foreground thread is never its `hwndFocus`.
- **GetAncestor**: `GA_PARENT` "does not include the owner"; `GA_ROOT` walks the parent chain ([docs](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getancestor)).
- **GetWindowTextW** on another process's window "retrieves the window caption text" without sending a message, "so applications can call GetWindowText without becoming unresponsive if the process that owns the target window is not responding" ([docs](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowtextw)). The probe read the page names this way.
- **Property sheets**: the sheet and its pages are dialog boxes, and each page is a child window of the sheet ([About Property Sheets](https://learn.microsoft.com/en-us/windows/win32/controls/property-sheets)).
- **IsDialog in XAML**: `ContentDialog` and `Flyout` default to `IsDialog` true; for a dialog a screen reader "typically speaks the title of the dialog, the focused control in the dialog, and then the content of the dialog" ([AutomationProperties.IsDialog](https://learn.microsoft.com/en-us/uwp/api/windows.ui.xaml.automation.automationproperties.isdialog)).
- **NVDA** does not enter a child that is itself a dialog when reading dialog text ("Stops double reporting when focus is in a property page in a dialog", source/NVDAObjects/behaviors.py, [nvaccess/nvda](https://github.com/nvaccess/nvda)).

## Options

### Which window the title and the dialog check key on

W1. **The foreground window, as ADR 0015, plus a window key.** `Reading` carries the foreground window's handle as an opaque `window_key`. The filter sets `window_changed` on a foreground WinEvent **or** when the key differs from the last published event's. That covers a read that sees the new foreground before its WinEvent arrives (the 53 ms at 64.81 s), and a foreground change whose event was missed. `CV_FOCUS_LOG` gains a line per foreground WinEvent, so the next 9.6b2-style miss shows whether the dialog was ever the foreground. Cost: none per read. The same window can be announced twice when a read sees the new foreground and its WinEvent then arrives (`Kind::Focus` cuts the first off). A dialog that never becomes the foreground still says no title.
W2. **ADR 0017's read window** (the root of `GetGUIThreadInfo(0).hwndFocus` when it differs from the foreground and is titled or a dialog class). Cost: in this run it changed nothing in any case it was meant for. It cannot see a dialog on a thread that is not the foreground thread, which is the likely 9.6b2 case. It picks a closing sheet and "Task Switching" ahead of the foreground (Finding 38), so it would need extra guards.
W3. **The newest focus event's window root, gated** (same program as the foreground, titled or a dialog class). This is the only source that would see a dialog on a non-foreground thread. Cost: at 64.81 s, the one good case, the event's window was Explorer's file view while UIA was already in the dialog. The 9.2 logs had a third to two thirds of events from a root that was not the foreground. It would be built on one run that is not understood.

### How a dialog inside the foreground window is found

I1. **None (today).** Misses Notepad's prompt and property pages.
I2. **The Win32 chain, then Normalize only in XAML content.** `GetGUIThreadInfo(0).hwndFocus` is used to start the chain, never to choose the window. Only when its `GA_ROOT` is the foreground window, walk `GA_PARENT` from it up to the foreground window (at most 16 steps), and take the nearest dialog-class window as the **inner dialog**. Local calls only. Only when that found none and the focused element's cached `FrameworkId` is `XAML` or `WinUI`, call `NormalizeElementBuildCache` once from the focused element with an `IsDialog == true` walker. A result is an inner dialog only when it is not the foreground window's own element. Cost: one cross-process call per read in XAML content, about 1 to 4 ms in the probe. Win32, DirectUI and Chrome pay nothing.
I3. **Normalize on every read.** It finds the pages too, so the chain could go. Cost: one call per read in every app. Most calls fail (189 of 223) at about 3.6 ms each, worst 48 ms. Its failure is an error code that the docs say should not happen.

### A dialog-like window that is no dialog by class or `IsDialog` ("Replace or Skip Files")

C1. **Add `OperationStatusWindow` to the dialog class list.** Cost: one name, from evidence. The same class is the copy progress window, so its text ("Copying … items …") is read once when it takes the foreground. What the walk gives inside DirectUI is not measured.
C2. **MSAA role of the foreground window's client.** One MSAA call per new foreground window, dialog if role `dialogue`. Cost: here the `dialogue` role was on the child `DirectUIHWND`'s client, not on the root's. A general rule would need the role per level, which is cross-process.
C3. **Leave it.** The title and the control are spoken (it took the foreground in 9.6c1); the conflict text ("The destination already has a file named …") is not.

### Property sheets, cost and Finding 29

ADR 0017's P1 (each page is an inner dialog; its name and text are spoken on entry; the walk does not enter a nested dialog), T2 (timing split, `DIALOG_MS`, early switch to MSAA) and Finding 29 rule do not depend on W2. The probe supports P1: the chain finds the page and its name comes from `GetWindowTextW`. No new option is weighed for them here.

## Decision

Recommendation: **W1, I2, C1, with ADR 0017's P1, T2 and Finding 29 rule kept.**

It supersedes ADR 0017's W2 (the read window) and all that rests on it: the read window as the key for title, program, dialog check and hung mark; the own-process rule on the read window; Finding 38's hazards. It keeps from 0017 the inner dialog (narrowed below), P1, T2, the cv-core fields, the wording and the Finding 29 rule. Against ADR 0015 it supersedes only "`window_changed` is a foreground WinEvent" (the window key is added). Against ADR 0016 it supersedes, as 0017 did, its "D3, later" section and the walk entering nested dialogs. The dialog check stays keyed per foreground window, now with an inner dialog inside it. Changing the Status lines of 0015, 0016 and 0017 is yours.

### The window (W1, cv-platform-win and cv-core)

- The window of every reading stays the foreground window: title, program, dialog check, hung mark and own-process rule as today.
- `Reading` gains `window_key: u64`, the foreground handle's value. It is opaque, compared only for equality, and not in the event (ADR 0004).
- `FocusFilter`: `window_changed` is a pending foreground WinEvent **or** a `window_key` different from the last published one. A reading with no program (the shell's staging window) publishes nothing and changes no key.
- `CV_FOCUS_LOG=1` adds `[focus] foreground event: class "…"` per `EVENT_SYSTEM_FOREGROUND`. When the newest focus event's root is not the foreground window, a read adds `event root class "…" is not the foreground`. Classes only; titles stay in the existing `new window` line.

### Dialogs inside the foreground window (I2)

- **Win32 chain**: `GetGUIThreadInfo(0)`. If `hwndFocus` is set and its `GA_ROOT` is the foreground window, walk `GA_PARENT` from `hwndFocus` up to (not including) the foreground window, at most 16 steps. The nearest window with a dialog class is the inner dialog; its name is `GetWindowTextW`. If the focus root is any other window (a closing dialog, Alt+Tab), there is no chain and no inner dialog.
- **XAML**: only when the chain found none and the focused element's cached `FrameworkId` is `XAML` or `WinUI`. One `NormalizeElementBuildCache` from the focused element, walker `IsDialog == true`, cache request Name, `NativeWindowHandle` and the runtime id. No inner dialog when the result's handle is the foreground window. A failure is "no inner dialog", unless `map::is_hung` says it hung; then the foreground window is marked hung as today.
- **The dialog to walk** is the inner dialog if there is one, else the foreground window when ADR 0016's check says it is a dialog. ADR 0016's walk gains one rule: a node that is itself a dialog (dialog class, cached `IsDialog`, MSAA role dialogue or property page) is kept but not entered. The walk's cache request gains `ClassName` and `IsDialog`.
- The text is cached per (foreground window, inner dialog) pair, at most 8 pairs, newest kept. Returning to a page does not walk it again.

### "Replace or Skip Files" (C1)

`OperationStatusWindow` joins `map::is_dialog_class`, with a test. 9.6c3's Verify records what the walk gives there. If it says nothing useful, the class is taken out again in that step and the finding stays open.

### What the core sees (cv-core, as ADR 0017)

- `Reading` gains `window_key: u64` (above) and `dialog_key: Option<u64>` (the inner dialog's handle, or a hash of its runtime id when it has none).
- `WindowInfo` gains `dialog_name: String`: the inner dialog's name, empty when there is none. `dialog_text` is the text of the dialog to walk.
- `FocusChanged` gains `dialog_changed: bool`: the window did not change, `dialog_key` is `Some` and it differs from the last published one. Leaving an inner dialog for the window's own controls changes nothing that is spoken. The duplicate rule compares both keys.

### Wording (cv-core `wording`, as ADR 0017 plus one rule)

- `window_changed`: the title, then `dialog_name`, then the dialog text, then the control.
- `dialog_changed` only: `dialog_name`, then the dialog text, then the control.
- Neither: the control, as now. All `Kind::Focus`, one batch, every verbosity level.
- `dialog_name` is dropped when it is empty or the window title contains it, ignoring case. Notepad's prompt is named "Notepad" inside a window titled "… - Notepad", so it is dropped there; a page named "General" in "CLAUDE.md Properties" is kept. A name equal to the control's spoken name is said once, by the control.
- When joining the dialog text, a piece ending in `:` is joined to the next one by a space ("Type of file: Text Document").

### Cost (T2, as ADR 0017)

- The `[focus] dialog` line gives ms and calls for the UIA root, the UIA walk and the MSAA walk separately, and the inner-dialog lookup gets its own ms (chain, Normalize).
- `DIALOG_MS`, a cv-core constant, starts at 250 ms for the whole dialog read; past it the walk stops as incomplete (says nothing). 9.6c3 sets the final value from what it measures and says so in its PR.
- A `#32770` whose first UIA level holds only `Pane` nodes skips the rest of the UIA walk and goes to MSAA.

### STATUS Finding 29 ("Desktop 1"), as ADR 0017

The control read's cache request gains `ClassName`. A UIA `Pane` of class `#32769` is no UIA reading, by a pure rule in map.rs with a test.

### Speech off

- `--no-default-features`: no focus thread, nothing changes.
- `tts` on, speech off: the focus thread reads nothing (ADR 0015), so no chain, Normalize or log line either. The caret thread and the magnifier are untouched.

### Roadmap

9.6c1 (the probe) is done as far as this ADR needs: it is recorded when you approve PR #109. 9.6c2 and 9.6c3 are rewritten (pending this ADR):

- **9.6c2** cv-core: `Reading::window_key` and `dialog_key`, `WindowInfo::dialog_name`, `FocusChanged::dialog_changed`, the filter rules, the wording (both cases, the dropped dialog name, the `:` join), `DIALOG_MS`. Unit tests for each rule. No Windows code.
- **9.6c3** cv-platform-win: the window key, the foreground-event and event-root log lines, the inner dialog (chain, gated Normalize), the walk's nested-dialog rule, the per-pair cache, `OperationStatusWindow`, the timing split and `DIALOG_MS`, the `#32770` early switch to MSAA, the Finding 29 rule. Verify: "Replace or Skip Files" (title, its text if any, the control; the log says whether and when the foreground event came), Notepad's "save changes" prompt (its text, then the button), a file's Properties (title, page name and text, control; Ctrl+Tab speaks the new page; OK says only the button), Save As (not slower; ms from the log), Alt+Tab and a combo box drop-down (no extra titles), no "Desktop 1", a Settings dialog if one comes up, a run with speech off and one with `--no-default-features`.

## Consequences

- Easier: the window rule stays ADR 0015's, so Alt+Tab, closing dialogs and the hung mark behave as they were tested in 9.6b2. Win32, DirectUI and Chrome apps pay only local calls. Property pages and XAML prompts reuse ADR 0016's walk and cv-core's text rules.
- Harder: two keys in the filter, a framework gate, and one more class in a hand-kept list.
- Risks: a dialog that never takes the foreground still says no title or text (the likely 9.6b2 case). The new log line exists to show it when it happens again. A window can be announced twice when its read beats its foreground event. A `ContentDialog` in XAML content whose focused element reports another framework is missed. The XAML prompt's text from the walk is not measured. `OperationStatusWindow`'s copy progress text may be spoken once. `DIALOG_MS` may cut a slow dialog's real message.
- Not covered: browser web dialogs (`role="dialog"`), the UAC prompt (secure desktop, ADR 0016), text that changes while a dialog is open, groups and positions (9.7).
- Revisit: when a `CV_FOCUS_LOG` run shows a dialog's buttons read while the foreground is another window (then W3, with that log as evidence). Also after 9.6c3's Verify (`DIALOG_MS`, the gate, `OperationStatusWindow`, whether `dialog_changed` is too chatty on property pages), and at 9.7 and 9.8.
