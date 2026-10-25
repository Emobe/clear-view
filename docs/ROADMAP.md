# ROADMAP

Windows v1 only. Read docs/PRODUCT.md first: it says what v1 is and what it isn't, and it wins if this file disagrees with it. Speech (v1.1) is planned in "Speech track (v1.1)" below. Other work after v1 is listed under "Later" in PRODUCT.md and is not planned here.

Numbered items. Commands take the number: `/step 1.3`, `/adr 2.1`. Status lives in docs/STATUS.md, decisions in docs/adr/. Each phase ends in a state that builds, runs and does not regress the one before. Items marked (ADR first) need an Accepted ADR before any code. Items marked (you) are done by you, not the agent: `/next` tells you what to do, and `/step` with that number records it once you confirm it's done.

## How work proceeds

`/clear`, `/next`, then the command it names (`/adr` or `/step`). A step that needs an ADR stops itself and records why in docs/NEXT.md. You accept ADRs and merge branches; the agent does neither.

## Rules for every step

- No speech work unless the step names it. Steps in the speech track (Phases 8 to 13) follow "Rules for speech steps" below. cv-tts stays behind its feature flag and is not touched unless a step names it.
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
- 0003 Single TTS thread with UIA and SAPI: parked with the reader. Its rule that only the TTS thread owns UIA conflicts with caret tracking in the core; the ADR from 4.3 (0008, Accepted) supersedes that part. The rest (MTA first, plain data over channels, no COM pointers across threads) carries over to the core thread.
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
2.5a Capture survives a lock (STATUS Finding 16): a failed reconnect no longer ends the capture thread; it retries with a short sleep until capture is available again (lock screen, UAC prompt, remote session). Verify: unlock with fullscreen on shows the live desktop, and the view does not slide to the top-left while locked (Finding 15).
2.6 wgpu 22 to 30, naga with it; `bench_modes_4k` before and after.
2.7 eframe and egui 0.29 to 0.36 (needs Rust 1.95 or newer), or 0.35 if 0.36 is too much churn.

## Phase 3: Docked overlay (M2)

3.1 Prototype on a throwaway branch, not merged: a topmost, click-through window excluded from capture (WDA_EXCLUDEFROMCAPTURE) over a live DXGI capture, without AppBar or ClipCursor. The current overlay already sets these styles (ADR 0002), so start from it. Measure whether the overlay's own updates cause extra frames (a known issue on Windows 11 24H2), whether the mouse passes through, and the cost. Write results to docs/prototypes/docked-overlay.md.
3.2 (ADR first) Docked overlay design from the prototype, superseding the AppBar docking part of ADR 0002: window styles, capture exclusion, handling extra frames, what the panel shows when the mouse is under it, panel size limits.
3.3 Overlay docked mode on the primary monitor, top edge only (pending ADR 0007): `SetWindowPos` to `geometry::docked_rect`, no AppBar or ClipCursor for that edge; system cursor hidden while the pointer is inside the panel (`OverlayHost::set_system_cursor`); startup log of the Windows build and MPO support per output. Verify includes whether clicking the taskbar covers the panel.
3.4 All four edges and the panel size setting, limited to 10–90% (pending ADR 0007).
3.5 Remove AppBar docking and ClipCursor (appbar.rs, `update_clip_cursor`, `reposition_appbar`, the `ABN_POSCHANGED` path) (pending ADR 0007). `display_mode` and `panel_size` stay.
3.6 Tester build 2 with docked mode, tag v0.2.0. (you) Before handing it over, read the Windows build and MPO lines on the tester's machine (ADR 0007). Feedback into docs/FEEDBACK.md.

## Phase 4: Caret tracking (M3)

The riskiest part of v1. Every app reports the caret differently, so this phase starts with measurement.

4.1 Caret probe: a small dev binary that logs the caret rectangle from each available source (GetGUIThreadInfo, MSAA caret object, UIA text patterns) once a second. Not shipped.
4.2 (you) Run the probe in Notepad, Word, Chrome, Edge, File Explorer (rename and address bar) and the tester's email app. Record which sources give correct positions in docs/prototypes/caret-sources.md.
4.2a (you) Run the caret probe in desktop Word (body text, a table cell, a header, a long document scrolled down), and in Chrome where it is installed. Record the answering source, the UIA method the probe prints and the costs in docs/prototypes/caret-sources.md. Before 4.8. If Word needs a source outside ADR 0008's chain, that is a new ADR first (pending ADR 0008).
4.3 (ADR first) Caret sources, superseding ADR 0003's rule that only the TTS thread owns UIA (UIA moves to a core thread): which source per app type, fallback order, how the core reports a caret position, and what happens in elevated windows without UIAccess.
4.4 Core: caret source for Windows per the ADR, emitting caret-moved events with a screen rectangle. No magnifier change yet (pending ADR 0008): `CoreEvent` (`CaretMoved`, `CaretLost`), `CaretSource`, `AppId` and a fan-out hub in cv-core with unit tests; the caret thread in cv-platform-win (MTA, no window, lowered UIA timeouts) with the chain msaa → uia → gui → focus rectangle, woken by out-of-context WinEvents and polling at 20 Hz only while the answer came from uia or the focus rectangle; gui converted to physical pixels; `[caret]` log lines. Verify includes Notepad and Explorer rename at 150% scaling.
4.5 Tracking policy as pure logic in cv-core with unit tests: follow the caret while typing, return to the mouse when it moves past a threshold, smoothing between targets. Input is `CaretMoved` and `CaretLost`; the policy decides what part of a `FocusRect` to show (pending ADR 0008).
4.6 Wire the policy into fullscreen mode. The render thread drains its event receiver at the start of each tick (pending ADR 0008).
4.7 Wire the policy into docked mode.
4.8 (you) Run the app list from 4.2 with tracking on, plus Word (4.2a) and an admin Notepad (elevated, ADR 0008 option a). Failures become sub-items of 4.8 (pending ADR 0008).
4.8a Explorer address bar and search box do not move the view (STATUS Finding 21, found in 4.8 on 2026-10-11): find the root cause by reading the code and the `[caret]` log, then fix. ADR 0008 chain i already ends in the focus rectangle for these boxes, so no new ADR is expected.
4.8b Follow the text while typing in the Explorer address bar and search box (STATUS Finding 22, found approving 4.8a on 2026-10-11). Try the uia ranges Finding 22 lists first, with the `caret_probe` or `CV_TRACK_LOG=1`; they stay inside ADR 0008's uia source. If none gives a rectangle, stop: another source is a new ADR (pending ADR 0008).
4.9 Tester build 3, tag v0.3.0. (you) Feedback into docs/FEEDBACK.md.

## Phase 5: Multiple monitors and performance (M4)

5.1 Decide with you how docked mode behaves on multiple monitors: which monitor the panel sits on, and what happens when the mouse moves to another one. Record the decision in the PR or an ADR if it gets complicated.
5.2 Implement 5.1 (Findings 10 and 18). Decided in 5.1: by default the panel stays on the primary monitor and shows whichever monitor the target (pointer or caret) is on; a saved setting, off by default, moves the panel to the target's monitor instead (same edge, same percentage of that monitor). No ADR: no monitor identity is saved, and `Layout::Docked` already names the monitor.
5.3 Mixed display scaling across monitors.
5.4 Monitors connected or disconnected while running (pending ADR 0009): the overlay's `WM_DISPLAYCHANGE` sets a flag, the backend's loop re-enumerates and calls `Magnifier::outputs_changed` when the list differs (once more about 1 s after the last message); the magnifier re-picks the active and primary monitors, rewrites `desired_output`, recreates the frame texture and re-applies the layout; `switch_output` never uses a stale factory. Fake-host tests for removal, primary change, addition and resolution change. Verify includes whether `WM_DISPLAYCHANGE` arrives for a plain plug and unplug.
5.5 Measure CPU and GPU use, idle and while moving, at native resolution and at 4K. Set thresholds with you. Split on 2026-10-10 because the owner cannot run the app for now:
5.5a Measuring tools: an opt-in `[perf]` line (`CV_PERF=1`), `scripts/perf-sample.ps1` for CPU and GPU, and two ignored headless 4K benches (capture copy, GPU upload). Method and matrix in docs/prototypes/performance.md.
5.5b (you) Run the matrix from docs/prototypes/performance.md and send the numbers (4K needs the desktop at 3840x2160, for example NVIDIA DSR). Thresholds are set with you then.
5.5c (you) On hold: proper performance test, later (your decision 2026-10-11, when you approved 5.5b as done for basic purposes without the matrix; you also held off any more performance work, so 5.6 is on hold too): fill in the matrix in docs/prototypes/performance.md on a quiet desktop, fullscreen and docked, still and moving, with the 4K rows if possible, then set the thresholds with you. It settles Finding 4 (24% in the partial run against 100–170% from the 3.1 prototype).
5.6 Fix what 5.5b finds. Likely the per-frame allocation in capture (Finding 4).

## Phase 6: v1 (M5)

6.1 (ADR first) Cursor enhancement: drawing our own enlarged pointer in the magnified view vs changing the system cursor; size, colour (blue default) and outline options.
6.2 Cursor enhancement per the ADR. Split by ADR 0010 into 6.2a to 6.2c, measurement first:
6.2a Pointer probe: a dev-only `pointer_probe` example (not shipped) that hides the cursor with `MagShowSystemCursor(false)` for a few seconds and shows it again, logging what `GetCursorInfo` (flags, handle, shape via `GetIconInfo`) and the duplication's `PointerPosition` and pointer shape report in both states: arrow, over text, over a link, at a resize edge, busy cursor (pending ADR 0010).
6.2b (you) Run the probe and send the output. It decides whether the real cursor shape can be read while hidden (pending ADR 0010).
6.2c Enhanced pointer in the magnified view: the real cursor shape as a coverage mask from the backend (`GetCursorInfo` + `GetIconInfo` on the render thread), drawn by the shader at the pointer with size, fill colour and outline settings, replacing the circle (the circle stays as the fallback when the shape is unknown). 6.2b showed the shape is readable while the cursor is hidden, so ADR 0010's own-arrow fallback is not needed. Split on 2026-10-11 (you approved) into 6.2c1 to 6.2c3 (pending ADR 0010):
6.2c1 Read the pointer shape, no drawing change: cv-core `PointerShape`, `PointerLook { Hidden, Unknown, Shape(Arc<PointerShape>) }` and `PointerSource::look`; cv-platform-win reads `GetCursorInfo`, `GetIconInfo` and `GetDIBits` and converts to a coverage mask (monochrome: drawn unless AND = 1 and XOR = 0; colour: alpha > 0, or AND = 0 if every alpha is 0), deleting the GDI bitmaps every time; `WinHost::look` returns the same `Arc` while `hCursor` is unchanged. The conversion is a pure function with unit tests, and the fake host in the magnifier tests implements `look`. One `[pointer]` stderr line per shape change. Verify (you): move over text, a link, a resize edge and a busy cursor and read the lines; whether an animated cursor changes handle every frame.
6.2c2 Draw it: a second (R8) texture and binding, recreated only when the mask size changes and uploaded only for a new `Arc`; uniforms grow past 48 bytes; the shader draws the mask at the pointer minus hotspot times size, filled, with an outline made by dilating the mask, after the colour filter; `Hidden` draws nothing, `Unknown` keeps the circle, out of view draws nothing. Size, fill and outline are fixed defaults here (blue fill, contrasting outline). Fake-renderer tests for upload only on change and for the three looks. Verify (you), at 100% and 150%: fullscreen and docked, arrow, I-beam, hand, resize and busy, and colour filters leave the pointer alone.
6.2c3 Settings: saved `AppState` fields for pointer size, fill colour, outline width and outline colour (`#[serde(default)]`, clamped in `sanitize`), panel controls, and the `apply_changes` list. Also a checkbox (`pointer_shape`, default on) that switches the magnified pointer between the real shape and the circle (a hidden pointer still draws nothing). Tests for round-trip, clamping, an old file loading and `apply_changes`. Verify (you): each setting changes the pointer live and is kept after a relaunch.
6.3 (ADR first) UIAccess, signing and installer.
6.4 UIAccess manifest and build script.
6.5 Signing.
6.6 Installer.
6.7 (you) Test on a clean Windows 11 install.
6.8 (you) One week of normal use by the tester instead of ZoomText. v1 is done when this passes.

## Phase 7: Docked pan (found 2026-10-11)

A gap in docked mode, found by you after 6.2c3: the overlay panel hides its share of the real desktop (25% at a 25% panel) and the unmagnified area does not pan to show it. The mouse can reach under the panel (ADR 0007) and the panel shows that spot magnified, but the normal view of the covered strip is gone. This is the "fake pan underneath it" from STATUS Finding 11, which the overlay design did not deliver. It is a gap in PRODUCT.md v1 Must 2, so it belongs before v1 closes (6.7 and 6.8). The numbering is only an id; run it when you choose.

Wanted: the unmagnified area pans visually, smoothly and continuously as the pointer nears the edge beside the panel, up to the width of the covered strip. It does not jump to show everything at once. Real windows are not moved or resized (the AppBar approach stays rejected, ADR 0007). A second overlay is expected, excluded from capture like the panel, showing a shifted copy of the captured desktop.

7.1 (ADR first, Opus high) Docked pan design, superseding the part of ADR 0007 that leaves the unmagnified area as the plain desktop, and changing PRODUCT.md v1 Must 2 (the ADR step records that change). Questions for it:
- The shifted image and the real desktop disagree by the pan offset. One candidate: leave the real pointer alone and draw our own pointer displaced by the offset (6.2c already draws the shape), so no clicks are forwarded. Does that work, or does it break something?
- What triggers the pan: how near the edge, how fast, and whether it returns when the pointer leaves.
- The range (the covered strip) on all four edges, on several monitors and with `panel_follows_monitor`.
- The magnified panel must keep showing the real desktop, not the shifted overlay: the new overlay must be excluded from capture.
- A constantly presenting overlay and the 24H2 extra-frames issue (ADR 0007); the cost.
- The taskbar, the system cursor hiding rules, and how the tracking policy (pointer or caret target) interacts with the shifted view.
7.2 Build 7.1. Split into sub-items in docs/NEXT.md if the ADR makes it bigger than one step (pending ADR from 7.1). ADR 0011 (Accepted) splits it:
7.2a PRODUCT.md v1 Must 2 wording from ADR 0011; `geometry::uncovered_rect` and `geometry::pan_offset` (pinned push, all four edges) in cv-core with unit tests. No behaviour change (ADR 0011, Accepted).
7.2b The pan window in cv-platform-win: created hidden with the panel's styles and capture exclusion, placed over the uncovered area by `apply_layout`, destroyed in teardown; `OverlayHost::set_pan_visible` and `pan_handles`, recorded by the fake host. Never shown yet (ADR 0011, Accepted).
7.2c The second surface on the same device, the eased whole-pixel offset, show and hide (only while docked and the panel's monitor is captured), the system cursor hidden while panning, and our pointer at 1x at the real pointer plus the offset, in `Magnifier::tick`, with fake-host and fake-renderer tests (ADR 0011, Accepted).
7.3 (you) Verify by hand on all four edges: the covered strip is reachable by panning, the pan is smooth, clicks land where the pointer is drawn, and exit leaves nothing behind.

## Speech track (v1.1)

Speech is the next release after v1 (v1.1). It does not change the v1 gate. It is built in stages, each ending in a tester build, so the tester can use one stage while the next is built. Order: what's on the window and what has focus, then typing, then the pointer, then reading whole documents.

### Rules for speech steps

Since 8.2, these apply to speech steps in place of the blanket "No speech work" in "Rules for every step". All other rules stay.

- Speech work happens only in steps that name it. With speech switched off, the magnifier behaves exactly as before. Every speech step's Verify list includes a run with speech off.
- Password fields are never spoken: no characters, no words, no length. Use the field's password property, and treat "can't tell" as a password field.
- No UI Automation or MSAA call runs on the render thread. A hung or slow app must not freeze the magnifier or stop speech from being silenced.
- The text that gets spoken (built from a control's name, role, state and value) is pure logic in cv-core with unit tests, so wording can change without touching Windows code.
- Defaults follow what the tester actually uses (8.3), not what ZoomText ships with.

## Phase 8: Speech groundwork

8.1 Audit what exists: docs/later/tts-plan.md, ADR 0003, the cv-tts crate and the tts_* settings fields. Record in docs/STATUS.md what is reusable, what conflicts with later decisions (UIA now lives on the core caret thread; ADR 0004's core event API), and what should be deleted. No code changes. Move tts-plan.md to docs/archive/ once its useful parts are recorded.
8.2 Update docs/PRODUCT.md: move speech out of "Later" into a "Speech (v1.1)" section listing the stages in this track and what stays later ("Later, not planned here" at the end of this track). Update CLAUDE.md and the rules list as above.
8.3 (you) Write down how the tester uses ZoomText's speech today, in docs/prototypes/zoomtext-speech-usage.md: which echo modes are on (typing, mouse, program), voice and rate, which hotkeys she uses (for example Ctrl to stop speech), and what annoys her.
8.4 (ADR first) Voice engine: SAPI 5, OneCore voices through Windows.Media.SpeechSynthesis, or both; how voices are listed and chosen; rate, volume and pitch ranges; what happens with no voices or a voice that's been removed; audio output device. Check whether Windows natural (Narrator) voices can be used by third-party apps. As of October 2026 the NaturalVoiceSAPIAdapter project says Microsoft hasn't allowed this and that its adapter can stop working after updates: https://github.com/gexgd0419/NaturalVoiceSAPIAdapter. Decide whether cv-tts is kept, rewritten or replaced.
8.5 (ADR first) Speech pipeline: queue, priorities and interruption (what cuts off what), which thread talks to the engine, how speech receives events from the core, and timeouts for hung apps. Supersedes ADR 0003 where they disagree.
8.6 (ADR first) Speech hotkeys: revisit the scheme from 1.2 now that many more bindings are coming; whether to add a Caps Lock modifier like ZoomText's; how a single Ctrl press is detected to silence speech without breaking Ctrl shortcuts in other apps; default bindings for every command in this track. ADR 0014 (Accepted) splits it into 8.6a and 8.6b, which run after 8.8a and before 8.8b:
8.6a cv-core `hotkeys`: `Key`, `Chord`, `Binding`, `DEFAULT_BINDINGS` (the three magnifier bindings and speech on/off) and the lone-Ctrl `SilenceKey` rule, with unit tests; cv-platform-win registers from the table instead of its own `BINDINGS`, and the magnifier hotkeys behave as before. No raw input yet (ADR 0014, Accepted).
8.6b Raw input silence and the speech on/off chord in cv-platform-win, behind a `speech_keys` flag that app sets only with `tts`: a message-only window registered for raw keyboard input with `RIDEV_INPUTSINK`, `WM_INPUT` fed to `SilenceKey`, Ctrl+Alt+Shift+S registered, the `Win32_UI_Input` feature; app logs both actions and applies speech on/off. Verify: a Ctrl tap or hold logs silence once; Ctrl+C, Ctrl+V and Ctrl+click still work; AltGr on the UK layout; an admin Notepad in the foreground (recorded); a build without `tts` registers no raw input and behaves as before (ADR 0014, Accepted).
8.7 Engine per 8.4: speak a string, stop, list voices, set voice, rate and volume. The panel's speech controls come back when the `tts` feature is on: speech on/off, voice, rate, volume and a "Test voice" button. Verify: test voice speaks and can be stopped; with the feature off, nothing changes. ADR 0012 (Accepted) splits it:
8.7a cv-core: the neutral rate (0 to 100, 50 normal) and volume mapping for SAPI and OneCore, and the voice-choice rules (no saved voice, saved voice missing, no voices), with unit tests. No Windows code (ADR 0012, Accepted).
8.7b cv-tts rewrite with the SAPI backend: speak, stop, list voices, set voice, rate and volume; the UIA parts and the startup phrase deleted. Checked through a dev example, not wired to the panel (ADR 0012, Accepted).
8.7c cv-tts OneCore backend (`Windows.Media.SpeechSynthesis`) with WASAPI playback on the default device, and one voice list across both engines. Same dev check (ADR 0012, Accepted).
8.7d Panel speech controls with the `tts` feature on: speech on/off, voice (with the missing-voice and no-voices states), rate, volume, "Test voice". Verify: test voice speaks and can be stopped; with the feature off, nothing changes (ADR 0012, Accepted).
8.8 Pipeline per 8.5 and hotkeys per 8.6: speech on/off and silence. Verify: a long sentence stops at once on silence; the magnifier stays smooth while speaking. ADR 0013 splits it, and 8.8a and 8.8b come before 8.7d, because "Test voice" speaks through the speech thread (ADR 0013, Accepted):
8.8a cv-core `speech`: `Utterance`, `Kind`, `SpeechQueue` (foreground and background levels, batches, silence, pointer expiry, the late-event rule, utterance ids and stale "finished" reports), `utterances_for` (returns nothing yet), `EventHub::subscribe_with_wake`, and `Action` variants for silence and speech on/off. Unit tests. No Windows code (ADR 0013, Accepted).
8.8b cv-tts speech thread and `SpeechHandle` on the 8.7b and 8.7c backends: one Win32 wait over commands, core events, SAPI and WASAPI; engines created when speech is first turned on; the stop path; engine timeouts; shutdown. app wiring behind `tts`, with silence and on/off on the 8.6 bindings. Verify: a long sentence stops within about 100 ms of silence (`[tts]` log timestamps); the magnifier stays smooth while speaking; speech off and the feature off change nothing (ADR 0013, Accepted).
8.9 Speech settings persist. Reconcile the old tts_* fields with the ADRs; an old settings file still loads.

## Phase 9: Focus and program echo (speech stage 1)

Goal: hear what has focus and which window you're in. Everything after this builds on it.

9.1 Focus probe: extend the caret probe from 4.1, or add a dev binary, that logs on every focus change the control's name, role, value, state and window title from UIA and MSAA. Not shipped.
9.2 (you) Run the probe in the 4.2 app list plus Settings, a Save As dialog, the Start menu, the taskbar and desktop icons. Record what each source gets right in docs/prototypes/focus-sources.md.
9.3 (ADR first) Focus events in the core: sources and fallback order per app type, what an event carries, filtering duplicate and noisy events, and how it shares UIA with the caret thread.
9.4 Core focus events per 9.3. No speech yet. Unit tests for the filtering logic. ADR 0015 splits it:
9.4a cv-core `focus`: the `FocusChanged` event and its plain-data types, `value_allowed` (the password rule, moved from the probe), `merge` of the UIA and MSAA readings, and `FocusFilter` (settle, newest only, stale readings, duplicates, unreadable windows); the tracking policy ignores focus events. Unit tests. No Windows code (ADR 0015, Accepted).
9.4b cv-platform-win focus thread: MTA, its own `CUIAutomation8` with 500 ms timeouts, foreground and focus WinEvents, reads only while speech is on and never in our own process, one cached UIA read, MSAA from the newest event, values only after the password rule, role mapping with tests, hung windows, `[focus]` lines and `CV_FOCUS_LOG`; app spawns it with `tts`. Verify includes a Save As from a non-elevated Notepad, Task Manager, a password field, the settings panel, a run with speech off and one with `--no-default-features` (ADR 0015, Accepted).
9.5 What gets spoken for a focused control, as pure logic in cv-core with unit tests: name, role, state and value, at two or three verbosity levels decided with you. Wire it to speech.
9.6 Window and dialog changes: speak the title when the foreground window changes, and the dialog's text when a dialog opens.
9.7 Menus, list and tree items, tabs, checkboxes and radio buttons: position ("3 of 7"), checked state, expanded state. Split into sub-items if large.
9.8 Reading commands on hotkeys: read window title, repeat focused item, read the whole active window or dialog in order.
9.9 Magnifier follows keyboard focus using the same events, with a setting to turn it off. Skip this and move it to "Later" if you'd rather keep this track to speech only.
9.10 (you) Run the app list with program echo on. Failures become sub-items of 9.10.
9.11 Tester build with speech stage 1, next minor tag. TESTER.md gets the speech hotkeys. (you) Feedback into docs/FEEDBACK.md.

## Phase 10: Typing echo (speech stage 2)

10.1 (ADR first) Typing echo: where typed input comes from (keyboard hook, UIA text events, or both); characters, words or both; shortcut keys; dead keys and the UK layout; Backspace and Delete; how the password rule is enforced.
10.2 Character echo. Verify includes a browser login form and an app's password field: nothing about the password is spoken.
10.3 Word echo. Word boundary logic in cv-core with unit tests.
10.4 Decide with you what moving the caret speaks (arrows, Home, End, Ctrl+arrows: character, word or line). Implement it.
10.5 (you) App list run. Tester build, next minor tag. Feedback into docs/FEEDBACK.md.

## Phase 11: Mouse echo (speech stage 3)

11.1 Pointer probe: log what UIA returns for the word, line and element under the pointer. Reuse the 9.1 probe.
11.2 (you) Run it in the app list. Record results in docs/prototypes/pointer-text.md.
11.3 (ADR first) Mouse echo: hover delay; word, line or element; not repeating the same text; fast pointer movement; what happens over our own docked panel and during docked pan; how it shares the queue with program echo.
11.4 Mouse echo per 11.3: off, word and line modes, with a hotkey to cycle them.
11.5 (you) App list run. Tester build. Feedback into docs/FEEDBACK.md.

## Phase 12: Reading documents (speech stage 4, AppReader-style)

12.1 (ADR first) Reading: where reading starts (caret, pointer, top of document); continuous reading and pausing; stepping by word, line, sentence and paragraph; which apps come first; how the magnifier follows and highlights the spoken word.
12.2 Continuous reading from the caret in Notepad, stopped by the silence key.
12.3 Step by word, line, sentence and paragraph.
12.4 The magnifier follows the word being spoken, in fullscreen and docked.
12.5 Highlight the spoken word in the magnified view, with colour and shape settings.
12.6 Word, Edge, Chrome and the tester's email app. Failures become sub-items.
12.7 (you) Tester build. Feedback into docs/FEEDBACK.md.

## Phase 13: Speech release (v1.1)

13.1 Speech on by default: the `tts` feature already became a default feature on 2026-10-15 (speech stays off until switched on); this item turns speech itself on by default and may remove the feature, per 8.4 and 8.5. On first run, pick a working voice, and say so in the panel if there is none.
13.2 Check speech in elevated windows with the UIAccess build from Phase 6. Record what doesn't work.
13.3 (you) Clean Windows 11 install test with speech.
13.4 (you) One week of normal use by the tester with Clearview's speech instead of ZoomText's. v1.1 is done when this passes.

## Speech track: later, not planned here

DocReader-style reformatted reading view, Background Reader, Recorder, OCR and camera, Braille, per-app scripting, Linux speech.

## Open decisions

- Hotkey scheme and defaults (1.2).
- Docked behaviour on multiple monitors (5.1).
- Performance thresholds (5.5).
- When to upgrade wgpu and eframe: decided in ADR 0004, after the refactor and before 3.1 (items 2.6 and 2.7).
- How the unmagnified area pans in docked mode (7.1).
- Speech: voice engine and natural voices (8.4): decided in ADR 0012 (Accepted).
- Speech: how speech is silenced, and the speech hotkey scheme (8.5, 8.6). 8.5 is decided in ADR 0013 (Accepted); 8.6 is decided in ADR 0014 (Accepted).
- Speech: verbosity levels (9.5).
- Speech: whether magnifier focus tracking joins the track (9.9).
- Speech: what caret movement speaks (10.4).
