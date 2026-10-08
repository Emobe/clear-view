# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 1.1 Clean exit (Finding 9)
Branch: none yet. Branch from master after PR for step/0.8-delete-merged-branches is merged (it holds the 0.7 and 0.8 records).
Notes:
- Phase 0 (0.1 to 0.8) is done.
- Locally only master and step/0.8-delete-merged-branches remain. Old branches still on GitHub do not matter; leave them.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings. Keep them there. Docs-only steps skip the gate.
- 1.1: read Finding 9 in STATUS first. Closing the settings window must stop the render thread, destroy the overlay, show the system cursor, remove the cursor clip and release the work area; same on panic. Find out and record what happens on kill. PRODUCT.md principle 2 applies.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field must be added to that macro list.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
