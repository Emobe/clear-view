# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: awaiting-test
Item: 1.4 Zoom range 1x to 20x
Branch: step/1.4-zoom-range (built, gate passed, PR open, not merged)
Notes:
- Waiting for you to run the Verify list in the PR, say "I approve", and merge. Only then does 1.4 go in STATUS "Done"/"Works" and does this file move to 1.5.
- Built: `ZOOM_MAX` 20. Hotkey zoom step is two-tier: `ZOOM_STEP_FINE` 0.5 below `ZOOM_COARSE_FROM` (4x), `ZOOM_STEP_COARSE` 1.0 from 4x up; stepping out from exactly 4.0 uses the fine step so in/out retrace. Results stay on the slider's 0.1 grid. `ZOOM_STEP` is gone.
- Next item after approval: 1.5 Readability at 10x and above (you judge bilinear vs bicubic and smooth-follow shimmer at high zoom; the agent fixes what you find). 20x is now reachable, so test there too.
- Finding 12 (mouse cannot reach the taskbar) is docked-only, so it is covered by roadmap 3.5 (remove ClipCursor). Finding 13 (taskbar vanishes in fullscreen with a game open) is ignored by your decision.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
