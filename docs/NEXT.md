# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 0.7 Update CLAUDE.md (PRODUCT.md first, Finding 8 claims, gate, speech out of scope)
Branch: step/0.7-claude-md (from master at 085858e; first commit records 0.6 as done)
Notes:
- 0.1 to 0.6 are done. 0.6 was done by you directly on master (085858e).
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings. Keep them there.
- 0.7 also fixes CLAUDE.md's tts-plan.md paths (it now lives at docs/later/tts-plan.md).
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. Any thread that changes state the panel shows must call it. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field must be added to that macro list.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- The "Existing ADRs" section of docs/ROADMAP.md says which ADRs stand and which later items supersede them.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
