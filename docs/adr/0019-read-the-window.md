# 0019: Read the whole active window or dialog

Status: Proposed
Date: 2026-11-02

## Context

Roadmap 9.8b, the third reading command of speech stage 1 (PRODUCT.md "Speech (v1.1)"): Ctrl+Alt+Shift+W reads the whole active window or dialog in order. The chord is decided by ADR 0014. 9.8a built the other two commands (read the title, repeat the focused item) and was merged as PR #114 without a hand test. The roadmap split 9.8 on 2026-11-02 because this third command needs a read that no Accepted ADR bounds. This ADR decides what the read visits and in what order, its bounds, how a command asks the focus thread for it, what stops it, the password rule for every node, and what it says for a document or a Terminal buffer.

### Rules this must keep (ROADMAP "Rules for speech steps", PRODUCT.md)

- With speech switched off, the magnifier behaves exactly as before.
- Password fields are never spoken: no characters, no words, no length. "Can't tell" counts as a password field.
- No UI Automation or MSAA call on the render thread. A hung app must not freeze the magnifier or stop speech from being silenced.
- The text that gets spoken is pure logic in cv-core with unit tests.
- Only cv-platform-win and cv-tts depend on `windows`.

### Earlier decisions this touches

- **ADR 0013** (Accepted): the speech thread makes no UIA or MSAA call; "events carry the text they need ... the speech thread never asks UIA for more". `Kind::Command` is foreground and cuts off everything; a batch cuts off what its first item's rule says, then queues the rest; a core event whose `at` is earlier than the last silence is dropped. The queue holds at most `QUEUE_MAX` (32) utterances and drops the oldest.
- **ADR 0014** (Accepted): Ctrl+Alt+Shift+W reads the active window or dialog, registered from the step that adds its `Action`, only with `tts`. A lone Ctrl silences (raw input, on key down).
- **ADR 0015** (Accepted): the focus thread owns the process's focus UIA object (MTA, 500 ms timeouts, no UIA handlers), reads only while `tts_enabled` is true and never when the foreground window is ours, wakes on WinEvents and on `FocusWaker::wake()`, and marks a window hung on a UIA timeout until the foreground changes. A value is never fetched before the password decision, and the Text pattern is never read (the whole Terminal buffer, Finding 25(h)); reading text is Phases 10 and 12.
- **ADR 0016** (Accepted): the bounded dialog walk. Children only, level by level, containers (`Pane`, `Group`, `Custom`, `Unknown`, `Window`) entered; `List`, `Tree`, `Table`, `Document`, `Edit`, `ComboBox`, `ToolBar`, `Menu`, `MenuBar`, `Tab` and anything focusable never entered; `DIALOG_CALLS` 8, `DIALOG_DEPTH` 3, `DIALOG_NODES` 64; the MSAA fallback for a `#32770` where UIA finds no text; password names emptied before a node leaves the platform code; an incomplete walk says nothing. Its Revisit list names 9.8: "may reuse the walk with a larger budget".
- **ADR 0018** (Accepted): the dialog to walk is the inner dialog that holds the focus if there is one (`GA_PARENT` chain, or one `NormalizeElementBuildCache` in XAML), else the foreground window when it is a dialog; `DIALOG_MS` 250 bounds one dialog read; dialog text is cached per (window, inner dialog) pair. Its Revisit list names 9.8.
- **ADR 0004** (Accepted): one `#[non_exhaustive]` `CoreEvent` enum of plain data, fan-out delivery, a consumer never blocks a source.

### What the code does today

- **The focus thread** (crates/cv-platform-win/src/focus/mod.rs) waits in `MsgWaitForMultipleObjectsEx(QS_ALLINPUT)` and runs `pump`, which dispatches every message and stops on `WM_QUIT`. `FocusWaker::wake` posts `WM_USER` with no payload; the loop treats it as "read `tts_enabled` again". A read runs only when `FocusFilter::due`. While a read runs, no message is pumped, so WinEvents wait in the queue; after it, `pump` runs once more and the filter drops a reading that a newer event made stale. There is no way to ask the thread for anything else.
- **The walk** (walk.rs) is generic over a `Source` (UIA or MSAA, dialog.rs) and enforces the ADR 0016 constants and the `DIALOG_MS` deadline. `is_container` is the enter list. It is pure and tested with a fake tree.
- **The node** (cv-core focus.rs) is `DialogNode { name, role, focusable, offscreen, depth }`, `#[non_exhaustive]`. The UIA node request caches Name, ControlType, IsKeyboardFocusable, IsOffscreen, IsPassword, ClassName and IsDialog; no value, no pattern, no state.
- **9.8a** (cv-core speech.rs, wording.rs; cv-tts speech.rs): the hotkey closure in app calls `SpeechHandle::read(ReadCommand)`. The speech thread answers from `FocusMemory`, the window and control of the last `FocusChanged` it saw, with one `Kind::Command` utterance. The last `WindowInfo` already carries the title, the inner dialog's name and the dialog text (ADR 0016, 0018), so "read the dialog" can be answered from memory with no new read.
- **Events reach speech** through the speech thread's `EventHub` subscription with a wake (ADR 0013); `utterances_for` turns each one into utterances. The render thread's `Tracker::handle` matches every variant with no catch-all, so a new variant needs an arm there.

### What has been measured

- 9.2 (docs/prototypes/focus-sources.md): UIA median 2 to 28 ms per read, about 500 ms for the first read into a new window, 9.1 s once with a hung Steam. Terminal is a UIA `Text` element of class `TermControl` whose Text pattern holds the whole buffer, reported `not-password`. A Brave page is a `Document` named by the page title whose Value pattern is the page address. Save As holds a file view (one item per file) and a navigation tree.
- 9.6b2 and 9.6c1: an MSAA walk of a classic dialog took 0.3 to 0.4 s (STATUS Findings 31 to 34, the reason for `DIALOG_MS`).
- Not measured: how long a children-only UIA walk of a whole Explorer, Notepad or Settings window takes; how many nodes such windows hold outside their lists and documents; whether Explorer's file list returns only its realized items to a `FindAll` of its children.

### External facts

- **NVDA+B** "Reads all controls in the active window" (`script_speakForeground`, source/globalCommands.py, [nvaccess/nvda](https://github.com/nvaccess/nvda/blob/master/source/globalCommands.py)). It calls `SayAllHandler.readObjects(api.getForegroundObject())`. `_ObjectsReader` (source/speech/sayAll.py) walks the object and every descendant depth first through `simpleFirstChild` and `simpleNext` (NVDA's simple review tree), with no budget, and reads one object at a time: the next object is fetched when the previous one starts speaking. Pressing the key again starts over; NVDA's stop-speech key ends it.
- **Narrator+W** reads "the title and contents of the window that has focus" ([Narrator: reading text](https://support.microsoft.com/en-us/help/22799/windows-10-narrator-reading-text)).
- **Caching a subtree.** A cache request's `TreeScope` may include the element, its children or its descendants; cached children come from `GetCachedChildren`. `TreeFilter` limits caching to the control view by default, or the content view. With `AutomationElementMode_None` the elements carry cached data only, and Microsoft names "a screen reader, which might prefetch the name and control type properties of elements in a window" as the example ([Caching UI Automation Properties and Control Patterns](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-cachingforclients), [put_TreeScope](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationcacherequest-put_treescope)). A filter condition selects elements; it does not stop the request descending below an element it skips. `FindAllBuildCache` with `TreeScope_Descendants` "could iterate through thousands of items" ([docs](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationelement-findallbuildcache), cited in ADR 0016).
- In the pinned `windows` 0.62.2 (src/Windows/Win32/UI/Accessibility/mod.rs in the crate source, feature `Win32_UI_Accessibility`, already enabled): `IUIAutomationCacheRequest::SetTreeScope`, `SetTreeFilter`, `SetAutomationElementMode`, `IUIAutomationElement::FindAllBuildCache`, `BuildUpdatedCache`, `GetCachedChildren`, `IUIAutomation::ContentViewCondition`, `TreeScope_Subtree`, and `AccessibleChildren`. No new feature and no new dependency is needed by any option below.
- **PostThreadMessageW** posts a message with two pointer-sized parameters to a thread's queue; the focus thread already has one (it calls `PeekMessageW(PM_NOREMOVE)` at start for this reason, and `FocusWaker` uses it).

## Options

### How the command asks for the read

Q1. **Answer from memory, as 9.8a.** The speech thread says the last `FocusChanged`'s title, inner dialog name, dialog text and control. No new read and no new path. Cost: nothing is read that a focus event did not already carry, so a window that is not a dialog gives only its title and the focused control. That misses what the roadmap item asks for ("a window's panes, lists, documents and toolbars").

Q2. **A request message into the focus thread; the answer is a core event.** app's hotkey closure calls a new `FocusWaker` method, which posts a request to the focus thread. The thread walks the window and publishes `CoreEvent::WindowRead` to the hub; the speech thread turns it into utterances with `utterances_for` like any other event. Cost: a new `CoreEvent` variant (an arm in `Tracker::handle` and in `describe`), a second message in `pump`, and a reply that reaches the render thread too, where it is ignored. Gains: no new channel, no new wake in cv-tts, and the speech thread stays as it is; the silence rule (`accepts_event`) applies to the answer unchanged.

Q3. **A request message, the answer on a private channel** from the focus thread to the speech thread. Cost: a second wake path into cv-tts's wait, and app hands a sender into cv-platform-win. It is the private channel the 8.1 audit listed as conflict 3 (the old cv-tts focus handler bypassed `CoreEvent`). Gains: the render thread never sees the answer. Little else.

Q4. **The speech thread reads the window itself.** Contradicts ADR 0013 (no UIA or MSAA call on the speech thread, so a hung app can never hold up a silence). Rejected.

### What the read visits

V1. **Dialogs only, from what is cached.** For a dialog: title, inner dialog name, dialog text, focused control. For any other window: title and focused control. Cost: as Q1. Free.

V2. **ADR 0016's walk, widened for windows and bounded.** The same children-only walk from the same root (the inner dialog, else the foreground window), with a wider enter list and its own budget. Every node is spoken in tree order, text as text and controls by their phrase, as NVDA's object reader does. Lists, trees, tables, documents, text boxes and combo boxes are said by name and role and not entered. Cost: up to tens of cross-process calls per press (unmeasured for whole windows), and a second set of constants. One snapshot per press, so a window that changes while it is being spoken is not followed.

V3. **One subtree cache request.** `ElementFromHandleBuildCache` with `TreeScope_Subtree`, the control view and `AutomationElementMode_None`: the whole window's names, roles and states in one call. Cost: no way to stop it entering a file view, a page or a Terminal; Save As, Explorer and Brave return every item. One call cannot be cut by our budget: a slow one hits the 500 ms timeout, which marks the window hung and silences its focus announcements until the foreground changes.

V4. **NVDA's incremental reader.** The focus thread keeps a walk position and fetches the next node each time the speech thread starts speaking the previous one; no total budget. Cost: a request and reply per node between two threads, a walk state that must be dropped on focus change, silence and speech off, and a "started speaking" report from cv-tts that does not exist. That machinery is what continuous reading (12.1) has to decide, and it is not needed to read a window of a few dozen controls.

### When it stops

S1. **Silence, a focus change, or another press.** Ctrl silences (ADR 0014) and drops an answer still on its way (ADR 0013's late-event rule). A focus change is `Kind::Focus`, which cuts off a `Kind::Command` batch (ADR 0013). A second press cuts off the first and reads again from the start (NVDA's behaviour). No new state.

S2. **S1, but a second press while reading stops it** (a toggle). Cost: the speech thread must know a window read is speaking, and the focus thread must know not to read again; a press after the answer finished must still read. Ctrl already stops it with one key.

## Decision

Recommendation: **Q2, V2, S1.** It extends ADR 0004 with a `CoreEvent` variant, ADR 0015's focus thread with one request message, and ADR 0016's walk with a second enter list and budget. It supersedes nothing: the dialog walk, its constants and its cache are unchanged, and the window read writes nothing into them.

### The request (app and cv-platform-win)

- `Action::ReadWindow` on Ctrl+Alt+Shift+W, a speech binding registered only with `tts` (ADR 0014). app's hotkey closure logs `[keys] read window` and calls `FocusWaker::read_window()`. It does not call the speech thread.
- `read_window` records the press time in a slot the waker shares with the thread (an `Arc<Mutex<Option<Instant>>>`; `FocusWaker` becomes `Clone` instead of `Copy`) and posts `WM_APP` to the focus thread. A newer press overwrites an older one that has not been taken yet.
- The focus thread takes the request in `pump`. It answers only while `tts_enabled` is true; with speech off the request is dropped, as WinEvents are. A focus read that is already due runs first, so the answer and the last focus event describe the same window.
- The answer's `at` is the press time. A silence pressed after W, before the answer arrives, makes `accepts_event` drop it.

### What the event carries (cv-core)

```rust
CoreEvent::WindowRead {
    at: Instant,               // the press, not the end of the read
    app: Option<AppId>,        // None: our own window, or no program; says "No window"
    window: WindowInfo,        // title, inner dialog name, cached dialog text
    nodes: Vec<DialogNode>,    // tree order, depth first, at most WINDOW_NODES
    outcome: ReadOutcome,      // Complete, Cut (a budget ran out), Unreadable (hung or no UIA)
}
```

- `DialogNode` gains `states: States` (checked, expanded, selected, enabled; the password field as today). The dialog walk leaves them empty, and `dialog_text` ignores them.
- `describe` gives the program, the node count and the outcome, never a name.
- `Tracker::handle` ignores it, with a test that it changes neither the mode nor the target. `FocusMemory` ignores it (it is not a focus change).

### The walk (cv-platform-win focus thread)

- **Root.** The inner dialog if ADR 0018's lookup finds one, else the foreground window. Neither our own process nor a window marked hung is walked; those answer `app: None` or `Unreadable`.
- **Request.** The dialog node request plus the Toggle, ExpandCollapse and SelectionItem state properties and IsEnabled, which the focus read already caches. No value property, no pattern, no Text pattern, anywhere.
- **Entered** (`walk.rs`, a second enter rule beside `is_container`, unit-tested): `Pane`, `Group`, `Custom`, `Unknown`, `Window`, `ToolBar`, `MenuBar`, `StatusBar`, `Tab` (its tab items) and `AppBar`. A focusable container is entered too: unlike the dialog walk, the window read speaks controls.
- **Never entered:** `List`, `Tree`, `Table`, `Document`, `Edit`, `ComboBox`, `Menu`, and a nested dialog (ADR 0018's rule). Their own node is kept and spoken.
- **Skipped:** offscreen nodes and their children, and a `TitleBar` with its buttons (the title is already said).
- **Budget:** `WINDOW_CALLS` (32) find calls, `WINDOW_DEPTH` (8), `WINDOW_NODES` (160) nodes, `WINDOW_MS` (1000 ms) for the whole read. Past any of them the walk stops and the outcome is `Cut`. These are cv-core constants and starting values; 9.8b2 sets them from what its Verify measures and says so in its PR.
- **MSAA** for a `#32770` whose UIA walk finds no `Text` node or only panes at its first level, as ADR 0016 and 0018, with the same budget.
- **Password:** a node that UIA says is a password (`IsPassword` true or unreadable) or MSAA says is `protected` (or whose state is unreadable) has its name emptied before it leaves the platform code, and `states.password` set, as the dialog walk does today.
- **Hung:** a UIA timeout marks the window hung (ADR 0015) and the outcome is `Unreadable`; the nodes found so far are dropped.
- **Stale:** if a foreground WinEvent is pumped right after the walk, the window changed while it ran: no answer is published (the new window's focus event speaks instead). A focus event inside the same window keeps the answer.
- **Cost of waiting:** while the walk runs (at most `WINDOW_MS`), focus events wait, as they do behind a dialog read today.
- `CV_FOCUS_LOG=1` adds one line per read: root, source, calls, nodes, ms, outcome. Never a name.

### What is spoken (cv-core `wording`, pure, unit-tested)

`window_read_utterances(app, window, nodes, outcome, verbosity) -> Vec<Utterance>`, called from `utterances_for`:

1. The title, then the inner dialog's name if the title does not contain it (ADR 0018's rule), at every verbosity level.
2. Then each node in tree order:
   - a `Text` node that is not focusable gives its name; ADR 0016's label rule (dropped when it equals the next focusable node's name at the same depth) and duplicate rule apply, so "File name:" is not said before "File name, edit";
   - any other node with a name gives its control phrase, `describe_control` at the current verbosity with no value (a list item says its checked or selected state, a checkbox "checked");
   - unnamed nodes, unnamed containers and every `Pane` say nothing;
   - a password node says its role only;
   - a `Document`, an `Edit` and a Terminal `Text` element say their name and role, cut by the existing `NAME_MAX` rule (a name that long is the text itself and is not read). Their contents are Phases 10 and 12.
3. `Cut` adds "More not read". `Unreadable` gives the title (Windows adds "(Not Responding)") and then the cached dialog text and "Window can't be read". `app: None` gives "No window".
4. Pieces are joined with ". " into utterances of at most `DIALOG_SPOKEN_MAX` (300) characters, at most `WINDOW_UTTERANCES` (24, below `QUEUE_MAX` so the queue never drops the start), cut at a word.
5. Every utterance is `Kind::Command`: the batch cuts off what was speaking, and the rest queue behind it (ADR 0013, no change to its table).

### Stopping (S1)

- Ctrl: silence clears the queue; an answer still on its way is dropped by its `at`.
- Tab or a click to another control: the focus event is `Kind::Focus`, which cuts off the read.
- W again: a new read from the start; the old one is cut off when the new answer arrives.
- Speech turned off: the speech thread silences, and the focus thread answers no further request.

### Dialogs: what is new compared with the cached text

The cached dialog text is what a focus event already spoke: the message and nothing else. The window read gives the same message (the same label and duplicate rules) followed by every control in order, so in a message box it says the text and then its buttons. The cached text is used only when the walk fails (`Unreadable`), so W still says something in a dialog that hangs.

### Speech off

- `--no-default-features`: no focus thread, no W chord, nothing changes.
- `tts` on, speech off: the W chord is registered (as T and F are), logs `[keys] read window`, and the focus thread drops the request: no walk, no event. The caret thread and the magnifier are untouched.

### Roadmap

9.8b is split (pending ADR 0019):

- **9.8b1** cv-core: `CoreEvent::WindowRead` and `ReadOutcome`, `DialogNode::states`, the `Tracker` arm, `describe`, `WINDOW_*` constants, `wording::window_read_utterances` and its arm in `utterances_for`. Unit tests: tree order, the label and duplicate rules, panes and unnamed nodes silent, a password node gives its role only and never its name, a document and a Terminal element give name and role only, `Cut`, `Unreadable` with cached dialog text, `app: None`, the utterance cap is below `QUEUE_MAX`, `Kind::Command`. No Windows code.
- **9.8b2** cv-platform-win and app: `Action::ReadWindow` and its binding, `FocusWaker::read_window` with the press-time slot, the `WM_APP` request in `pump`, the window enter rule in walk.rs (tests with the fake tree), the state properties in the node request, the budget, MSAA for `#32770`, the stale and hung rules, the `CV_FOCUS_LOG` line. Verify: W in Notepad (menu bar, text area by name only, status bar), Explorer (toolbar, address, navigation tree and file list by name only), a Brave page (the page title and "document", never the page), Windows Terminal (the tab name, never the buffer), Settings, Save As (ms from the log, whether it is `Cut`), a message box (text, then buttons), a file's Properties page, a Brave login form's password field (role only, nothing from the field in the log), Ctrl during a read stops it, Tab during a read stops it and speaks the new control, W twice starts over, a run with speech off and one with `--no-default-features`.

## Consequences

- Easier: one more request into a thread that already owns the reads, and the answer travels the same path as every other event. What is said is one tested function in cv-core, which a Linux backend can feed from AT-SPI. The walk, its MSAA fallback and its password rule are reused, not copied.
- Harder: a second message in the focus thread, a second enter list and budget, and a `CoreEvent` that is a reply to a command. Focus announcements wait behind a window read for up to `WINDOW_MS`.
- Risks: the budget is a guess until 9.8b2 measures Explorer, Settings and Save As; a window whose controls sit deeper than `WINDOW_DEPTH` or past `WINDOW_NODES` says "More not read" early. Not entering lists means a file list is "Items View, list" and nothing more, which may be less than the tester wants. Speaking every named control may be noisy in apps with many toolbars. Text that changes while it is spoken is not followed (one snapshot).
- Not covered: reading a list's items, a document's text or a Terminal buffer (Phases 10 and 12); stepping through the window node by node (that is a review cursor, not planned); browser web dialogs (`role="dialog"`); the UAC prompt (ADR 0016). T inside an inner dialog still says only the foreground window's title (9.8a's open point); W says the dialog's name.
- Revisit: after 9.8b2's Verify (the constants, the enter list, whether lists should give their first items or their count); at 12.1, if continuous reading builds the incremental reader of V4, which this command could then move to; when tester feedback after 9.11 says whether a second press should stop instead (S2).
