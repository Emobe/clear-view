---
description: Plan, then implement one roadmap item on its own branch. Never merges.
argument-hint: <roadmap number, e.g. 1.1. Empty means the Item in docs/NEXT.md>
---
You are already in the project root. Do not cd anywhere.

Target: $ARGUMENTS

If that is a number like 1.1, it is an item in docs/ROADMAP.md: look it up and use that item's full text. I will normally give only the number, so never ask me for a title or description. If it is empty, use the Item in docs/NEXT.md. Anything else is free text describing the work.

Read CLAUDE.md, docs/PRODUCT.md, docs/NEXT.md, docs/STATUS.md, docs/ROADMAP.md (including "Rules for every step") and the ADRs that touch this area. If the target conflicts with docs/PRODUCT.md, stop and tell me rather than building it.

Items marked (you) in the roadmap are mine, not yours. For one of those, ask me whether it's done. Only when I say yes, add it to the Done list in docs/STATUS.md, record anything I report (for example in docs/FEEDBACK.md if the item says so), update docs/NEXT.md for the next item, and tell me to /clear and run /next. Write no code.

Phase A, plan only. Write no code. Give me:
- the roadmap number and title you are working on
- what you checked in the current Microsoft docs and the pinned crate docs, with links
- the files you will create or change
- any public API change
- thread, shutdown and platform-seam implications
- how you will verify it: cargo build, cargo clippy --workspace, cargo test --workspace, plus a short Verify list for me to run by hand on Windows
- anything in the code or an ADR that conflicts with the target

If the plan depends on a decision that no Accepted ADR covers, do not continue. Go to "Blocked on an ADR" below.

If the item is bigger than one branch and one PR should be, propose numbered sub-items (for example 4.8a, 4.8b) instead of a plan, and stop.

Otherwise stop and wait for my approval.

Phase B, only after I approve:
1. Create branch step/<number>-<short-name> from master, for example step/1.3-zoom-hotkeys (or continue the existing one named in NEXT.md).
2. Implement the target and nothing else.
3. Run the gate.
4. Commit on the branch.
5. Completion is recorded only after I say "I approve" following my own testing. Then add the item number to the "Done" list in docs/STATUS.md, move what I verified into "Works", and update docs/NEXT.md for the next item. Do none of that before I say it.
6. Report what changed, repeat the Verify list, and tell me to test, say "I approve", merge, then run /next.

If during Phase B you find the work needs an ADR after all, stop. Commit any partial work to the branch with the message "WIP: blocked on ADR", then go to "Blocked on an ADR".

Blocked on an ADR:
1. Write docs/NEXT.md with State: blocked-on-adr, the item number and title, the branch (or none), the decision needed in one sentence, the options you saw, and 3 to 6 lines on where to resume.
2. Tell me to /clear and run /adr with the item number, then /next.
3. Write no code beyond what is already committed.

Do not merge. Do not start the next item. No speech work unless the item names it. If you find yourself looping on the same failure, stop and report what you know instead of guessing at fixes.
