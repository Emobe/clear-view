---
description: Plan, then implement one roadmap item or reader stage on its own branch. Never merges.
argument-hint: <roadmap number, e.g. 1.1. Empty means the Item in docs/NEXT.md>
---
You are already in the project root. Do not cd anywhere.

Target: $ARGUMENTS

If that is a number like 1.1, it is an item in docs/ROADMAP.md: use that item's text. If it is empty, use the Item in docs/NEXT.md. Anything else is free text describing the work.

Read CLAUDE.md, docs/NEXT.md, docs/STATUS.md, docs/ROADMAP.md and the ADRs that touch this area. If the target is a reader stage, read tts-plan.md too.

Phase A, plan only. Write no code. Give me:
- the files you will create or change
- any public API change
- thread, shutdown and platform-seam implications
- how you will verify it: the stage's Verify list, plus cargo clippy and cargo test
- anything in the code or an ADR that conflicts with the target

If the plan depends on a decision that no Accepted ADR covers, do not continue. Go to "Blocked on an ADR" below.

Otherwise stop and wait for my approval.

Phase B, only after I approve:
1. Create branch step/<short-name> from master (or continue the existing one named in NEXT.md).
2. Implement the target and nothing else.
3. Run the gate.
4. Commit on the branch.
5. Update docs/STATUS.md and tick the stage in tts-plan.md.
6. Write docs/NEXT.md (Item as the roadmap number plus its title) with State: step-in-review, the branch, and what I need to test by hand.
7. Report what changed and tell me to test, merge, then run /next.

If during Phase B you find the work needs an ADR after all, stop. Commit any partial work to the branch with the message "WIP: blocked on ADR", then go to "Blocked on an ADR".

Blocked on an ADR:
1. Write docs/NEXT.md with State: blocked-on-adr, the item, the branch (or none), the decision needed in one sentence, the options you saw, and 3 to 6 lines on where to resume.
2. Tell me to /clear and run /adr with that topic, then /next.
3. Write no code beyond what is already committed.

Do not merge. Do not start the next item. If you find yourself looping on the same failure, stop and report what you know instead of guessing at fixes.
