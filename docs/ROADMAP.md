# ROADMAP

Windows v1 only. Read docs/PRODUCT.md first: it says what v1 is and what it isn't, and it wins if this file disagrees with it. Work after v1 is listed under "Later" in PRODUCT.md and is not planned here.

Numbered items. Commands take the number: `/step 1.3`, `/adr 2.1`. Status lives in docs/STATUS.md, decisions in docs/adr/. Each phase ends in a state that builds, runs and does not regress the one before. Items marked (ADR first) need an Accepted ADR before any code. Items marked (you) are done by you, not the agent: `/next` tells you what to do, and `/step` with that number records it once you confirm it's done.

## How work proceeds

`/clear`, `/next`, then the command it names (`/adr` or `/step`). A step that needs an ADR stops itself and records why in docs/NEXT.md. You accept ADRs and merge branches; the agent does neither.

## Rules for every step

- No speech work. cv-tts stays behind its feature flag and is not touched unless a step names it.
- Any step that uses a Windows API or a crate API checks the current Microsoft docs and the docs for the pinned crate version before writing code. Don't rely on memory. Record what was checked, with links, in the PR description.
- One step, one branch, one PR. If a step turns out bigger than expected, stop and propose sub-items in docs/NEXT.md instead of carrying on.
- Every step ends with a short "Verify" list for you to run by hand on Windows. Nothing goes in STATUS "Works" until you've run it.
- `cargo build`, `cargo clippy --workspace` and `cargo test --workspace` pass before a PR is opened.

## Done before this roadmap

Approved items from the previous roadmap, kept under their old numbers in STATUS.md:

- Old 1.1: old plans and the unused shader moved to docs/archive/ (PR #1)
- Old 2.1: pure view math moved to cv-core::geometry with unit tests (PR #4)
- Old 2.2: settings persistence, JSON in the user config dir (PR #5)

## Existing ADRs

Reviewed against docs/PRODUCT.md on 2026-10-08. Their files are not edited; changes happen through the roadmap items named here.

- 0001 CPU readback for capture: stands. Revisited with numbers at 5.5 and 5.6.
- 0002 wgpu renderer and AppBar docking: the wgpu part stands. The AppBar docking part is replaced by overlay docking; the ADR from 3.2 supersedes it. 0002 notes the overlay already uses WS_EX_TRANSPARENT, WS_EX_NOACTIVATE and WDA_EXCLUDEFROMCAPTURE.
- 0003 Single TTS thread with UIA and SAPI: parked with the reader. Its rule that only the TTS thread owns UIA conflicts with caret tracking in the core; the ADR from 4.3 supersedes that part. The rest (MTA first, plain data over channels, no COM pointers across threads) carries over to the core thread.
- 0004 Platform seams and core boundaries: rewritten in place for v1 scope by 2.1 and Accepted 2026-10-09. Questions 4 to 6 of the old brief (Wayland, Wayland cursor, reader interfaces) are deferred, not answered.

## Phase 0: Clean starting point

0.1 Update docs/STATUS.md for this roadmap: relabel the Done list as "old 1.1, old 2.1, old 2.2", remove "Needs your run" items that only concern speech, and clear any docs/NEXT.md entries that refer to the old roadmap. Move tts-plan.md to docs/later/tts-plan.md (the commands already expect it there) with a one-line note at the top saying it is parked until after v1.
0.2 Put cv-tts behind a cargo feature `tts`, off by default. Without the feature the TTS thread is not started and the TTS controls are hidden in the panel. The tts_* settings fields stay in the settings file so nothing is lost. Verify: no "clear-view ready" on launch; magnifier unchanged.
0.3 Remove the 100 ms repaint thread and the redundant `request_repaint_after` in app.rs, and the comment that gives the wrong cause (STATUS Finding 2). With speech off, the thread has no job. Verify: the panel still updates and hotkeys still work with the panel unfocused.
0.4 Stop taking the AppState write lock every frame in the egui panel: snapshot, edit a local copy, write back on change (Finding 3).
0.5 Fix the build and clippy warnings listed in STATUS.
0.6 Stop tracking clear-view.zip in git and add it to .gitignore.
0.7 Update CLAUDE.md: read docs/PRODUCT.md first; remove the claims that don't match the code (Finding 8: ABM_ACTIVATE, AppBar released on exit); record build, clippy and test as the gate; state that speech is off and out of scope until after v1.
0.8 (you) Delete the merged branches listed in STATUS.

## Phase 1: Tester build 1 (M1)

Goal: something your mum can use for real in fullscreen mode, so feedback starts early.

1.1 Clean exit (Finding 9): closing the settings window stops the render thread, destroys the overlay window, shows the system cursor, removes any cursor clip and releases the work area. Same on panic. Find out and record what happens when the process is killed, and whether anything can be done about it.
1.2 (ADR first) Hotkey scheme: RegisterHotKey combinations vs a low-level keyboard hook (needed for a Caps Lock modifier like ZoomText's); default bindings for on/off, zoom in, zoom out; what happens when a binding is already taken.
1.3 Hotkeys from the ADR: on/off, zoom in, zoom out. A failed registration is shown in the settings window, not only on stdout.
1.4 Zoom range 1x to 20x. `sanitize` currently clamps to 10. Zoom steps for the hotkeys decided in this step and recorded in the PR.
1.5 Readability at 10x and above: add sharp bilinear as a third interpolation mode and a naga test that validates shader.wgsl (ADR 0006); compare bilinear, bicubic and sharp at 10x and 20x, check for shimmer or jitter while smooth-following at high zoom, pick the default. You judge; the agent fixes what you find.
1.6 Display scaling: cursor circle and view line up at 100%, 125% and 150%. Fix if not.
1.7 Tester build: a release build you can copy to the tester's machine by hand, a one-page TESTER.md with the hotkeys and how to quit, and a git tag v0.1.0.
1.8 (you) Tester uses it. Record what's missing or wrong in docs/FEEDBACK.md. Feedback items get added to later phases by you.
1.9 Edge-smoothing mode (ADR 0006): port cleanEdge (MIT, single pass, any scale) as a fourth interpolation mode and measure its cost at 4K. If it fails on anti-aliased text, record why; a multi-pass Super-xBR pipeline then needs its own ADR after ADR 0004. Runs before 1.7 instead if you judge sharp bilinear not good enough for the tester.

## Phase 2: Platform seams

2.1 (ADR first) ADR 0004 with Opus, rewritten in place (see Existing ADRs): boundaries between core (pointer and caret sources, later focus and text), magnifier (capture, render, tracking policy, modes) and the Windows backend (window host, hotkeys, capture). The core's event API must not rule out a reader or per-app scripting later. Also decides when to upgrade wgpu 22 and eframe 0.29. Linux stays possible through the seam, but the ADR does not design the Linux backends or the reader. Its output replaces 2.2 with numbered sub-items.
2.2 to 2.7 replace the single refactor item (Finding 5). None of them may change behaviour; each Verify list uses the regression list in ADR 0004.

2.2 cv-platform-win: new crate absorbing cv-capture unchanged; DPI awareness and `RegisterHotKey` move in from app; the hotkey `Action` and its effect on `AppState` move to cv-core with tests; app drops its `windows` dependency.
2.3 Portable renderer: gfx.rs takes raw display and window handles instead of `HWND`; the Win32 overlay (window, loop, AppBar, cursor clip, cursor hiding, clean exit) and appbar.rs move to cv-platform-win; cv-render drops `windows` and is renamed cv-magnifier.
2.4 Seam traits `PointerSource`, `OverlayHost`, `CaptureSource`; the capture loop moves from main.rs to cv-magnifier behind `CaptureSource`.
2.5 `Magnifier::tick`: the per-tick logic moves out of `on_timer` behind `OverlayHost` and `Layout`, with fake-host unit tests. Stop and propose sub-items if it is bigger than one step.
2.6 wgpu 22 to 30, naga with it; `bench_modes_4k` before and after.
2.7 eframe and egui 0.29 to 0.36 (needs Rust 1.95 or newer), or 0.35 if 0.36 is too much churn.

## Phase 3: Docked overlay (M2)

3.1 Prototype on a throwaway branch, not merged: a topmost, click-through window excluded from capture (WDA_EXCLUDEFROMCAPTURE) over a live DXGI capture, without AppBar or ClipCursor. The current overlay already sets these styles (ADR 0002), so start from it. Measure whether the overlay's own updates cause extra frames (a known issue on Windows 11 24H2), whether the mouse passes through, and the cost. Write results to docs/prototypes/docked-overlay.md.
3.2 (ADR first) Docked overlay design from the prototype, superseding the AppBar docking part of ADR 0002: window styles, capture exclusion, handling extra frames, what the panel shows when the mouse is under it, panel size limits.
3.3 Overlay docked mode on the primary monitor, top edge only.
3.4 All four edges and the panel size setting.
3.5 Remove AppBar docking and ClipCursor (appbar.rs, `update_clip_cursor`) and their settings.
3.6 Tester build 2 with docked mode, tag v0.2.0. (you) Feedback into docs/FEEDBACK.md.

## Phase 4: Caret tracking (M3)

The riskiest part of v1. Every app reports the caret differently, so this phase starts with measurement.

4.1 Caret probe: a small dev binary that logs the caret rectangle from each available source (GetGUIThreadInfo, MSAA caret object, UIA text patterns) once a second. Not shipped.
4.2 (you) Run the probe in Notepad, Word, Chrome, Edge, File Explorer (rename and address bar) and the tester's email app. Record which sources give correct positions in docs/prototypes/caret-sources.md.
4.3 (ADR first) Caret sources, superseding ADR 0003's rule that only the TTS thread owns UIA (UIA moves to a core thread): which source per app type, fallback order, how the core reports a caret position, and what happens in elevated windows without UIAccess.
4.4 Core: caret source for Windows per the ADR, emitting caret-moved events with a screen rectangle. No magnifier change yet.
4.5 Tracking policy as pure logic in cv-core with unit tests: follow the caret while typing, return to the mouse when it moves past a threshold, smoothing between targets.
4.6 Wire the policy into fullscreen mode.
4.7 Wire the policy into docked mode.
4.8 (you) Run the app list from 4.2 with tracking on. Failures become sub-items of 4.8.
4.9 Tester build 3, tag v0.3.0. (you) Feedback into docs/FEEDBACK.md.

## Phase 5: Multiple monitors and performance (M4)

5.1 Decide with you how docked mode behaves on multiple monitors: which monitor the panel sits on, and what happens when the mouse moves to another one. Record the decision in the PR or an ADR if it gets complicated.
5.2 Implement 5.1 (Finding 10).
5.3 Mixed display scaling across monitors.
5.4 Monitors connected or disconnected while running.
5.5 Measure CPU and GPU use, idle and while moving, at native resolution and at 4K. Set thresholds with you.
5.6 Fix what 5.5 finds. Likely the per-frame allocation in capture (Finding 4).

## Phase 6: v1 (M5)

6.1 (ADR first) Cursor enhancement: drawing our own enlarged pointer in the magnified view vs changing the system cursor; size, colour (blue default) and outline options.
6.2 Cursor enhancement per the ADR.
6.3 (ADR first) UIAccess, signing and installer.
6.4 UIAccess manifest and build script.
6.5 Signing.
6.6 Installer.
6.7 (you) Test on a clean Windows 11 install.
6.8 (you) One week of normal use by the tester instead of ZoomText. v1 is done when this passes.

## Open decisions

- Hotkey scheme and defaults (1.2).
- Docked behaviour on multiple monitors (5.1).
- Performance thresholds (5.5).
- When to upgrade wgpu and eframe: decided in ADR 0004, after the refactor and before 3.1 (items 2.6 and 2.7).
