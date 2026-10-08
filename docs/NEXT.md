# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: blocked-on-adr
Item: 1.3 Hotkeys from the ADR
ADR: docs/adr/0005-hotkey-scheme.md (Proposed), written for 1.2 on branch step/1.2-hotkey-adr. 1.3 resumes with `/step 1.3` after you set its Status to Accepted. If you reject it, run `/adr 1.2` again with what to change.
Branch: none yet for 1.3. Branch from master after the 0005 PR is merged.
Notes:
- Phase 0 and 1.1 are done. 1.2 is recorded as done when you accept 0005 and say "I approve".
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings. Keep them there. Docs-only steps skip the gate.
- 1.3 per 0005 (recommended option 3): RegisterHotKey only, `MOD_NOREPEAT` on all. On/off Ctrl+Alt+Shift+Z, zoom in Ctrl+Alt+Shift+Up, zoom out Ctrl+Alt+Shift+Down. Register once at startup; a failed binding is shown in the settings window (combo plus "in use by another program") and logged with its GetLastError code; no fallback combo, no retry. Bindings are constants, not saved. The hotkey thread blocks in GetMessageW instead of polling. Zoom step comes from 1.4 (placeholder until then). Verify list must include an elevated window in the foreground.
- Open: Finding 12 (mouse cannot reach the taskbar, seen while testing 1.1, not caused by it) is not on the roadmap. Tell me where it goes; the likely place is with docking (Finding 11, roadmap 3.5) unless it shows in fullscreen too.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field must be added to that macro list.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
