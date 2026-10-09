# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: awaiting-test
Item: 1.3 Hotkeys from the ADR
Branch: step/1.3-hotkeys (committed, PR open, not merged). Test it by hand, say "I approve", merge it yourself, then run /next.
Notes:
- Built per docs/adr/0005-hotkey-scheme.md (Accepted). On/off Ctrl+Alt+Shift+Z, zoom in +Up, zoom out +Down, all with `MOD_NOREPEAT`. Registered once at startup in crates/app/src/hotkey.rs; the thread blocks in `GetMessageW`. A failed binding is logged with its error code and shown in the settings window (error 1409 says "in use by another program", any other error says "could not be registered (error N)"). No fallback combo, no retry.
- The failure list is an app-crate `Arc<Mutex<Vec<String>>>` (`hotkey::HotkeyFailures`), not an `AppState` field: it is runtime-only and the panel never writes it, so `apply_changes` is unchanged.
- Zoom step is a placeholder: `cv_core::ZOOM_STEP = 0.5` with `AppState::step_zoom`. 1.4 decides the real step and widens `ZOOM_MAX` to 20 (it is 10 now; `sanitize` and the slider use the constants).
- Recorded as done only after you say "I approve" (1.2 as well: it is still not in the STATUS Done list). Then the next item is 1.4 Zoom range 1x to 20x.
- Open: Finding 12 (mouse cannot reach the taskbar, seen while testing 1.1, not caused by it) is not on the roadmap. Tell me where it goes; the likely place is with docking (Finding 11, roadmap 3.5) unless it shows in fullscreen too.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- The panel window is a fixed 320x340. Up to three hotkey failure lines are added to it; check they do not push the controls off the bottom.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
