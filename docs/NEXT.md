# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 1.7 Tester build: a release build you can copy to the tester's machine by hand, a one-page TESTER.md with the hotkeys and how to quit, and a git tag v0.1.0.
Branch: none yet. Create step/1.7-<short-name> off master after PR #21 (1.6) is merged.

Before 1.7: merge PR #21 (step/1.6-display-scaling) yourself. 1.6 is approved and recorded in STATUS.

Where to start:
- Hotkeys (ADR 0005, app/src/hotkey.rs): Ctrl+Alt+Shift+Z on/off, Ctrl+Alt+Shift+Up zoom in, Ctrl+Alt+Shift+Down zoom out. Quit: close the settings window (1.1 restores the machine).
- No `windows_subsystem` attribute and no `[profile.release]` exist, so a release build opens a console window next to the panel. Decide in the plan whether the tester build keeps it (it shows errors such as the `[dpi]` line and hotkey failures, which are also listed in the settings window).
- Settings live in `%APPDATA%\clear-view\settings.json`; the magnifier always starts off.
- Tagging v0.1.0 and pushing the tag is outward-facing: confirm with you before pushing it.
- 1.9 (cleanEdge) runs before 1.7 only if you judge sharp bilinear not good enough for the tester; you kept the order and chose Bicubic.

Still true:
- Display scaling (1.6): the process is Per-Monitor v2, so all cursor, monitor, frame and window coordinates are physical pixels. Keep it that way; a `[dpi]` line on stderr means it isn't. Mixed scaling across monitors is 5.3.
- Interpolation default is Bicubic (your choice in 1.5). Modes: Bilinear 0, Bicubic 1, Sharp 2; the shader.wgsl header, `as_u32` and gfx.rs `write_uniforms` comment must agree. `cargo test` validates shader.wgsl.
- 1.9 (cleanEdge edge smoothing) stays after 1.8; you kept the order. Sharp only sharpens anti-aliased edge pixels (Finding 14).
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Not an option in code: the native Windows Magnifier and the Magnification API for zooming or smoothing (your decision). `MagShowSystemCursor` for cursor hiding stays.
- Jitter: none noticeable at high zoom in 1.5. You expect it may show with caret tracking (Phase 4). Unconfirmed candidates if it does: the 16 ms `SetTimer` (cv-render/src/lib.rs:292) against 60 Hz vsync, `dt` measured at timer time not present time.
- Finding 12 (mouse cannot reach the taskbar) is docked-only and goes with roadmap 3.5. Finding 13 is ignored by your decision.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
