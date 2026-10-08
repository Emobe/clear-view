# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 0.2 Put cv-tts behind a cargo feature `tts`, off by default
Branch: none yet (0.1 is on step/0.1-status-for-roadmap, unmerged)
Notes:
- 0.1 is done and approved, on step/0.1-status-for-roadmap. The older step/tts-mode-toggles branch (old 2.3 TTS toggles, PR #6) is separate and not approved; 0.2 will need to deal with that code.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- The "Existing ADRs" section of docs/ROADMAP.md says which ADRs stand and which later items supersede them.
- CLAUDE.md still names tts-plan.md at the root; 0.7 fixes that.
