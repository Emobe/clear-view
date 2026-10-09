# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 3.1 Prototype on a throwaway branch, not merged: a topmost, click-through window excluded from capture (WDA_EXCLUDEFROMCAPTURE) over a live DXGI capture, without AppBar or ClipCursor. Measure extra frames caused by the overlay's own updates (Windows 11 24H2), mouse pass-through and cost. Results to docs/prototypes/docked-overlay.md.
Branch: none yet. Use step/3.1-overlay-prototype off master for the prototype code (never merged). Only docs/prototypes/docked-overlay.md goes to master, on its own branch and PR.

Notes for 3.1:
- Phase 2 is finished. The current overlay already sets WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_EX_NOACTIVATE, WS_EX_LAYERED and WDA_EXCLUDEFROMCAPTURE (ADR 0002), so start from cv-platform-win/src/overlay.rs and `WinHost`.
- Docked placement is `Layout` in cv-magnifier and `WinHost` (AppBar, `update_clip_cursor`) in cv-platform-win. The prototype replaces only the backend side; `Magnifier::tick` should not need to change.
- Your docked-mode requirements are in STATUS Finding 11: desktop stays at 100%, panel on top, mouse can move under the panel and the panel shows that area magnified. Finding 12 (mouse can't reach the taskbar) goes away with ClipCursor.
- Extra frames: count `AcquireNextFrame` successes per second with the mouse still, overlay shown vs hidden, and compare. That is the 24H2 issue the 3.2 ADR has to answer.
- wgpu 27+ has DirectComposition swapchains (`Dx12SwapchainKind::DxgiFromVisual`); worth trying in the prototype (ADR 0004).
- The output is measurements and a recommendation for the 3.2 ADR, not shippable code.

After 2.7 (done):
- eframe and egui 0.36.2, glow, default features off. `App::ui(ui)` replaces `App::update(ctx)`; the panel is `CentralPanel::default().show(ui, …)`. Pinned to 0.36.2 for the busy-loop fix (egui PR #8398).
- Toolchain is Rust 1.99.0.

What comes next:
- 3.2 (ADR first) uses the 3.1 results.
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
