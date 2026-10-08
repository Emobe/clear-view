# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 0.2 Put cv-tts behind a cargo feature `tts`, off by default
Branch: none yet (0.1 is on step/tts-mode-toggles, unmerged)
Notes:
- 0.1 is done and approved. Its commits sit on step/tts-mode-toggles, which also holds the old-roadmap per-mode TTS toggles (old 2.3, not approved). Merge or branch decisions are yours.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- The "Existing ADRs" section of docs/ROADMAP.md says which ADRs stand and which later items supersede them.
- CLAUDE.md still names tts-plan.md at the root; 0.7 fixes that.
