# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: awaiting-test
Item: 1.1 Clean exit (Finding 9)
Branch: step/1.1-clean-exit (built, gate passed, PR open; not merged)
Notes:
- Waiting for you to run the Verify list in the PR, then say "I approve" and merge. Only then does 1.1 go into the Done list and Finding 9 get updated.
- What changed: `cv_render::spawn_overlay` returns an `OverlayHandle`; `shutdown()` posts `WM_CLOSE`, the render thread runs `teardown` (AppBar remove, system cursor, cursor clip, destroy window) from a `Drop` guard, so it also runs on panic. `main` holds the handle in a guard. A console control handler (Ctrl+C, closing the console, logoff, shutdown) does the same. Cargo.toml gains `Win32_System_Console`.
- Record in Finding 9 what you see after killing the process in Task Manager (fullscreen and docked): cursor, clip, work area. The code cannot run on kill. Options if something is left behind: restore-on-next-start (cursor and clip only) or a watchdog process. Neither is built.
- After 1.1 is approved and merged, the next item is 1.2 (ADR first): hotkey scheme.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings. Keep them there. Docs-only steps skip the gate.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field must be added to that macro list.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
