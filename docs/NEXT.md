# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 2.2 Settings persistence (JSON in the user config dir) for all AppState fields.
Branch: none yet (create step/<short-name> from master)
Blocked on: nothing
Notes:
- 1.1 and 2.1 are approved and in the Done list in docs/STATUS.md. Both were recorded in PR #4 (branch step/core-geometry), which also changed the global hotkey from Win+= to Ctrl+Alt+Shift+Z because Win+= opens Windows Magnifier. PR #3 duplicates the 1.1 record and can be closed unmerged.
- 2.2 will probably add serde and serde_json plus a config-dir crate. CLAUDE.md says a new dependency can need an ADR; the plan must say whether it does. Enums in cv-core (ColorFilter, Interpolation, DisplayMode, Edge) have no serde derives yet. The hotkey binding is hardcoded; tts-plan.md's HotkeyBinding stub is a natural fit if bindings are ever made configurable.
- You said docking works but needs to change later. Details to come from you (STATUS Finding 11); no roadmap item yet.
- Phase 0 item 0.1 (the "Needs your run" list in STATUS) is still mostly unrecorded. Its results feed 4.1.
- CLAUDE.md's ABM_ACTIVATE and "released on exit" lines are still wrong (STATUS Findings 8 and 9); no roadmap item covers them yet.

Next: merge PR #4, run `/clear`, then `/step 2.2`.
