# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 1.2 (ADR first) Hotkey scheme
Branch: none yet. Branch from master after PR #15 (step/1.1-clean-exit, holds the 1.1 code and its records) is merged.
Notes:
- Phase 0 and 1.1 are done.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings. Keep them there. Docs-only steps skip the gate.
- 1.2: decide RegisterHotKey combinations vs a low-level keyboard hook (Caps Lock modifier like ZoomText), default bindings for on/off, zoom in, zoom out, and what happens when a binding is taken. Needs `/adr 1.2`.
- Open: Finding 12 (mouse cannot reach the taskbar, seen while testing 1.1, not caused by it) is not on the roadmap. Tell me where it goes; the likely place is with docking (Finding 11, roadmap 3.5) unless it shows in fullscreen too.
- Finding 9 kill case: you reported it works, so nothing further is planned for it.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field must be added to that macro list.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
