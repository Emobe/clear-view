# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 4.4 Core: caret source for Windows per the ADR, emitting caret-moved events with a screen rectangle. No magnifier change yet.
ADR: docs/adr/0008-caret-sources.md (roadmap 4.3), Accepted by you 2026-10-09 (PR #39). Run `/step 4.4`. It supersedes ADR 0003's "only the TTS thread adds or removes UIA handlers" rule; ADR 0003's own Status line is yours to change. 4.3 goes in the STATUS Done list when you say "I approve".

Notes for 4.4 (from ADR 0008's recommendation; if you change the ADR, the ADR wins):
- cv-core: `#[non_exhaustive] CoreEvent { CaretMoved { at, rect, source, app }, CaretLost { at, app } }`, `CaretSource { Msaa, Uia, Gui, FocusRect }`, `AppId { pid, exe }`, and a fan-out hub over `std::sync::mpsc` (`subscribe`, `publish` drops closed receivers). Unit tests. No new dependency.
- cv-platform-win: `spawn_caret_source(hub)` with a `shutdown` handle (`PostThreadMessageW(WM_QUIT)`, join, unhook). MTA first; `CUIAutomation8` with `ConnectionTimeout` and `TransactionTimeout` lowered to a few hundred ms (log the value). No window, no UIA event handlers. Message loop via `MsgWaitForMultipleObjectsEx`.
- Wake: out-of-context WinEvents with `WINEVENT_SKIPOWNPROCESS` for foreground, focus and caret show, hide and location change (`OBJID_CARET`). Poll at 20 Hz only while the last answer came from uia or the focus rectangle, until focus or foreground changes.
- Chain, first non-empty wins: msaa (`OBJID_CARET` on `hwndFocus`, `accLocation`) → uia (`GetFocusedElement`, `TextPattern2.GetCaretRange`, else `GetSelection()[0]`, expanded to a character when empty) → gui (`rcCaret`, converted to physical with `LogicalToPhysicalPointForPerMonitorDPI` on `hwndCaret`) → focus rectangle (the focused element's UIA `BoundingRectangle`, else the `hwndFocus` window rect). The probe (cv-platform-win/examples/caret_probe.rs) has working code for the first three.
- Events only on change. `[caret]` log lines on source changes and `[caret] no source in <exe>` once per foreground change.
- The UIA and MSAA `windows` features (`Win32_UI_Accessibility`, `Win32_System_Com`, `Win32_System_Ole`, `Win32_System_Variant`) move from cv-platform-win's dev-dependencies to normal ones. `windows` still only in cv-platform-win and cv-tts.
- No magnifier change in 4.4. app spawns the thread and shuts it down with the overlay; something must hold a receiver or log the events so you can see them (a `[caret]` log is enough).
- Verify: each 4.2 app with the log on; Notepad and Explorer rename at 150% with an outline of the reported rect (reuse the probe's outline code in a dev-only form); exit paths leave nothing behind.
- If the step is bigger than one PR, split (for example hub and types first, then the thread) and propose sub-items here.
- Model: Sonnet high (CLAUDE.md model guide for /step).

4.2a (you) can run any time before 4.8: the caret probe in desktop Word and in Chrome if installed (ROADMAP 4.2a).

On hold by your decision (2026-10-09): everything involving the tester, until you raise it: 1.8, 3.6, 4.9, 6.8, and the tester's email app in 4.2. /next skips them. 4.2 was recorded without the email app; that app is measured when you pick the tester work up again.

Held, for when you pick it up again — notes for 3.6:
- The agent's part, like 1.7 (PR #22): plain `cargo build --release` with no `RUSTFLAGS` gives `target\release\clear-view.exe` (static CRT from `.cargo/config.toml`); update TESTER.md; tag v0.2.0. v0.1.0 is an annotated tag ("Tester build 1 (roadmap 1.7)") on bb81f33; tag the merged commit the same way only after you approve, and ask before pushing the tag.
- TESTER.md line 29 still says "use Fullscreen. The docked modes … are old and being replaced; in them the mouse cannot reach the taskbar." That is wrong since 3.4/3.5: docking is an overlay on any edge, the desktop is not resized, the mouse moves under the panel (which shows that area magnified, cursor hidden inside it), panel size 10–90%.
- Your part (ADR 0007): before handing the build over, run it on the tester's machine and read the `[system] Windows … build …` and `[system] output N …: multiplane overlay support …` lines. If the build is older than 26100.2314 with MPO true, or the panel causes extra frames, present-only-on-change becomes a sub-item before 3.6 is done. The step should ask you for those values and record them.
- Feedback goes into docs/FEEDBACK.md (does not exist yet; 1.8 creates it too).
- Model: Sonnet high (CLAUDE.md model guide for /step).

What comes next:
- 3.5 (PR #36) finished ADR 0007: no AppBar, no `ClipCursor` anywhere (exit included), `Magnifier::resized` gone.
- 4.1 (PR #37) added the caret probe, cv-platform-win/examples/caret_probe.rs. Its extra `windows` features are dev-dependencies, so the app doesn't compile them.
- 4.2 (you) measured the caret sources; results in docs/prototypes/caret-sources.md.
- 4.3 wrote ADR 0008 (Accepted) from those results; 4.4 to 4.8 build it.
- 1.8 (you) is still on hold until the tester is free. When they are, `/step 1.8` records their feedback in docs/FEEDBACK.md. Ask which app they compare ZoomText in, whether ClearType is on, which ZoomText hotkeys they rely on and which email app they use (4.2).
- Contour sharpening and toggleable text enhancements (docs/later/text-smoothing.md) still have no roadmap item; adding a "1.10 (ADR first)" is your call.
- Side finding from 3.1: pointer-only capture frames are copied in full and cost about one CPU core while the mouse moves (STATUS Finding 4). Planned for 5.6; moving it earlier is your call.

Still true:
- Display scaling (1.6): the process is Per-Monitor v2, so all cursor, monitor, frame and window coordinates are physical pixels. Keep it that way; a `[dpi]` line on stderr means it isn't. Mixed scaling across monitors is 5.3.
- Docked panel monitor: the panel goes on the monitor active when the layout is applied, origin included since 3.3. Multi-monitor docking is 5.1.
- Interpolation default is Bicubic (your choice in 1.5). Modes: Bilinear 0, Bicubic 1, Sharp 2, CleanEdge 3; the shader.wgsl header, `as_u32` and gfx.rs `write_uniforms` comment must agree. shader.wgsl has clean_edge.wgsl appended at compile time (gfx.rs `SHADER`). Uniforms are 48 bytes; `uniform_size_matches_the_shader_struct` checks gfx.rs against the WGSL struct. `cargo test` validates the shader.
- `cargo test --release -p cv-magnifier bench_modes_4k -- --ignored --nocapture` measures every mode at 3840x2160 headless. Bench only with the GPU idle.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- `windows` must stay out of every crate but cv-platform-win and cv-tts: `cargo tree -i windows@0.62.2 --workspace -e normal --depth 1` (gpu-allocator and wgpu-hal also appear; they are not workspace crates).
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Not an option in code: the native Windows Magnifier and the Magnification API for zooming or smoothing (your decision). `MagShowSystemCursor` for cursor hiding stays.
- Screenshots of the magnifier don't show the zoomed view (the overlay is excluded from capture). To compare filters by eye, run an ordinary unzoomed screenshot through the shader offscreen (docs/later/text-smoothing.md).
- Jitter: none noticeable at high zoom in 1.5. You expect it may show with caret tracking (Phase 4). Unconfirmed candidates if it does: the 16 ms `SetTimer` (cv-platform-win/src/overlay.rs `TICK_MS`) against 60 Hz vsync, `dt` measured at timer time not present time.
- Finding 12 (mouse cannot reach the taskbar) is resolved by 3.4. Finding 13 is ignored by your decision; 4.1 found its likely cause (a topmost window covering the monitor drops the taskbar), see STATUS.
- Panel size is 10–90% (`cv_core::PANEL_SIZE_MIN`, `PANEL_SIZE_MAX`, ADR 0007); the numbers can change in a step without a new ADR.
- Work in roadmap order.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
