# 0008: Caret sources

Status: Accepted
Date: 2026-10-09

## Context

Roadmap 4.3. PRODUCT.md v1 Must 5: follow the mouse by default, follow the text caret while typing, go back to the mouse when it moves, in Chrome, Edge, File Explorer, Word, Notepad and the tester's email app. PRODUCT puts caret position in the core layer, with per-app differences handled there once for the magnifier and the later reader. This ADR decides which caret source is used for which app, the fallback order, which thread reads them, how the core reports a caret position, and what happens in elevated windows without UIAccess. Speech stays out of scope; cv-tts is not touched.

### Earlier decisions this touches

- **ADR 0003** (Accepted): "Only that thread [the TTS thread] adds or removes UIA handlers", with MTA COM first, plain data over channels, never COM pointers across threads. ROADMAP says the 4.3 ADR supersedes the "only the TTS thread owns UIA" part and that the rest carries over to the core thread.
- **ADR 0004** (Accepted), core event rules for 4.3 and 4.4: one `#[non_exhaustive]` enum in cv-core; plain data in physical virtual-screen pixels plus a timestamp, no OS handles or COM pointers; each event names its source; fields identifying the foreground application are added when 4.3 decides what it needs; fan-out delivery, one receiver per consumer, a consumer never blocks a source; continuous values (the pointer) are sampled, not sent as events. "Which thread owns UIA, the fallback order and elevated windows are 4.3's decision."

### What 4.2 measured (docs/prototypes/caret-sources.md)

The 4.1 probe (crates/cv-platform-win/examples/caret_probe.rs) reads three sources for the foreground window 10 times a second: **gui** (`GetGUIThreadInfo` `rcCaret` through `ClientToScreen`), **msaa** (`AccessibleObjectFromWindow(focus window, OBJID_CARET)`, `accLocation`) and **uia** (the focused element's `TextPattern2.GetCaretRange`, else `TextPattern.GetSelection()[0]`, expanded to one character when the range has no rectangle).

| App | gui | msaa | uia |
|---|---|---|---|
| Notepad | yes | yes | yes |
| File Explorer rename (F2) | yes | yes | yes |
| File Explorer address bar and search box (XAML `TextBox`) | no | no | no |
| Brave, Edge (Chromium) | no | yes | yes |
| Windows Terminal | no | no | yes |
| LibreOffice (not on the v1 list) | no | no | no |

- No source gave a wrong position: each one either matched the caret or gave nothing.
- uia gave a position in every app that had one. In the Explorer XAML boxes `GetCaretRange` returns an active range but `GetBoundingRectangles` is empty, also after expanding to a character and with the caret mid-text.
- Costs in Explorer: gui 0.0 ms, msaa 0.1–0.7 ms (75 ms on the first call), uia 2.6–12 ms (36 ms on the first call), 6–9 ms on the XAML box. The uia figure covers the probe's whole read: `GetFocusedElement`, the pattern query, the caret range, `GetBoundingRectangles` and two property reads for the log.
- Not measured: Word (not installed; a v1 Must app), Chrome (Brave and Edge are Chromium and are expected to match, unconfirmed), the tester's email app (on hold), elevated windows, display scaling above 100% for any source, which UIA method answered in Brave, Edge and Terminal, and costs outside Explorer.

### What the code does today

- The core has no event types yet. cv-core has `ScreenPoint`, `ScreenRect` (`contains`) and `PointerSource`; the magnifier samples the pointer once per tick through `OverlayHost: PointerSource` (crates/cv-magnifier/src/magnifier.rs).
- Threads: main (egui), capture (cv-magnifier `spawn_capture`), render (cv-platform-win `spawn_overlay`, which calls `Magnifier::tick` on a 16 ms `WM_TIMER`), hotkey (cv-platform-win) and, only with `--features tts`, the TTS thread.
- The TTS thread (crates/cv-tts/src/lib.rs, feature off by default) initialises MTA COM, creates `CUIAutomation8` and calls `AddFocusChangedEventHandler`.
- The probe follows ADR 0003's rules: MTA first, and its UIA thread owns no window (the outlines run on their own thread).
- The `windows` features for UIA and MSAA (`Win32_UI_Accessibility`, `Win32_System_Com`, `Win32_System_Ole`, `Win32_System_Variant`) are dev-dependencies of cv-platform-win, used only by the probe.

### External facts

- **UIA client threading.** UIA calls belong on a thread that owns no windows and is MTA. Handlers are added and removed on one non-UI MTA thread, and "A UI Automation client should not use multiple threads to add or remove event handlers. Unexpected behavior can result if one event handler is being added or removed while another is being added or removed in the same client process" ([Understanding Threading Issues](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-threading)).
- **UIA timeouts.** `IUIAutomation2::ConnectionTimeout` defaults to 2 seconds ([docs](https://learn.microsoft.com/en-us/windows/desktop/api/UIAutomationClient/nn-uiautomationclient-iuiautomation2)); `TransactionTimeout` defaults to 20 seconds ([docs](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation2-get_transactiontimeout)). A hung target app can block a UIA call that long. Both are settable on `CUIAutomation8`.
- **UIA caret events.** Some text controls raise `TextSelectionChanged` when the caret moves as a zero-width selection, but clients are advised not to depend on it for insertion-point changes ([TextPatternIdentifiers.TextSelectionChangedEvent](https://learn.microsoft.com/en-us/dotnet/api/system.windows.automation.textpatternidentifiers.textselectionchangedevent)). `GetCaretRange` returns a degenerate range at the caret; `isActive` says whether the control has keyboard focus ([GetCaretRange](https://learn.microsoft.com/en-us/windows/desktop/api/uiautomationclient/nf-uiautomationclient-iuiautomationtextpattern2-getcaretrange)).
- **MSAA caret object and WinEvents.** The caret object has no window handle of its own; "clients must set a WinEventProc and wait for the caret object to generate events". The rich edit control in Riched20.dll sends no `EVENT_OBJECT_LOCATIONCHANGE` while Shift+arrows extend a selection ([Caret](https://learn.microsoft.com/en-us/windows/win32/winauto/caret)).
- **Chromium** keeps a fake caret object for screen magnifiers, including Windows Magnifier, and fires `EVENT_OBJECT_LOCATIONCHANGE` with `OBJID_CARET` when the selection bounds change, whether or not full accessibility is on ([Chromium review 2781613003](https://codereview.chromium.org/2781613003)). This matches msaa working in Brave and Edge.
- **SetWinEventHook.** With `WINEVENT_OUTOFCONTEXT` nothing is injected into other processes; events are queued and arrive in order on the thread that called `SetWinEventHook`, which "must have a message loop in order to receive events". `WINEVENT_SKIPOWNPROCESS` drops our own process's events ([SetWinEventHook](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook)). A message loop does not need a window.
- **GetGUIThreadInfo** "succeeds even if the active window is not owned by the calling process". `rcCaret` is in client coordinates of `hwndCaret` and "are logical coordinates in terms of the window associated with the caret. They are not virtualized into the mode of the calling thread." For an edit control `rcCaret` includes text-direction padding, so it "may not give the correct position of the cursor" ([GetGUIThreadInfo](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getguithreadinfo), [GUITHREADINFO](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-guithreadinfo)). `LogicalToPhysicalPointForPerMonitorDPI` converts a point from a window's logical coordinates to physical ones, whatever the caller's awareness ([docs](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-logicaltophysicalpointforpermonitordpi)). The probe does no such conversion, and 4.2 was not run above 100% scaling.
- **Elevated windows.** "An application that doesn't have UIAccess in the manifest starts with medium IL and cannot access elevated ('medium+' IL) process UI." With UIAccess, a process launched by an admin user runs at high IL and can; one launched by a non-admin runs at medium+ and cannot reach high-IL apps. UIAccess needs Authenticode signing, a secure install location such as Program Files, and `uiAccess="true"` in the manifest ([Security Considerations for Assistive Technologies](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-securityoverview)). PRODUCT.md accepts no elevated tracking for tester builds; UIAccess is roadmap 6.3 to 6.4. Whether `GetGUIThreadInfo` and out-of-context WinEvents work for an elevated foreground window from a medium-IL process is not documented in what I found; one third-party article says WinEvent callbacks stop or become incomplete then. Unmeasured here.

## Options

### Who reads the caret (supersedes part of ADR 0003)

1. **One core thread in cv-platform-win, the "caret thread".** MTA COM first, owns no window, runs a message loop with no window (`MsgWaitForMultipleObjectsEx` with a timeout), holds every core UIA and MSAA object, and sends plain data out. Cost: one more thread. The `windows` UIA and MSAA features move from dev-dependencies to normal dependencies of cv-platform-win, so the app compiles them. UIA calls block only this thread, never the render tick.
2. **Read on the render thread inside the tick.** No new thread. Cost: the render thread owns the overlay window, which UIA's rules forbid. A uia read takes 3–12 ms of a 16 ms tick, and a hung app can block it for up to the 20 s transaction timeout, which freezes the view. Rejected.
3. **Reuse the parked TTS thread.** Cost: the core would depend on a crate that is off by default and out of scope until after v1. Rejected.

### When the caret thread reads

A. **Poll the whole chain at a fixed rate** (for example 20 Hz), as the probe does at 10 Hz. Cost: the simplest loop, and the only approach measured. UIA reads run 20 times a second whenever a text element has focus, about 5–24% of one core by 4.2's Explorer numbers, and up to 50 ms of latency in every app, including those that announce caret moves.

B. **Wake on WinEvents; poll only where no event covers the caret.** Out-of-context hooks, skipping our own process, on foreground change (`EVENT_SYSTEM_FOREGROUND`), focus change (`EVENT_OBJECT_FOCUS`) and caret show, hide and move (`EVENT_OBJECT_SHOW`, `EVENT_OBJECT_HIDE`, `EVENT_OBJECT_LOCATIONCHANGE` for `OBJID_CARET`). Each event triggers one read of the chain. While the last answer came from uia or the focus-rectangle fallback (Terminal, the Explorer XAML boxes, possibly Word), the thread also polls at 20 Hz until focus or foreground changes. Cost: the hooks and a small state machine. Win32 and Chromium carets cost almost nothing at idle and arrive with no polling delay. UIA-only apps still pay the poll. The Riched20 selection gap is not covered by events; none of the measured v1 apps uses Riched20 for its main text, and the tester's email app is unknown. No UIA event handlers, so nothing conflicts with cv-tts's handler when the `tts` feature is on.

C. **UIA event handlers plus WinEvents, no polling.** Focus-changed and `TextSelectionChanged` handlers, re-registered on the focused element at each focus change. Cost: the cheapest at idle, but Microsoft advises against relying on `TextSelectionChanged` for the caret; whether Terminal and the Explorer XAML boxes raise it is unmeasured. Handlers run on UIA's own threads and are added and removed on every focus change. With `--features tts`, two threads in one process add UIA handlers, which Microsoft warns against. The most code and the least evidence.

### Fallback order

i. **One fixed chain for every app: msaa → uia → gui → focused element's rectangle.** The first source that gives a non-empty rectangle wins. msaa first because it is under 1 ms, gives screen coordinates through `accLocation` (physical or logical at scaling above 100% is unmeasured), and is the source that the WinEvents announce, in Win32 and Chromium. uia next, because it covers every app that 4.2 found with a caret position. gui third: in 4.2 it never answered where msaa did not, but it needs neither COM nor UIA, so it is the candidate for elevated windows. The focus rectangle comes last, for controls with a caret but no caret rectangle (the Explorer XAML boxes): the focused element's UIA `BoundingRectangle`, else the `hwndFocus` window rectangle from `GetGUIThreadInfo`. Cost: in UIA-only apps every read first spends under 1 ms on a failed msaa call. A source that gives a present but wrong position early in the chain would win; 4.2 saw none, and 4.8 checks.

ii. **A per-app table keyed on window class or UIA framework.** Chromium (`Chrome_WidgetWin_*`) uses msaa, XAML and Terminal use uia, classic Win32 uses gui. Cost: a list to keep up to date by hand, and a wrong or missing entry means no tracking in that app. It encodes the same facts as option i, which finds them at run time.

iii. **uia first everywhere.** Cost: 3–12 ms on every read even where msaa answers in under 1 ms, and no event to wake on in apps where UIA is the only source. No accuracy gain in any measured app.

### Elevated windows without UIAccess

a. **Degrade to the mouse.** The chain runs as usual. uia and msaa fail against the elevated window, gui and the `hwndFocus` rectangle may still answer, and if nothing answers the core reports that there is no caret, so tracking stays on the mouse. One log line per foreground change: `[caret] no source in <exe>`. Cost: no caret tracking in elevated windows for tester builds, which PRODUCT.md accepts. Whether gui works there is learned in 4.8.

b. **Bring UIAccess forward** (6.3 and 6.4 before 4.8). Cost: a code-signing certificate, an install into Program Files and an installer before any tester build has caret tracking, which is the M5 work PRODUCT defers. Tester builds would also stop running from a copied folder, the way 1.7 hands them over.

## Decision

Recommendation: **option 1 (a core caret thread), B (WinEvent wake with a poll only where needed), i (one fixed chain), a (degrade to the mouse in elevated windows).** It supersedes ADR 0003's rule that only the TTS thread adds or removes UIA handlers; ADR 0003's other rules (MTA first, plain data over channels, no COM pointers across threads) apply to the caret thread. The part of ADR 0003 about the reader is untouched: when the reader comes back after v1, its ADR decides whether it reads UIA through the core or keeps its own thread. ADR 0004 stands; this fills in the core event fields it left open.

### The caret thread (cv-platform-win)

- Spawned by the backend, like the hotkey thread: `platform::spawn_caret_source(events)` returns a handle whose `shutdown` ends the loop (`PostThreadMessageW(WM_QUIT)`) and joins. On exit it unhooks its WinEvent hooks. It changes nothing system-wide, so there is nothing to restore on a kill (PRODUCT Principle 2).
- MTA COM first, then `CUIAutomation8` with `ConnectionTimeout` and `TransactionTimeout` lowered from 2 s and 20 s to a few hundred milliseconds (the step picks the number and logs it), so one hung app cannot stall caret tracking for long. No window, and no UIA event handlers.
- Reads run only on this thread. No COM object leaves it.
- A read gets the foreground window, its thread's `GUITHREADINFO`, then walks the chain. gui coordinates are converted to physical pixels with the caret window's DPI (`LogicalToPhysicalPointForPerMonitorDPI`) before they leave the thread. msaa and uia coordinates are checked at 150% scaling in 4.4's Verify list; if either is logical for some apps, the step converts them the same way and says so in the PR.
- Polling (option B) is 20 Hz while the last answer came from uia or the focus rectangle. The rate is a constant the step may tune with numbers; 5.5 measures it.

### What the core reports (cv-core)

Sketch for 4.4; names may change in the step, the content may not.

```rust
#[non_exhaustive]
pub enum CoreEvent {
    /// The caret moved, appeared, or focus moved to a new caret. Sent only when the rectangle
    /// or source changes.
    CaretMoved { at: Instant, rect: ScreenRect, source: CaretSource, app: AppId },
    /// The focused element has no caret any more, or no source gives one.
    CaretLost { at: Instant, app: AppId },
}

#[non_exhaustive]
pub enum CaretSource {
    Msaa,
    Uia,
    Gui,
    /// Not a caret: the focused element's rectangle (no source gave a caret position).
    FocusRect,
}

/// The foreground application: process id and executable file name (for example
/// "explorer.exe"). Read once per foreground change.
pub struct AppId { pub pid: u32, pub exe: String }
```

- `rect` is physical virtual-screen pixels, like `ScreenPoint` and the monitor rects (1.6). A caret rectangle is usually a few pixels wide; `FocusRect` can be a whole text box, and the tracking policy (4.5) decides what part of it to show.
- `CaretSource` and `AppId` are there for per-app policy, the 4.8 logs and the later reader. The policy in 4.5 needs only `CaretMoved`, `CaretLost` and the source kind.
- Delivery: a small fan-out hub in cv-core over `std::sync::mpsc` (no new dependency). `subscribe()` returns a `Receiver<CoreEvent>`; `publish` sends to every live receiver and drops closed ones. Unbounded channels, so the caret thread never blocks on a consumer. The render thread drains its receiver with `try_recv` at the start of each tick and keeps the newest caret state; wiring that in is 4.6.
- The pointer stays sampled per tick through `PointerSource` (ADR 0004).

### Per-app behaviour, as expected from 4.2

| App type | Answering source | What wakes the read |
|---|---|---|
| Classic Win32 edit (Notepad, Explorer rename) | msaa | caret WinEvents |
| Chromium (Chrome, Edge, Brave) | msaa | caret WinEvents (the fake caret) |
| Windows Terminal | uia | 20 Hz poll |
| XAML text boxes (Explorer address bar and search) | focus rectangle | 20 Hz poll |
| Word | unmeasured (4.2a) | |
| Elevated windows | gui if it works, else none (`CaretLost`) | caret WinEvents if delivered, else none |

### Word and the other gaps

Word is a v1 Must app and has not been measured. A new item, 4.2a (you), runs the probe in desktop Word before 4.8: body text, a table cell, a header, and a long document scrolled down. It records which source answered, the UIA method the probe prints, and the costs. If Word needs a source outside the chain, that is a new ADR before 4.8. 4.2a also covers Chrome where it is installed. The tester's email app stays on hold with the other tester items.

### Roadmap

- New: 4.2a (you) Word measurement, as above.
- 4.4 (pending ADR 0008): the event types and hub in cv-core with unit tests; the caret thread with the chain, the WinEvent wake and the poll; `[caret]` log lines for each source change and for no source; no magnifier change. Verify: run each app from 4.2 with the log on, and Notepad and Explorer rename at 150% scaling with the outline of the reported rectangle (the probe's outline code can be reused for a dev-only flag or example).
- 4.5 to 4.8 keep their text and use `CaretMoved` and `CaretLost`. 4.8 includes an admin Notepad to learn what option a gives in elevated windows.

## Consequences

- Easier: the magnifier never calls UIA or MSAA and never blocks on another app. The tracking policy (4.5) is pure logic over plain events and can be unit-tested with made-up events. A source added later (IA2 for LibreOffice, or OCR) is one more link in the chain and one more `CaretSource` variant. The reader can subscribe to the same hub later.
- Harder: a new long-lived thread with COM, hooks and a message loop, and so a new shutdown path. The app now compiles the UIA and MSAA `windows` features. UIA-only apps cost a 20 Hz poll while they have focus; 5.5 measures it.
- Risks: a source that gives a wrong position early in the chain (4.2 saw none); gui coordinates in DPI-unaware apps (converted, checked at 150%); unknown Word behaviour (4.2a); elevated windows track only if gui and WinEvents work across integrity levels (4.8 finds out; UIAccess in 6.3 to 6.5 fixes it for release).
- Revisit: after 4.2a if Word needs another source; after 4.8 if any app tracks wrongly; at 5.5 if the poll costs too much (then option C's UIA events are the next thing to measure); when the reader comes back (who owns UIA for it); at 6.3 (UIAccess makes elevated windows reachable by msaa and uia).
