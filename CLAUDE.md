# CLAUDE.md

Guidance for Claude Code in this repository. This file is read at session start only; start a new session after editing it.

## What this project is

`clear-view` is a screen magnifier and screen reader (ZoomText alternative) written in Rust. Windows is the working target. X11 and Wayland support is planned after Windows v1; keep platform-specific code from spreading (see docs/adr/0004).

It uses DXGI Desktop Duplication for capture, wgpu for rendering, egui for the settings panel, and UI Automation plus SAPI for the reader.

## Working rules

- You are already in the project root. Do not cd anywhere.
- Never merge branches to master. Create branches, commit to them, then stop and wait for explicit instruction.
- Plan before code on anything non-trivial and wait for approval.
- Do the task given. Do not start the next stage or item on your own.
- When debugging, find the root cause by reading the code. Do not paper over it.
- Before changing architecture, read the ADRs in docs/adr. A change that contradicts an Accepted ADR needs a new ADR first. Never mark an ADR Accepted.
- Keep docs/STATUS.md and the stage checkboxes in tts-plan.md true after each step.
- Context is cleared between commands. End every session with docs/NEXT.md accurate.

## Where things are written down

- docs/STATUS.md: what works, what is missing, findings
- docs/ROADMAP.md: phases and order of work
- docs/adr/: decisions and why
- tts-plan.md: reader stages, architecture and dead ends
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

Windows only for now. Will not compile on other platforms.

## Crate structure

- `cv-core`: shared types (Frame, AppState, SharedState, DisplayMode, ColorFilter, Interpolation). Platform-neutral; keep it that way.
- `cv-capture`: DXGI Desktop Duplication, staging texture, CPU readback
- `cv-render`: wgpu pipeline (gfx.rs, shader.wgsl), overlay window, AppBar, cursor handling
- `cv-tts`: reader thread (SAPI, UIA)
- `app`: main.rs, egui settings panel, hotkey thread

## Architecture

Threads sharing `Arc<RwLock<AppState>>`:

- **Main thread**: eframe (egui settings panel), always on top. app.rs also spawns a 100 ms repaint thread; see docs/STATUS.md finding 2.
- **Capture thread**: DXGI Desktop Duplication to D3D11 staging texture to CPU BGRA8, stored in `Arc<Mutex<Option<Arc<Frame>>>>`. Switches monitor when the render thread changes `desired_output`.
- **Render thread**: overlay window and wgpu pipeline. Uploads the frame when it changed, blits through the shader, lerps the cursor for smooth follow, handles docking and monitor changes.
- **Hotkey thread**: `RegisterHotKey` (Win+=) toggles `AppState.enabled`.
- **TTS thread**: owns all COM, UIA and SAPI state (MTA). See tts-plan.md and ADR 0003.

## Key design decisions

- **Self-capture prevention**: `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` keeps the overlay out of its own DXGI capture.
- **Overlay flags**: `WS_POPUP | WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_LAYERED`. Clicks pass through, window stays on top.
- **Overlay starts hidden**: created without `WS_VISIBLE`; shown and hidden with `ShowWindow` when the enabled state changes.
- **Smooth follow**: frame-rate-independent lerp, `alpha = 1.0 - (1.0 - smooth_speed).powf(dt * 60.0)`.
- **Interpolation**: bilinear or bicubic (Catmull-Rom) in the shader. Colour filters (none, inverted, greyscale, greyscale+inverted) are shader-level.
- **Cursor**: system cursor hidden in fullscreen; a software circle is drawn in the shader at the cursor position in output pixel space. Cursor and frame coordinates must be in the same space before any layout math.
- **Docked panel**: SHAppBarMessage (ABM_NEW, ABM_SETPOS, ABM_ACTIVATE, ABM_REMOVE) on any of four edges. Work area shifts to fit. AppBar released on toggle off or app exit. Panel size is a percentage of the screen dimension.
- **Display modes**: fullscreen or docked (top, bottom, left, right). Switching is immediate. Win+= toggles on and off in all modes and restores the work area when hidden.
- **windows crate**: version 0.62. `D3D11CreateDevice` software param is `HMODULE::default()`, not `None`.
- **Rust 2024 edition**: needs explicit `unsafe {}` blocks inside `unsafe fn` bodies.

## Dead ends: do not retry

- **D3D11 shared device context**: the immediate context is single-threaded and frames never rendered. Use CPU readback (ADR 0001).
- **Mixing screen-space and frame-space coordinates**: convert first, then do layout math.
- **GDI StretchBlt for rendering**: replaced by wgpu for shader support (ADR 0002).
- Reader dead ends are listed in tts-plan.md.

## DXGI error handling

| Error | Action |
|---|---|
| `DXGI_ERROR_WAIT_TIMEOUT` | Retry `AcquireNextFrame` |
| `DXGI_ERROR_ACCESS_LOST` | Re-create `IDXGIOutputDuplication` |
| `DXGI_ERROR_DEVICE_REMOVED` | Re-create D3D11 device and all resources |

## Windows crate

Use the `windows` crate (not `winapi` or `windows-sys`). Add new features to the `windows` dependency in the workspace `Cargo.toml`. `RegisterHotKey`, `MOD_WIN` and `VK_OEM_PLUS` are in `Win32::UI::Input::KeyboardAndMouse`, not `WindowsAndMessaging`.
