---
description: Read the project state and tell me which command to run next, and with which model. Writes nothing.
---
You are already in the project root. Do not cd anywhere.

Read CLAUDE.md (including the model guide), docs/PRODUCT.md, docs/NEXT.md, docs/STATUS.md, docs/ROADMAP.md and the Index in docs/adr/README.md. Run git status and list branches, noting any step/* branch not yet merged to master.

Roadmap numbers refer to docs/ROADMAP.md only. Done items listed in STATUS.md as "old 1.1", "old 2.1" and so on come from a previous roadmap and are not the same items as 1.1 or 2.1 now. If STATUS.md still shows them without the "old" label, treat them as old items anyway.

Work out the single next action:

1. NEXT.md says a step is blocked on an ADR:
   - ADR file missing: recommend /adr with the roadmap number.
   - ADR is Proposed: tell me to review it and set its Status to Accepted myself (or reject it). Do not recommend building yet.
   - ADR is Accepted: recommend resuming the blocked step with /step and its number.
2. A step/* branch is finished and unmerged:
   - NEXT.md says `Stacking: on`: unmerged branches do not block. Treat the items NEXT.md lists on the integration branch as coded but not done, and go to 3 for the first item after them. Put "test the integration branch when you're back" in "Before that", not a merge.
   - Otherwise: tell me to test it by hand and merge it myself, listing what STATUS.md or NEXT.md says to check.
3. Otherwise take the first unfinished numbered item in docs/ROADMAP.md that STATUS.md does not show as done. If NEXT.md's Item is free text from an older run, map it to the matching roadmap number.
   - Item marked (you): tell me in one or two lines what to do. Run: /step with its number, which records it once I confirm.
   - Item marked (ADR first), or one that needs a decision no Accepted ADR covers (a new dependency, a new crate or trait boundary, anything touching the platform seam, anything that contradicts an ADR or docs/PRODUCT.md): recommend /adr with its number.
   - Anything else: recommend /step with its number.

Answer in this shape and nothing more:

Run: the exact command with the roadmap number only, for example /step 1.1 or /adr 3.1. Never a sentence or a title as the argument.
Item: the number and title from the roadmap, so I can see what it is
Model: which model and effort, from the model guide
Why: two lines at most
Before that: anything I must do first (merge, accept an ADR, test), or "nothing"

Do not write or change any file. Do not start the work.
