# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: awaiting-your-test
Item: 1.5 Readability at 10x and above
Branch: step/1.5-readability, PR #20 (open, not merged)
ADR: 0006 Accepted (set on your instruction in this branch).

Built: `Interpolation::Sharp` (third mode, sharp bilinear in shader.wgsl `sample_sharp`), a "Sharp" radio button, and a cv-render test that validates shader.wgsl through `wgpu::naga` (no new dependency). Default is still Bilinear.

Waiting on you: run the Verify list in PR #20. Compare Bilinear, Bicubic and Sharp at 10x and 20x, check for shimmer or jitter while smooth-following, and pick the default.

Where to resume:
- Your chosen default: change `#[default]` on `Interpolation` in cv-core/src/lib.rs and fix the tests that assume Bilinear. Commit on step/1.5-readability.
- Jitter you report: find the cause in the code first. Unconfirmed candidates: the 16 ms `SetTimer` (cv-render/src/lib.rs:292) against 60 Hz vsync with `PresentMode::Fifo`; `dt` measured at timer time, not present time; about a 60 fps cap on faster monitors. If the fix is large, propose 1.5b here instead.
- 1.5 is recorded in STATUS Done only after you say "I approve".
- 1.9 (cleanEdge edge smoothing) runs before 1.7 if you judge Sharp not good enough for the tester.

Still true:
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Not an option in code: the native Windows Magnifier and the Magnification API for zooming or smoothing (your decision). `MagShowSystemCursor` for cursor hiding stays.
- Finding 12 (mouse cannot reach the taskbar) is docked-only and goes with roadmap 3.5. Finding 13 is ignored by your decision.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
