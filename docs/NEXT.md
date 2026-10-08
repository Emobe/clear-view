# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 0.6 Stop tracking clear-view.zip in git and add it to .gitignore
Branch: none yet. 0.5 is on step/0.5-fix-warnings (PR #12, includes the records commit); branch 0.6 from master after you have merged it.
Notes:
- 0.1 to 0.5 are done and approved. Merge PR #12 before starting 0.6.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings. Keep them there.
- 0.6: clear-view.zip is tracked and was modified in the working tree earlier (see STATUS Git section); check `git status` first and use `git rm --cached`, not a delete of your local copy.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. Any thread that changes state the panel shows must call it. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field must be added to that macro list.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- The "Existing ADRs" section of docs/ROADMAP.md says which ADRs stand and which later items supersede them.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
- CLAUDE.md still names tts-plan.md at the root; 0.7 fixes that.
