# Architecture decision records

One file per decision: `NNNN-short-title.md`, numbered in order, never renumbered. A decision is changed by a new ADR that supersedes the old one, not by editing history.

Status values: Proposed, Accepted, Superseded by NNNN, Rejected.

Rules for agents:
- Read the ADRs touching the area before changing it.
- Never mark an ADR Accepted. Only the owner does.
- A change that contradicts an Accepted ADR needs a new ADR first.

## Template

```
# NNNN: Title

Status: Proposed
Date: YYYY-MM-DD

## Context
What forces this decision. Facts only.

## Options
1. Option, with what it costs.
2. Option, with what it costs.

## Decision
What we do and why this option.

## Consequences
What gets easier, what gets harder, what to revisit and when.
```

## Index

- 0001 CPU readback for capture (Accepted)
- 0002 wgpu renderer and AppBar docking (Accepted)
- 0003 Single TTS thread with UIA and SAPI (Accepted)
- 0004 Platform seams and core boundaries (Accepted; rewritten in place for v1 by roadmap 2.1)
- 0005 Hotkey scheme (Accepted)
- 0006 Upscaling filter for readable text at 10x to 20x (Accepted)
