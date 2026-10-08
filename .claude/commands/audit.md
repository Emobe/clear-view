---
description: Read-only audit of the repo against its docs. Rewrites docs/STATUS.md and nothing else.
---
You are already in the project root. Do not cd anywhere.

Read CLAUDE.md, docs/PRODUCT.md, docs/ROADMAP.md, docs/STATUS.md and docs/adr/README.md first.

Do not change any source file. The only files you write are docs/STATUS.md and docs/NEXT.md.

1. Git: current branch, uncommitted changes, every branch with its last commit date, and which are merged into master. Name the dead-end branches. Do not delete or merge anything.
2. Build gate: cargo build, cargo clippy, cargo test. Report pass or fail and warning counts, not full output.
3. Check every claim in the existing STATUS.md (Present in code, Not present, Findings) against the code and correct it. If a claim can only be settled by running the app, move it to "Needs your run" instead of guessing.
4. Speech is parked until after v1 (docs/PRODUCT.md). Only check that cv-tts is still off by default; don't audit reader stages unless I ask.
5. Rewrite docs/STATUS.md with these sections: Works (only what a command proved), Broken, Not tested, Missing, Findings, Needs your run. Facts only, with file paths.

6. Rewrite docs/NEXT.md with State: ready and Item set to the number and title of the first roadmap item that STATUS.md shows is not done. Tell me to /clear and run /next.

Then stop and wait for me.
