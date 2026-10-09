# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 2.6 wgpu 22 to 30, naga with it; `bench_modes_4k` before and after.
Branch: none yet. Use step/2.6-wgpu-30 off master.

Notes for 2.6:
- ADR 0004 (timing B) decides the upgrade order: wgpu first, in cv-magnifier only, then eframe in 2.7. No behaviour change; the Verify list uses the ADR 0004 regression list.
- The workspace Cargo.toml pins `wgpu = "22"` (lock: wgpu 22.1.0, naga 22.1.0). cv-magnifier uses wgpu, raw-window-handle 0.6 and pollster 0.3. `cargo tree -i wgpu --workspace -e normal --depth 1` shows only cv-magnifier using wgpu today (eframe does not pull it in); check it again afterwards so two wgpu versions don't end up in the build.
- Code that touches wgpu: cv-magnifier/src/gfx.rs (`WgpuState::new` takes raw display and window handles, surface, pipeline, uniforms) and the `gfx::tests` naga check (`wgpu::naga`). Read the wgpu changelog from 22 to 30 for the breaking changes in surface creation, `RenderPassDescriptor`, pipeline descriptors and the naga re-export before writing code.
- Run `cargo test --release -p cv-magnifier bench_modes_4k -- --ignored --nocapture` on master first and record the numbers in the PR, then again after.
- Teardown order matters: overlay.rs drops the magnifier and its surface before `DestroyWindow` (`WinHost`'s `Drop`). Keep it.
- If the jump is too big for one step, stop and propose sub-items here.

After 2.5a (done):
- The capture loop retries a failed reconnect or factory every 250 ms (cv-magnifier/src/capture.rs `RECONNECT_RETRY`) and never ends on its own. Tests end it through a private stop flag (`spawn_capture_with`).
- `Capturer::recreate` releases the lost duplication before making a new one. Every error still rebuilds the whole device; rebuilding only the duplication on `ACCESS_LOST` was not needed.
- Phase 3 (overlay docking) replaces `WinHost`'s AppBar and clip code without touching `Magnifier`. Your docked-mode requirements are in STATUS Finding 11. Use them as input to the 3.2 ADR.

What comes next:
- 2.7 eframe. It needs Rust 1.95 or newer for eframe 0.36; the local toolchain is 1.93.1. Update it (`rustup update stable`) before that step.
- 1.8 (you) is still on hold until the tester is free. When they are, `/step 1.8` records their feedback in docs/FEEDBACK.md. Ask which app they compare ZoomText in, whether ClearType is on, which ZoomText hotkeys they rely on and which email app they use (4.2).
- Contour sharpening and toggleable text enhancements (docs/later/text-smoothing.md) still have no roadmap item; adding a "1.10 (ADR first)" is your call.

Still true:
- Display scaling (1.6): the process is Per-Monitor v2, so all cursor, monitor, frame and window coordinates are physical pixels. Keep it that way; a `[dpi]` line on stderr means it isn't. Mixed scaling across monitors is 5.3.
- Interpolation default is Bicubic (your choice in 1.5). Modes: Bilinear 0, Bicubic 1, Sharp 2, CleanEdge 3; the shader.wgsl header, `as_u32` and gfx.rs `write_uniforms` comment must agree. shader.wgsl has clean_edge.wgsl appended at compile time (gfx.rs `SHADER`). Uniforms are 48 bytes; `uniform_size_matches_the_shader_struct` checks gfx.rs against the WGSL struct. `cargo test` validates the shader.
- `cargo test --release -p cv-magnifier bench_modes_4k -- --ignored --nocapture` measures every mode at 3840x2160 headless. Use it for any new shader mode.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- `windows` must stay out of every crate but cv-platform-win and cv-tts: `cargo tree -i windows@0.62.2 --workspace -e normal --depth 1`.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Not an option in code: the native Windows Magnifier and the Magnification API for zooming or smoothing (your decision). `MagShowSystemCursor` for cursor hiding stays.
- Screenshots of the magnifier don't show the zoomed view (the overlay is excluded from capture). To compare filters by eye, run an ordinary unzoomed screenshot through the shader offscreen (docs/later/text-smoothing.md).
- Jitter: none noticeable at high zoom in 1.5. You expect it may show with caret tracking (Phase 4). Unconfirmed candidates if it does: the 16 ms `SetTimer` (cv-platform-win/src/overlay.rs `TICK_MS`) against 60 Hz vsync, `dt` measured at timer time not present time.
- Finding 12 (mouse cannot reach the taskbar) is docked-only and goes with roadmap 3.5. Finding 13 is ignored by your decision.
- Work in roadmap order.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
