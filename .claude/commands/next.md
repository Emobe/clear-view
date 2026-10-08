---
description: Read the project state and tell me which command to run next, and with which model. Writes nothing.
---
You are already in the project root. Do not cd anywhere.

Read CLAUDE.md (including the model guide), docs/NEXT.md, docs/STATUS.md, docs/ROADMAP.md and the Index in docs/adr/README.md. Run git status and list branches, noting any step/* branch not yet merged to master.

Work out the single next action:

1. NEXT.md says a step is blocked on an ADR:
   - ADR file missing: recommend /adr with the topic.
   - ADR is Proposed: tell me to review it and set its Status to Accepted myself (or reject it). Do not recommend building yet.
   - ADR is Accepted: recommend resuming the blocked step with /step.
2. A step/* branch is finished and unmerged: tell me to test it by hand and merge it myself, listing what STATUS.md or NEXT.md says to check.
3. Otherwise take the first unfinished numbered item in docs/ROADMAP.md that STATUS.md does not show as done. If NEXT.md's Item is free text from an older run, map it to the matching roadmap number. Decide whether it needs a decision that no Accepted ADR covers: a new dependency, a new crate or trait boundary, anything touching the platform seam, or anything that contradicts an ADR. If yes, recommend /adr first. If no, recommend /step. Items marked (ADR first) always need an Accepted ADR.

Answer in this shape and nothing more:

Run: the exact command with the roadmap number only, for example /step 1.1 or /adr 3.1. Never a sentence as the argument.
Model: which model and effort, from the model guide
Why: two lines at most
Before that: anything I must do first (merge, accept an ADR, test), or "nothing"

Do not write or change any file. Do not start the work.
