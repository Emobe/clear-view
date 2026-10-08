# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: step-in-review
Item: 1.1 Move old plans and the unused shader to docs/archive/
Branch: step/archive-old-docs (not merged)
Blocked on: nothing
Notes:
- Moved with `git mv`: PLAN.md, PHASE2PLAN.md, handoff.md, shaders/magnify.hlsl, all now in docs/archive/. magnify.hlsl was confirmed unused (no reference in .rs, .toml, .wgsl). The shaders/ directory is gone.
- Gate unchanged from the STATUS baseline: build passes, clippy 12 warnings and no errors, 0 tests.
- docs/adr/0002 still says "the original PLAN.md"; left alone because it is an ADR. Update the path only if you want.
- To test by hand: `cargo run` starts and Win+= toggles the magnifier. No code changed, so this is only a sanity check.
- Next roadmap item: 1.2, checkboxes in tts-plan.md. CLAUDE.md's ABM_ACTIVATE and "released on exit" lines are still wrong (STATUS Findings 8 and 9); no roadmap item covers them yet.

Next: test, merge step/archive-old-docs yourself, run `/clear`, then `/next`.
