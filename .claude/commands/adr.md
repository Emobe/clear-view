---
description: Draft a new architecture decision record in docs/adr
argument-hint: <roadmap number, e.g. 3.1. Or the decision in a sentence. Empty means the topic in docs/NEXT.md>
---
You are already in the project root. Do not cd anywhere.

Decision topic: $ARGUMENTS

If that is a number like 3.1, it is an item in docs/ROADMAP.md: use that item's text as the topic. If it is empty, use the blocked topic in docs/NEXT.md. Anything else is the decision in free text.

1. Read docs/PRODUCT.md, docs/NEXT.md, docs/adr/README.md for the rules and template, and every ADR that touches this area. If NEXT.md names a blocked step, use its notes as starting context. If this decision conflicts with docs/PRODUCT.md, say so and stop. If it contradicts an Accepted ADR, say so and name it as superseded in the new file. Do not edit the old ADR's status yourself; tell me.
2. Read the relevant code so Context is facts, not guesses. Search the web for external facts where they matter (current Microsoft docs, the docs for the crate versions pinned in Cargo.toml) and cite the sources in Context. Don't rely on memory for API details.
3. Take the next number (highest existing plus one). Create docs/adr/NNNN-slug.md from the template with Status: Proposed, today's date, and two to four real options with their costs. Put your pick in Decision, labelled as a recommendation.
4. Add the entry to the Index in docs/adr/README.md.
5. If this ADR changes the roadmap, add the resulting numbered items to docs/ROADMAP.md under the right phase, marked "(pending ADR NNNN)".
6. Update docs/NEXT.md (Item as the roadmap number plus its title): if a step was blocked on this topic, keep State: blocked-on-adr, name the new ADR, and say the step resumes after I set its Status to Accepted. Otherwise set State: ready.
7. Tell me to review the ADR, set Status to Accepted myself if I agree, then /clear and run /next.

Do not implement anything. Never set a status to Accepted.
