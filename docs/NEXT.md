# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 2.4 Seam traits `PointerSource`, `OverlayHost`, `CaptureSource`; the capture loop moves from main.rs to cv-magnifier behind `CaptureSource`.
Branch: none yet. Use step/2.4-seam-traits off master.

Notes for 2.4:
- Read ADR 0004 (Traits and who owns which thread; How each item is checked). Behaviour must not change; the Verify list is the ADR's regression list. Names in the ADR sketch may change; the split may not.
- After 2.3: cv-magnifier (neutral) holds gfx.rs and the shaders and exports `WgpuState` (`unsafe fn new(RawDisplayHandle, RawWindowHandle, ...)`). cv-platform-win holds capture.rs, dpi.rs, hotkey.rs, overlay.rs (the old cv-render lib.rs, 709 lines, unchanged) and appbar.rs, and depends on cv-magnifier. app calls `platform::spawn_overlay` and has no cv-magnifier dependency; add it back when the capture loop moves there.
- The capture loop is still inline in app/src/main.rs (`platform::Capturer`, `desired_output`, `next_frame(100)`, `reconnect`). `Capturer` methods return `windows::core::Result`; `CaptureSource` needs a neutral `CaptureError` (timeout stays `Ok(None)`). Per the ADR the source is built by a factory closure inside the capture thread, so D3D11 and COM objects never cross threads. `enumerate_outputs` stays in the backend.
- `PointerSource` goes in cv-core with `ScreenPoint`/`ScreenRect`; `OverlayHost` and `Layout` in cv-magnifier. In 2.4 they are declared and implemented; moving the per-tick logic out of `on_timer` is 2.5, not this step.
- `windows` must stay out of every crate but cv-platform-win and cv-tts: `cargo tree -i windows@0.62.2 --workspace -e normal --depth 1`.

What comes next:
- 2.7 needs Rust 1.95 or newer for eframe 0.36; the local toolchain is 1.93.1. Update it (`rustup update stable`) before that step.
- 1.8 (you) is still on hold until the tester is free. When they are, `/step 1.8` records their feedback in docs/FEEDBACK.md. Ask which app they compare ZoomText in, whether ClearType is on, which ZoomText hotkeys they rely on and which email app they use (4.2).
- Contour sharpening and toggleable text enhancements (docs/later/text-smoothing.md) still have no roadmap item; adding a "1.10 (ADR first)" is your call.

Still true:
- Display scaling (1.6): the process is Per-Monitor v2, so all cursor, monitor, frame and window coordinates are physical pixels. Keep it that way; a `[dpi]` line on stderr means it isn't. Mixed scaling across monitors is 5.3.
- Interpolation default is Bicubic (your choice in 1.5). Modes: Bilinear 0, Bicubic 1, Sharp 2, CleanEdge 3; the shader.wgsl header, `as_u32` and gfx.rs `write_uniforms` comment must agree. shader.wgsl has clean_edge.wgsl appended at compile time (gfx.rs `SHADER`). Uniforms are 48 bytes; `uniform_size_matches_the_shader_struct` checks gfx.rs against the WGSL struct. `cargo test` validates the shader.
- `cargo test --release -p cv-magnifier bench_modes_4k -- --ignored --nocapture` measures every mode at 3840x2160 headless. Use it for any new shader mode.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Not an option in code: the native Windows Magnifier and the Magnification API for zooming or smoothing (your decision). `MagShowSystemCursor` for cursor hiding stays.
- Screenshots of the magnifier don't show the zoomed view (the overlay is excluded from capture). To compare filters by eye, run an ordinary unzoomed screenshot through the shader offscreen (docs/later/text-smoothing.md).
- Jitter: none noticeable at high zoom in 1.5. You expect it may show with caret tracking (Phase 4). Unconfirmed candidates if it does: the 16 ms `SetTimer` (cv-platform-win/src/overlay.rs) against 60 Hz vsync, `dt` measured at timer time not present time.
- Finding 12 (mouse cannot reach the taskbar) is docked-only and goes with roadmap 3.5. Finding 13 is ignored by your decision.
- Work in roadmap order.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
