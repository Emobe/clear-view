# Caret sources per app

Roadmap 4.2, run by you on 2026-10-09 with the 4.1 caret probe (`cargo run -p cv-platform-win --example caret_probe`, PR #37). Input to the 4.3 ADR (caret sources).

The probe reads three sources for the foreground window 10 times a second and outlines each one on screen:

- **gui** (red): `GetGUIThreadInfo` `rcCaret`, mapped with `ClientToScreen`.
- **msaa** (green): `AccessibleObjectFromWindow(focus window, OBJID_CARET)`, then `accLocation`.
- **uia** (blue): the focused element's `TextPattern2.GetCaretRange`, else the first `TextPattern.GetSelection` range. An empty range is expanded to one character.

An outline on the caret means that source is right. No outline means the source gave nothing.

## Results

| App | gui | msaa | uia | Notes |
|---|---|---|---|---|
| Notepad | yes | yes | yes | Also seen while testing 4.1. |
| File Explorer, rename (F2) | yes | yes | yes | |
| File Explorer, address bar and search box | no | no | no | XAML text boxes. See below. |
| Brave | no | yes | yes | Chromium. |
| Edge | no | yes | yes | Same as Brave. |
| Windows Terminal | no | no | yes | Draws its own caret. Seen while testing 4.1. |
| LibreOffice | no | no | no | Not on the v1 app list; your stand-in for Word. |

Which UIA method Brave, Edge and Terminal used (`GetCaretRange` or `GetSelection`), the costs in those apps and line-end behaviour were not recorded.

Not measured:

- **Word**: not installed on your machine. It is a v1 Must app (PRODUCT.md), so it is still unmeasured.
- **Chrome**: not installed. Brave and Edge are Chromium and gave the same result, so Chrome is expected to match. That is unconfirmed.
- **The tester's email app**: on hold with the other tester items.
- **Elevated windows** (admin Notepad, Task Manager): not tried.

## File Explorer address bar and search box

From the log you pasted (fg `explorer.exe`, focused element `fw "XAML" class "TextBox"`):

- gui: `none (no caret window)`. These are XAML controls with no Win32 caret.
- msaa: `none (empty location 0,0)` for the focus window.
- uia: `TextPattern2.GetCaretRange` returns an **active** caret range, but `GetBoundingRectangles` gives no rectangle. Expanding the range to one character gives none either: `none (no rectangle: off-screen or hidden)`.

You typed in both boxes and moved the caret around, and no outline appeared at any point, so this is not only the caret-at-end-of-text case. With the probe's methods, no source gives a caret position in these boxes. 4.3 needs a fallback for them, for example the focused element's bounding rectangle.

Costs in that log: gui 0.0 ms; msaa 0.1–0.7 ms (74.9 ms on the very first call); uia 2.6–12 ms (36 ms on the first call), 6–9 ms on the XAML text box.

## For 4.3

- No single source covers the list. gui is missing in Chromium and Terminal, msaa is missing in Terminal, and uia covers everything measured except the Explorer XAML boxes and LibreOffice.
- Every app that had a caret position had it from uia.
- The Explorer XAML boxes need a non-caret fallback.
- Word is unmeasured. Measure it before 4.8 at the latest.
