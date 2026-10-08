# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 0.5 Fix the build and clippy warnings listed in STATUS
Branch: none yet. 0.4 is on step/0.4-panel-snapshot (PR #11, includes the records commit); branch 0.5 from master after you have merged it.
Notes:
- 0.1 to 0.4 are done and approved. Merge PR #11 before starting 0.5.
- The warnings are listed in docs/STATUS.md under `cargo build` and `cargo clippy --workspace`: 4 unused `BOOL` results in cv-render (the Mag* calls), 1 unused import in cv-capture, 4 collapsible `if`, 1 `map_or`, 1 `let...else`, 1 derivable `impl`. Re-run both commands first; the list may have drifted.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. Any thread that changes state the panel shows must call it. The panel now writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field must be added to that macro list.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- The "Existing ADRs" section of docs/ROADMAP.md says which ADRs stand and which later items supersede them.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
- CLAUDE.md still names tts-plan.md at the root; 0.7 fixes that.
