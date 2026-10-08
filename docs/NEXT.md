# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: ROADMAP Phase 1, make the docs true. Phase 0 is done: this audit replaced docs/STATUS.md.
Branch: none yet. The step should create one from master and carry the uncommitted files onto it (CLAUDE.md, docs/, .claude/commands/).
Blocked on: nothing
Notes:
- Phase 1 has three parts. Replacing CLAUDE.md is done in the working tree but uncommitted. Not done: move PLAN.md, PHASE2PLAN.md, handoff.md and shaders/magnify.hlsl to docs/archive/ (all four still tracked at the old paths), and add checkboxes to the stage headings in tts-plan.md.
- Checkbox state from STATUS.md: Stages 1 to 4 have code but no Verify run, so none is proven. Stages 5 to 9 have no code.
- Two CLAUDE.md lines do not match the code (STATUS Finding 8): it lists `ABM_ACTIVATE`, which appbar.rs never sends, and says the AppBar is released on app exit, which has no code path (Finding 9). Correct them in this step or note them.
- Deleting merged branches is yours. `git branch --no-merged master` is empty, so there are no unmerged dead ends; the list of redundant branches is in STATUS.md under Findings.
- Phase 2 follows. Its first item is moving pure logic from cv-render/src/lib.rs into cv-core with tests.

Next: run `/clear`, then `/next`.
