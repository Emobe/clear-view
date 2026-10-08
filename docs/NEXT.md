# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 2.3 Per-mode TTS toggles from tts-plan.md (selection, caret, typing, AppReader, alongside hover).
Branch: none yet (create step/<short-name> from master)
Blocked on: nothing, once PR #5 is merged
Notes:
- 1.1, 2.1 and 2.2 are approved and in the Done list in docs/STATUS.md. 2.2 was recorded in PR #5 (branch step/settings-persistence).
- 2.2 left a persistence layer that 2.3 builds on: `AppState` derives serde with `#[serde(default)]`, so new fields load from older files with their defaults. A new field only needs a sensible default and a range in `AppState::sanitize()` (cv-core/src/lib.rs) if it has one. `enabled` is `#[serde(skip)]` and must stay that way. Saving is done by the saver thread in crates/app/src/settings.rs; nothing else needs wiring.
- 2.3 adds `AppState` fields only (STATUS "Missing" lists the ones tts-plan.md names: `tts_selection_enabled`, `tts_caret_enabled`, `tts_typing_enabled`, `tts_appreader_enabled`) plus egui controls. Stages 5 to 7 that would use them have no code yet, so check what the toggles do before adding them. Read tts-plan.md for what it says about the state machine.
- You said docking works but needs to change later. Details to come from you (STATUS Finding 11); no roadmap item yet.
- Phase 0 item 0.1 (the "Needs your run" list in STATUS) is still mostly unrecorded. Its results feed 4.1.
- CLAUDE.md's ABM_ACTIVATE and "released on exit" lines are still wrong (STATUS Findings 8 and 9); no roadmap item covers them yet.
- The settings panel label still reads "(Win += to toggle)" (crates/app/src/app.rs:42) but the hotkey is Ctrl+Alt+Shift+Z. A one-line fix, not yet on the roadmap.

Next: merge PR #5, run `/clear`, then `/step 2.3`.
