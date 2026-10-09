# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 2.1 Platform seams ADR (ADR 0004, rewritten in place)
Branch: step/2.1-platform-seams-adr (docs only, PR open).
ADR: docs/adr/0004-platform-abstraction.md (Proposed). Recommends traits at the seam (`PointerSource` in cv-core; `OverlayHost` with a `Layout` enum and `CaptureSource` in a neutral cv-magnifier, which is cv-render renamed) and one Windows backend crate, cv-platform-win, that absorbs cv-capture and every `windows` import outside cv-tts. The backend owns the render and hotkey threads and calls `Magnifier::tick`. Frames stay CPU `Arc<Frame>`. Core event rules for 4.4 are set, nothing is built for them in Phase 2. wgpu 22 to 30 and eframe 0.29 to 0.36 come after the refactor, before 3.1.

What comes next:
- You review ADR 0004 and set its Status to Accepted yourself, or reject or change it. If you change the decision, change 2.2 to 2.7 in ROADMAP.md to match.
- After you accept it and say "I approve", 2.1 is recorded in STATUS, then `/step 2.2` (cv-platform-win crate). 2.2 to 2.7 are marked "(pending ADR 0004)" until then.
- 2.7 needs Rust 1.95 or newer for eframe 0.36; the local toolchain is 1.93.1. Update it (`rustup update stable`) before that step.
- 1.8 (you) is still on hold until the tester is free. When they are, `/step 1.8` records their feedback in docs/FEEDBACK.md. Ask which app they compare ZoomText in, whether ClearType is on, which ZoomText hotkeys they rely on and which email app they use (4.2).
- Contour sharpening and toggleable text enhancements (docs/later/text-smoothing.md) still have no roadmap item; adding a "1.10 (ADR first)" is your call.

Still true:
- Display scaling (1.6): the process is Per-Monitor v2, so all cursor, monitor, frame and window coordinates are physical pixels. Keep it that way; a `[dpi]` line on stderr means it isn't. Mixed scaling across monitors is 5.3.
- Interpolation default is Bicubic (your choice in 1.5). Modes: Bilinear 0, Bicubic 1, Sharp 2, CleanEdge 3; the shader.wgsl header, `as_u32` and gfx.rs `write_uniforms` comment must agree. shader.wgsl has clean_edge.wgsl appended at compile time (gfx.rs `SHADER`). Uniforms are 48 bytes; `uniform_size_matches_the_shader_struct` checks gfx.rs against the WGSL struct. `cargo test` validates the shader.
- `cargo test --release -p cv-render bench_modes_4k -- --ignored --nocapture` measures every mode at 3840x2160 headless. Use it for any new shader mode.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Not an option in code: the native Windows Magnifier and the Magnification API for zooming or smoothing (your decision). `MagShowSystemCursor` for cursor hiding stays.
- Screenshots of the magnifier don't show the zoomed view (the overlay is excluded from capture). To compare filters by eye, run an ordinary unzoomed screenshot through the shader offscreen (docs/later/text-smoothing.md).
- Jitter: none noticeable at high zoom in 1.5. You expect it may show with caret tracking (Phase 4). Unconfirmed candidates if it does: the 16 ms `SetTimer` (cv-render/src/lib.rs) against 60 Hz vsync, `dt` measured at timer time not present time.
- Finding 12 (mouse cannot reach the taskbar) is docked-only and goes with roadmap 3.5. Finding 13 is ignored by your decision.
- Work in roadmap order.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
