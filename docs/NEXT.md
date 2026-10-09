# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 1.8 (you) Tester uses it. Record what's missing or wrong in docs/FEEDBACK.md. Feedback items get added to later phases by you.
Branch: none. 1.8 is yours; `/step 1.8` only records it once you say it's done.

Before 1.8: merge PR #22 (step/1.7-tester-build) yourself. 1.7 is approved and recorded in STATUS.

Where to start:
- The tester build: plain `cargo build --release` (no `RUSTFLAGS`, it would drop the static C runtime), then copy `target\release\clear-view.exe` and TESTER.md. The console window stays by your decision.
- The tag v0.1.0 is on bb81f33. It is pushed only when you say so.
- docs/FEEDBACK.md does not exist yet. `/step 1.8` creates it from what you report: what's missing or wrong, and which ZoomText hotkeys the tester relies on (PRODUCT.md open question). Also ask which email app the tester uses (needed for 4.2).
- After 1.8 comes 1.9 (cleanEdge edge smoothing, ADR 0006), then Phase 2. Tester feedback may change that order; you decide.

Still true:
- Display scaling (1.6): the process is Per-Monitor v2, so all cursor, monitor, frame and window coordinates are physical pixels. Keep it that way; a `[dpi]` line on stderr means it isn't. Mixed scaling across monitors is 5.3.
- Interpolation default is Bicubic (your choice in 1.5). Modes: Bilinear 0, Bicubic 1, Sharp 2; the shader.wgsl header, `as_u32` and gfx.rs `write_uniforms` comment must agree. `cargo test` validates shader.wgsl.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Not an option in code: the native Windows Magnifier and the Magnification API for zooming or smoothing (your decision). `MagShowSystemCursor` for cursor hiding stays.
- Jitter: none noticeable at high zoom in 1.5. You expect it may show with caret tracking (Phase 4). Unconfirmed candidates if it does: the 16 ms `SetTimer` (cv-render/src/lib.rs:292) against 60 Hz vsync, `dt` measured at timer time not present time.
- Finding 12 (mouse cannot reach the taskbar) is docked-only and goes with roadmap 3.5. Finding 13 is ignored by your decision.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
