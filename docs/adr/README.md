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
- 0002 wgpu renderer and AppBar docking (Accepted; AppBar part superseded by 0007)
- 0003 Single TTS thread with UIA and SAPI (Accepted; superseded in parts by 0008, 0012 and 0013, nothing left standing alone)
- 0004 Platform seams and core boundaries (Accepted; rewritten in place for v1 by roadmap 2.1)
- 0005 Hotkey scheme (Accepted; partly superseded by 0014)
- 0006 Upscaling filter for readable text at 10x to 20x (Accepted)
- 0007 Docked overlay (Accepted; supersedes the AppBar docking part of 0002; partly superseded by 0011)
- 0008 Caret sources (Accepted; supersedes the "only the TTS thread adds or removes UIA handlers" part of 0003)
- 0009 Monitors connected or disconnected while running (Accepted; extends 0004, supersedes nothing)
- 0010 Cursor enhancement (Accepted)
- 0011 Docked pan (Accepted; supersedes the "Under the panel" and system cursor parts of 0007)
- 0012 Voice engine (Accepted; supersedes the engine choice and the SAPI-only speak rule of 0003)
- 0013 Speech pipeline (Accepted; supersedes the threading rule of 0003, extends the `EventHub` of 0004)
- 0014 Speech hotkeys (Accepted; extends 0005, supersedes only its "bindings are constants in hotkey.rs")
