# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: ready
Item: 3.2 (ADR first) Docked overlay design from the prototype, superseding the AppBar docking part of ADR 0002: window styles, capture exclusion, handling extra frames, what the panel shows when the mouse is under it, panel size limits.
Branch: none yet. Run `/adr 3.2`; it writes the ADR on its own branch. No code until you accept it.

Notes for 3.2:
- Read docs/prototypes/docked-overlay.md first. Its "Recommendation for the 3.2 ADR" section is the starting point, and your verdict is "overlay is the way forward".
- Prototype result on your machine: the current overlay window and styles (topmost, `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_LAYERED`, `WDA_EXCLUDEFROMCAPTURE`), placed with `SetWindowPos` at the monitor edge, no AppBar, no `ClipCursor`. No extra frames, clicks pass through, the taskbar is reachable, the desktop stays at 100%, the panel follows the pointer under itself. You verified it all by hand (STATUS Works, 3.1).
- Questions the ADR still has to settle:
  - The 24H2 extra-frame issue was MPO-only and fixed in build 26100.2314; your outputs have no MPO, so the tester's build and MPO support are unchecked. The prototype showed that filtering frames by dirty rects inside the panel is unsafe (a video under the panel produces them); presenting only on change is the fallback.
  - The system cursor shows on top of the panel at its real position while the pointer is under the panel: keep it, hide it, or draw it differently?
  - Panel size limits: not measured.
  - DirectComposition (`DxgiFromVisual`) gave no measured benefit; the prototype recommends staying on `DxgiFromHwnd`.
- The change is backend-only inside `WinHost::apply_layout` (ADR 0004); 3.3 to 3.5 build it, 3.5 deletes appbar.rs and `update_clip_cursor`.
- Model: Opus high (window host at the platform seam, supersedes part of ADR 0002).

After 3.1 (done):
- step/3.1-overlay-prototype holds the prototype code (73bdf0a). It is never merged; keep it for reference until 3.3 is done.
- Side finding: pointer-only capture frames are copied in full and cost about one CPU core while the mouse moves (STATUS Finding 4). Planned for 5.6; moving it earlier is your call.

What comes next:
- 3.3 to 3.5 build the ADR from 3.2.
- 1.8 (you) is still on hold until the tester is free. When they are, `/step 1.8` records their feedback in docs/FEEDBACK.md. Ask which app they compare ZoomText in, whether ClearType is on, which ZoomText hotkeys they rely on and which email app they use (4.2).
- Contour sharpening and toggleable text enhancements (docs/later/text-smoothing.md) still have no roadmap item; adding a "1.10 (ADR first)" is your call.

Still true:
- Display scaling (1.6): the process is Per-Monitor v2, so all cursor, monitor, frame and window coordinates are physical pixels. Keep it that way; a `[dpi]` line on stderr means it isn't. Mixed scaling across monitors is 5.3.
- Interpolation default is Bicubic (your choice in 1.5). Modes: Bilinear 0, Bicubic 1, Sharp 2, CleanEdge 3; the shader.wgsl header, `as_u32` and gfx.rs `write_uniforms` comment must agree. shader.wgsl has clean_edge.wgsl appended at compile time (gfx.rs `SHADER`). Uniforms are 48 bytes; `uniform_size_matches_the_shader_struct` checks gfx.rs against the WGSL struct. `cargo test` validates the shader.
- `cargo test --release -p cv-magnifier bench_modes_4k -- --ignored --nocapture` measures every mode at 3840x2160 headless. Bench only with the GPU idle.
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- `windows` must stay out of every crate but cv-platform-win and cv-tts: `cargo tree -i windows@0.62.2 --workspace -e normal --depth 1` (gpu-allocator and wgpu-hal also appear; they are not workspace crates).
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Not an option in code: the native Windows Magnifier and the Magnification API for zooming or smoothing (your decision). `MagShowSystemCursor` for cursor hiding stays.
- Screenshots of the magnifier don't show the zoomed view (the overlay is excluded from capture). To compare filters by eye, run an ordinary unzoomed screenshot through the shader offscreen (docs/later/text-smoothing.md).
- Jitter: none noticeable at high zoom in 1.5. You expect it may show with caret tracking (Phase 4). Unconfirmed candidates if it does: the 16 ms `SetTimer` (cv-platform-win/src/overlay.rs `TICK_MS`) against 60 Hz vsync, `dt` measured at timer time not present time.
- Finding 12 (mouse cannot reach the taskbar) is docked-only and goes with roadmap 3.5. Finding 13 is ignored by your decision.
- Work in roadmap order.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
