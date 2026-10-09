# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 3.3 Overlay docked mode on the primary monitor, top edge only.
ADR: docs/adr/0007-docked-overlay.md (roadmap 3.2), Accepted by you 2026-10-09 (PR #33). Run `/step 3.3`. 3.2 goes in the STATUS Done list when you say "I approve".

Notes for 3.3 (from ADR 0007's recommendation; if you change the ADR, the ADR wins):
- Window styles unchanged, `DxgiFromHwnd`. Docked placement is `SetWindowPos` to a new pure `geometry::docked_rect(monitor, edge, thickness)` in cv-core (monitor origin included), replacing `appbar::panel_rect`. Top edge only in 3.3; the other edges keep AppBar docking until 3.4.
- No AppBar and no `ClipCursor` while docked on the top edge.
- New `OverlayHost::set_system_cursor(visible)`: the magnifier hides the system cursor while the pointer is inside the panel rect, calling only on change; the hit test is pure and unit-tested; the fake host records the calls. Fullscreen hiding stays in `apply_layout`; `Hidden` and teardown show the cursor.
- Startup log, one line each: the Windows build and `IDXGIOutput2::SupportsOverlays` per output.
- Verify includes whether clicking the taskbar covers the panel. If it does, `apply_layout` re-asserts `HWND_TOPMOST` and the PR says so.
- Nothing against extra frames is built; captured frames are never filtered by dirty rect.
- Model: Sonnet high (CLAUDE.md model guide for /step).

Then: 3.4 all edges and the 10–90% panel size limits; 3.5 removes appbar.rs, `update_clip_cursor`, `reposition_appbar` and the `ABN_POSCHANGED` path; before 3.6 you read the build and MPO lines on the tester's machine.

After 3.1 (done):
- step/3.1-overlay-prototype holds the prototype code (73bdf0a). It is never merged; keep it for reference until 3.3 is done.
- Side finding: pointer-only capture frames are copied in full and cost about one CPU core while the mouse moves (STATUS Finding 4). Planned for 5.6; moving it earlier is your call.

What comes next:
- 3.3 to 3.5 build ADR 0007.
- 1.8 (you) is still on hold until the tester is free. When they are, `/step 1.8` records their feedback in docs/FEEDBACK.md. Ask which app they compare ZoomText in, whether ClearType is on, which ZoomText hotkeys they rely on and which email app they use (4.2).
- Contour sharpening and toggleable text enhancements (docs/later/text-smoothing.md) still have no roadmap item; adding a "1.10 (ADR first)" is your call.

Still true:
- Display scaling (1.6): the process is Per-Monitor v2, so all cursor, monitor, frame and window coordinates are physical pixels. Keep it that way; a `[dpi]` line on stderr means it isn't. Mixed scaling across monitors is 5.3.
- Interpolation default is Bicubic (your choice in 1.5). Modes: Bilinear 0, Bicubic 1, Sharp 2, CleanEdge 3; the shader.wgsl header, `as_u32` and gfx.rs `write_uniforms` comment must agree. shader.wgsl has clean_edge.wgsl appended at compile time (gfx.rs `SHADER`). Uniforms are 48 bytes; `uniform_size_matches_the_shader_struct` checks gfx.rs against the WGSL struct. `cargo test` validates the shader.
- `cargo test --release -p cv-magnifier bench_modes_4k -- --ignored --nocapture` measures every mode at 3840x2160 headless. Bench only with the GPU idle.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- `windows` must stay out of every crate but cv-platform-win and cv-tts: `cargo tree -i windows@0.62.2 --workspace -e normal --depth 1` (gpu-allocator and wgpu-hal also appear; they are not workspace crates).
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Not an option in code: the native Windows Magnifier and the Magnification API for zooming or smoothing (your decision). `MagShowSystemCursor` for cursor hiding stays.
- Screenshots of the magnifier don't show the zoomed view (the overlay is excluded from capture). To compare filters by eye, run an ordinary unzoomed screenshot through the shader offscreen (docs/later/text-smoothing.md).
- Jitter: none noticeable at high zoom in 1.5. You expect it may show with caret tracking (Phase 4). Unconfirmed candidates if it does: the 16 ms `SetTimer` (cv-platform-win/src/overlay.rs `TICK_MS`) against 60 Hz vsync, `dt` measured at timer time not present time.
- Finding 12 (mouse cannot reach the taskbar) is docked-only and goes with roadmap 3.5. Finding 13 is ignored by your decision.
- Work in roadmap order.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
