# 0011: Docked pan

Status: Accepted
Date: 2026-10-11

## Context

Roadmap 7.1. In docked mode the panel covers its share of the real desktop (25% of the monitor at a 25% panel), and nothing shows the covered strip at 1x. The mouse can go under the panel (ADR 0007) and the panel shows that spot magnified, but the unmagnified view of the strip is gone. You found the gap after 6.2c3. It is the "fake pan underneath it" from STATUS Finding 11, which the overlay design did not deliver. What you want (ROADMAP Phase 7): the unmagnified area pans visually, smoothly and continuously as the pointer nears the edge beside the panel, up to the width of the covered strip. It does not jump to show everything at once. Real windows are not moved or resized (the AppBar approach stays rejected, ADR 0007). You expect a second overlay, excluded from capture like the panel, showing a shifted copy of the captured desktop.

PRODUCT.md v1 Must 2 says: "The desktop is not resized and windows do not shrink. The mouse can move under the panel, and the panel shows that area magnified. The panel is excluded from capture so it never magnifies itself." Nothing in this ADR conflicts with that. The pan adds to it, and ROADMAP 7.1 says this step records the change (see Decision, PRODUCT.md).

### What the code does today

- **One overlay window** (crates/cv-platform-win/src/overlay.rs `run_overlay`): `WS_POPUP`, `WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TRANSPARENT`, alpha 255, `WDA_EXCLUDEFROMCAPTURE`, `WM_NCHITTEST` answers `HTTRANSPARENT`. It starts hidden. `WinHost::apply_layout` places it with `SetWindowPos` (`SWP_NOZORDER | SWP_NOACTIVATE`) for fullscreen or for `geometry::docked_rect`, shows it with `SW_SHOW` and sets `MagShowSystemCursor` (hidden in fullscreen, shown otherwise). `teardown`, run from `WinHost`'s `Drop`, shows the cursor and destroys the window.
- **The seam** (crates/cv-magnifier/src/host.rs): `OverlayHost: PointerSource` has `apply_layout(&Layout) -> (u32, u32)`, `set_system_cursor(bool)` and `raw_handles()`. `Layout::Docked { monitor, edge, thickness }`.
- **System cursor while docked** (magnifier.rs `update_system_cursor`): hidden while the raw pointer is inside the applied panel rect, shown when it leaves, called only on change. `apply_layout` resets it.
- **One renderer** (crates/cv-magnifier/src/gfx.rs `WgpuState`): its own DX12 instance, adapter, device, one surface (`PresentMode::Fifo`), one pipeline, the frame texture (the active monitor's whole captured image, BGRA8), the R8 pointer mask texture (ADR 0010), one 96-byte uniform buffer and one bind group. The shader samples the frame through a normalised crop and draws the pointer at `cursor_x`/`cursor_y` in output pixels (shape, circle or nothing).
- **The captured frame includes what the panel covers.** Both the capture and the panel's own surface exclude the overlay, so the frame texture already holds the real desktop under the panel. A shifted copy needs no new capture.
- **Capture follows the target** (`View::follow_target`): the frame texture holds the monitor under the tracking target (pointer or caret). The docked panel stays on the primary monitor unless `panel_follows_monitor` is on (roadmap 5.2), so the panel's monitor and the captured monitor can differ.
- **Pointer drawing** (ADR 0010, 6.2c): `PointerSource::look` gives the real cursor shape as a coverage mask even while `MagShowSystemCursor(false)` hides the cursor (measured in 6.2b). The shader draws it in the user's fill and outline colours at `pointer_size` times Windows' own size.
- **Tick** (magnifier.rs `View::tick`): every 16 ms on the render thread, pointer read, tracker, monitor follow, layout, system cursor, draw. Uniform writes and frame uploads are skipped when unchanged.

### External facts

- `WDA_EXCLUDEFROMCAPTURE`: "The window is displayed only on a monitor. Everywhere else, the window does not appear at all" ([SetWindowDisplayAffinity](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity)). A second window needs the same call. Without it, the shifted window would be captured and shifted again on the next frame.
- Layered windows with `WS_EX_TRANSPARENT` pass mouse input to the windows underneath ([window features](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features)). The 3.1 prototype confirmed clicks, right-clicks, scroll and drag through the panel (ADR 0007).
- `ShowWindow` with `SW_SHOWNA` "displays the window in its current size and position … similar to SW_SHOW, except that the window is not activated". `SW_HIDE` hides it ([ShowWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow)).
- `MagShowSystemCursor` has no reference count: "the specified visibility state takes effect immediately, regardless of any previous calls" ([MagShowSystemCursor](https://learn.microsoft.com/en-us/windows/win32/api/magnification/nf-magnification-magshowsystemcursor)). It is system-wide, so it hides the cursor on every monitor.
- wgpu 30.0.1: `Surface::configure(&self, device: &Device, config)` takes the device as a parameter ([Surface](https://docs.rs/wgpu/30.0.1/wgpu/struct.Surface.html)), so a second surface can be configured with the existing device and draw from its textures. The page says nothing about several surfaces on one device. Sharing one device across windows is the usual pattern ([wgpu.rs Surface](https://wgpu.rs/doc/wgpu/struct.Surface.html)), but the step confirms it by running.
- The Start menu and other shell surfaces show above `HWND_TOPMOST` windows until UIAccess (ADR 0007, roadmap 6.3 to 6.5).
- From the 3.1 prototype (docs/prototypes/docked-overlay.md): an excluded overlay presenting about 40 times a second caused no extra captured frames on your machine. The 24H2 extra-frames issue is still unchecked on the tester's machine (ADR 0007, extra frames A). Pointer-only capture frames cost about one CPU core while the mouse moves, in the modes where the system cursor is visible (STATUS Finding 4).
- STATUS Finding 13: a topmost window covering a whole monitor made the shell drop the taskbar's topmost place while a game was open; smaller windows that never cover the monitor did not.

### The one constraint every option must meet

A click goes where the real pointer is. If the real pointer is at screen point P and the unmagnified image is shifted by an offset D, the content under P is drawn at P + D. So either the pointer the user sees is drawn at P + D (and the real one is hidden), or the real pointer is moved, which moves the click. There is no third way without changing where input goes.

## Options

### What shows the shifted desktop

1. **A second overlay window, the pan window.** Same styles and capture exclusion as the panel, covering the uncovered part of the panel's monitor (the monitor rect minus the panel rect). Hidden while the offset is 0, so the real desktop shows as it does today. While panning, it draws the frame texture at 1x, shifted by the offset, and our pointer at the real pointer plus the offset. Cost:
   - a second window in cv-platform-win, created hidden with the panel, placed with each docked layout, shown and hidden by the magnifier, destroyed in `teardown`;
   - `OverlayHost` grows a method to show or hide it and one for its raw handles (ADR 0004 allows new methods; "names may change in the steps, the split may not");
   - a second wgpu surface in cv-magnifier on the same device, sharing the frame and mask textures, with its own uniform buffer and bind group (no second upload);
   - one more presenting window while panning, at 1x over most of a monitor.

   Neither window ever covers the whole monitor by itself, which is what Finding 13 suggests matters (not documented).

2. **One window over the whole monitor while docked, drawing the panel and the shifted area.** One surface and one present, so the two parts can never be a frame apart. Cost: the window covers the monitor all the time, the Finding 13 case. The uncovered area is shown through capture even when nothing is panned, at capture latency, unless the window is resized or given a window region every time the pan starts and stops. The panel's own rect and the cursor hit test change meaning. The renderer needs two viewports with different crops, zooms, filters and pointer sizes in one pass.

3. **Real panning: move or scroll the real windows.** Rejected. It moves the user's windows, cannot shift the desktop or the taskbar, and leaves the machine changed after a kill (PRODUCT principle 2). ADR 0007 already rejected reserving space for the same reasons.

### Where the pointer is and where clicks go

a. **The real pointer stays where it is; we draw our pointer displaced by the offset.** This is ROADMAP 7.1's candidate. The system cursor is hidden while the pan window shows (it would point at the wrong content); our pointer is drawn in the pan window at the real pointer plus the offset, with the shape from `PointerSource::look`. Every click, hover, tooltip, drag and scroll goes to the content drawn under our pointer, because that content is what is really under the real pointer. Menus and tooltips the app opens appear in the capture and are shifted with everything else. Nothing is injected and nothing system-wide changes beyond the cursor hiding we already do. It holds for any offset, eased or not, as long as the pointer and the image are drawn in the same tick from the same offset. Cost: our pointer replaces Windows' at 1x while panning; it is a coverage mask in our colours, not Windows' own colours. Where it falls under the panel (possible for a tick while the offset eases), it is hidden in the pan window but still drawn magnified in the panel at the real position.

b. **Show the system cursor at the displayed position and forward input.** A low-level mouse hook catches clicks, moves the real cursor by minus the offset, injects the click with `SendInput` and moves it back. Cost: hover, tooltips, drag and the app's own cursor shape all see the pointer at the displayed position, over other content, so they are wrong. Injected input is blocked for elevated windows (UIPI). A hook that misbehaves stalls all mouse input. Rejected.

### How the offset follows the pointer

Measured across the edge the panel is on, from the panel's border into the uncovered area. T is the panel thickness (the covered strip), m a margin inside the uncovered area, p the real pointer's distance from the monitor edge the panel sits on (so p < T is under the panel).

i. **Pinned push.** Offset = clamp(T + m − p, 0, T). With the pointer farther than T + m from the edge nothing pans. From there toward the panel our pointer stays put at T + m while the content slides under it, as if pushed. Under the panel's last m pixels the strip is fully shown and our pointer moves again to reach the outermost row. Moving back reverses it exactly. The offset is a function of the pointer position, so it is as smooth as the mouse and continuous: no jump, and the whole strip shows only when the pointer reaches the far side of it. Cost: none beyond the formula; m is a tunable number.

ii. **Proportional.** The pointer range 0 to T + m is squeezed into the displayed range T to T + m, so our pointer moves slower instead of stopping. Cost: a gain below 1 (m / (T + m), about 0.3 for m = 0.4 T); the pointer feels sluggish over the whole zone and a small hand movement does more to the image than to the pointer.

iii. **Edge auto-scroll by time.** The offset grows while the pointer rests near the edge. Under option a it cannot work: our pointer is the real pointer plus the offset, so as the offset grows our pointer moves away from the edge on its own and the scroll stops; and holding still does not move the real pointer to the content being revealed. Rejected.

iv. **Sticky viewport.** The offset only grows when pushed and stays when the pointer moves away; the far edge of the screen pushes it back. Cost: to restore the normal view the user must go to the other side of the monitor, and the image stays shifted (with our pointer and a hidden system cursor) indefinitely. Rejected.

## Decision

Recommendation: **window option 1, pointer option a, offset rule i.** This supersedes two parts of ADR 0007's Decision: the "Under the panel" bullet ("the real desktop stays put") and the system cursor rule (hidden only while the pointer is inside the panel rect), which this ADR extends. The rest of ADR 0007 stands. It does not contradict ADR 0004: the second window lives in cv-platform-win; the magnifier says what (a rect, shown or hidden), the backend says how; `OverlayHost` grows methods, as the sketch allowed. It extends ADR 0010: the same pointer shape is also drawn at 1x in the pan window.

- **Pan window (backend).** A second window, created hidden next to the panel at startup, with the panel's styles, `WDA_EXCLUDEFROMCAPTURE`, alpha 255 and `HTTRANSPARENT`. `apply_layout` for `Docked` places it with `SetWindowPos` over `geometry::uncovered_rect(monitor, edge, thickness)` (new, pure, in cv-core: the monitor rect minus `docked_rect`) and keeps it hidden; `Hidden` and `Fullscreen` hide it. The magnifier shows it with `SW_SHOWNA` and hides it with `SW_HIDE` through the host, only on change. `teardown` destroys it with the panel. Sketch (names may change in the step):

  ```rust
  pub trait OverlayHost: PointerSource {
      fn apply_layout(&mut self, layout: &Layout) -> (u32, u32); // also places the pan window, hidden
      fn set_system_cursor(&mut self, visible: bool);
      fn set_pan_visible(&mut self, visible: bool);
      fn raw_handles(&self) -> (RawDisplayHandle, RawWindowHandle);
      fn pan_handles(&self) -> (RawDisplayHandle, RawWindowHandle);
  }
  ```

- **Offset (cv-core, pure).** `geometry::pan_offset(pointer, monitor, edge, thickness, margin) -> f32` implements rule i for all four edges; it is 0 when the pointer is off the panel's monitor. The magnifier eases the drawn offset toward it with `geometry::smooth_alpha` at a fixed rate of its own (not `smooth_speed`, which users set for the magnified view), and rounds it to whole physical pixels so the 1x image stays an exact copy. The margin and the rate start as constants the step picks (for example a margin of a tenth of the uncovered dimension) and are tuned with you; changing them needs no new ADR. Unit tests cover each edge, the boundaries, the margin and a pointer on another monitor.
- **Drawing (cv-magnifier).** A second surface on the existing device, created from `pan_handles()` when the magnifier is made, configured to the pan window's size on each docked layout. It reuses the pipeline and shares the frame and pointer mask textures; it has its own uniform buffer and bind group. Its uniforms: a crop of the window's own size at 1x, moved back by the offset (top panel: frame rows T − D to H − D; bottom: D to H − T + D; left and right the same across x), no colour filter (the unmagnified area is never filtered), bilinear (exact at a whole-pixel offset), and the pointer at the real pointer plus the offset in window pixels, with the current look at size 1 (Windows' own size) in the user's fill and outline colours. The circle is the fallback for an unknown shape, as in the panel. Uniform writes are skipped when unchanged, as for the panel.
- **When it shows.** The pan window shows while the drawn offset is at least one pixel, the layout is docked, and the panel's monitor is the captured (active) monitor. Otherwise it is hidden at once. The last condition covers the target being a caret on another monitor, and the panel staying on the primary while the pointer is elsewhere (5.2); the frame texture then holds another monitor. When it first shows, the surface is rendered before the window is shown, so no stale or black frame appears. At an offset of one pixel the copy looks the same as the desktop, so the change is not seen.
- **System cursor while docked.** Hidden while the pointer is inside the panel rect (ADR 0007) or while the pan window shows; shown otherwise, called only on change. Fullscreen, `Hidden` and teardown are unchanged.
- **The magnified panel** keeps following the tracking target over the real desktop, as now. Both overlays are excluded from capture, so the panel never shows the shifted copy, and the pan window never captures itself. The tracker is unchanged: the pan follows the real pointer only, never the caret. A caret in the covered strip is read in the panel, which already follows it.
- **Range.** On every edge the offset goes from 0 to the panel thickness on the panel's monitor, so every row or column the panel covers can be brought into view at 1x. With `panel_follows_monitor` the pan window moves with the panel. On an edge shared with another monitor, crossing onto the other monitor ends the pan at once (the pointer is off the panel's monitor and capture switches to the other one).
- **Cost.** While panning: one more 1x present per tick over the uncovered area, no extra upload or capture. While not panning: nothing, the window is hidden. Nothing is built against the 24H2 extra-frames issue; ADR 0007's extra frames A (read the build and MPO lines on the tester's machine before handing over a build) covers the second window too. Performance work is on hold by your decision, so nothing is measured in 7.2; `CV_PERF` and `perf-sample.ps1` stay available.
- **PRODUCT.md v1 Must 2**, proposed new wording, to be applied by the first 7.2 item once this ADR is Accepted: "**Docked mode, overlay style.** The panel sits on one screen edge on top of the desktop. The desktop is not resized and windows do not shrink. The mouse can move under the panel, and the panel shows that area magnified. As the pointer nears the panel, the unmagnified area pans smoothly to show the strip the panel covers, without moving real windows; clicks land where the pointer is drawn. The panel and the pan view are excluded from capture so they never magnify themselves. This replaces the current AppBar docking."
- **Roadmap.** 7.2 becomes 7.2a to 7.2c (ROADMAP Phase 7):
  - 7.2a (agent): PRODUCT.md wording; `geometry::uncovered_rect` and `geometry::pan_offset` in cv-core with unit tests. No behaviour change.
  - 7.2b (agent): the pan window in cv-platform-win and the two `OverlayHost` methods; the fake host records them; `apply_layout` places it hidden; teardown destroys it. It is never shown yet, so no visible change.
  - 7.2c (agent): the second surface, the eased offset, show and hide, the system cursor rule and the 1x pointer in `Magnifier::tick`, with fake-host and fake-renderer tests.
  - 7.3 (you) stays the hand check.

Why: option 1 is the smallest change that shows the real desktop when nothing is panned and never puts a window over a whole monitor. It reuses the window code, the pipeline and the captured frame that already exist. Option 2 trades that for a full-monitor window all the time. Pointer a is the only way to keep clicks, hover and drag correct without touching input, and 6.2c already draws the real shape. Rule i is continuous in the pointer's position, so it pans as smoothly as the mouse moves and never reveals the strip all at once, and moving back undoes it exactly. Easing on top keeps it smooth for fast movements and cannot break the click rule, because the pointer and the image always use the same offset.

## Consequences

- Easier: the covered strip is reachable and readable at 1x on every edge, including a taskbar under a bottom panel. No input is intercepted, nothing new is changed system-wide, and exit has one more window to destroy and nothing to restore. The pan policy is pure and unit-tested.
- Harder: two windows to keep in step on every layout change, monitor change and exit path. `OverlayHost` grows two methods every backend must implement. The renderer gets a second surface. While panning, the unmagnified area is seen through capture, so it lags the real desktop by a frame or so, a video in it plays at the capture rate, and the far rows (the D rows opposite the panel) are pushed out of view. While panning, Windows' own pointer is replaced at 1x by ours in our colours. The system cursor is hidden and shown more often.
- Known limits, not fixed here: the Start menu and other shell surfaces draw above both overlays at their real position until UIAccess (6.3 to 6.5), so while panned they appear twice (real and shifted). The settings window under the panel is reached through the shifted copy like any other window.
- Unknown until 7.2c and 7.3: whether two surfaces on one device behave on your machine (expected; the step confirms by running); whether two topmost windows that together cover the monitor trigger Finding 13's taskbar behaviour (7.3 checks with a bottom panel); whether a lag of our pointer behind the image is noticeable; the margin and easing rate; the cost of the second present (performance work is on hold).
- Revisit: after 7.3 (the numbers, and whether the pointer should be drawn in Windows' colours instead); if the tester's machine shows the 24H2 extra frames (present the pan window only on change, or only on a new frame or offset); if a caret in the covered strip should pan the 1x view too; with UIAccess (6.3 to 6.5), which changes what can draw above the shell.
