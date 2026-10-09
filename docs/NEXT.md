# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 2.5a Capture survives a lock (STATUS Finding 16): a failed reconnect no longer ends the capture thread; it retries with a short sleep until capture is available again.
Branch: none yet. Use step/2.5a-capture-survives-lock off master.

Notes for 2.5a:
- Cause (Finding 16): locking switches to the secure desktop, so `AcquireNextFrame` gives `DXGI_ERROR_ACCESS_LOST`. `reconnect`, which calls `DuplicateOutput`, then fails with `E_ACCESSDENIED` while the lock screen is up. `run_capture` (crates/cv-magnifier/src/capture.rs) `break`s on a failed reconnect, so the thread ends for good and the view freezes on the last (black) frame.
- The fix belongs in the neutral loop in `run_capture`. cv-platform-win's `Capturer` probably needs no change. Retry `reconnect` with a short sleep (about 250 ms) instead of ending. Log the first failure once, not every retry. While retrying, still honour `desired_output` switches.
- With the retry, the loop never ends on its own. Today the tests end their scripts by refusing a reconnect (`Fake` returns an error when `reconnects` runs out). They need another way to stop: a retry limit or a stop flag that only the tests use, or a scripted fake that panics or returns a sentinel. The 2.4 test `failed_reconnect_ends_the_loop` changes meaning: replace it with "a failed reconnect is retried, then capture resumes".
- `spawn_capture` still ends if the factory fails at startup. Decide whether that should also retry (for example, starting while locked). Keep it as is unless it's cheap.
- Check the Microsoft docs for `DuplicateOutput` errors (`E_ACCESSDENIED`, `DXGI_ERROR_UNSUPPORTED`, `DXGI_ERROR_SESSION_DISCONNECTED`) and `AcquireNextFrame` (`ACCESS_LOST`), and link them in the PR.
- Verify: Win+L with fullscreen on, then unlock: the live desktop comes back, and the view does not slide to the top-left (this is the Finding 15 check that 2.5 could not show). Also try a UAC prompt (for example, run something as administrator) and second-monitor follow afterwards.

After 2.5 (done):
- `Magnifier::tick` is in cv-magnifier/src/magnifier.rs, over a crate-private `Renderer` trait, with 21 fake-host tests. `WinHost` in cv-platform-win/src/overlay.rs implements `OverlayHost`. The message loop runs the tick on WM_TIMER, and `wnd_proc` borrows no state. `ABN_POSCHANGED` sets a flag that is handled before the next tick.
- Phase 3 (overlay docking) replaces `WinHost`'s AppBar and clip code without touching `Magnifier`. Your docked-mode requirements are in STATUS Finding 11. Use them as input to the 3.2 ADR.

What comes next:
- 2.6 wgpu 22 to 30, then 2.7 eframe. 2.7 needs Rust 1.95 or newer for eframe 0.36; the local toolchain is 1.93.1. Update it (`rustup update stable`) before that step.
- 1.8 (you) is still on hold until the tester is free. When they are, `/step 1.8` records their feedback in docs/FEEDBACK.md. Ask which app they compare ZoomText in, whether ClearType is on, which ZoomText hotkeys they rely on and which email app they use (4.2).
- Contour sharpening and toggleable text enhancements (docs/later/text-smoothing.md) still have no roadmap item; adding a "1.10 (ADR first)" is your call.

Still true:
- Display scaling (1.6): the process is Per-Monitor v2, so all cursor, monitor, frame and window coordinates are physical pixels. Keep it that way; a `[dpi]` line on stderr means it isn't. Mixed scaling across monitors is 5.3.
- Interpolation default is Bicubic (your choice in 1.5). Modes: Bilinear 0, Bicubic 1, Sharp 2, CleanEdge 3; the shader.wgsl header, `as_u32` and gfx.rs `write_uniforms` comment must agree. shader.wgsl has clean_edge.wgsl appended at compile time (gfx.rs `SHADER`). Uniforms are 48 bytes; `uniform_size_matches_the_shader_struct` checks gfx.rs against the WGSL struct. `cargo test` validates the shader.
- `cargo test --release -p cv-magnifier bench_modes_4k -- --ignored --nocapture` measures every mode at 3840x2160 headless. Use it for any new shader mode.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- `windows` must stay out of every crate but cv-platform-win and cv-tts: `cargo tree -i windows@0.62.2 --workspace -e normal --depth 1`.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Not an option in code: the native Windows Magnifier and the Magnification API for zooming or smoothing (your decision). `MagShowSystemCursor` for cursor hiding stays.
- Screenshots of the magnifier don't show the zoomed view (the overlay is excluded from capture). To compare filters by eye, run an ordinary unzoomed screenshot through the shader offscreen (docs/later/text-smoothing.md).
- Jitter: none noticeable at high zoom in 1.5. You expect it may show with caret tracking (Phase 4). Unconfirmed candidates if it does: the 16 ms `SetTimer` (cv-platform-win/src/overlay.rs `TICK_MS`) against 60 Hz vsync, `dt` measured at timer time not present time.
- Finding 12 (mouse cannot reach the taskbar) is docked-only and goes with roadmap 3.5. Finding 13 is ignored by your decision.
- Work in roadmap order.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
