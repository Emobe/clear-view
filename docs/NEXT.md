# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 1.6 Display scaling: cursor circle and view line up at 100%, 125% and 150%. Fix if not.
Branch: none yet. Create step/1.6-<short-name> off master after PR #20 (1.5) is merged.

Before 1.6: merge PR #20 (step/1.5-readability) yourself. 1.5 is approved and recorded in STATUS.

Where to start:
- Cursor and view math are in cv-core/src/geometry.rs (`to_monitor_local`, `compute_crop`, `cursor_in_output`); the render thread reads the cursor with `GetCursorPos` (cv-render/src/lib.rs `on_timer`) and monitor rects from the capture outputs (`OutputInfo`).
- Check what DPI awareness the process declares (no manifest exists, STATUS Missing) and what that means for `GetCursorPos`, DXGI output coordinates and window rects. Check the current Microsoft docs first, per the roadmap rules.
- STATUS "Needs your run" still has: cursor circle lines up at 100%, 125%, 150% (confirmed on your current scaling only).
- Mixed scaling across monitors is 5.3, not 1.6.

Still true:
- Interpolation default is Bicubic (your choice in 1.5). Modes: Bilinear 0, Bicubic 1, Sharp 2; the shader.wgsl header, `as_u32` and gfx.rs `write_uniforms` comment must agree. `cargo test` validates shader.wgsl.
- 1.9 (cleanEdge edge smoothing) stays after 1.8; you kept the order. Sharp only sharpens anti-aliased edge pixels (Finding 14).
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Not an option in code: the native Windows Magnifier and the Magnification API for zooming or smoothing (your decision). `MagShowSystemCursor` for cursor hiding stays.
- Jitter: none noticeable at high zoom in 1.5. You expect it may show with caret tracking (Phase 4). Unconfirmed candidates if it does: the 16 ms `SetTimer` (cv-render/src/lib.rs:292) against 60 Hz vsync, `dt` measured at timer time not present time.
- Finding 12 (mouse cannot reach the taskbar) is docked-only and goes with roadmap 3.5. Finding 13 is ignored by your decision.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
