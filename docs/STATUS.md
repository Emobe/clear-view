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
- Old 2.3 (per-mode TTS toggles) was dropped, not approved. Its two commits are kept as docs/archive/old-2.3-tts-mode-toggles.patch and the step/tts-mode-toggles branch is deleted (local and GitHub; PR #6 was already closed).
- Verified by you by hand while approving old 2.1 (2026-10-08): the global toggle hotkey Ctrl+Alt+Shift+Z works (the old Win+= opened Windows Magnifier and was replaced); the cursor circle sits on the real pointer; smooth follow works; the zoom slider works; all four colour filters work; docked mode works; in fullscreen, moving the mouse to the second monitor moves the magnified view there. You said docking needs to change later; details to come (Finding 11).
- Verified by you by hand while approving old 2.2 (2026-10-08): settings are saved to `%APPDATA%\clear-view\settings.json` and restored on relaunch; deleting the file recreates it with defaults; an invalid file prints the parse error to the CLI, is moved to `settings.json.bad`, and defaults are used; `zoom` 99 and `panel_size` 0 load as 10 and 1; a change made more than a second before killing the process in Task Manager is kept; the file has no `enabled` key. `enabled` is deliberately never persisted, so the magnifier always starts off (your decision).

- `cargo build`: passes, 0 warnings (0.5).
- `cargo clippy --workspace --all-targets`: passes, 0 warnings, also with `--features app/tts` (0.5).
- `cargo test --workspace`: passes, 37 tests. cv-core 27 (17 in `geometry::tests`, 10 in `tests` for serde round-trips and `sanitize`), app 10 (6 in `settings::tests`, 4 in `app::tests` for `apply_changes`). cv-capture, cv-render and cv-tts have none.
- The workspace has 5 crates (cv-core, cv-capture, cv-render, cv-tts, app), 2508 lines of Rust in total (counted by `wc -l` on crates/**/*.rs; the earlier figure of 2928 was not reproduced).

## Broken

Nothing is proven broken by a command. Code-read defects that a run must confirm or clear are Findings 9 and 10.

## Not tested

Code is present and builds; you have not verified the behaviour. Anything you did verify is under "Works".

Magnifier (read from code, file paths given)
- DXGI capture details: `DXGI_ERROR_WAIT_TIMEOUT` retry and reconnect on error (crates/cv-capture/src/lib.rs). Capture itself and output switching are seen working.
- Bilinear vs Catmull-Rom bicubic, frame upload skipped when the `Arc<Frame>` is unchanged: crates/cv-render/src/lib.rs, gfx.rs, shader.wgsl. The four colour filters are seen working.
- System cursor hidden in fullscreen via `MagShowSystemCursor` (lib.rs:391)
- `ClipCursor` to the work area every tick while docked (lib.rs:472)
- AppBar details on each of the four edges, and unregister on toggle-off (appbar.rs, lib.rs:324). Docking in general is seen working; which edges you tried is not recorded.
- egui panel controls not yet verified: display mode and panel size beyond what docking showed, interpolation: crates/app/src/app.rs. Zoom, follow speed and colour filter are seen working.

Reader stages (docs/later/tts-plan.md vs crates/cv-tts/src/lib.rs). No Verify list has been run.

| Stage | Code | Verify list | Result |
|---|---|---|---|
| 1 SAPI smoke test | Present: MTA init first (lib.rs:92), `ISpVoice`, unconditional "clear-view ready" (lib.rs:122), `Arc<AtomicBool>` shutdown joined in main.rs:94-95 | Hear the phrase; open and close without hang | Not run |
| 2 UIA bootstrap + logging | Present: `CUIAutomation8`, `GetPhysicalCursorPos`, `UIA_E_ELEMENTNOTAVAILABLE` handled (lib.rs:227-236), name dedupe | Sensible names in stdout on taskbar, Start, Notepad, browser | Not run. Logging now only happens when `tts_enabled` is true (lib.rs:195). |
| 3 Hover into SAPI | Present: `Speak` with `SPF_ASYNC \| SPF_PURGEBEFORESPEAK`, volume and rate applied only on change (lib.rs:199-209), egui toggle, hover checkbox, sliders | Hear names, toggle off is silent, sliders change voice, no speech while typing in Notepad | Not run. Turning TTS off does not purge speech already queued. |
| 4 Focus change detection | Present: `AddFocusChangedEventHandler` on the TTS thread, `TextFocusGained` or `TextFocusLost` over mpsc, `TtsMode::{Idle, TextFocus}` logged to stdout, own-process events dropped, handler removed on shutdown | No Verify list in the plan; Tasks 1-4 and Notes are all in code | Not run |

## Missing

- Tests: only cv-core (geometry and state) and app (settings, panel merge). None in cv-capture, cv-render or cv-tts.
- UIAccess manifest, build script or signing: no build.rs, no manifest, no match for "manifest" or "uiaccess" in the tree.
- Zoom in/out hotkeys: hotkey.rs registers only Ctrl+Alt+Shift+Z (`HOTKEY_ID = 1`). A failed registration only prints to stdout; the settings window does not show it.
- Reader Stage 5 (selection), 6 (caret), 7 (typing echo), 8 (AppReader), 9 (IA2): no code.
- AppState fields from docs/later/tts-plan.md that do not exist: `tts_selection_enabled`, `tts_caret_enabled`, `tts_typing_enabled`, `tts_appreader_enabled`, `tts_granularity`, `tts_char_mode`, `tts_verbosity`. Only `tts_enabled`, `tts_hover_enabled`, `tts_volume`, `tts_rate` exist.
- docs/later/tts-plan.md architecture stubs: no `AccessibilityBackend`, `AppContext`, `describe_element`, `TtsVerbosity`, `TtsGranularity` or `HotkeyBinding` anywhere in crates/. Hover speaks the element name directly (lib.rs:253-262).
- Docked-mode support for a non-primary monitor (Finding 10).

## Findings

1. **Text focus silences hover again.** `TtsMode::TextFocus` is set when the focused element exposes `IUIAutomationTextPattern` (cv-tts/src/lib.rs:69-72) and hover is skipped in that mode (lib.rs:216). Commit 141750c removed the same predicate (`GetFocusedElement` + TextPattern check) because it "silenced hover any time a real app had focus, which is nearly always". Stage 4 brought it back event-driven; the predicate is unchanged, so the risk is unchanged. Two more facts from the code: focus events from our own process are dropped (lib.rs:62-66), so clicking the settings panel while in `TextFocus` leaves the mode at `TextFocus`; and mode starts `Idle` with no initial focus query (lib.rs:161), so an already-focused text box is not seen until focus moves. Nothing else speaks in `TextFocus` until Stage 5 to 7.
2. **The repaint thread's stated cause was wrong. Resolved by 0.3.** The old comment said the write lock from `update()` "is never released"; the guard drops when the `CentralPanel::show` closure returns. It also said `request_repaint_after` inside `update()` is dropped as stale. In eframe 0.29.1 the pass-number check runs when the `RequestRepaint` event arrives, not when the delay ends, so it is accepted. The thread and the polling are removed. The real cause of speech stopping with the panel unfocused (if there was one) is still unproven; speech is parked.
3. **`update()` took `state.write()` every frame. Resolved by 0.4.** The panel now snapshots under a read lock, edits a local copy and writes back only changed fields (app.rs `apply_changes`). Side effect from egui 0.29.1: `Slider` re-applies `step_by` rounding every frame, so an off-grid value from a hand-edited settings file is snapped and saved once.
4. **Capture allocates a full frame per captured frame.** `read_staging` does `vec![0u8; w*h*4]` and a row copy each time (cv-capture/src/lib.rs:188-213): about 14.7 MB at 1440p, 33 MB at 4K. Performance is unmeasured.
5. **Platform code is not isolated.** crates/cv-render/src/lib.rs mixes the Win32 window, AppBar callback, ClipCursor, Mag cursor, monitor switching and timer with uniform write caching. The pure crop, zoom, lerp and cursor-mapping math moved to cv-core::geometry in old 2.1. Correction to the old claim: gfx.rs is not fully portable, because `WgpuState::new` takes a Win32 `HWND` and builds a `Win32WindowHandle` (gfx.rs:3-6, 28-42). shader.wgsl is portable. cv-core imports only `parking_lot` and `std`. This is the seam ADR 0004 must cut.
6. **`shaders/magnify.hlsl` was referenced by nothing** (no match in .rs, .toml or .wgsl). Resolved by old 1.1: moved to docs/archive/magnify.hlsl.
7. **Old dependencies.** Cargo.toml pins wgpu 22 and egui/eframe 0.29. The upgrade touches the same code as the platform split, so it needs a decision in an ADR.
8. **Docs.** Old 1.1 moved PLAN.md, PHASE2PLAN.md, handoff.md and shaders/magnify.hlsl to docs/archive/ (merged). tts-plan.md now lives in docs/later/ (roadmap 0.1) and deliberately has no stage checkboxes; completion is recorded in the Done list above. Two CLAUDE.md claims do not match the code: it lists `ABM_ACTIVATE`, but appbar.rs sends ABM_NEW, QUERYPOS, SETPOS, WINDOWPOSCHANGED and REMOVE only; and it says the AppBar is released "on app exit" (see 9). Both fixed in CLAUDE.md by 0.7.
9. **No exit cleanup path. Resolved by 1.1 for normal exit, Ctrl+C, console close and panic; kill works in your test.** `cv_render::spawn_overlay` returns an `OverlayHandle`; `shutdown()` posts `WM_CLOSE`, and the render thread runs `teardown` (AppBar remove, cursor, clip, `DestroyWindow`) from a `Drop` guard. A console control handler does the same for Ctrl+C and console close. When the process is killed in Task Manager no code can run and the Microsoft docs for ClipCursor, MagShowSystemCursor and ABM_REMOVE do not say what happens; you tested it by hand (fullscreen and docked, per the Verify list) and reported that it works, so no restore-on-next-start or watchdog is planned. The result is not itemised, so a different kill path (for example a crash of the whole session) is not covered. The original finding: `appbar::unregister`, `MagShowSystemCursor(true)` and `update_clip_cursor(false)` run only after `GetMessageW` returns (cv-render/src/lib.rs:176-194). Nothing posts `WM_QUIT` or `WM_CLOSE` or destroys the overlay window (grep for `WM_CLOSE|WM_QUIT|PostThreadMessage|DestroyWindow|impl Drop` in crates/: no hits), and the render thread is detached (app/src/main.rs:63). When eframe returns, main returns (main.rs:94-97) and the process ends. Whether the work area, system cursor and cursor clip are restored on exit therefore depends on Windows, not on this code.
10. **Docked mode on a second monitor looks unsupported.** `appbar::panel_rect` builds rects from origin (0,0) using the active monitor's width and height (appbar.rs:36-43) and ignores `monitor_left`/`monitor_top`. `update_clip_cursor` uses `SPI_GETWORKAREA`, which is the primary monitor's work area (lib.rs:567-578). While docked, a monitor switch updates `screen_w`/`screen_h` and the capture target but does not move the window (lib.rs:452-463). Fullscreen follow across monitors is implemented, and you saw it work.
11. **Docking needs to change.** You tested docked mode while approving old 2.1: it works, but you want it changed. Details to come from you; nothing is planned or on the roadmap yet. Finding 10 is related.
12. **Mouse cannot reach the taskbar.** You reported it while testing 1.1: "like an invisible wall". You remember it appearing before 1.1, and 1.1 did not touch the cursor clip logic, so it is not from that work. Cause not found. Which mode it happens in was not recorded. A candidate from the code, unconfirmed: while docked, `update_clip_cursor` clips the cursor to `SPI_GETWORKAREA` every tick (cv-render/src/lib.rs), and the work area excludes the taskbar. Not on the roadmap yet; it overlaps Finding 11 and roadmap 3.5 (remove ClipCursor). You decide where it goes.

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
- [ ] Docked: work area shrinks on enable, is restored on disable, and is restored on exit (Finding 9; covered by 1.1, reported working, not itemised)
- [ ] After exiting the app while magnifying fullscreen: system cursor is visible, cursor is not clipped (Finding 9; covered by 1.1, reported working, not itemised)
- [x] After killing the process in Task Manager, fullscreen and docked: nothing left behind (reported working with 1.1, not itemised)
- [ ] Mouse reaches the taskbar in each mode (Finding 12: it does not in some case)
- [x] Zoom slider, follow speed slider and all four colour filters (confirmed by you while approving old 2.1)
- [ ] Magnifier starts off after a relaunch that restores saved settings (old 2.2; implied by `enabled` not being saved, not watched)
- [ ] Bilinear vs bicubic
- [ ] Cursor circle lines up with the real pointer at 100%, 125%, 150% scaling (confirmed on your current display scaling only)
- [x] Second monitor, fullscreen: active monitor switches, circle and capture follow (confirmed while approving old 2.1)
- [ ] Second monitor, docked: where the panel lands and where the cursor can go (Finding 10)
- [ ] Idle CPU and GPU use with the magnifier on, at native resolution
