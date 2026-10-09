# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 4.3 (ADR first) Caret sources, superseding ADR 0003's rule that only the TTS thread owns UIA (UIA moves to a core thread): which source per app type, fallback order, how the core reports a caret position, and what happens in elevated windows without UIAccess.
ADR: needed. Run `/adr 4.3` with Opus high: it touches a platform seam and the core's thread model, and supersedes part of ADR 0003.

On hold by your decision (2026-10-09): everything involving the tester, until you raise it: 1.8, 3.6, 4.9, 6.8, and the tester's email app in 4.2. /next skips them. 4.2 was recorded without the email app; that app is measured when you pick the tester work up again.

Notes for 4.3, from 4.2 (docs/prototypes/caret-sources.md):
- On the caret: Notepad and Explorer rename all three; Brave and Edge msaa and uia (no gui); Windows Terminal uia only. uia gave a position in every app that had one.
- No source in the Explorer address bar and search box (XAML `TextBox`): `TextPattern2.GetCaretRange` is active but `GetBoundingRectangles` is empty, also after expanding to a character and with the caret mid-text. Needs a non-caret fallback (for example the focused element's rectangle).
- LibreOffice: none (not on the v1 list).
- Not measured: Word (not installed; a v1 Must app), Chrome (not installed; Brave and Edge are Chromium), the tester's email app (on hold), elevated windows. The ADR should say how Word gets measured, before 4.8 at the latest.
- UIA method per app (`GetCaretRange` vs `GetSelection`), costs outside Explorer and line-end behaviour were not recorded. Explorer costs: gui 0 ms, msaa under 1 ms, uia 3–12 ms (first calls 75 ms msaa, 36 ms uia).
- ADR 0003's carry-over rules (MTA first, plain data over channels, no COM pointers across threads) apply to the core thread. The probe follows them (UIA thread owns no window).

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
