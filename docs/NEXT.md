# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 2.3 Portable renderer: gfx.rs takes raw display and window handles instead of `HWND`; the Win32 overlay (window, loop, AppBar, cursor clip, cursor hiding, clean exit) and appbar.rs move to cv-platform-win; cv-render drops `windows` and is renamed cv-magnifier.
Branch: none yet. Use step/2.3-portable-renderer off master.

Before anything: merge PR #25 (step/2.2-platform-win) yourself. 2.2 is approved and recorded in STATUS.

Notes for 2.3:
- Read ADR 0004 (Crates; Traits and who owns which thread; How each item is checked). Behaviour must not change; the Verify list is the ADR's regression list.
- After 2.2: cv-platform-win (`#![cfg(windows)]`) has capture.rs, dpi.rs and hotkey.rs; app reaches it as `platform::` and has no `windows` dependency. cv-render (lib.rs 699 lines, appbar.rs, gfx.rs) is the last crate besides cv-tts that depends on `windows`.
- After 2.3, cv-platform-win depends on cv-magnifier (ADR 0004 crate table), app calls `platform::spawn_overlay(...)` and gets the same `OverlayHandle`, and `cargo tree -i windows@0.62.2 --workspace -e normal --depth 1` lists only cv-platform-win and cv-tts.
- The traits (`OverlayHost`, `CaptureSource`) are 2.4 and the `Magnifier::tick` untangling is 2.5: in 2.3 the overlay code moves as it is. If moving lib.rs plus the rename is too big for one PR, stop and propose sub-items.
- The `bench_modes_4k` command below changes package name with the rename (`-p cv-magnifier`).

What comes next:
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
