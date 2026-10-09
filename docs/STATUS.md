# STATUS

Audited 2026-10-08 on Windows, master at 89cc84c plus uncommitted docs changes. Replaces the static draft. "Works" holds only what a command proved or what you verified by hand and approved. Runtime behaviour you have not verified is under "Not tested" or "Needs your run".

## Done

Roadmap items you have approved after testing. Added only when you say "I approve".

The roadmap was rewritten on 2026-10-08. These three come from the previous roadmap and are not the same items as 1.1, 2.1 and 2.2 in docs/ROADMAP.md now.

- old 1.1 Move old plans and the unused shader to docs/archive/ (merged in PR #1)
- old 2.1 Move pure view math into cv-core::geometry with unit tests (PR #4)
- old 2.2 Settings persistence, JSON in the user config dir (PR #5)

Current roadmap:

- 0.1 Update docs/STATUS.md for this roadmap (branch step/0.1-status-for-roadmap)
- 0.2 Put cv-tts behind an off-by-default `tts` feature (branch step/0.2-tts-feature, PR #8)
- 0.3 Remove the 100 ms repaint thread (branch step/0.3-remove-repaint-thread, PR #10)
- 0.4 Panel draws from a snapshot and writes back only changed fields (branch step/0.4-panel-snapshot, PR #11)
- 0.5 Fix the build and clippy warnings listed in STATUS (branch step/0.5-fix-warnings, PR #12)
- 0.6 Stop tracking clear-view.zip in git and add it to .gitignore (done by you on master, commit 085858e)
- 0.7 Update CLAUDE.md (branch step/0.7-claude-md, PR #13)
- 0.8 Delete the merged branches (2026-10-09: every local branch merged into master deleted with `git branch -d`; GitHub branches left as they are)
- 1.1 Clean exit (branch step/1.1-clean-exit, PR #15)
- 1.2 Hotkey scheme ADR (ADR 0005, branch step/1.2-hotkey-adr, PR #16)
- 1.3 Hotkeys from the ADR (branch step/1.3-hotkeys, PR #17)
- 1.4 Zoom range 1x to 20x (branch step/1.4-zoom-range, PR #18)
- 1.5 Readability at 10x and above (ADR 0006, branch step/1.5-readability, PR #20)
- 1.6 Display scaling (branch step/1.6-display-scaling, PR #21)
- 1.7 Tester build (branch step/1.7-tester-build, PR #22)
- 1.9 Edge-smoothing mode, cleanEdge (ADR 0006, branch step/1.9-clean-edge, PR #23). Approved as built; you said it needs more work (Finding 14).
- 2.1 Platform seams ADR (ADR 0004 rewritten in place, branch step/2.1-platform-seams-adr, PR #24)
- 2.2 cv-platform-win backend crate (ADR 0004, branch step/2.2-platform-win, PR #25)
- 2.3 Portable renderer (ADR 0004, branch step/2.3-portable-renderer, PR #26)
- 2.4 Seam traits; capture loop moves to cv-magnifier (ADR 0004, branch step/2.4-seam-traits, PR #27)
- 2.5 `Magnifier::tick`; per-tick logic moves out of `on_timer` (ADR 0004, branch step/2.5-magnifier-tick, PR #28)
- 2.5a Capture survives a lock (Finding 16, branch step/2.5a-capture-survives-lock, PR #29)
- 2.6 wgpu 22 to 30, naga with it (ADR 0004, branch step/2.6-wgpu-30, PR #30)
- 2.7 eframe and egui 0.29 to 0.36.2 (ADR 0004, branch step/2.7-eframe, PR #31)
- 3.1 Docked overlay prototype (code on branch step/3.1-overlay-prototype, never merged; results in docs/prototypes/docked-overlay.md, branch step/3.1-docked-overlay-results, PR #32)
- 3.2 Docked overlay ADR (ADR 0007, branch step/3.2-docked-overlay-adr, PR #33)
- 3.3 Overlay docked mode, top edge (ADR 0007, branch step/3.3-docked-overlay-top, PR #34)
- 3.4 Overlay docking on all four edges, panel size 10–90% (ADR 0007, branch step/3.4-docked-overlay-all-edges, PR #35)
- 3.5 Remove AppBar docking and ClipCursor (ADR 0007, branch step/3.5-remove-appbar, PR #36)
- 4.1 Caret probe (branch step/4.1-caret-probe, PR #37)
- 4.2 Caret sources measured (you; results in docs/prototypes/caret-sources.md, branch step/4.2-caret-sources)
- 4.3 Caret sources ADR (ADR 0008, branch step/4.3-caret-sources-adr, PR #39)
- 4.4 Core caret source for Windows (ADR 0008, branch step/4.4-caret-source, PR #40)
- 4.5 Tracking policy in cv-core (ADR 0008, branch step/4.5-tracking-policy, PR #41)

## Works (a command proved it, or you verified it)

- Old plans archived: PLAN.md, PHASE2PLAN.md, handoff.md and magnify.hlsl are in docs/archive/; magnify.hlsl had no references in .rs, .toml or .wgsl. Approved by you (old 1.1).
- Docs for the new roadmap (0.1, approved by you 2026-10-08): the Done list labels the previous roadmap's items "old 1.1, old 2.1, old 2.2"; speech-only items are gone from "Needs your run"; tts-plan.md is at docs/later/tts-plan.md with a parked note.
- cv-tts behind the `tts` feature (0.2, approved by you 2026-10-08): the step's Verify list was no "clear-view ready" on launch and the magnifier unchanged. Plain `cargo build` and `cargo run` skip cv-tts; `--workspace` still builds it.
- Panel no longer polls (0.3, approved by you 2026-10-08): the repaint thread and `request_repaint_after` are gone; the hotkey thread wakes the panel after a toggle. You ran the step's Verify list (hotkey with the panel unfocused, panel controls, settings saving, idle CPU) and reported that it works; individual items were not itemised.
- Panel no longer holds the write lock per frame (0.4, approved by you 2026-10-08): `update()` clones `AppState` under a read lock, draws against the copy, and writes back only the fields the panel changed, under one short write lock. You ran the step's Verify list and reported that it works; individual items were not itemised. 4 unit tests cover `apply_changes` (app now has 10 tests).
- Build and clippy warnings cleared (0.5, approved by you 2026-10-09): `cargo build`, `cargo clippy --workspace --all-targets` and `cargo clippy --workspace --all-targets --features app/tts` report 0 warnings and `cargo test --workspace` passes. You ran the step's Verify list (hotkey on and off, docked work area, second monitor, restart with saved settings, closing the window while magnifying) and reported that it works; individual items were not itemised. The four `MagInitialize`, `MagUninitialize` and `MagShowSystemCursor` results are ignored with `let _ =` on purpose: cursor hiding is best-effort.
- clear-view.zip no longer tracked (0.6, done by you 2026-10-09 in commit 085858e): the file is removed from the index and `.gitignore` lists `clear-view.zip`. Git history still contains it; nothing was rewritten.
- CLAUDE.md matches the v1 roadmap (0.7, approved by you 2026-10-09): PRODUCT.md read first, speech out of scope until after v1, the Finding 8 AppBar claims corrected, the build/clippy/test gate recorded (docs-only steps skip it), stale tts-plan.md and repaint-thread references fixed.
- Clean exit (1.1, approved by you 2026-10-09): closing the settings window, Ctrl+C in the console and closing the console window all end the process; it is gone from Task Manager each time (it shows as "app.exe"). `cargo build`, `cargo clippy --workspace --all-targets` and `cargo test --workspace` pass. You ran the step's whole Verify list (settings window close, docked edges, magnifier toggled off first, Ctrl+C and console close, and ending the process in Task Manager) and reported that every one of them works; individual items were not itemised. Killing in Task Manager therefore leaves nothing behind that you noticed, which the code cannot explain: no code runs on a kill, so Windows does the restoring.
- Hotkey scheme decided (1.2, approved by you 2026-10-09): ADR 0005 is Accepted, option 3: `RegisterHotKey` only with fixed Ctrl+Alt+Shift bindings kept as data, and a Caps Lock hook layer only if the tester asks for it after 1.8.
- Hotkeys (1.3, approved by you 2026-10-09): Ctrl+Alt+Shift+Z toggles on and off, Ctrl+Alt+Shift+Up zooms in and Ctrl+Alt+Shift+Down zooms out (ADR 0005, `RegisterHotKey`, `MOD_NOREPEAT`, registered once at startup; the thread blocks in `GetMessageW`). A binding that fails to register is logged with its error code and listed in the settings window. You ran the step's whole Verify list (toggle in fullscreen and docked with the panel focused and unfocused, zoom keys and limits, no repeat on hold, zoom saved across relaunch, idle CPU, a second copy showing the conflict line, an elevated window in the foreground, the old "Win +=" label gone, panel controls reachable with failure lines showing) and reported that all of it works; individual items were not itemised, so the elevated-window result is not recorded separately.
- Zoom range 1x to 20x (1.4, approved by you 2026-10-09): `ZOOM_MAX` is 20 and the slider reaches it. Hotkey zoom step is 0.5 below 4x and 1.0 from 4x up (`ZOOM_STEP_FINE`, `ZOOM_STEP_COARSE`, `ZOOM_COARSE_FROM`); stepping out from exactly 4x uses the fine step, so in and out presses retrace the same values. You ran the step's whole Verify list and reported that it works; individual items were not itemised. You judged 20x "quite blurry but not terrible" (Finding 14, roadmap 1.5). Saved zoom of 10 or lower still loads; a hand-edited 99 loads as 20.
- Readability at high zoom (1.5, approved by you 2026-10-09): ADR 0006 is Accepted (option 2). A third interpolation mode, Sharp (sharp bilinear: texels drawn flat, edges blended over about 1 output pixel, scale from `fwidth`), sits next to Bilinear and Bicubic in the panel and can be switched at any zoom. You compared the three and chose Bicubic as the default (`Interpolation::default()`); a saved choice in settings.json still wins. A cv-render test validates shader.wgsl through `wgpu::naga`, so shader errors fail `cargo test`. You ran the step's whole Verify list and reported that it works; individual items were not itemised. You saw no noticeable jitter while smooth-following at high zoom, so no jitter fix was made; you expect it may show up with caret tracking (Phase 4). Sharp makes edges crisp, but on anti-aliased text it turns the soft edge pixels into sharp grey blocks (Finding 14).
- Display scaling (1.6, approved by you 2026-10-09): the view and cursor circle line up at 100%, 125% and 150%. No alignment fix was needed: the process sets Per-Monitor v2 DPI awareness first thing in `main`, so the cursor, monitor rects, captured frames, work area and window rects are all physical pixels. The one change: the effective awareness is now checked at startup and `[dpi] process is not Per-Monitor v2 DPI aware ...` is printed to stderr if it isn't. You moved the Windows scale slider up and down with the magnifier on: it always lined up, the view recovered within a second or two of each change, and the console printed no `[dpi]` line. The circle radius is fixed in physical pixels, so it looks smaller at 150% (size, not alignment; roadmap 6.1/6.2). Mixed scaling across monitors is roadmap 5.3.
- Tester build (1.7, approved by you 2026-10-09): plain `cargo build --release` (no `RUSTFLAGS`) gives `target\release\clear-view.exe` (the bin is renamed from `app`). The C runtime is linked statically by `.cargo/config.toml` (`+crt-static` for `x86_64-pc-windows-msvc`); the release exe references no `vcruntime140.dll` or `api-ms-win-crt-*` DLL. The console window stays in release builds by your decision (you will be with the tester). TESTER.md (repo root) covers starting, hotkeys, settings, quitting and installing. You ran the step's whole Verify list (release exe from Explorer, hotkeys, quit leaves nothing in Task Manager, copied to another folder or machine with settings kept, TESTER.md read, `cargo run`, a machine without the VC++ redistributable if available) and reported that all of it works; individual items were not itemised, so whether the no-redistributable machine was tried is not recorded. The annotated tag v0.1.0 is on bb81f33.
- Edge-smoothing mode (1.9, approved by you 2026-10-09): a fourth interpolation mode, "Smooth edges" (`Interpolation::CleanEdge`, `as_u32` 3), is a WGSL port of torcado's cleanEdge (MIT, libretro `include/cleanEdge.inc` at 94a7736) in cv-render/src/clean_edge.wgsl, appended to shader.wgsl at compile time. Its colour similarity threshold is a saved setting, `edge_threshold` (0–1, default 0.1), with a slider shown only in that mode; uniforms are 48 bytes. An ignored test, `bench_modes_4k`, measures every mode headless at 3840x2160: cleanEdge 0.9 to 1.5 ms per frame on an RTX 3070, about 4.5x Bicubic. You ran the tests and approved it; individual Verify items were not itemised. Result: no lag, but it "just makes things rounder" and is not as clean as ZoomText, because on anti-aliased text it rounds the grey edge pixels instead of using them. You said it needs more work; the next filters to try are in docs/later/text-smoothing.md (Finding 14). Bicubic stays the default.
- Platform seams decided (2.1, approved by you 2026-10-09): ADR 0004 is Accepted, option 2 with upgrade timing B. Traits at the seam (`PointerSource` in cv-core; `OverlayHost` with a `Layout` enum and `CaptureSource` in cv-magnifier, which is cv-render renamed); one Windows backend crate, cv-platform-win, absorbing cv-capture and every `windows` import outside cv-tts; the backend owns the render and hotkey threads and calls `Magnifier::tick`; frames stay CPU `Arc<Frame>`; core event rules set for 4.4. wgpu 22 to 30 and eframe 0.29 to 0.36 come after the refactor, before 3.1. Roadmap 2.2 is replaced by 2.2 to 2.7.
- Windows backend crate (2.2, approved by you 2026-10-09): cv-platform-win (`#![cfg(windows)]`) holds DXGI capture (cv-capture's lib.rs moved unchanged as capture.rs; cv-capture is gone), `set_dpi_awareness` (moved from main.rs) and the hotkey bindings and loop (moved from app; the backend spawns the thread and reports failures once and each press as an `Action`). cv-core has `Action { Toggle, ZoomIn, ZoomOut }` and `AppState::apply` with 4 tests. app picks the backend with `#[cfg(windows)] use cv_platform_win as platform;` and no longer depends on `windows`; `cargo tree -i windows@0.62.2 --workspace -e normal --depth 1` lists cv-platform-win, cv-render and cv-tts. You ran the step's Verify list (the ADR 0004 regression list) and approved; individual items were not itemised.
- Portable renderer (2.3, approved by you 2026-10-09): cv-render is renamed cv-magnifier (gfx.rs and the shaders) and has no `windows` dependency; `WgpuState::new` is `unsafe` and takes a `RawDisplayHandle` and `RawWindowHandle` instead of `HWND`. The Win32 overlay (old cv-render lib.rs, now cv-platform-win/src/overlay.rs) and appbar.rs moved to cv-platform-win line for line; overlay.rs builds the Win32 handles itself and still drops the surface before `DestroyWindow`. app calls `platform::spawn_overlay` and no longer depends on the renderer crate. `cargo tree -i windows@0.62.2 --workspace -e normal --depth 1` lists only cv-platform-win and cv-tts. You tested it and reported that it works; individual Verify items were not itemised.
- Seam traits (2.4, approved by you 2026-10-09):
  - cv-core has `ScreenPoint`, `ScreenRect` and `PointerSource`.
  - cv-magnifier has `CaptureSource`, `CaptureError` (`AccessLost`, `DeviceLost`, `Other`, each with the OS message) and `spawn_capture`. `spawn_capture` runs the capture loop that was in main.rs. The source is built by a factory on the capture thread with the starting output index.
  - `Layout` and `OverlayHost` are declared only; the Windows implementation comes with 2.5.
  - cv-platform-win's `Capturer` implements `CaptureSource`. Every error still rebuilds device and duplication through `reconnect`.
  - `CursorPointer` (`GetCursorPos`) implements `PointerSource` and is what `on_timer` reads; a failed read still gives (0, 0) (Finding 15).
  - app depends on cv-magnifier again.
  - You tested it and reported that it works; individual Verify items were not itemised, so the lock/unlock reconnect check is not recorded separately. (Testing 2.5 later showed that capture does not survive a lock: Finding 16.)
- `Magnifier::tick` (2.5, approved by you 2026-10-09):
  - cv-magnifier/src/magnifier.rs holds the per-tick logic that was in `on_timer`: pointer follow, layout decisions, lerp, crop, frame upload and uniform caching. `Magnifier` (`new`, `tick`, `resized`) wraps a private `View` over a crate-private `Renderer` trait that `WgpuState` implements; 21 unit tests use a fake host and renderer. `WgpuState` is no longer exported.
  - cv-platform-win's `WinHost` implements `OverlayHost` (AppBar, window placement, `ShowWindow`, `MagShowSystemCursor`) and clips the cursor each tick while docked. The message loop runs the tick on WM_TIMER; `WIN_DATA` is gone and `wnd_proc` borrows nothing. `ABN_POSCHANGED` sets a flag the loop handles before the next tick. Teardown runs from `WinHost`'s `Drop`, after the magnifier and its surface are dropped. A `GetMessageW` return of -1 now ends the loop.
  - Finding 15 fix: a failed pointer read keeps the last position.
  - You ran the step's Verify list and reported that everything works except the unlock check: unlocking with fullscreen on showed a black view (Finding 16, not caused by 2.5, roadmap 2.5a). That check also covered Finding 15, so the Finding 15 fix is not yet seen working by hand.
- Capture survives a lock (2.5a, approved by you 2026-10-09):
  - `run_capture` (cv-magnifier/src/capture.rs) no longer ends on a failed reconnect. After a frame error it retries `reconnect` every 250 ms (`RECONNECT_RETRY`) without reading frames. It logs the first failure and the recovery once each, and makes a monitor switch asked for during the outage once capture is back.
  - `spawn_capture` retries a failed factory at startup the same way; its factory bound is now `FnMut`.
  - cv-platform-win's `Capturer::recreate` releases the lost duplication before creating a new one, as the `AcquireNextFrame` docs require after `DXGI_ERROR_ACCESS_LOST`. `duplication` is an `Option`.
  - You tested it and reported that it works; individual Verify items (lock and unlock, Finding 15 view not sliding, UAC prompt, second-monitor follow afterwards, lock with the magnifier off, normal exit) were not itemised.
- wgpu 30 (2.6, approved by you 2026-10-09):
  - wgpu and naga 22.1.0 → 30.0.1. Only cv-magnifier/src/gfx.rs changed; `cargo tree -i wgpu --workspace -e normal --depth 1` shows one wgpu, used only by cv-magnifier.
  - The DX12 shader compiler is set to FXC explicitly (`dx12_instance`), as wgpu 22 used; wgpu 30's default would pick DXC wherever `dxcompiler.dll` is on the PATH. Colour space `Auto` (sRGB for Bgra8Unorm), `Fifo`, frame latency 2 and the DX12 swapchain defaults are as before.
  - `get_current_texture` now returns an enum: `Success` and `Suboptimal` draw and present (`Queue::present`); anything else logs `[render] surface error` and the tick reconfigures the surface, as before.
  - `cargo tree -i windows@0.62.2 --workspace -e normal --depth 1` now also lists gpu-allocator and wgpu-hal: wgpu 30 uses the same `windows` 0.62.2. No workspace crate besides cv-platform-win and cv-tts depends on it.
  - `bench_modes_4k` on an idle RTX 3070, 22 vs 30 run alternately: Bilinear, Bicubic and Sharp unchanged or slightly faster (0.16–0.38 ms); cleanEdge 12–19% slower on flat and stripes frames (1.32–1.50 → 1.49–1.78 ms), unchanged on noise (0.91–1.02 ms). Cause not investigated; the shader source is unchanged. Table in PR #30.
  - You ran the step's Verify list (the ADR 0004 regression list) and reported that it works; individual items were not itemised.
- eframe 0.36 (2.7, approved by you 2026-10-09):
  - eframe and egui 0.29.1 → 0.36.2, still `default-features = false` with `default_fonts` and `glow`. Pinned to 0.36.2 because 0.36.0 can leave the event loop polling and busy a core (egui PR #8398). Only app changed: `App::update(ctx)` is now `App::ui(ui)` and `CentralPanel::default().show(ui, …)`; main.rs is unchanged. wgpu stays at 30.0.1.
  - Toolchain is now Rust 1.99.0 (eframe 0.36 needs 1.95). Clippy 1.99 found no new lints in the existing code.
  - eframe now uses `windows-sys` 0.61.2 instead of `winapi`; `cargo tree -i windows@0.62.2 --workspace -e normal --depth 1` is unchanged from 2.6.
  - `Slider` still re-applies `step_by` rounding every frame (Finding 3 unchanged).
  - You ran the step's Verify list (the ADR 0004 regression list plus panel checks) and reported that it works; individual items were not itemised.
- Docked overlay prototype (3.1, approved by you 2026-10-09). Full numbers are in docs/prototypes/docked-overlay.md; the code is on step/3.1-overlay-prototype (73bdf0a) and is never merged.
  - Setup: the current overlay window and styles, placed at the monitor edge with `SetWindowPos`, with no AppBar and no `ClipCursor`. Measured on your machine: 25H2 build 26200.9457, RTX 3070, 2560x1440 at 179 Hz plus a second monitor, multiplane overlay support false on both outputs.
  - No extra frames: with the mouse still, the overlay presenting about 40 times a second added no captured frames. Docked and fullscreen gave 34–39 image frames/s against 34–36/s with the magnifier off. No frame had dirty rects only inside the panel. The same held with the DirectComposition swapchain (`DxgiFromVisual`).
  - The 24H2 issue was MPO-only and fixed in KB5046617 (build 26100.2314), so the tester's build and MPO support are unchecked.
  - Skipping idle presents saved about a quarter of the presents with no CPU change. DirectComposition worked and changed nothing measured.
  - A video under the panel produces frames whose dirty rects lie only inside the panel, so filtering such frames is not a safe mitigation.
  - What you checked by hand:
    - The magnified panel follows the pointer under the panel, and the real desktop stays at 100%.
    - Clicks pass through.
    - In a second session you reported that all of these work; individual items were not itemised: right-click, scroll and drag under the panel; the panel never takes focus; the taskbar is reachable under a bottom panel; maximised windows keep their full size; Win+Shift+S doesn't capture the panel; exit leaves nothing behind.
  - Your verdict: "overlay is the way forward."
- Docked overlay decided (3.2, accepted by you 2026-10-09, PR #33): ADR 0007 is Accepted. Choices: the current overlay window placed with `SetWindowPos` (`DxgiFromHwnd`, styles unchanged), no mitigation for extra frames beyond a startup log, the system cursor hidden while the pointer is inside the panel, and panel size limited to 10–90%. It supersedes the AppBar docking part of ADR 0002.
- Overlay docked mode, top edge (3.3, approved by you 2026-10-09, PR #34):
  - Docked Top is placed with `SetWindowPos` to `geometry::docked_rect` (cv-core, monitor origin included, clamped to the monitor). No AppBar and no `ClipCursor` on that edge, so the work area is unchanged and the mouse moves under the panel. Left, Bottom and Right still use AppBar docking (3.4 moves them).
  - `OverlayHost::set_system_cursor(visible)`: while docked, the magnifier hides the system cursor when the raw pointer is inside the applied panel rect (`ScreenRect::contains`) and shows it when the pointer leaves, calling only on change; every layout change resets it. `WinHost` implements it with `MagShowSystemCursor`. 6 fake-host tests.
  - `log_system_info()` at startup prints `[system] Windows <DisplayVersion> build <CurrentBuildNumber>.<UBR>` (registry; `GetVersionExW` reports 6.2 without a manifest) and one `[system] output N <device>: multiplane overlay support <bool>` line per output on the primary adapter.
  - You ran the step's whole Verify list and reported that all of it works; individual items were not itemised. That list included whether clicking the taskbar through a 100% top panel draws the taskbar over the panel; no `HWND_TOPMOST` re-assert was added, and the result is not recorded separately. The build and MPO values printed on your machine were not recorded.
- Overlay docking on all four edges (3.4, approved by you 2026-10-09, PR #35):
  - Every docked edge is placed with `SetWindowPos` to `geometry::docked_rect`. No edge registers an AppBar or clips the cursor, so the work area is never changed and the mouse moves under the panel on every edge. `WinHost::update_clip` releases the clip in fullscreen and docked.
  - Panel size is limited to 10–90% (`cv_core::PANEL_SIZE_MIN`, `PANEL_SIZE_MAX`): `sanitize` clamps to them, so a saved value outside loads clamped, and the panel slider uses the same range. Default stays 50.
  - `appbar::register`, `ABM_NEW` and `WinHost::callback_msg` are unused and marked `#[allow(dead_code)]` until 3.5 removes the AppBar path.
  - 2 new fake-host tests: the system cursor hides inside and shows outside the panel on Bottom, Left and Right at the boundary pixels; switching edges with the pointer inside hides it again.
  - You ran the step's whole Verify list (each edge flush with no shrinking, mouse under the panel, cursor hidden inside, taskbar clicks under a bottom panel, edge switching with the pointer inside, slider 10–90 resizing live, saved 5 and 95 loading as 10 and 90, toggle off while docked, close while docked) and reported that all of it works; individual items were not itemised, so whether the taskbar draws over a bottom panel after a click is not recorded separately. No `HWND_TOPMOST` re-assert was added.
- AppBar docking and ClipCursor removed (3.5, approved by you 2026-10-09, PR #36):
  - appbar.rs is gone, along with the AppBar callback message, the `ABN_POSCHANGED` flag and path, `WinHost::reposition_appbar` and `update_clip`, `update_clip_cursor`, `notify_moved` in `move_window`, `Magnifier::resized` (and its test) and the `Win32_UI_Shell` feature. No setting was removed.
  - No `ClipCursor` call is left anywhere, exit included (your approval of the plan): nothing sets a clip, and `ClipCursor(None)` is system-wide, so it would free a clip another app set. 3.4 called it every tick while magnifying; that stopped too. Teardown and the `OverlayHandle::shutdown` fallback show the system cursor and destroy the window.
  - CLAUDE.md describes overlay docking.
  - You ran the step's Verify list (each edge flush with no shrinking, mouse under the panel, cursor hidden inside, edge and size changes live, taskbar clicks under a bottom panel, fullscreen cursor hidden and every edge reachable, toggle on and off, exit by window close, Ctrl+C and console close while magnifying) and reported that all of it works; individual items were not itemised, so whether the optional check (another app's cursor confinement kept) was tried is not recorded.
- Caret probe (4.1, approved by you 2026-10-09, PR #37):
  - `cargo run -p cv-platform-win --example caret_probe` (crates/cv-platform-win/examples/caret_probe.rs, dev-only, not shipped). It reads three caret sources for the foreground window every 100 ms:
    - gui: `GetGUIThreadInfo` `rcCaret` mapped with `ClientToScreen`;
    - msaa: `AccessibleObjectFromWindow(focus window, OBJID_CARET)` then `accLocation`;
    - uia: the focused element's `TextPattern2.GetCaretRange`, else the first `TextPattern.GetSelection` range; an empty range is expanded to one character.
  - Each source's rectangle is outlined on screen (red gui, green msaa, blue uia) by one small topmost, click-through window per source on its own thread. Once a second it prints a block: the foreground process and pointer, then each source's rect in physical pixels, method, caret window DPI and awareness, UIA framework and class, and cost in ms.
  - Its extra `windows` features (`Win32_UI_Accessibility`, `Win32_System_Com`, `Win32_System_Ole`, `Win32_System_Variant`) are cv-platform-win dev-dependencies; `cargo tree -p app -e features -i windows@0.62.2` shows none of them in the app build.
  - What you checked by hand:
    - Notepad: all three outlines on the caret.
    - Windows Terminal: only blue (uia); it draws its own caret.
    - Once the outlines were redrawn every 100 ms, they "follow nicely". Before that, with redraws once a second, you found them laggy.
    - A game no longer draws over the taskbar while the probe runs. The first outline version caused that (see Finding 13).
    - Individual Verify items beyond these were not itemised.
- Caret sources measured (4.2, run by you 2026-10-09). Details are in docs/prototypes/caret-sources.md.
  - Notepad and File Explorer rename: gui, msaa and uia are all on the caret.
  - Brave and Edge: msaa and uia, no gui.
  - Windows Terminal: uia only.
  - File Explorer address bar and search box (XAML `TextBox`): none. uia finds an active caret range but gives no rectangle, even mid-text.
  - LibreOffice: none.
  - Not measured: Word and Chrome (not installed), the tester's email app (on hold), elevated windows.
- Caret sources decided (4.3, accepted by you 2026-10-09, PR #39): ADR 0008 is Accepted. Choices: one core caret thread in cv-platform-win (MTA, no window, lowered UIA timeouts), woken by WinEvents with a 20 Hz poll only where no event covers the caret, one fixed chain msaa → uia → gui → focus rectangle, and in elevated windows without UIAccess a fall back to the mouse. It supersedes ADR 0003's rule that only the TTS thread adds or removes UIA handlers (ADR 0003's Status line is yours to change). It added 4.2a (you: Word and Chrome).
- Core caret source for Windows (4.4, approved by you 2026-10-09, PR #40):
  - cv-core `events`: `CoreEvent { CaretMoved, CaretLost }` (`#[non_exhaustive]`), `CaretSource { Msaa, Uia, Gui, FocusRect }`, `AppId { pid, exe }` and `EventHub`, a fan-out over unbounded `std::sync::mpsc` (`subscribe`, `publish` drops closed receivers). 6 tests.
  - cv-platform-win `spawn_caret_source(Arc<EventHub>) -> CaretHandle`: MTA COM, no window, no UIA event handlers, `CUIAutomation8` with connection and transaction timeouts of 500 ms (logged at startup). Out-of-context WinEvents (foreground, focus, caret show, hide and move; own process skipped) wake one read of the chain. A 20 Hz poll runs after uia or the focus rectangle answered, until focus or foreground changes. gui is converted to physical pixels in the caret window's DPI context.
  - The focus rectangle is used only when the focused control has a caret but gave no position (a UIA text element without a caret rect, or a caret window with an empty rect); other focused controls give no caret. With the app's own window in the foreground no source is read.
  - Events only on change. `[caret] <source> in <exe>` on a source change and `[caret] no source in <exe>` at most once per foreground change. The pure logic is caret/tracker.rs with 14 tests.
  - app creates the hub, starts the thread and stops it after the overlay (`PostThreadMessageW(WM_QUIT)`, unhook, join with a 1 s deadline). Nothing subscribes yet; the magnifier is unchanged.
  - The UIA and MSAA `windows` features are in the workspace list now, so the app compiles them. `cargo tree -i windows@0.62.2 --workspace -e normal --depth 1` is unchanged.
  - Dev example `cargo run -p cv-platform-win --example caret_events` prints each event and outlines its rect; the outline code is shared with the probe (examples/common/outline.rs).
  - You ran the step's Verify list (the log line per app: Notepad, Explorer rename, address bar and search box, Edge or Brave, Terminal, LibreOffice; the settings panel with a text field focused; outlines at 150% in Notepad, Explorer rename and Edge; the three exit paths) and reported that it works; individual items were not itemised. So the Terminal poll CPU cost is not recorded, and no msaa or uia offset at 150% was reported, so no conversion was added for them.
- Tracking policy (4.5, approved by you 2026-10-09, PR #41):
  - cv-core `tracking`: `Tracker::update(now, pointer, events, view) -> (f32, f32)` once per tick, `Tracker::following() -> Following { Pointer, Caret }`; re-exported from `cv_core`. Pure logic, not wired into the magnifier yet (4.6, 4.7). 21 tests.
  - A caret event takes the view only after the pointer has been still for 250 ms (`POINTER_QUIET`; movement of 2 px or less is jitter, `POINTER_JITTER_PX`). The pointer takes it back after moving more than 24 px (`POINTER_RETURN_PX`, either axis) from where it rested; a caret event re-anchors that point only while the pointer is still. The pointer is read before the events, so the pointer wins a tie.
  - Caret (msaa, uia, gui): the view holds while the caret is inside the middle of the view (`CARET_MARGIN` 0.2 each side) and re-centres per axis when it leaves. `FocusRect`: centred if it fits the middle area, else its left/top edge at the start of the middle area. `CaretLost` holds the view until the pointer moves or a caret returns.
  - You ran the step's Verify list (fullscreen and docked follow the mouse as before, clean exit) and reported that it works; individual items were not itemised.
- Old 2.3 (per-mode TTS toggles) was dropped, not approved. Its two commits are kept as docs/archive/old-2.3-tts-mode-toggles.patch and the step/tts-mode-toggles branch is deleted (local and GitHub; PR #6 was already closed).
- Verified by you by hand while approving old 2.1 (2026-10-08): the global toggle hotkey Ctrl+Alt+Shift+Z works (the old Win+= opened Windows Magnifier and was replaced); the cursor circle sits on the real pointer; smooth follow works; the zoom slider works; all four colour filters work; docked mode works; in fullscreen, moving the mouse to the second monitor moves the magnified view there. You said docking needs to change later; details to come (Finding 11).
- Verified by you by hand while approving old 2.2 (2026-10-08): settings are saved to `%APPDATA%\clear-view\settings.json` and restored on relaunch; deleting the file recreates it with defaults; an invalid file prints the parse error to the CLI, is moved to `settings.json.bad`, and defaults are used; `zoom` 99 and `panel_size` 0 load as 10 and 1; a change made more than a second before killing the process in Task Manager is kept; the file has no `enabled` key. `enabled` is deliberately never persisted, so the magnifier always starts off (your decision).

- `cargo build`: passes, 0 warnings (0.5).
- `cargo clippy --workspace --all-targets`: passes, 0 warnings, also with `--features app/tts` (0.5).
- `cargo test --workspace`: passes, 137 tests and 1 ignored (4.5).
  - cv-core 71: 21 in `geometry::tests` (including `docked_rect`), 23 in `tests` for serde round-trips, `sanitize`, `step_zoom`, `apply` and `ScreenRect::contains`, 6 in `events::tests` for the hub, 21 in `tracking::tests` for the tracking policy.
  - app 10: 6 in `settings::tests`, 4 in `app::tests` for `apply_changes`.
  - cv-magnifier 42: `gfx::tests::shader_parses_and_validates`, `uniform_size_matches_the_shader_struct`, 12 in `capture::tests` for the capture loop with a scripted fake source (including reconnect and factory retries), 28 in `magnifier::tests` for the tick with a fake host and renderer (including the docked system cursor on every edge), plus the ignored GPU bench `bench_modes_4k`.
  - cv-platform-win 14 in `caret::tracker::tests` (what the caret thread publishes, logs and polls). cv-tts has none.
- The workspace has 5 crates (cv-core, cv-platform-win, cv-magnifier, cv-tts, app), 4403 lines of Rust in total (`cat crates/*/src/*.rs | wc -l`, 2.5a).

## Broken

Nothing is proven broken by a command. Code-read defects that a run must confirm or clear are Findings 9 and 10.

## Not tested

Code is present and builds; you have not verified the behaviour. Anything you did verify is under "Works".

Magnifier (read from code, file paths given)
- DXGI capture details on a real device: `DXGI_ERROR_WAIT_TIMEOUT` retry, and reconnect after errors other than a lock or UAC prompt (mode change, device removed, remote session) (crates/cv-platform-win/src/capture.rs, loop in crates/cv-magnifier/src/capture.rs). The loop's logic is unit-tested with a fake source (2.4, 2.5a). Capture itself, output switching and recovery after a lock (2.5a) are seen working.
- Frame upload skipped when the `Arc<Frame>` is unchanged: crates/cv-magnifier/src/magnifier.rs (unit-tested with a fake renderer in 2.5, not measured on a GPU). The four colour filters and the four interpolation modes are seen working.
- System cursor hidden in fullscreen via `MagShowSystemCursor` (cv-platform-win/src/overlay.rs)

Reader stages (docs/later/tts-plan.md vs crates/cv-tts/src/lib.rs). No Verify list has been run.

| Stage | Code | Verify list | Result |
|---|---|---|---|
| 1 SAPI smoke test | Present: MTA init first (lib.rs:92), `ISpVoice`, unconditional "clear-view ready" (lib.rs:122), `Arc<AtomicBool>` shutdown joined in main.rs:94-95 | Hear the phrase; open and close without hang | Not run |
| 2 UIA bootstrap + logging | Present: `CUIAutomation8`, `GetPhysicalCursorPos`, `UIA_E_ELEMENTNOTAVAILABLE` handled (lib.rs:227-236), name dedupe | Sensible names in stdout on taskbar, Start, Notepad, browser | Not run. Logging now only happens when `tts_enabled` is true (lib.rs:195). |
| 3 Hover into SAPI | Present: `Speak` with `SPF_ASYNC \| SPF_PURGEBEFORESPEAK`, volume and rate applied only on change (lib.rs:199-209), egui toggle, hover checkbox, sliders | Hear names, toggle off is silent, sliders change voice, no speech while typing in Notepad | Not run. Turning TTS off does not purge speech already queued. |
| 4 Focus change detection | Present: `AddFocusChangedEventHandler` on the TTS thread, `TextFocusGained` or `TextFocusLost` over mpsc, `TtsMode::{Idle, TextFocus}` logged to stdout, own-process events dropped, handler removed on shutdown | No Verify list in the plan; Tasks 1-4 and Notes are all in code | Not run |

## Missing

- Tests: none in cv-tts, and in cv-platform-win only for the caret tracker's pure logic. cv-core (geometry, state, event hub), app (settings, panel merge) and cv-magnifier (shader checks, capture loop, tick) have them.
- UIAccess manifest, build script or signing: no build.rs, no manifest, no match for "manifest" or "uiaccess" in the tree.
- Reader Stage 5 (selection), 6 (caret), 7 (typing echo), 8 (AppReader), 9 (IA2): no code.
- AppState fields from docs/later/tts-plan.md that do not exist: `tts_selection_enabled`, `tts_caret_enabled`, `tts_typing_enabled`, `tts_appreader_enabled`, `tts_granularity`, `tts_char_mode`, `tts_verbosity`. Only `tts_enabled`, `tts_hover_enabled`, `tts_volume`, `tts_rate` exist.
- docs/later/tts-plan.md architecture stubs: no `AccessibilityBackend`, `AppContext`, `describe_element`, `TtsVerbosity`, `TtsGranularity` or `HotkeyBinding` anywhere in crates/. Hover speaks the element name directly (lib.rs:253-262).
- Docked-mode support for a non-primary monitor (Finding 10).

## Findings

1. **Text focus silences hover again.** `TtsMode::TextFocus` is set when the focused element exposes `IUIAutomationTextPattern` (cv-tts/src/lib.rs:69-72) and hover is skipped in that mode (lib.rs:216). Commit 141750c removed the same predicate (`GetFocusedElement` + TextPattern check) because it "silenced hover any time a real app had focus, which is nearly always". Stage 4 brought it back event-driven; the predicate is unchanged, so the risk is unchanged. Two more facts from the code: focus events from our own process are dropped (lib.rs:62-66), so clicking the settings panel while in `TextFocus` leaves the mode at `TextFocus`; and mode starts `Idle` with no initial focus query (lib.rs:161), so an already-focused text box is not seen until focus moves. Nothing else speaks in `TextFocus` until Stage 5 to 7.
2. **The repaint thread's stated cause was wrong. Resolved by 0.3.** The old comment said the write lock from `update()` "is never released"; the guard drops when the `CentralPanel::show` closure returns. It also said `request_repaint_after` inside `update()` is dropped as stale. In eframe 0.29.1 the pass-number check runs when the `RequestRepaint` event arrives, not when the delay ends, so it is accepted. The thread and the polling are removed. The real cause of speech stopping with the panel unfocused (if there was one) is still unproven; speech is parked.
3. **`update()` took `state.write()` every frame. Resolved by 0.4.** The panel now snapshots under a read lock, edits a local copy and writes back only changed fields (app.rs `apply_changes`). Side effect from egui 0.29.1: `Slider` re-applies `step_by` rounding every frame, so an off-grid value from a hand-edited settings file is snapped and saved once.
4. **Capture allocates a full frame per captured frame.** `read_staging` does `vec![0u8; w*h*4]` and a row copy each time (cv-platform-win/src/capture.rs:188-213): about 14.7 MB at 1440p, 33 MB at 4K. Performance is unmeasured. Measured by the 3.1 prototype:
   - While the mouse moves, 150–210 frames a second are pointer-only (`LastPresentTime` 0): the desktop image is unchanged, but the full frame is still copied and uploaded.
   - That takes the process from about 15% of one core to 100–170%, with the magnifier off or docked.
   - Fullscreen hides the system cursor, so it gets none.
   - Skipping the copy and publish for pointer-only frames would remove it (roadmap 5.6, or earlier if you choose).
5. **Platform code is not isolated.** crates/cv-render/src/lib.rs mixes the Win32 window, AppBar callback, ClipCursor, Mag cursor, monitor switching and timer with uniform write caching. The pure crop, zoom, lerp and cursor-mapping math moved to cv-core::geometry in old 2.1. Correction to the old claim: gfx.rs is not fully portable, because `WgpuState::new` takes a Win32 `HWND` and builds a `Win32WindowHandle` (gfx.rs:3-6, 28-42). shader.wgsl is portable. cv-core imports only `parking_lot` and `std`. This is the seam ADR 0004 must cut. ADR 0004 is Accepted (2.1); the split is roadmap 2.2 to 2.5. After 2.3: gfx.rs takes raw handles and lives in cv-magnifier; the mixed file is now cv-platform-win/src/overlay.rs, still mixed until 2.5 untangles `on_timer`. After 2.4: the capture loop is in cv-magnifier behind `CaptureSource`, and the seam traits exist. Resolved by 2.5: `on_timer` is gone; the per-tick logic is `Magnifier::tick` in cv-magnifier, and overlay.rs keeps only the Win32 side (`WinHost`, the loop, teardown).
6. **`shaders/magnify.hlsl` was referenced by nothing** (no match in .rs, .toml or .wgsl). Resolved by old 1.1: moved to docs/archive/magnify.hlsl.
7. **Old dependencies.** Cargo.toml pins wgpu 22 and egui/eframe 0.29. The upgrade touches the same code as the platform split, so it needs a decision in an ADR. Decided by ADR 0004 (2.1): after the refactor, before 3.1, as roadmap 2.6 (wgpu 30) and 2.7 (eframe 0.36, needs Rust 1.95; local toolchain is 1.93.1). wgpu resolved by 2.6 (wgpu 30.0.1, PR #30). Resolved by 2.7: eframe and egui 0.36.2 (PR #31), toolchain Rust 1.99.0.
8. **Docs.** Old 1.1 moved PLAN.md, PHASE2PLAN.md, handoff.md and shaders/magnify.hlsl to docs/archive/ (merged). tts-plan.md now lives in docs/later/ (roadmap 0.1) and deliberately has no stage checkboxes; completion is recorded in the Done list above. Two CLAUDE.md claims do not match the code: it lists `ABM_ACTIVATE`, but appbar.rs sends ABM_NEW, QUERYPOS, SETPOS, WINDOWPOSCHANGED and REMOVE only; and it says the AppBar is released "on app exit" (see 9). Both fixed in CLAUDE.md by 0.7.
9. **No exit cleanup path. Resolved by 1.1 for normal exit, Ctrl+C, console close and panic; kill works in your test.** `cv_render::spawn_overlay` returns an `OverlayHandle`; `shutdown()` posts `WM_CLOSE`, and the render thread runs `teardown` (AppBar remove, cursor, clip, `DestroyWindow`) from a `Drop` guard. A console control handler does the same for Ctrl+C and console close. When the process is killed in Task Manager no code can run and the Microsoft docs for ClipCursor, MagShowSystemCursor and ABM_REMOVE do not say what happens; you tested it by hand (fullscreen and docked, per the Verify list) and reported that it works, so no restore-on-next-start or watchdog is planned. The result is not itemised, so a different kill path (for example a crash of the whole session) is not covered. The original finding: `appbar::unregister`, `MagShowSystemCursor(true)` and `update_clip_cursor(false)` run only after `GetMessageW` returns (cv-render/src/lib.rs:176-194). Nothing posts `WM_QUIT` or `WM_CLOSE` or destroys the overlay window (grep for `WM_CLOSE|WM_QUIT|PostThreadMessage|DestroyWindow|impl Drop` in crates/: no hits), and the render thread is detached (app/src/main.rs:63). When eframe returns, main returns (main.rs:94-97) and the process ends. Whether the work area, system cursor and cursor clip are restored on exit therefore depends on Windows, not on this code.
10. **Docked mode on a second monitor looks unsupported.** Since 3.4 docking uses `geometry::docked_rect` (monitor origin included) and no work-area clip, so the code points below about `panel_rect` and `SPI_GETWORKAREA` no longer apply; the panel still stays on the monitor active when the layout was applied (roadmap 5.1, 5.2). Original: `appbar::panel_rect` builds rects from origin (0,0) using the active monitor's width and height (appbar.rs:36-43) and ignores `monitor_left`/`monitor_top`. `update_clip_cursor` uses `SPI_GETWORKAREA`, which is the primary monitor's work area (lib.rs:567-578). While docked, a monitor switch updates `screen_w`/`screen_h` and the capture target but does not move the window (lib.rs:452-463). Fullscreen follow across monitors is implemented, and you saw it work.
11. **Docking needs to change.** You tested docked mode while approving old 2.1: it works, but you want it changed. Finding 10 is related. Details from you while testing 2.5 (2026-10-09):
    - The AppBar reserves the panel's share of the screen, so everything else is squeezed into what is left. At 20% panel size the desktop and maximised windows get 80%.
    - A window that cannot be resized, such as a fullscreen application, stays at full size and the panel covers part of it.
    - The mouse cannot go past the panel (the cursor clip to the work area, Finding 12), so you cannot pan to what is behind it.
    - What you want: the desktop stays at 100%, the panel sits on top of it, and moving the mouse under the panel pans the magnified view there ("a fake pan underneath it").

    This is PRODUCT.md v1 Must 2 ("Docked mode, overlay style") and roadmap Phase 3: the 3.1 prototype, then the 3.2 ADR (docked overlay design, superseding the AppBar part of ADR 0002), then 3.3 to 3.5. Use these points as input to the 3.2 ADR. The 3.1 prototype did all four points on your machine (docs/prototypes/docked-overlay.md). Since 3.4 all four edges are overlay-docked and you verified the four points by hand. Resolved by 3.5 (PR #36): the AppBar code and the cursor clip are removed.
12. **Mouse cannot reach the taskbar. Resolved by 3.4 (PR #35):** no docked edge clips the cursor any more, and you reported taskbar clicks under a bottom panel working. The original finding: you reported it while testing 1.1: "like an invisible wall". You remember it appearing before 1.1, and 1.1 did not touch the cursor clip logic, so it is not from that work. Cause not found. Which mode it happens in was not recorded. A candidate from the code, unconfirmed: while docked, `update_clip_cursor` clips the cursor to `SPI_GETWORKAREA` every tick (cv-render/src/lib.rs), and the work area excludes the taskbar. Not on the roadmap yet; it overlaps Finding 11 and roadmap 3.5 (remove ClipCursor). You decide where it goes. Update from testing 1.4: you confirmed it happens in docked mode, which matches the candidate above (the cursor clip to the work area). Fullscreen is not affected. It goes with roadmap 3.5 (remove AppBar docking and ClipCursor); no separate item. The 3.1 prototype, without `ClipCursor`, let the mouse reach the taskbar under a bottom panel (you checked it), which confirms the cause.
13. **Taskbar disappears in fullscreen with a game open. Not a bug, ignored by your decision.** Reported while testing 1.4. You said it only happens in fullscreen when something like a game is open, and to ignore it. Not investigated. The earlier candidates (shell hiding the taskbar behind a topmost fullscreen overlay, or the taskbar being out of view at high zoom) were never confirmed. Likely cause, seen with the 4.1 caret probe (PR #37):
    - The probe's first outline window was topmost and covered the whole virtual screen. While it ran, a game you had open drew over the taskbar.
    - Replacing it with small windows that never cover the monitor fixed that (you checked).
    - So a topmost window covering a monitor makes the shell treat that monitor as running a full-screen app and drop the taskbar's topmost place. The magnifier's fullscreen overlay is such a window.
    - The magnifier is unchanged; you still decide whether this matters.
14. **Image is blurry at high zoom. Partly addressed by 1.5.** You reported it while testing 1.4: "quite blurry but not terrible", wants smoothing or sharpening. 1.5 added the Sharp mode (ADR 0006). Testing it, you found it "definitely sharpens it", but most text is anti-aliased, so Sharp only makes the soft edge pixels sharper. Edge smoothing is roadmap 1.9 (cleanEdge), after tester feedback (1.8); if cleanEdge fails on anti-aliased text, a multi-pass Super-xBR pipeline needs its own ADR after ADR 0004. You kept 1.9 in its place after 1.8 and chose Bicubic as the default. Testing 1.9 (PR #23, approved as built, needs more work): cleanEdge does not lag but "just makes things rounder" and is not as clean as ZoomText; on anti-aliased text it rounds the grey edge pixels instead of using them. Notes on ZoomText xFont, redrawn text and the next filters to try (contour sharpening first, as toggleable enhancements) are in docs/later/text-smoothing.md; they need an ADR before code.
15. **The view jumps to the top-left when the pointer cannot be read.** `on_timer` uses `CursorPointer.position().unwrap_or((0, 0))` (cv-platform-win/src/overlay.rs). The fallback was kept on purpose in 2.4 so behaviour did not change; before 2.4 the code ignored the `GetCursorPos` error and used a zeroed `POINT`. `GetCursorPos` fails when the input desktop is not the current desktop ([Microsoft docs](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getcursorpos)), for example on the lock screen or a UAC prompt. Then the smoothed view drifts toward (0, 0) and snaps back when input returns. Not seen by you, read from code. Keeping the last known position is the likely fix; it fits 2.5, where the pointer is read through `OverlayHost`, or Phase 4 tracking. Fixed on the 2.5 branch (PR #28): a failed read keeps the last position. Resolved: with Finding 16 fixed by 2.5a, you ran the unlock check and reported that it works (not itemised separately).
16. **Capture stops for good after the PC is locked. Resolved by 2.5a (PR #29):** a failed reconnect is retried every 250 ms, and `Capturer::recreate` releases the lost duplication first. You tested it and reported that it works. The original finding: you reported it while testing 2.5: unlocking with fullscreen magnification on shows a black view with the cursor circle until you turn the magnifier off. The cause is in the code, and it was there before 2.5:
    - Locking switches to the secure desktop, so `AcquireNextFrame` returns `DXGI_ERROR_ACCESS_LOST` ([docs](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutputduplication-acquirenextframe): "Desktop switch").
    - The capture loop calls `reconnect`, which calls `DuplicateOutput`. While the secure desktop is up, that fails with `E_ACCESSDENIED` ([docs](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutput1-duplicateoutput): "only an application that runs at LOCAL_SYSTEM can access the secure desktop").
    - A failed reconnect ends the loop (`run_capture` in crates/cv-magnifier/src/capture.rs: `if source.reconnect().is_err() { break; }`), so the capture thread exits and never restarts. The renderer keeps drawing the last frame it got, which is black from the lock transition.
    - The same `break` was in main.rs before 2.4 moved the loop, so this is not a 2.5 regression. The 2.4 test `failed_reconnect_ends_the_loop` locks the behaviour in. The console should show one `[capture] access lost: ... — reconnecting` line and nothing after it.
    - Likely fix: keep retrying `reconnect` with a short sleep instead of ending the thread. The same applies to UAC prompts, and to `DXGI_ERROR_UNSUPPORTED` and `DXGI_ERROR_SESSION_DISCONNECTED` (remote desktop). Roadmap 2.5a (your choice: a separate item, not part of 2.5).

Git (collected by command)
- Branch: master at 89cc84c, equal to origin/master and to feat/tts-stage4. Nothing unpushed.
- Uncommitted: `M CLAUDE.md`, `M clear-view.zip` (binary, tracked, 67456 → 68831 bytes), untracked `.claude/commands/` and `docs/`.
- `git branch --no-merged master` is empty: every local branch is merged. There are no unmerged dead-end branches.
- Merged branches that add nothing and are candidates for you to delete. All deleted locally by 0.8:

| Branch | Last commit |
|---|---|
| phase-1 | 2026-03-08 |
| phase-2, phase-3, phase-4 | 2026-03-09 |
| phase-5, phase-6 | 2026-03-12 |
| fix/uniform-caching | 2026-03-09 |
| fix/cursor-coordinate-space, fix/cursor-hide-counter, fix/panel-size-percent | 2026-03-12 |
| fix/clipcursor-docked, fix/clipcursor-every-tick | 2026-03-13 |
| feat/tts-stage1, feat/tts-stage2 | 2026-03-15 |
| feat/tts-stage3 | 2026-03-16 |
| feat/tts-stage4 (same commit as master) | 2026-05-05 |
| tts-main (stale integration branch, 2 commits behind master at faf2cec) | 2026-03-16 |

- Commit 89cc84c, "ehh finish later, to come back to", is the tip of master. It added tts-plan.md and changed CLAUDE.md, Cargo.lock and the zip. The Stage 4 code is in 0a96e34.

## Needs your run (Windows)

Mark each: works, broken, or not tested.

- [x] Ctrl+Alt+Shift+Z toggles on and off (confirmed in fullscreen and docked while approving old 2.1; each docked edge not itemised)
- [x] Docked: work area unchanged on enable, disable and exit (no AppBar on any edge since 3.4; toggle off and close while docked reported working in 3.4, not itemised)
- [x] After exiting the app while magnifying fullscreen: system cursor is visible, cursor is not clipped (Finding 9; covered by 1.1 and again by 3.5's exit checks, reported working, not itemised)
- [x] After killing the process in Task Manager, fullscreen and docked: nothing left behind (reported working with 1.1, not itemised)
- [x] Mouse reaches the taskbar in each mode (Finding 12; docked fixed by 3.4, taskbar clicks under a bottom panel reported working; fullscreen was never affected)
- [x] Zoom slider, follow speed slider and all four colour filters (confirmed by you while approving old 2.1)
- [ ] Magnifier starts off after a relaunch that restores saved settings (old 2.2; implied by `enabled` not being saved, not watched)
- [x] Bilinear vs bicubic vs sharp at 10x and 20x (1.5; you chose Bicubic as the default)
- [x] Smooth edges (cleanEdge) at 10x and 20x (1.9; no lag, rounder but not cleaner, needs more work)
- [x] Cursor circle lines up with the real pointer at 100%, 125%, 150% scaling (1.6; single monitor, scale changed while running)
- [x] Second monitor, fullscreen: active monitor switches, circle and capture follow (confirmed while approving old 2.1)
- [ ] Second monitor, docked: where the panel lands and where the cursor can go (Finding 10)
- [ ] Idle CPU and GPU use with the magnifier on, at native resolution
