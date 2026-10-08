# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 0.3 Remove the 100 ms repaint thread and the redundant `request_repaint_after` in app.rs, and the comment that gives the wrong cause (STATUS Finding 2)
Branch: none yet (0.2 records are on step/0.2-tts-feature, PR open, unmerged)
Notes:
- 0.1 and 0.2 are done and approved. Both are merged to master except the 0.2 records commit.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- The "Existing ADRs" section of docs/ROADMAP.md says which ADRs stand and which later items supersede them.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
- CLAUDE.md still names tts-plan.md at the root; 0.7 fixes that.
