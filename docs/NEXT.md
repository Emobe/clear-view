# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 2.7 eframe and egui 0.29 to 0.36 (needs Rust 1.95 or newer), or 0.35 if 0.36 is too much churn.
Branch: none yet. Use step/2.7-eframe off master.

Before 2.7 (you): the local toolchain is rustc 1.93.1 and eframe 0.36 needs 1.95. Run `rustup update stable` first. If you don't, the step can only go to 0.35 (needs 1.92).

Notes for 2.7:
- ADR 0004 (timing B, Upgrades): app only, its own Verify list (the ADR 0004 regression list), no behaviour change. Stopping at 0.35 is allowed if 0.36 brings too much panel churn; record why in the PR.
- The workspace Cargo.toml pins `eframe = { version = "0.29", default-features = false, features = ["default_fonts", "glow"] }` and `egui = "0.29"`. Keep glow: eframe's wgpu renderer would need wgpu ^30 and would share nothing with the overlay; the panel and overlay share no graphics crate today (ADR 0004).
- Code that touches egui/eframe: crates/app/src/app.rs (the panel, `apply_changes` and its macro field list, `RepaintSlot`), main.rs (`eframe::run_native`, viewport options, always on top). Read the egui/eframe changelogs from 0.29 to 0.36 before writing code.
- STATUS Finding 3 notes egui 0.29.1's `Slider` re-applies `step_by` rounding every frame. Check whether that changed; `apply_changes` tests in app must still pass.
- The hotkey thread wakes the panel through `request_repaint()`; check the hotkey-with-panel-unfocused case still repaints.

After 2.6 (done):
- wgpu 30.0.1 in cv-magnifier only. `dx12_instance()` in gfx.rs sets FXC explicitly. `cargo tree -i windows@0.62.2 ... --depth 1` now also lists gpu-allocator and wgpu-hal (same `windows` version, not workspace crates).
- `bench_modes_4k` on wgpu 30, idle RTX 3070: Bilinear 0.16–0.19, Bicubic 0.29–0.38, Sharp 0.19–0.20, cleanEdge 0.91–1.78 ms. cleanEdge is 12–19% slower than on 22 for flat and stripes frames; cause not investigated (PR #30). Bench only with the GPU idle: a running game made every number 15–20x slower.
- wgpu 27+ has DirectComposition swapchains (`Dx12SwapchainKind::DxgiFromVisual`); 3.1 can try it (ADR 0004).

What comes next:
- Phase 3 (overlay docking) replaces `WinHost`'s AppBar and clip code without touching `Magnifier`. Your docked-mode requirements are in STATUS Finding 11. Use them as input to the 3.2 ADR.
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
