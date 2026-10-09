# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 1.4 Zoom range 1x to 20x
Branch: none yet. Branch from master after PR #17 (step/1.3-hotkeys) is merged.
Notes:
- 1.2 and 1.3 are approved and recorded as done.
- 1.4: `cv_core::ZOOM_MAX` is 10.0 and `ZOOM_STEP` is a placeholder 0.5 (crates/cv-core/src/lib.rs). `sanitize`, the panel slider and `AppState::step_zoom` all use these constants. The step also has to decide the hotkey zoom step and record it in the PR (the slider is `step_by(0.1)`; `step_zoom` rounds to that grid). Check whether a settings file saved with zoom 10 or lower still loads (it will; only the upper bound moves).
- Open: Finding 12 (mouse cannot reach the taskbar, seen while testing 1.1, not caused by it) is not on the roadmap. Tell me where it goes; the likely place is with docking (Finding 11, roadmap 3.5) unless it shows in fullscreen too.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
