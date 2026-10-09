# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 1.5 Readability at 10x and above
Branch: none yet. Branch from master after PR #18 (step/1.4-zoom-range) is merged.
Notes:
- 1.4 is approved and recorded as done.
- 1.5: you judge, the agent fixes what you find. You already reported 20x as "quite blurry but not terrible" and asked for smoothing or sharpening (STATUS Finding 14). Compare bilinear and bicubic at 10x and 20x, check for shimmer or jitter while smooth-following at high zoom, pick the default. Interpolation is in cv-render/src/shader.wgsl (`interp_mode`, set from `AppState.interpolation`); a sharpening option would be a new shader mode and a new `AppState` field, so check whether that needs more than a step.
- Finding 12 (mouse cannot reach the taskbar) is docked-only, so it is covered by roadmap 3.5 (remove ClipCursor). Finding 13 (taskbar vanishes in fullscreen with a game open) is ignored by your decision.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
