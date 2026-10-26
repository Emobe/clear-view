# 0016: Dialog text

Status: Accepted (you, 2026-10-26; set by the agent on your instruction)
Date: 2026-10-26

## Context

Roadmap 9.6b, the second half of 9.6 (speech stage 1, PRODUCT.md "Speech (v1.1)"): when a dialog opens, speak its text. Your decisions (2026-10-25): the order is the title, then the dialog's text, then the focused control, as one batch (ADR 0013); the title is spoken at every verbosity level. 9.6a built the title and is approved.

The roadmap sent 9.6b to `/step` with a condition: stop and raise `/adr 9.6b` if the read "needs more than ADR 0015's one cached read". It does. ADR 0015 defines one read as one cached UI Automation call for the control that has focus plus MSAA for the newest WinEvent's object. A dialog's text lives in other elements (the static text beside the buttons), so reading it means searching the dialog's tree, which 0015 does not cover. This ADR decides that search before any code is written.

### Rules this must keep (ROADMAP "Rules for speech steps", PRODUCT.md)

- With speech switched off, the magnifier behaves exactly as before.
- Password fields are never spoken. "Can't tell" counts as a password field.
- No UI Automation or MSAA call on the render thread. A hung app must not freeze the magnifier or stop speech from being silenced.
- The text that gets spoken is pure logic in cv-core with unit tests.

### Earlier decisions this touches

- **ADR 0015** (Accepted): the focus thread (MTA, its own `CUIAutomation8` with 500 ms timeouts, no UIA event handlers, reads only while `tts_enabled`, never in our own process); one read per settled burst; a UIA timeout marks the foreground window hung until the foreground changes; `WindowInfo` and `Control` are `#[non_exhaustive]`, and "a dialog's text and anything 9.6 and 9.7 need are added to these structs by those steps, as plain data, without a new ADR". The field is allowed. The extra reads are what needs this ADR.
- **ADR 0013** (Accepted): `Kind::Focus` is foreground and cuts off what was speaking; a batch queues its later items behind the first.
- **ADR 0004** (Accepted): events carry plain data, no OS handles or COM pointers.

### What the code does today

- `read_focus` (crates/cv-platform-win/src/focus/mod.rs) reads the foreground window (`caret::foreground`), the program name and the title (`read::window_title`, `GetWindowTextW`), then `read_control`: `GetFocusedElementBuildCache` with one cache request, MSAA from the newest event, values after `value_allowed`, `focus::merge`. The window's class is not read.
- `WindowInfo` (crates/cv-core/src/focus.rs) holds only `title`. `FocusFilter::finish_read` drops a reading whose app, window and control equal the last published one, so whatever is added to `WindowInfo` must be the same on every read in the same window, or every later focus change in a dialog becomes "new".
- `wording::focus_utterances` (crates/cv-core/src/wording.rs) returns the title (when `window_changed`) and the control's phrase as `Kind::Focus` utterances, in that order.

### What has been measured (9.2 probe logs)

- Save As from the elevated Notepad: the foreground window has class `#32770`; UIA reports it as a `Window` whose localised control type is "dialogue"; MSAA says `dialogue`; both name it "Save as". Inside it, UIA gave type `Pane` for every control and MSAA gave the right roles (docs/prototypes/focus-sources.md). If static text also comes back as `Pane` from UIA there, a UIA-only reader finds no text in the dialogs of an elevated app, and you often run Notepad elevated.
- Steam's "Steamwebhelper is not responding" box: class `vguiPopupWindow`, UIA `Window`. A program's own popup, not a standard dialog.
- Not measured: a message box, a task dialog, Notepad's "save changes" prompt, Settings dialogs, a property sheet, a browser's web dialog, the UAC prompt, and how long a walk of a dialog's tree takes.

### External facts

- **IsDialog.** `UIA_IsDialogPropertyId` (30174) "indicates whether the automation element is a dialog window. For example, assistive technology such as screen readers typically speak the title of the dialog, the focused control in the dialog, and then the content of the dialog up to the focused control". Default `FALSE`; the app sets it ([Automation Element Property Identifiers](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-automation-element-propids)). Microsoft's suggested order puts the control before the text. You chose text before the control; this ADR keeps your order.
- **NVDA's detection** (source/NVDAObjects/UIA/__init__.py, [nvaccess/nvda](https://github.com/nvaccess/nvda)): reads `IsDialog` ("easier on build 17682 and later") and falls back to window classes: `#32770`, `NUIDialog`, `Credential Dialog Xaml Host` (UAC), `Shell_Dialog`, `Shell_Flyout`, `Shell_SystemDialog` (Settings). For MSAA, roles `dialog`, `alert` and `property page`.
- **NVDA's dialog text** (`Dialog.getDialogText`, source/NVDAObjects/behaviors.py): walks the dialog's *children*, not all descendants; descends only into container roles (pane, panel, grouping, window, property page, option pane, section, unknown); skips invisible and unavailable objects; keeps static text, labels and links (and read-only single-line edits); skips a text whose name equals the next sibling's name ("almost certainly the label for the next object"); skips a text right after a grouping (its description). Its comment: the result is cached "as it is very slow due to getDialogText".
- **FindAllBuildCache** returns every element in the given scope that matches a condition, with the cached properties, in tree order, in one call. Microsoft warns that a `TreeScope_Descendants` search of the desktop "could iterate through thousands of items" ([docs](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationelement-findallbuildcache)). A dialog that holds a file view (Save As, Open) holds a list of every file in the folder.
- **NormalizeElementBuildCache** walks up from an element to the nearest ancestor matching a tree walker's condition, and returns the root if none matches ([docs](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationtreewalker-normalizeelementbuildcache)). With an `IsDialog == true` condition it would find a dialog inside a window (a XAML `ContentDialog`) in one call from us.
- **EVENT_SYSTEM_DIALOGSTART**: sent for standard dialogs, "not sent consistently by the system" ([Event Constants](https://learn.microsoft.com/en-us/windows/win32/winauto/event-constants)).
- **UAC.** Consent and credential prompts show on the secure desktop by default, and "only Windows processes can access the secure desktop" ([How UAC works](https://learn.microsoft.com/windows/security/identity-protection/user-account-control/how-user-account-control-works)). Our hooks and reads cannot see it. With the secure desktop off, the prompt is an elevated window, which gave the focus thread nothing in 9.4b (STATUS Finding 27(f)).

## Options

### When is the foreground window a dialog

D1. **Class list only.** `GetClassNameW` on the foreground window (a local call, no cross-process message) against NVDA's list. Cost: misses an app that marks its own window with `IsDialog`. Free.
D2. **UIA `IsDialog` on the foreground window, or the class list.** `ElementFromHandleBuildCache(foreground, {IsDialog})`, one cross-process call, only when the foreground window changes. Cost: one call per window change, timed out like any other.
D3. **D2, plus dialogs inside a window.** On every read, `NormalizeElementBuildCache` from the focused element with an `IsDialog == true` walker. Catches a XAML `ContentDialog` (no foreground change, so `window_changed` is false). Cost: one more cross-process call on every focus read, in every app, for a case nobody has measured.

### What is read

R1. **Win32 only.** `EnumChildWindows` for visible `Static` children of a `#32770`, text by `SendMessageTimeout(WM_GETTEXT)`. Cost: classic dialogs only; a task dialog draws its text in a `DirectUIHWND` child with no `Static`, XAML and browsers have none. Labels must be told apart by tab order. Cheap.
R2. **One UIA search of the whole dialog.** `FindAllBuildCache(TreeScope_Descendants)` for text elements and focusable controls. One call from us. Cost: in Save As and Open it walks the whole file view and navigation tree cross-process before it returns; how long is unmeasured, and a slow one hits the 500 ms timeout and marks the dialog hung, which also silences its controls. No way to tell it not to enter lists.
R3. **A bounded UIA walk, NVDA's shape.** `FindAllBuildCache(TreeScope_Children)` on the dialog, then on each child container (pane, group, custom, unknown), never into lists, trees, tables, documents, toolbars or menus; a budget of calls and depth; MSAA's children for a `#32770` where UIA found no text element (the elevated case). Cost: several calls per dialog (one per container visited), only when a dialog comes to the front. More code than R1 or R2.
R4. **MSAA only, NVDA's walk.** `AccessibleChildren` and per-child role, state and name. Cost: no caching, so three calls per child; weaker than UIA in XAML (9.2).

### Where the text is chosen

S1. **The platform walks; cv-core chooses.** The focus thread maps what it found into plain `DialogNode`s (name, neutral role, focusable, offscreen, depth) and a pure cv-core function picks the text: the label rule, the grouping rule, duplicates, the budget. Unit-tested without Windows.
S2. **The platform chooses.** Cost: the label rule can be tested only by running dialogs, against the ROADMAP rule.

## Decision

Recommendation: **D2, R3, S1**, read in the same read as the control so the event stays one batch (ADR 0015). D3 is allowed later under this ADR (below). It extends ADR 0015's read and supersedes nothing.

### When it runs

- Only when the foreground window differs from the window the last dialog check ran for. The focus thread keeps the result per window handle, so every later read in the same dialog carries the same text and the duplicate rule in `FocusFilter` still works.
- A window is a dialog when `GetClassNameW` gives one of `#32770`, `NUIDialog`, `Shell_Dialog`, `Shell_SystemDialog` (NVDA's list without `Shell_Flyout`, which is a flyout, and without the UAC host, which we can't reach), or UIA `IsDialog` on the window's element is true. The class check comes first and skips the UIA call when it answers.
- Not a dialog: nothing more is read; `dialog_text` is empty.

### The walk (cv-platform-win focus thread)

- One cache request: Name, ControlType, IsKeyboardFocusable, IsOffscreen, IsPassword. No value property and no pattern. A text element has no value, and a focusable control's value is never read here.
- `FindAllBuildCache(TreeScope_Children, true condition)` on the dialog's element. Each child becomes a `DialogNode`. A child whose role is a container (`Pane`, `Group`, `Custom`, `Unknown`, or a child `Window`) is walked the same way, depth first, in tree order.
- Never entered: `List`, `Tree`, `Table`, `Document`, `Edit`, `ComboBox`, `ToolBar`, `Menu`, `MenuBar`, `Tab` and anything focusable. Their own node is kept (the label rule needs it), not their children.
- Budget: at most `DIALOG_CALLS` (8) find calls, depth `DIALOG_DEPTH` (3), `DIALOG_NODES` (64) nodes. When the budget runs out the walk stops and reports itself incomplete.
- MSAA fallback, for a `#32770` only, when the UIA walk found no node whose role is `Text` (UIA's Win32 proxy answering `Pane` for everything, as in the elevated Save As): `AccessibleObjectFromWindow(dialog, OBJID_CLIENT)`, then `AccessibleChildren` with the same container, skip and budget rules, mapped through the existing `map::msaa_role` and `msaa_states` (invisible and unavailable skipped). Its nodes replace the UIA ones.
- A node that UIA says is a password (`IsPassword` true or unreadable) or MSAA says is `protected` (or whose state is unreadable) has its name emptied before it leaves the platform code.
- A timeout in any call marks the window hung, as ADR 0015 says. The control already read for this read is kept; `dialog_text` is empty.
- `CV_FOCUS_LOG=1` adds one line per dialog: the source (uia or msaa), the calls, nodes and ms, whether the walk was complete, and the chosen text. Never a focusable node's name.

### The choice (cv-core, pure, unit-tested)

`focus::dialog_text(nodes: &[DialogNode], complete: bool, title: &str) -> Vec<String>`:

1. An incomplete walk gives nothing. A dialog too big for the budget is a window full of controls, not a message.
2. Only nodes of role `Text` that are not focusable and not offscreen give text. A focusable node's name is never returned (a test checks it), so an edit whose name is its text (STATUS Finding 27(b)) is used only for the comparison in 3.
3. **Label rule:** a text whose cleaned name equals the cleaned name of the next focusable node at the same depth under the same parent is that control's label and is dropped ("File name:" before the file name box). Cleaning: one `&` removed (as `clean_name`), a trailing `:` removed, whitespace flattened, case ignored.
4. **Group rule:** a text right after a `Group` sibling is the group's description and is dropped.
5. A text equal to the title, or to an earlier text, is dropped. Empty texts are dropped.
6. At most `DIALOG_TEXTS` (8) pieces, each cut at `VALUE_MAX`.

`WindowInfo` gets `pub dialog_text: Vec<String>`, empty when the window is not a dialog or says nothing. `WindowInfo::new` keeps its signature (empty text).

### Wording (cv-core `wording`)

- `focus_utterances` returns the title, then the dialog text as one utterance (the pieces joined with ". ", flattened, cut at a word near `DIALOG_SPOKEN_MAX`, 300 characters), then the control. All `Kind::Focus`, one batch. Only when `window_changed`, at every verbosity level, like the title.
- The control's phrase is unchanged. A text equal to the control's spoken name is said once, by the control.

### Dialogs inside a window (D3), later

Not built in 9.6b. The 9.6b2 Verify list records, for Notepad's "save changes" prompt and one Settings dialog, whether the `[focus]` line shows a window change. If a common prompt does not change the foreground, an item (raised by you) may add D3 under this ADR: `NormalizeElementBuildCache` from the focused element with an `IsDialog == true` walker, only while speech is on, the dialog's runtime id kept in place of the window handle, its cost measured in that item. Anything beyond one more call per read needs a new ADR.

### Speech off

- `--no-default-features`: no focus thread, nothing changes.
- `tts` on, speech off: the focus thread reads nothing (ADR 0015), so no dialog walk either. The caret thread and the magnifier are untouched.

### Roadmap

9.6b is split (pending this ADR):

- **9.6b1** cv-core: `DialogNode`, `focus::dialog_text` with the rules above, `WindowInfo::dialog_text`, the constants, and the wording (title, text, control). Unit tests for each rule, including that a focusable node's name and a password node never come out. No Windows code.
- **9.6b2** cv-platform-win: the dialog check, the bounded UIA walk with its budget, the MSAA fallback for `#32770`, the per-window cache, the hung rule, the `CV_FOCUS_LOG` line. Verify: a message box (Notepad's "replace file?" in Save As), a task dialog (Explorer's "Replace or Skip Files"), Save As itself (should say nothing more than today), a property sheet (a file's Properties), one from an elevated Notepad (the MSAA fallback), Notepad's "save changes" prompt and a Settings dialog (record whether the foreground changed), ms per dialog from the log; a run with speech off and one with `--no-default-features`.

## Consequences

- Easier: the dialog text is chosen by tested rules in cv-core that a Linux backend can feed from AT-SPI. The event stays one batch, so the title, text and control are spoken in your order and silenced together. The walk never runs outside a dialog and never twice for one window.
- Harder: a second kind of read in the focus thread, with a budget and a fallback source. A dialog's first announcement waits for up to `DIALOG_CALLS` more cross-process calls.
- Risks: the label rule is NVDA's guess and may read a label or drop a message; the budget may be too small for a real message in a big dialog (then nothing is said) or too big for Save As (then its walk costs time, measured in 9.6b2). Text that changes while the dialog is open ("Copying 3 of 10") is not spoken again. A program's own popup (Steam's) is not recognised. UAC is not reachable at all.
- The constants (`DIALOG_CALLS`, `DIALOG_DEPTH`, `DIALOG_NODES`, `DIALOG_TEXTS`, `DIALOG_SPOKEN_MAX`) and the class list are starting values. A step may change them with a measurement and say so in its PR.
- STATUS Finding 29 ("Desktop 1") is separate: it needs the focused control's UIA class in `Control`, which ADR 0015 allows without a new ADR. It can be done before or after 9.6b.
- Revisit: after 9.6b2's Verify (budget, label rule, MSAA fallback, in-window prompts); at 9.8, whose "read the whole window or dialog" command may reuse the walk with a larger budget; at 6.3 or 13.2, when UIAccess changes what elevated windows and the UAC prompt give.
