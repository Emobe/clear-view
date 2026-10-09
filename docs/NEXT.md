# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 3.5 Remove AppBar docking and ClipCursor (ADR 0007).
ADR: docs/adr/0007-docked-overlay.md, Accepted. Run `/step 3.5`.

Notes for 3.5 (from ADR 0007 "Removed in 3.5" and 3.4; if you change the ADR, the ADR wins):
- 3.4 (PR #35) put every docked edge on the overlay. Nothing registers an AppBar any more; `appbar_active` is never set true. `appbar::register`, `ABM_NEW` and `WinHost::callback_msg` carry `#[allow(dead_code)] // removed in 3.5`.
- Remove: cv-platform-win/src/appbar.rs (and `mod appbar` in lib.rs), `appbar::notify_moved` in `move_window`, `WinHost::appbar_active`, `unregister_appbar`, `reposition_appbar`, `callback_msg`, the `CALLBACK_MSG` and `APPBAR_POS_CHANGED` thread-locals, `RegisterWindowMessageW`, the callback branch in `wnd_proc`, the AppBar part of `teardown`, `update_clip_cursor`, `WinHost::update_clip` and its call in the loop, and `Magnifier::resized` if nothing else calls it. Drop `windows` features that only these used (check `Win32_UI_Shell` is not used elsewhere).
- Shutdown: `OverlayHandle::shutdown` and `teardown` call `update_clip_cursor(false)`. Decide in the plan whether a single `ClipCursor(None)` stays on exit as cheap insurance (PRODUCT principle 2) or goes with the rest; nothing sets a clip any more.
- No setting is removed: `display_mode` and `panel_size` stay.
- CLAUDE.md still describes AppBar docking (Key design decisions "Docked panel", the render thread's `ABN_POSCHANGED` note, and cv-platform-win's appbar.rs in Crate structure). Update it in 3.5.
- Model: Sonnet high (CLAUDE.md model guide for /step).

Then: 3.6 tester build 2 with docked mode, tag v0.2.0; before handing it over you read the build and MPO lines (`[system] …` at startup) on the tester's machine.

What comes next:
- 3.5 finishes ADR 0007.
- 1.8 (you) is still on hold until the tester is free. When they are, `/step 1.8` records their feedback in docs/FEEDBACK.md. Ask which app they compare ZoomText in, whether ClearType is on, which ZoomText hotkeys they rely on and which email app they use (4.2).
- Contour sharpening and toggleable text enhancements (docs/later/text-smoothing.md) still have no roadmap item; adding a "1.10 (ADR first)" is your call.
- Side finding from 3.1: pointer-only capture frames are copied in full and cost about one CPU core while the mouse moves (STATUS Finding 4). Planned for 5.6; moving it earlier is your call.

Still true:
- Display scaling (1.6): the process is Per-Monitor v2, so all cursor, monitor, frame and window coordinates are physical pixels. Keep it that way; a `[dpi]` line on stderr means it isn't. Mixed scaling across monitors is 5.3.
- Docked panel monitor: the panel goes on the monitor active when the layout is applied, origin included since 3.3. Multi-monitor docking is 5.1.
- Interpolation default is Bicubic (your choice in 1.5). Modes: Bilinear 0, Bicubic 1, Sharp 2, CleanEdge 3; the shader.wgsl header, `as_u32` and gfx.rs `write_uniforms` comment must agree. shader.wgsl has clean_edge.wgsl appended at compile time (gfx.rs `SHADER`). Uniforms are 48 bytes; `uniform_size_matches_the_shader_struct` checks gfx.rs against the WGSL struct. `cargo test` validates the shader.
- `cargo test --release -p cv-magnifier bench_modes_4k -- --ignored --nocapture` measures every mode at 3840x2160 headless. Bench only with the GPU idle.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- `windows` must stay out of every crate but cv-platform-win and cv-tts: `cargo tree -i windows@0.62.2 --workspace -e normal --depth 1` (gpu-allocator and wgpu-hal also appear; they are not workspace crates).
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Not an option in code: the native Windows Magnifier and the Magnification API for zooming or smoothing (your decision). `MagShowSystemCursor` for cursor hiding stays.
- Screenshots of the magnifier don't show the zoomed view (the overlay is excluded from capture). To compare filters by eye, run an ordinary unzoomed screenshot through the shader offscreen (docs/later/text-smoothing.md).
- Jitter: none noticeable at high zoom in 1.5. You expect it may show with caret tracking (Phase 4). Unconfirmed candidates if it does: the 16 ms `SetTimer` (cv-platform-win/src/overlay.rs `TICK_MS`) against 60 Hz vsync, `dt` measured at timer time not present time.
- Finding 12 (mouse cannot reach the taskbar) is resolved by 3.4. Finding 13 is ignored by your decision.
- Panel size is 10–90% (`cv_core::PANEL_SIZE_MIN`, `PANEL_SIZE_MAX`, ADR 0007); the numbers can change in a step without a new ADR.
- Work in roadmap order.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
