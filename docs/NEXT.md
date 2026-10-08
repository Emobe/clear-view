# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 2.1 Move pure logic out of the render loop into cv-core with unit tests: lerp, crop and zoom math, cursor-to-output mapping.
Branch: none yet (create step/<short-name> from master)
Blocked on: nothing
Notes:
- 1.1 is approved and in the Done list in docs/STATUS.md. Roadmap 1.2 was dropped.
- 2.1 needs no new ADR on its face (cv-core exists, no new crate or dependency), but the plan must check that nothing it moves belongs behind the platform seam in ADR 0004 (Proposed).
- Phase 0 item 0.1 (the "Needs your run" list in STATUS) is still unrecorded. Its results feed 4.1.
- CLAUDE.md's ABM_ACTIVATE and "released on exit" lines are still wrong (STATUS Findings 8 and 9); no roadmap item covers them yet.

Next: merge the docs PR, run `/clear`, then `/step 2.1`.
