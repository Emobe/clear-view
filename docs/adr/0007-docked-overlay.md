# 0007: Docked overlay

Status: Proposed
Date: 2026-10-09

## Context

Roadmap 3.2. PRODUCT.md v1 Must 2: the docked panel sits on one screen edge on top of the desktop. The desktop is not resized and windows do not shrink. The mouse can move under the panel, and the panel shows that area magnified. The panel is excluded from capture so it never magnifies itself. This replaces the AppBar docking that ADR 0002 decided. Your requirements are in STATUS Finding 11: desktop at 100%, the panel on top, the mouse free to move under it ("a fake pan underneath it"). The 3.1 prototype tried this; its results are in docs/prototypes/docked-overlay.md and your verdict was "overlay is the way forward."

### What the code does today

- **Window** (crates/cv-platform-win/src/overlay.rs `run_overlay`): `WS_POPUP` with `WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TRANSPARENT`, `SetLayeredWindowAttributes` alpha 255, `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`, and `WM_NCHITTEST` answers `HTTRANSPARENT`. It starts hidden. The same window serves fullscreen and docked.
- **Docked placement** (`WinHost::apply_layout`, appbar.rs): `appbar::register` sends `ABM_NEW`, `ABM_QUERYPOS` and `ABM_SETPOS`, which shrinks the work area; the window is moved to the rect the shell returns. `ABN_POSCHANGED` sets a flag the loop handles before the next tick (`reposition_appbar`, then `Magnifier::resized`). `appbar::panel_rect` builds the rect from (0, 0) with the active monitor's size, ignoring the monitor origin (STATUS Finding 10).
- **Cursor clip** (`WinHost::update_clip`, `update_clip_cursor`): every tick while docked, `ClipCursor` to `SPI_GETWORKAREA`. This is the wall in STATUS Finding 12.
- **System cursor** (`apply_layout`): `MagShowSystemCursor(false)` in fullscreen, `true` otherwise. No reference count: "the specified visibility state takes effect immediately, regardless of any previous calls" ([Microsoft docs](https://learn.microsoft.com/en-us/windows/win32/api/magnification/nf-magnification-magshowsystemcursor)). Teardown shows it again (1.1).
- **Magnifier** (crates/cv-magnifier/src/magnifier.rs): `Layout::Docked { monitor, edge, thickness }`, with `thickness = geometry::panel_pct_to_px(panel_size, monitor dimension)`. The crop is centred on the smoothed pointer and clamped to the frame (`geometry::compute_crop`). The software cursor circle (10 px radius, shader.wgsl) is drawn in every mode at `geometry::cursor_in_output`, clamped to the window. A monitor switch while docked moves capture but not the panel (`docked_monitor_switch_moves_capture_but_not_the_panel`; Finding 10, roadmap 5.1).
- **Panel size**: `AppState::panel_size`, a percentage. `sanitize` clamps it to 1–100 (cv-core/src/lib.rs), the panel slider is 1..=100 (app/src/app.rs), default 50.

### What the 3.1 prototype found (docs/prototypes/docked-overlay.md)

The current window, placed at the monitor edge with `SetWindowPos`, no AppBar, no `ClipCursor`. Measured on your machine: 25H2 build 26200.9457, RTX 3070, 2560x1440 at 179 Hz plus a second monitor, multiplane overlay (MPO) support false on both outputs.

- **No extra frames.** With the mouse still and the overlay presenting 40–41 times a second, image frames were 34–36/s docked against 34–36/s with the magnifier off. No frame had dirty rects only inside the panel. The same with the DirectComposition swapchain.
- **The 24H2 issue is unchecked for the tester.** It appeared only on outputs with MPO and was reported fixed by KB5046617, OS build 26100.2314 ([Win32CaptureSample#83](https://github.com/robmikh/Win32CaptureSample/issues/83), [KB5046617](https://www.elevenforum.com/t/kb5046617-windows-11-cumulative-update-build-26100-2314-24h2-nov-12.30510/latest)). `IDXGIOutput2::SupportsOverlays` returns TRUE "if the output adapter is the primary adapter and it supports multiplane overlays" ([Microsoft docs](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_3/nf-dxgi1_3-idxgioutput2-supportsoverlays)). The tester's build and MPO support are not known.
- **Filtering captured frames is not safe.** A video under the panel produced real frames whose dirty rects lay only inside the panel. Dropping them would freeze what the panel must show.
- **Presenting only on change** cut presents from 40–41 to 27–32 a second with no measurable CPU change.
- **DirectComposition** (`Dx12SwapchainKind::DxgiFromVisual`) worked and changed nothing measured. Unlike `DxgiFromHwnd` it has no RenderDoc support ([wgpu 30.0.1 docs](https://docs.rs/wgpu/30.0.1/wgpu/enum.Dx12SwapchainKind.html)).
- **Input.** Layered windows with `WS_EX_TRANSPARENT` pass mouse events to the windows underneath ([window features](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features)). You checked by hand: clicks, right-clicks, scroll and drag pass through; the panel never takes focus; the taskbar is reachable under a bottom panel; maximised windows keep their full size; Win+Shift+S does not capture the panel; exit leaves nothing behind.
- **Under the panel.** The magnified view follows the pointer under the panel; the real desktop stays where it is. The system cursor stays visible on top of the panel at its real position, an unmagnified arrow over unrelated magnified content, alongside the software circle.
- **Panel size limits** were not measured.

### Other facts

- `WDA_EXCLUDEFROMCAPTURE`: "The window is displayed only on a monitor. Everywhere else, the window does not appear at all." Introduced in Windows 10 2004; earlier versions behave as `WDA_MONITOR` (the window appears with no content). It works only while DWM composes the desktop ([Microsoft docs](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity)). Windows 11 always composes.
- Z-order: `HWND_TOPMOST` "places the window above all non-topmost windows" ([SetWindowPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos)). Other topmost windows, such as the taskbar, share that group. The Start menu shows above `HWND_TOPMOST` windows; per a Microsoft answer, a window gets above it only with `uiAccess=true`, a signed exe and a secure install location ([Microsoft Q&A](https://learn.microsoft.com/answers/questions/54196/magnifier-control-unable-to-magnify-the-taskbar-st.html)). That is roadmap 6.3 to 6.5, and it applies to fullscreen today as well. `move_window` passes `SWP_NOZORDER`, so the overlay's place among topmost windows is set once at creation and never re-asserted.
- ADR 0004: "AppBar registration, `ClipCursor` and `MagShowSystemCursor` stay inside the backend's `apply_layout` and timer", and the trait sketch's "names may change in the steps, the split may not".

## Options

### Window and presentation

1. **The current overlay window as prototyped, `DxgiFromHwnd`.** In docked mode, place it with `SetWindowPos` at the monitor edge, inside the monitor rect (origin included). No AppBar, no `ClipCursor`, no work-area change. Cost: a backend change in `WinHost::apply_layout`; appbar.rs, `update_clip_cursor`, `reposition_appbar` and the `ABN_POSCHANGED` path go in 3.5. The panel now covers part of every window under it, which is what PRODUCT asks for.
2. **The same window with the DirectComposition swapchain (`DxgiFromVisual`).** Cost: one wgpu setting. No measured gain, no need for transparency, and RenderDoc stops working on the overlay.
3. **Option 1 plus AppBar docking kept as an optional "reserve space" setting.** Cost: appbar.rs, the `ABN_POSCHANGED` path and the clip stay; two docking paths to test on four edges; Findings 10 and 12 stay open for that path. PRODUCT's open question says the default is to remove it, and nobody has asked for it.

Not an option: a Magnification API window (your decision).

### Extra frames from the overlay's own presents

A. **No mitigation now; find out on the tester's machine.** At startup the backend logs the Windows build and `SupportsOverlays` for each output, one line each, as the prototype did. You read them on the tester's machine before 3.6. If the build is older than 26100.2314 with MPO, or the problem is seen, present only on change (the prototype's idle skip, which has a unit test on step/3.1-overlay-prototype). Cost: a few lines of logging.
B. **Build present-only-on-change now.** Cost: a tick-side change and a unit test, no measured benefit on your machine, and one more state in the tick (the surface is not presented for a while) for every later change to respect.

### System cursor while the pointer is under the panel

a. **Keep it visible (as prototyped).** Cost: none. Two pointers on screen while under the panel: the real arrow at 1x at an unrelated place over the magnified image, and the circle at the magnified position.
b. **Hide it while the pointer is inside the panel rect.** The software circle already marks the pointer in the magnified view. The magnifier decides and the backend does it: a new `OverlayHost::set_system_cursor(visible)`, called only when the answer changes; Windows implements it with `MagShowSystemCursor`, which stays in the backend. The hit test is a pure function in cv-core with unit tests, and the fake host records the calls. Cost: one trait method, a few lines in the tick. The arrow reappears up to one tick (16 ms) after the pointer leaves the panel.
c. **Hide it whenever docked.** The user loses the unmagnified pointer outside the panel, where it is the only pointer shown at its real place. Rejected.

### Panel size limits

i. **Keep 1–100%.** 1% of 1440 is 14 px. 100% covers the whole monitor: fullscreen with a visible system cursor and no unmagnified area.
ii. **10–90%**, in `sanitize` and the slider; default stays 50%. A saved value outside loads clamped, as zoom 99 did. 10% is not a usable size at high zoom (a 144 px top panel at 10x shows 14 source rows, under one line of text at 100% scaling), but it is a floor against a panel too thin to see, not a recommendation. 90% always leaves a strip of unmagnified desktop.

## Decision

Recommendation: **window option 1, extra frames A, cursor b, panel size ii.** This supersedes the AppBar docking part of ADR 0002; its wgpu part stands. It does not contradict ADR 0004: the change is inside cv-platform-win behind `Layout`, and `set_system_cursor` adds a method to `OverlayHost`, as the sketch allowed, while `MagShowSystemCursor` stays in the backend.

- **Window.** Unchanged styles: `WS_POPUP`, `WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_LAYERED`, alpha 255, `WDA_EXCLUDEFROMCAPTURE`, `HTTRANSPARENT`. `DxgiFromHwnd`. Docked placement is `SetWindowPos` to a rect from a new pure `geometry::docked_rect(monitor, edge, thickness) -> ScreenRect` in cv-core, with the monitor origin included. It replaces `appbar::panel_rect` and is shared with the cursor hit test. Which monitor the panel sits on stays as today (the monitor `Layout` names); multi-monitor behaviour is 5.1.
- **Z-order.** Keep `SWP_NOZORDER`. 3.3's Verify checks whether clicking the taskbar covers a bottom panel. If it does, `apply_layout` re-asserts `HWND_TOPMOST` when it places the window, and that goes in the PR. The Start menu and other shell surfaces covering the panel are expected until UIAccess (6.3 to 6.5), as in fullscreen today.
- **Extra frames.** Nothing is built against them. The startup log goes in with 3.3. Before handing over tester build 2 (3.6), you read the Windows build and MPO lines on the tester's machine. If they show risk, present-only-on-change becomes a sub-item before 3.6. Captured frames are never filtered by dirty rect.
- **Under the panel.** The panel keeps magnifying the area around the pointer, wherever the pointer is; the real desktop stays put. While the pointer is inside the panel rect, the system cursor is hidden and the circle shows where it is. Fullscreen keeps hiding the cursor through `apply_layout`; `Hidden` and teardown show it again, as today.
- **Panel size.** 10–90% of the monitor dimension the panel spans, default 50%. If you or the tester find these wrong in 3.4 or 3.6, the numbers change in a step; no new ADR is needed.
- **Removed in 3.5:** appbar.rs, `update_clip_cursor` and `WinHost::update_clip`, `reposition_appbar`, the `ABN_POSCHANGED` flag and callback message, `Magnifier::resized` if nothing else calls it, and the AppBar parts of teardown. No setting is removed: `display_mode` and `panel_size` stay.

Why: option 1 is what you tried and approved by hand, and it is the smallest change: the window already exists, and placement is a few lines in one backend function. Options 2 and 3 add cost for nothing measured or asked for. A over B because nothing was measured to fix, and the startup log turns the open question into a fact on the tester's machine. Cursor b because the arrow over the panel points at nothing the user can see there, and the circle already shows the real position. Limits ii because 1% and 100% are not docked panels.

## Consequences

- Easier: Findings 11 and 12 close with 3.5. Docking no longer touches the shell's work area, so a crash or kill cannot leave the desktop squeezed (PRODUCT principle 2). Placing the panel on a non-primary monitor becomes a rect calculation, which helps 5.2.
- Harder: the panel covers windows under it. Text there is seen only magnified, including the settings window and a bottom taskbar; both stay clickable. `OverlayHost` grows one method that every backend must implement. The system cursor is hidden and shown more often while docked, and every exit path must still leave it visible (1.1's teardown already does).
- Unknown until tested: the taskbar covering the panel when it is clicked (3.3 Verify); the extra-frame issue on the tester's machine (before 3.6).
- Revisit: if the tester's machine shows extra frames; with 4.7 (caret tracking docked), where the view follows the caret and the pointer may be anywhere; with 5.1 (which monitor the panel sits on); with 6.1 (cursor enhancement), which may replace the circle and change what is drawn under the panel.
