# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 0.4 Stop taking the AppState write lock every frame in the egui panel: snapshot, edit a local copy, write back on change (Finding 3)
Branch: none yet. 0.3 is on step/0.3-remove-repaint-thread (PR #10); branch 0.4 from master after you have merged it.
Notes:
- 0.1, 0.2 and 0.3 are done and approved. 0.3's PR (#10) includes the records commit; merge it before starting 0.4.
- Since 0.3 the panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. Any thread that changes state the panel shows must call it. 0.4 must keep that working: write back only what the panel changed, so a hotkey toggle of `enabled` is not overwritten by a stale local copy.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- The "Existing ADRs" section of docs/ROADMAP.md says which ADRs stand and which later items supersede them.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
- CLAUDE.md still names tts-plan.md at the root; 0.7 fixes that.
