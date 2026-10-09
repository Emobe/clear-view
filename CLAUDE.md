# CLAUDE.md

Guidance for Claude Code in this repository. This file is read at session start only; start a new session after editing it.

Read docs/PRODUCT.md first. It says what v1 is and what it isn't, and it wins if any other doc disagrees with it.

## What this project is

`clear-view` is a screen magnifier (ZoomText alternative) written in Rust, built on a core that a screen reader can use later. Windows is the working target. X11 and Wayland support is planned after Windows v1; keep platform-specific code from spreading (see docs/adr/0004).

It uses DXGI Desktop Duplication for capture, wgpu for rendering and egui for the settings panel. The parked reader uses UI Automation plus SAPI.

## Speech is out of scope until after v1

Speech is off and is not worked on before Windows v1 ships. cv-tts is behind the `tts` cargo feature, off by default: a plain `cargo build` or `cargo run` does not start the TTS thread and hides the TTS controls. Do not touch cv-tts unless a step names it. The reader plan is parked in docs/later/tts-plan.md.

## Working rules

- You are already in the project root. Do not cd anywhere.
- Never merge branches to master. Create branches, commit to them, then stop and wait for explicit instruction.
- After finishing a `/step` or `/adr`, push the branch and open a PR with the `gh` CLI. The `origin` remote is SSH and does not work here, so push over HTTPS with the `gh` credential helper: `git -c credential.helper= -c credential.helper='!gh auth git-credential' push -u https://github.com/Emobe/clear-view.git <branch>`. Then `gh pr create`. Never merge a PR; stop once it is open and give me the URL.
- Plan before code on anything non-trivial and wait for approval.
- Do the task given. Do not start the next stage or item on your own.
- When debugging, find the root cause by reading the code. Do not paper over it.
- Before changing architecture, read the ADRs in docs/adr. A change that contradicts an Accepted ADR needs a new ADR first. Never mark an ADR Accepted.
- Completion is recorded only after I say "I approve" following my own testing. Then add the item number to the "Done" list in docs/STATUS.md, move what I verified into "Works", and update docs/NEXT.md for the next item. Do none of that before I say it.
- Context is cleared between commands. End every session with docs/NEXT.md accurate.

## Where things are written down

- docs/PRODUCT.md: what v1 is, who it's for, what is out of scope. Read first.
- docs/STATUS.md: what works, what is missing, findings
- docs/ROADMAP.md: phases and order of work
- docs/adr/: decisions and why
- docs/later/tts-plan.md: reader stages, architecture and dead ends (parked until after v1)
- docs/NEXT.md: the baton between sessions
- Roadmap items are numbered (1.1, 3.1). Commands take the number: `/step 1.1`, `/adr 3.1`.
- Commands in .claude/commands: `/next` (says what to run and with which model), `/audit`, `/step <item>`, `/adr <decision>`

## Model guide

/next quotes this, so keep it current.

- `/next` and `/audit`: Sonnet. `/next` only routes, so medium effort is enough; `/audit` on high.
- `/step`: Sonnet high, for both the plan and the build.
- `/adr`: Opus high when the decision touches a platform seam, a crate or trait boundary, a new dependency, or more than one crate. Sonnet high for small local decisions.
- The same failure twice in a row: switch to Opus for one diagnosis, then back.
- Run `/clear` between `/next`, `/adr` and `/step`. Opus then starts from the short files, not a long build history.

## Build and run

```bash
cargo build
cargo run
cargo build --release
```

When running cargo build, suppress warnings with `RUSTFLAGS="-Awarnings" cargo build` unless specifically debugging a warning. Only show errors.

The tester exe is built with plain `cargo build --release` and no `RUSTFLAGS`: the variable replaces the static C runtime flag in `.cargo/config.toml`. Output is `target\release\clear-view.exe`; TESTER.md goes with it.

Windows only for now. Will not compile on other platforms.

### Gate before every PR

All three must pass before a PR is opened. Steps that change only docs skip the gate.

```bash
cargo build
cargo clippy --workspace --all-targets   # 0 warnings
cargo test --workspace
```

## Crate structure

- `cv-core`: shared types (Frame, AppState, SharedState, DisplayMode, ColorFilter, Interpolation, the hotkey `Action` and `AppState::apply`), pure view math in `geometry` (crop, zoom, lerp, cursor mapping), and core events in `events` (`CoreEvent` with `CaretMoved` and `CaretLost`, `CaretSource`, `AppId`, and the fan-out `EventHub`, ADR 0008), and the tracking policy in `tracking` (`Tracker`: pointer or caret, return threshold, caret margin; roadmap 4.5, run by the magnifier in fullscreen since 4.6), all with unit tests. Platform-neutral; keep it that way.
- `cv-platform-win`: the Windows backend (ADR 0004). DXGI Desktop Duplication capture (staging texture, CPU readback), DPI awareness, the hotkey thread and its `RegisterHotKey` bindings, the overlay (overlay.rs: window, render thread and loop, fullscreen and docked placement, cursor hiding, clean exit), and the caret thread (caret/, ADR 0008: the source chain in read.rs, the pure publish and poll logic in tracker.rs with unit tests). Starts with `#![cfg(windows)]`. Dev-only examples, not shipped: `caret_probe` (roadmap 4.1) reads every caret source; `caret_events` runs the caret thread and outlines each reported rect. Both share `examples/common/outline.rs`.
- `cv-magnifier`: the platform-neutral magnifier. `Magnifier::tick` (magnifier.rs: pointer follow, layout decisions, lerp, crop, upload and uniform caching, unit-tested with a fake host and renderer); the `OverlayHost`/`Layout` and `CaptureSource` seams and the capture loop; wgpu pipeline (gfx.rs, shader.wgsl, clean_edge.wgsl), which takes raw display and window handles. No `windows` dependency.
- `cv-tts`: reader thread (SAPI, UIA). Built only with the `tts` feature.
- `app`: main.rs wiring, egui settings panel, settings persistence. Picks the backend with `#[cfg(windows)] use cv_platform_win as platform;` and has no `windows` dependency.

Only cv-platform-win and parked cv-tts may depend on `windows`. Check with `cargo tree -i windows@0.62.2 --workspace -e normal --depth 1`.

## Architecture

Threads sharing `Arc<RwLock<AppState>>`:

- **Main thread**: eframe (egui settings panel), always on top. No polling: the panel repaints on input, or when another thread calls `request_repaint()` through `app::RepaintSlot`. Any thread that changes state the panel shows must call it. Each frame the panel clones `AppState` under a read lock and writes back only the fields it changed (app.rs `apply_changes`).
- **Capture thread**: DXGI Desktop Duplication to D3D11 staging texture to CPU BGRA8, stored in `Arc<Mutex<Option<Arc<Frame>>>>`. Switches monitor when the render thread changes `desired_output`.
- **Render thread**: owned by cv-platform-win. Creates the overlay window; its message loop runs `Magnifier::tick` on each 16 ms timer, passing `WinHost` (the `OverlayHost`: window placement and cursor hiding). The loop runs the tick itself on `WM_TIMER`; `wnd_proc` borrows no state, and the magnifier must never be called from inside its own `apply_layout`. Each tick drains the render thread's `EventHub` receiver (enabled or not). In fullscreen the `Tracker` picks the target (pointer or caret) and the active monitor follows the target; docked follows the pointer until 4.7. The software cursor circle is drawn at the real pointer, and not at all when the pointer is out of view.
- **Hotkey thread**: spawned by cv-platform-win. `RegisterHotKey` (Ctrl+Alt+Shift+Z toggle, +Up and +Down zoom; no Win-key combos, the shell owns them) reports each press as an `Action`; app applies it with `AppState::apply` and wakes the panel.
- **Caret thread**: spawned by cv-platform-win (`spawn_caret_source`, ADR 0008). MTA COM, no window, no UIA event handlers, UIA timeouts lowered to 500 ms. Wakes on out-of-context WinEvents (foreground, focus, caret show, hide and move) and reads msaa → uia → gui → focus rectangle; polls at 20 Hz only after uia or the focus rectangle answered, until focus or foreground changes. Publishes `CaretMoved` and `CaretLost` to the `EventHub` only on change and logs `[caret]` lines. app shuts it down after the overlay; it changes nothing system-wide.
- **TTS thread**: only with the `tts` feature. Owns all COM, UIA and SAPI state (MTA). See docs/later/tts-plan.md and ADR 0003.

## Key design decisions

- **Self-capture prevention**: `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` keeps the overlay out of its own DXGI capture.
- **Overlay flags**: `WS_POPUP | WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_LAYERED`. Clicks pass through, window stays on top.
- **Overlay starts hidden**: created without `WS_VISIBLE`; shown and hidden with `ShowWindow` when the enabled state changes.
- **Smooth follow**: frame-rate-independent lerp in `cv-core::geometry`, `alpha = 1.0 - (1.0 - smooth_speed).powf(dt * 60.0)`.
- **Interpolation**: bilinear or bicubic (Catmull-Rom) in the shader. Colour filters (none, inverted, greyscale, greyscale+inverted) are shader-level.
- **Cursor**: system cursor hidden in fullscreen; a software circle is drawn in the shader at the cursor position in output pixel space. Cursor and frame coordinates must be in the same space before any layout math.
- **Docked panel** (ADR 0007): the same overlay window, placed with `SetWindowPos` to `geometry::docked_rect` on any of four edges, on top of the desktop. No AppBar and no `ClipCursor`: the work area is never changed and the mouse moves under the panel, which shows that area magnified. The system cursor is hidden while the pointer is inside the panel (`OverlayHost::set_system_cursor`). Panel size is 10–90% of the monitor dimension it spans. Exit (window close, Ctrl+C, console close, panic) shows the system cursor again and destroys the window; there is no work area or clip to restore.
- **Display modes**: fullscreen or docked (top, bottom, left, right). Switching is immediate. Ctrl+Alt+Shift+Z toggles on and off in all modes and shows the system cursor when hidden.
- **windows crate**: version 0.62. `D3D11CreateDevice` software param is `HMODULE::default()`, not `None`.
- **Rust 2024 edition**: needs explicit `unsafe {}` blocks inside `unsafe fn` bodies.

## Dead ends: do not retry

- **D3D11 shared device context**: the immediate context is single-threaded and frames never rendered. Use CPU readback (ADR 0001).
- **Mixing screen-space and frame-space coordinates**: convert first, then do layout math.
- **GDI StretchBlt for rendering**: replaced by wgpu for shader support (ADR 0002).
- Reader dead ends are listed in docs/later/tts-plan.md.

## DXGI error handling

| Error | Action |
|---|---|
| `DXGI_ERROR_WAIT_TIMEOUT` | Retry `AcquireNextFrame` |
| `DXGI_ERROR_ACCESS_LOST` | Re-create `IDXGIOutputDuplication` |
| `DXGI_ERROR_DEVICE_REMOVED` | Re-create D3D11 device and all resources |

## Windows crate

Use the `windows` crate (not `winapi` or `windows-sys`). Add new features to the `windows` dependency in the workspace `Cargo.toml`. `RegisterHotKey`, `MOD_CONTROL`, `MOD_ALT`, `MOD_SHIFT` and `VK_Z` are in `Win32::UI::Input::KeyboardAndMouse`, not `WindowsAndMessaging`.
