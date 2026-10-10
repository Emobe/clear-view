# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

Stacking: on (your instruction, 2026-10-10, until you say otherwise). Integration branch: step/4.6-fullscreen-tracking (PR #42 to master, which you merge after testing everything bit by bit). Unmerged work does not block /next. Each new /step or /adr item gets its own step/<number>-<name> branch created from the integration branch; its PR targets the integration branch, and once the gate passes and the PR is open the agent merges it there (`gh pr merge <n> --merge`). Nothing goes into master by the agent. STATUS Done and the State/Item below change only after "I approve", so they lag the stack.
On the integration branch, coded but not yet approved:
- 4.6 wire the policy into fullscreen (PR #42 itself)
- 4.7 wire the policy into docked mode (PR #43, merged into the integration branch)
Also on the integration branch, approved: 5.1 (docs only, PR #44), 5.2 (PR #45) and 5.3 (PR #46), all approved 2026-10-10. 4.8 (you) is skipped while you cannot test. Also on it once PR #47 is merged: ADR 0009 (Accepted), from /adr 5.4.

What 5.3 built (STATUS "Works" has the full list): no alignment fix was needed for mixed scaling; each `[system] output N` startup line now gives the display scale and physical desktop rect. Finding 19 (msaa caret rect in DPI-unaware or system-aware apps, undocumented) is open for a by-hand check with `caret_probe`.

State: blocked-on-adr
Item: 5.4 Monitors connected or disconnected while running. Branch step/5.4-<name> from the integration branch.
ADR: 0009 Monitors connected or disconnected while running (docs/adr/0009-display-changes.md), Accepted by you 2026-10-10 as recommended. 5.4 resumes with `/step 5.4`.
Model: Sonnet high (CLAUDE.md model guide for /step).

Notes for 5.4 (ADR 0009 as accepted: option 1, addressing A):
- Backend: `wnd_proc` sets an atomic flag on `WM_DISPLAYCHANGE`; the loop in `run_overlay` sees it before the next tick, runs `enumerate_outputs()`, and on a changed list reprints the `[system] output N` lines (reuse sysinfo.rs `log_outputs`) and calls `magnifier.outputs_changed(list)`. Re-check about 1 s after the last message. Never call the magnifier from inside `apply_layout`.
- `Capturer::duplicate` (switch) reuses its old device; make it check `IDXGIFactory1::IsCurrent` and rebuild the device when stale. `recreate` already builds a new one.
- Magnifier (`View::outputs_changed`): replace `outputs`, recompute `primary`, re-pick `active` from the target (primary centre if the target is on no monitor), always rewrite `desired_output`, always recreate the frame texture and clear `last_frame`, and reset `applied` so the layout is applied again. `Applied` compares indices only, so without the reset a monitor that keeps its index but changes rect is never re-placed.
- Side fix that comes with it: a resolution change on the active monitor freezes the view today (`upload_frame` drops frames of another size; the texture is resized only on an index change).
- First thing to check by hand: whether `WM_DISPLAYCHANGE` arrives for a plug or unplug that keeps the resolution (undocumented). If it doesn't, stop and record it here; another signal is an ADR change.
- You cannot test right now: code and unit-test it with the fake host and renderer, and leave the plug and unplug checks in the Verify list (ADR 0009 lists them).
4.6 and 4.7 are coded on the integration branch and still wait for your test and "I approve"; their notes stay below.

What 4.5 built (PR #41, approved 2026-10-09), for 4.6 and 4.7:
- cv-core `tracking`: `Tracker::update(now, pointer, events, view) -> (f32, f32)`, once per tick; `Tracker::following() -> Following { Pointer, Caret }`. Re-exported from `cv_core`. `view` is the magnified area in screen pixels (window size / zoom). The return value is the virtual-screen point to ease toward: feed it to `geometry::lerp_toward` in place of `self.pointer` in `View::draw` (cv-magnifier/src/magnifier.rs); keep `smooth_speed`.
- Rules and values (starting points; 4.8 tunes them): caret takes the view only after the pointer has been still 250 ms (`POINTER_QUIET`, jitter ≤ 2 px); pointer takes it back past 24 px (`POINTER_RETURN_PX`); caret margin 0.2 per side (`CARET_MARGIN`, 0.5 = always centre); `FocusRect` centred if it fits, else its left/top edge shown; `CaretLost` holds the view.
- `update` reads the pointer before the events. Pass the events drained this tick in order (`rx.try_iter()`); don't split them across calls.

Notes for 4.6:
- Subscribe a receiver from the hub (`events` in main.rs) for the render thread and pass it into `spawn_overlay` / the magnifier; drain with `try_recv` at the start of each tick (ADR 0008).
- The active monitor should follow the target, not the raw pointer: `follow_pointer` uses `self.pointer` today, so a caret on another monitor would not switch capture.
- The software cursor circle stays on the pointer, not the target.
- Decide whether the tracker runs while the magnifier is disabled (events still need draining so the channel does not grow).
- Fullscreen only; docked is 4.7 (its system-cursor hiding stays pointer-based).
- Verify by hand: Notepad, Explorer rename, address bar (focus rect), Edge or Brave, Terminal at 10x: typing follows, a mouse move returns, drag-selecting does not fight the mouse.

4.2a (you) can run any time before 4.8: the caret probe in desktop Word and in Chrome if installed (ROADMAP 4.2a).

On hold by your decision (2026-10-09): everything involving the tester, until you raise it: 1.8, 3.6, 4.9, 6.8, and the tester's email app in 4.2. /next skips them. 4.2 was recorded without the email app; that app is measured when you pick the tester work up again.

Held, for when you pick it up again — notes for 3.6:
- The agent's part, like 1.7 (PR #22): plain `cargo build --release` with no `RUSTFLAGS` gives `target\release\clear-view.exe` (static CRT from `.cargo/config.toml`); update TESTER.md; tag v0.2.0. v0.1.0 is an annotated tag ("Tester build 1 (roadmap 1.7)") on bb81f33; tag the merged commit the same way only after you approve, and ask before pushing the tag.
- TESTER.md line 29 still says "use Fullscreen. The docked modes … are old and being replaced; in them the mouse cannot reach the taskbar." That is wrong since 3.4/3.5: docking is an overlay on any edge, the desktop is not resized, the mouse moves under the panel (which shows that area magnified, cursor hidden inside it), panel size 10–90%.
- Your part (ADR 0007): before handing the build over, run it on the tester's machine and read the `[system] Windows … build …` and `[system] output N …: multiplane overlay support …` lines. If the build is older than 26100.2314 with MPO true, or the panel causes extra frames, present-only-on-change becomes a sub-item before 3.6 is done. The step should ask you for those values and record them.
- Feedback goes into docs/FEEDBACK.md (does not exist yet; 1.8 creates it too).
- Model: Sonnet high (CLAUDE.md model guide for /step).

What comes next:
- 3.5 (PR #36) finished ADR 0007: no AppBar, no `ClipCursor` anywhere (exit included), `Magnifier::resized` gone.
- 4.1 (PR #37) added the caret probe, cv-platform-win/examples/caret_probe.rs. Since 4.4 its `windows` features are normal ones (the caret thread needs them).
- 4.2 (you) measured the caret sources; results in docs/prototypes/caret-sources.md.
- 4.3 wrote ADR 0008 (Accepted) from those results; 4.4 to 4.8 build it.
- 4.4 (PR #40) built the caret thread and the core events; nothing consumes them yet.
- 4.5 (PR #41) added the tracking policy (cv-core `tracking`); not wired yet.
- 1.8 (you) is still on hold until the tester is free. When they are, `/step 1.8` records their feedback in docs/FEEDBACK.md. Ask which app they compare ZoomText in, whether ClearType is on, which ZoomText hotkeys they rely on and which email app they use (4.2).
- Contour sharpening and toggleable text enhancements (docs/later/text-smoothing.md) still have no roadmap item; adding a "1.10 (ADR first)" is your call.
- Side finding from 3.1: pointer-only capture frames are copied in full and cost about one CPU core while the mouse moves (STATUS Finding 4). Planned for 5.6; moving it earlier is your call.

Still true:
- Display scaling (1.6): the process is Per-Monitor v2, so all cursor, monitor, frame and window coordinates are physical pixels. Keep it that way; a `[dpi]` line on stderr means it isn't. Mixed scaling across monitors is 5.3.
- Docked panel monitor (5.2): the primary by default, the target's monitor with `panel_follows_monitor`; origin included since 3.3.
- Interpolation default is Bicubic (your choice in 1.5). Modes: Bilinear 0, Bicubic 1, Sharp 2, CleanEdge 3; the shader.wgsl header, `as_u32` and gfx.rs `write_uniforms` comment must agree. shader.wgsl has clean_edge.wgsl appended at compile time (gfx.rs `SHADER`). Uniforms are 48 bytes; `uniform_size_matches_the_shader_struct` checks gfx.rs against the WGSL struct. `cargo test` validates the shader.
- `cargo test --release -p cv-magnifier bench_modes_4k -- --ignored --nocapture` measures every mode at 3840x2160 headless. Bench only with the GPU idle.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- `windows` must stay out of every crate but cv-platform-win and cv-tts: `cargo tree -i windows@0.62.2 --workspace -e normal --depth 1` (gpu-allocator and wgpu-hal also appear; they are not workspace crates).
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Not an option in code: the native Windows Magnifier and the Magnification API for zooming or smoothing (your decision). `MagShowSystemCursor` for cursor hiding stays.
- Screenshots of the magnifier don't show the zoomed view (the overlay is excluded from capture). To compare filters by eye, run an ordinary unzoomed screenshot through the shader offscreen (docs/later/text-smoothing.md).
- Jitter: none noticeable at high zoom in 1.5. You expect it may show with caret tracking (Phase 4). Unconfirmed candidates if it does: the 16 ms `SetTimer` (cv-platform-win/src/overlay.rs `TICK_MS`) against 60 Hz vsync, `dt` measured at timer time not present time.
- Finding 12 (mouse cannot reach the taskbar) is resolved by 3.4. Finding 13 is ignored by your decision; 4.1 found its likely cause (a topmost window covering the monitor drops the taskbar), see STATUS.
- Panel size is 10–90% (`cv_core::PANEL_SIZE_MIN`, `PANEL_SIZE_MAX`, ADR 0007); the numbers can change in a step without a new ADR.
- Work in roadmap order.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
