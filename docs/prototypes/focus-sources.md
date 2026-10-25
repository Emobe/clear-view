# Focus sources per app

Roadmap 9.2, run by you on 2026-10-25 with the 9.1 focus probe (`cargo run -p cv-platform-win --example focus_probe`, PR #98). Input to the 9.3 ADR (focus events in the core). Read from your three runs: `probe.txt` (about 330 events), `probe-taskbar.txt` (68) and `probe-desktop.txt` (260). The files hold terminal text and page addresses and are not committed.

The probe logs one block per foreground or focus WinEvent, with two sources:

- **uia**: `IUIAutomation::GetFocusedElement`, read when the loop gets to the event. It describes whatever has focus now, not the event's object.
- **msaa**: `AccessibleObjectFromEvent` for the event's own window, object and child.

Each source is either right (the control a user would hear: a useful name, a role that fits), partly right, or empty or wrong. "Right" below is my reading of the log lines, not a listening test.

## Results

| Place | uia | msaa | Notes |
|---|---|---|---|
| Notepad, text area (admin, per you) | partly | partly | Run as administrator every time, you say. Class `RichEditD2DPT`. uia type is `Pane`, its name is the start of the document text, and it has no Value or Text pattern. msaa role is `client` with the same text as name. Neither says "edit". No access error from either source in either Notepad window, so an elevated Notepad was read as well as any other (see below). |
| Notepad, Save As dialog | partly | right | Classic Win32 controls. uia gives type `Pane` for every control (button, combo, edit, toolbar) and keeps the `&` of the accelerator in the name (`&Save`). msaa gives the right roles (push, combo, editable, tool) and clean names (`Save`, `Cancel`). The file name box: uia names it by its text, msaa names it empty and calls it a combo. The dialog itself: uia `Window` and msaa `dialogue`, both named "Save as". |
| File Explorer, file list | right | right | uia `ListItem` (class `UIItem`, DirectUI) with the file name as name and as value. msaa `list`, name and value the same. Toolbar view buttons: uia `RadioButton`, msaa `radio`. Tabs: uia `TabItem`. |
| File Explorer, address bar and search box | not in these runs | not in these runs | Not visited. 4.2 measured the caret here, not the focus. |
| Desktop icons | right | right | uia `ListItem` named by the icon (class `ListViewItem-N`, framework Win32). msaa `list` with the same name. The desktop window itself is "Program Manager" in both. |
| Taskbar buttons | right | right | Each button is its own focus event. uia `Button`, e.g. "File Explorer - 1 running window pinned", "Terminal - 2 running windows and 1 group pinned", "Start". msaa `push` with the same name, but its value read fails with `0x80020003` (member not found). Finding 25(g) said only the container came through; in this run the buttons did, so (g) is answered (how you moved focus onto them was not recorded). |
| System tray | right | right | uia `Button` with a status in the name (volume with its percentage, a VPN's "Connected"). msaa `push`, same name. The overflow window is its own foreground window ("System tray overflow window."). |
| Start menu | right | right | uia `ListItem` (tiles and folders, with the item count in the name), `Button` (view selector, user account). msaa `list` and `push` with the same names. The Start window is named "Start" in both. |
| Search (Start search) | right | partly | uia `Edit`, "Search box", Value pattern, value the typed text. msaa `editable`, "Search box", value the typed text; the window around it is named "Search". |
| Settings | right | right | Windows Settings (ApplicationFrameHost, XAML). uia names buttons, list items, a hyperlink, radio buttons, a toggle switch (type `Button`, class `ToggleSwitch`) and a slider, all with automation ids. msaa agrees on name and has the right role (push, list, link, radio, slider) and the slider's value (`100`). uia exposes the slider through the RangeValue pattern, which the probe does not read, so its value line says "none"; msaa's value is the only slider value in the log. |
| Windows Terminal | right | partly | uia `Text`, class `TermControl`, name "Command Prompt" (the tab), and the whole buffer through the Text pattern (value cut at 80 characters by the probe). msaa `text`, name "Command Prompt", value read fails (`0x80020003`). Cleanly reported `not-password` although the buffer can hold anything (Finding 25(h)). |
| Brave, page | right | partly | uia `Document` ("RootWebArea") named by the page title, value is the page address (Value pattern); `Group` for a page widget (e.g. a video player) with its name. msaa gives `client` with the window title or `grouping` with the widget name, no value. |
| Brave, window | partly | partly | The first focus event after switching in is a `Pane` (`BraveContentsContainerView`, unnamed) from uia and the browser window's title from msaa; the page `Document` arrives a moment later. |
| clear-view settings window | partly | partly | One control: uia `Window`, "clear-view settings"; msaa `window` or `client`. egui draws everything inside it (Finding 26). Our overlay window raises its own foreground event (class `clear_view_overlay`, Finding 25(f)). |
| Task Manager | fails | fails | uia: `0x80070005 Access is denied` from `GetFocusedElement` on 19 events. msaa: `0x80004005 Unspecified error` from `AccessibleObjectFromEvent`. Only the foreground program, title and class (from the window handle, no accessibility call) are known. It sometimes answers with the desktop (`Desktop 1`, `#32769`) or the `DirectUIHWND` pane, but never a control. Task Manager and your Notepad are both elevated at the same level, so elevation does not explain it; why Task Manager is refused was not checked. clear-view was not elevated. |
| A game (Euro Truck Simulator 2) | partly | partly | uia `Window` named by the game's title; msaa `pane` with no name on the focus event and `window` with the title on the foreground event. No controls. uia reads here took a median of about 120 ms. |
| Steam and its web helper | partly | partly | A hung Steam shows up as a ghost window, `Steam (Not Responding)`, class `Ghost`, in both. Its web content gives `Document` and `Group` elements. One uia read took 9.1 s while Steam was not responding (cause not checked). |
| Alt+Tab ("Task Switching") | partly | partly | uia says only `Pane` (class `Windows.UI.Input.InputSite.WindowClass`) with no name. msaa names the switcher and, on an item, the window it stands for. A switch raises many events in a row (Finding 25(c)). |
| Windows with no program name | n/a | n/a | Some foreground events have no readable program (`fg` shows an empty name, 11 in `probe.txt`, 3 in `probe-desktop.txt`): the shell's `ForegroundStaging` window and the desktop (`Desktop 1`, `#32769`), plus the taskbar window. They carry no information; they come just before or after the real target. |

## What stands out

1. **Neither source is enough alone.** UIA is the better one in XAML (Start, Search, Settings, the taskbar) and Terminal and Brave (Text pattern, Value pattern, names). MSAA is the better one in classic Win32 dialogs (Save As) and on Alt+Tab. They agree on names for Explorer's list, the desktop icons and Settings.
2. **Different objects.** UIA reads the current focus; MSAA reads the event's object. In a burst of events they point at different controls (Finding 25(b)). The logs show it often, in every run (e.g. a Start button under uia beside the desktop's "Recycle Bin" list under msaa in the same event); I did not count it. 9.3 must say which one an event means.
3. **Errors.** MSAA's `AccessibleObjectFromEvent` failed on 61 events (21 in `probe.txt`, 40 in `probe-desktop.txt`, none in `probe-taskbar.txt`): 58 with `0x80004005` Unspecified error (Task Manager, and transient objects) and 3 with `0x80070057`. UIA failed only on the 19 events in Task Manager. I did not check each failing event's cause beyond Task Manager. Neither source fills in for the other's failure.
4. **Cost.** UIA median 2 to 28 ms per read in every app but the game (about 120 ms), with outliers (479 ms and 309 ms for the first read into a clear-view window, 520 ms once in Explorer, 9.1 s for the hung Steam); MSAA median about 1 to 5 ms, max 336 ms. Both ran serially on one thread, as in 9.1.
5. **Elevation does not decide it.** With clear-view not elevated, your admin Notepad (two windows) gave full answers from both sources, while Task Manager gave nothing from either. This matches STATUS Finding 20 (admin Notepad was caret-tracked from a non-elevated clear-view) and is better than ADR 0008 assumed. Checked on 2026-10-25 by reading process tokens (PowerShell, `GetTokenInformation`): the Notepad you have open now (not the one from the probe runs, which had closed) has an elevated token at high integrity (0x3000), the same as Task Manager (0x3000); Windows Terminal, Explorer and this shell are medium (0x2000), so the probe ran non-elevated. So the admin Notepad result is real: an elevated process, read from a non-elevated one, answered fully from UIA and MSAA. For Task Manager a reader needs a way to say "can't read this window" (the title is still known).
6. **Taskbar names carry state.** "pinned", "N running windows" and "group" are in the name itself; 9.5's wording must decide whether to read all of it.
7. **No password field was in these runs**, so the withholding rule saw only "can't tell" (6 values withheld: msaa on the Save As file name box and buttons, and on tray icons). No value was shown for those.

## Not measured

- **Why Task Manager is refused when admin Notepad is not**: both are elevated at the same level. The reason is not checked. Not measured: the elevation of the exact Notepad process in the probe runs (it had closed; the probe does not print elevation). Not measured: an elevated Terminal, Explorer or a dialog's UAC prompt.
- **LibreOffice**: not in these runs (it had no caret source in 4.2).
- **Word**: not installed.
- **File Explorer address bar and search box**, a rename (F2): not visited.
- **Chrome**: not installed; Brave stands in.
- **A Start menu search result list, the Win+X menu, and flyouts other than the tray overflow**: not visited.
