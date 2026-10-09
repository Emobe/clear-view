# 0004: Platform seams and core boundaries

Status: Proposed
Date: 2026-10-09 (rewritten in place for v1 scope by roadmap 2.1; the 2026-10-08 brief covered Windows, X11, Wayland and the reader)

## Context

Roadmap 2.1. PRODUCT.md has three layers: a **core** that watches the system and reports positions (v1: pointer and caret; later focus, element under the pointer, text), the **magnifier** (capture, rendering, tracking policy, modes, hotkeys, settings UI) and, after v1, a **reader** on the same core. Platform code stays behind a seam so Linux can come later, but no Linux work happens before Windows v1, and crate names and trait boundaries are decided in an ADR. This is that ADR. It also decides when to upgrade wgpu 22 and eframe 0.29 (ROADMAP Open decisions), and its output replaces roadmap 2.2 with numbered sub-items.

Out of scope, deferred and not answered here: the Wayland capture strategy, the cursor position on Wayland while the overlay passes input through, and the reader's AccessibilityBackend and SpeechEngine interfaces (questions 4 to 6 of the old brief). cv-tts is not touched.

### What the code does today

Five crates, 3190 lines of Rust (`wc -l crates/*/src/*.rs`).

- **cv-core** (parking_lot, serde): `AppState`, `SharedState`, `Frame`, `FrameState`, `OutputInfo`, enums, and pure view math in `geometry` with tests. No platform types.
- **cv-capture** (windows): DXGI Desktop Duplication, staging texture, CPU readback to `Frame` (`Capturer::next_frame`, `switch_output`, `reconnect`), and `enumerate_outputs`, whose `idx` is the DXGI output index the duplication uses. Errors are `windows::core::Result`.
- **cv-render** (windows, wgpu 22, raw-window-handle 0.6, pollster):
  - gfx.rs is wgpu only, except that `WgpuState::new` takes a Win32 `HWND` and builds a `Win32WindowHandle` for `create_surface_unsafe` (gfx.rs:32-50). The rest of gfx.rs and the shaders are portable.
  - lib.rs (699 lines) mixes everything else on one thread: Win32 window creation with the overlay styles and `WDA_EXCLUDEFROMCAPTURE`, the message loop driven by a 16 ms `SetTimer`, `wnd_proc`, the AppBar callback, `ClipCursor` every tick while docked, `MagShowSystemCursor`, monitor follow via `GetCursorPos` and `geometry::output_at`, the per-tick lerp, crop, frame upload and uniform-write caching, and the clean-exit machinery (`OverlayHandle::shutdown`, `TeardownGuard`, `console_ctrl_handler`) from 1.1. Per-tick logic and Win32 calls are interleaved in `on_timer` (lib.rs:374-654), which also has to keep `SetWindowPos` outside any `WIN_DATA` borrow because it re-enters `wnd_proc`.
  - appbar.rs: `SHAppBarMessage`, replaced by overlay docking in Phase 3 and deleted in 3.5.
- **app** (eframe 0.29 with glow, egui, windows): main.rs sets Per-Monitor v2 DPI awareness (main.rs:34-47), runs the capture loop inline (main.rs:59-91), spawns the render, hotkey and settings threads; hotkey.rs is `RegisterHotKey` plus the action dispatch (`Toggle`, `ZoomIn`, `ZoomOut` → `AppState`); app.rs is the egui panel; settings.rs is JSON in the config dir via `dirs` (portable).
- **cv-tts**: parked behind the `tts` feature (ADR 0003).

Threads share `Arc<RwLock<AppState>>` and the `FrameState` slot. Nothing has a test seam: the per-tick behaviour (enable and disable transitions, mode changes, monitor switch, uniform caching) can only be checked by running the app by hand. That is STATUS Finding 5.

Coming work that lands in this code: overlay docking (3.1 to 3.5) replaces AppBar and ClipCursor; caret tracking (4.3 to 4.7) adds a caret source in the core and a tracking policy; multi-monitor docked (5.1, 5.2); performance (5.5, 5.6) may revisit the CPU frame path (ADR 0001).

### Dependencies (checked 2026-10-09)

Locked versions: wgpu 22.1.0, naga 22.1.0, eframe and egui 0.29.1, winit 0.30.13, raw-window-handle 0.6.2, windows 0.62.2. Local toolchain rustc 1.93.1; rustup reports 1.99.0 available.

- Latest wgpu is 30.0.1 (2026-08-22), MSRV 1.87. wgpu 29 set a policy of never requiring more than stable minus 3 ([crates.io versions](https://crates.io/crates/wgpu/versions), [CHANGELOG](https://github.com/gfx-rs/wgpu/blob/trunk/CHANGELOG.md), v29.0.0 "MSRV update").
- Breaking changes between 22 and 30 that hit gfx.rs, from the CHANGELOG: `entry_point` became `Option<&str>` (v23); `Instance::new` takes `&InstanceDescriptor` (v24); `request_adapter` returns `Result` (v25); `Maintain` renamed `PollType` and `poll` returns `Result` (v25), and `PollType::Wait` gained timeout fields (v27); `InstanceDescriptor` lost `Default` in favour of constructors that take or omit a display handle (v29). `ImageCopyTexture` and `ImageDataLayout` are gone by 30 ([22.1.0](https://docs.rs/wgpu/22.1.0/wgpu/type.ImageCopyTexture.html) has `ImageCopyTexture`, [30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/type.TexelCopyTextureInfo.html) has `TexelCopyTextureInfo`). gfx.rs uses every one of these. The naga test in gfx.rs moves with wgpu (ADR 0006).
- wgpu 27 added DXGI swapchains on DirectComposition visuals for DX12 (`Dx12SwapchainKind::DxgiFromVisual`, or `SurfaceTargetUnsafe::CompositionVisual` for an app-managed visual), which the CHANGELOG says lets the DX12 backend support transparent windows (CHANGELOG v27.0.0). The overlay today is an opaque layered window. Whether this matters for the docked overlay is for the 3.1 prototype to find out; wgpu 22 does not have it.
- Latest eframe and egui are 0.36.2 (2026-09-08), MSRV 1.95, above the local 1.93.1. 0.35 needs 1.92 ([crates.io versions](https://crates.io/crates/eframe/versions)). eframe 0.36's optional wgpu renderer requires wgpu ^30 ([dependencies](https://crates.io/crates/eframe/0.36.2/dependencies)).
- eframe is built with `glow`, not `wgpu` (workspace Cargo.toml), so the panel and the overlay share no graphics crate. wgpu and eframe can be upgraded separately and in either order. They only need matching versions if egui is ever drawn through the overlay's wgpu device.

## Options

### Structure

1. **Platform modules behind `cfg`, no traits.** Split cv-render/src/lib.rs into files inside the existing crates and put each Win32 piece in a `#[cfg(windows)] mod windows` with free functions; Linux later adds sibling modules with the same function names. Cost: the smallest diff. But there is still no test seam (the tick still calls the OS directly), and Linux support would add `cfg` forks in cv-capture, cv-render and app, which is the spreading CLAUDE.md asks to avoid. Phase 3 and 4 would grow inside the same mixed code.

2. **Traits at the seam, one neutral magnifier crate, one Windows backend crate.** The neutral side defines small traits for what it needs from the OS (pointer, capture, overlay window) and owns the per-tick logic. One crate per platform implements them; app picks the backend with `cfg`. Cost: two crate moves (cv-capture absorbed into the backend, cv-render renamed and made neutral), a trait design now, and the per-tick logic in `on_timer` untangled from Win32 calls, which is the risky part. Gains a fake-host test seam for the tick, and Phase 3 replaces AppBar inside the backend without touching the magnifier.

3. **The same traits, with one crate per platform concern** (cv-capture-dxgi, cv-window-win32, cv-hotkey-win32, later cv-capture-pipewire and so on). Cost: everything in option 2 plus more manifests and wiring. It matches how Linux will differ per concern (Wayland capture is per portal, docking per compositor), but that is a Linux design question this ADR defers, and splitting a Windows backend later is cheap.

4. **Defer: split lib.rs into files only, design the seam when Linux starts.** Cost: almost nothing now. But the caret source (4.4) and tracking policy (4.5) need a home before then, Phase 3 rewrites docking in mixed code, and the refactor gets bigger with every phase.

### When to upgrade wgpu and eframe

A. **Before the refactor.** Upgrade breakage and refactor breakage cannot then be told apart in one Verify run, and gfx.rs changes twice (API, then the seam).
B. **After the refactor, before Phase 3.** The refactor is checked on the code you already verified. The wgpu upgrade then touches one neutral crate. The 3.1 prototype gets wgpu 30, including the DirectComposition swapchain option.
C. **After v1.** No upgrade risk before v1, but 3.1 cannot try DirectComposition, every later shader and render change is written against an API two years old, and the gap keeps growing (8 major wgpu versions today).

## Decision

Recommendation: **option 2, with upgrade timing B.** This ADR supersedes nothing. ADR 0001 (CPU frames), the wgpu half of 0002, 0005 and 0006 stand; 0005 left the hotkey code's place to this ADR, and 0006 noted that the naga test moves with wgpu.

### Crates

| Crate | Layer | Platform | Holds | Depends on |
|---|---|---|---|---|
| cv-core | core + shared | neutral | `AppState`, `Frame`, `OutputInfo`, enums, `geometry`; new: screen-space types, the hotkey `Action` and its effect on `AppState`, the `PointerSource` trait; later the core event types (4.4) and tracking policy (4.5) | parking_lot, serde |
| cv-magnifier (cv-render renamed) | magnifier | neutral | gfx.rs and the shaders, the per-tick logic as a `Magnifier` struct, the capture loop, the `OverlayHost` and `CaptureSource` traits | cv-core, wgpu, raw-window-handle, pollster |
| cv-platform-win (new; absorbs cv-capture) | Windows backend | Windows | DXGI capture, the overlay window and its thread and loop, AppBar and ClipCursor until 3.5, system cursor hiding, clean-exit machinery and console handler, `RegisterHotKey`, `GetCursorPos`, DPI awareness | cv-core, cv-magnifier, windows |
| app | magnifier UI and wiring | neutral except one `cfg` | main.rs wiring, egui panel, settings | cv-core, cv-magnifier, cv-platform-win on Windows, eframe, egui |
| cv-tts | reader (parked) | Windows | unchanged | unchanged |

- No crate but cv-platform-win (and parked cv-tts) depends on `windows`. That is the rule that keeps the seam: a new `windows` import anywhere else is a review failure.
- Backend selection: app lists cv-platform-win under `[target.'cfg(windows)'.dependencies]` and does `#[cfg(windows)] use cv_platform_win as platform;`, with a `compile_error!` on other targets until a Linux backend exists. cv-platform-win starts with `#![cfg(windows)]` so a `--workspace` build elsewhere sees an empty crate.
- Static dispatch: the magnifier is generic over the traits, one backend per target. No `dyn`, no plugin loading.

### Traits and who owns which thread

Sketch for the steps; names may change in the steps, the split may not.

```rust
// cv-core: core layer
pub struct ScreenPoint { pub x: i32, pub y: i32 }           // physical px, virtual screen
pub struct ScreenRect  { pub left: i32, pub top: i32, pub width: u32, pub height: u32 }
pub trait PointerSource { fn position(&self) -> Option<ScreenPoint>; }

// cv-magnifier
pub enum Layout {
    Hidden,
    Fullscreen { monitor: OutputInfo },
    Docked { monitor: OutputInfo, edge: Edge, thickness: u32 },
}
pub trait OverlayHost: PointerSource {
    /// Make the window match `layout`: size, position, visibility, system cursor and any
    /// work-area reservation. Returns the client size in physical px.
    fn apply_layout(&mut self, layout: &Layout) -> (u32, u32);
    fn raw_handles(&self) -> (RawDisplayHandle, RawWindowHandle);
}
pub trait CaptureSource {
    fn next_frame(&mut self, timeout_ms: u32) -> Result<Option<Frame>, CaptureError>;
    fn switch_output(&mut self, idx: u32) -> Result<(), CaptureError>;
    fn reconnect(&mut self) -> Result<(), CaptureError>;
}
pub enum CaptureError { AccessLost, DeviceLost, Other(String) }   // timeout is Ok(None)
pub struct Magnifier { /* WgpuState, smoothing, active monitor, applied layout, uniform caches */ }
impl Magnifier {
    pub fn tick(&mut self, host: &mut impl OverlayHost, now: Instant);
    pub fn resized(&mut self, w: u32, h: u32);   // the OS moved or resized the window
}
```

- **The magnifier says what, the backend says how.** `Layout` is the whole contract for the window. AppBar registration, `ClipCursor` and `MagShowSystemCursor` stay inside the backend's `apply_layout` and timer. Phase 3 swaps AppBar docking for overlay docking inside cv-platform-win, and 3.5 deletes appbar.rs, without changing `Magnifier`.
- **Render thread: owned by the backend.** A window's messages must be pumped on the thread that created it, and Linux event loops differ again, so the backend creates the window, owns the loop and timer, and calls `Magnifier::tick` on each tick. The magnifier never runs a loop of its own. The backend calls `Magnifier::resized` when the OS moves the window (the AppBar `ABN_POSCHANGED` path today). The re-entrancy rule from lib.rs carries over: the backend never calls into `Magnifier` from inside a call the magnifier made (`apply_layout` can send messages synchronously).
- **Clean exit stays in the backend.** `OverlayHandle::shutdown`, the `Drop` teardown and the console handler move unchanged. app calls `platform::spawn_overlay(...)` and gets the same handle as today.
- **Capture thread: spawned by cv-magnifier**, running the loop that is in main.rs today over a `CaptureSource`. The source is made by a factory closure inside the thread, so D3D11 and COM objects never cross threads. `enumerate_outputs` stays in the backend because `OutputInfo.idx` must match the duplication's output index.
- **Hotkey thread: owned by the backend.** It registers the bindings (constants, ADR 0005) and calls back with an `Action`; applying an `Action` to `AppState` moves to cv-core with unit tests.
- **Pointer: sampled per tick** through `PointerSource` on the render thread, as `GetCursorPos` is today. The host implements it for v1; a separate source is allowed later.
- **Shared state is unchanged**: `Arc<RwLock<AppState>>`, the `FrameState` slot, `desired_output`, `app::RepaintSlot`. Threading does not change in Phase 2.

### Frame transport

Keep `Arc<Frame>` with CPU BGRA8 on every platform (ADR 0001). No GPU-handle variant is designed now. If 5.5 measures the copy as the bottleneck, 5.6 gets an ADR that supersedes 0001; `CaptureSource` returning `Frame` is the line it would change.

### Core event rules (for 4.3 and 4.4; nothing is built in Phase 2)

The caret source in 4.4 is the first core event. So that the reader and per-app scripting are not ruled out later:

- Core events are one `#[non_exhaustive]` enum in cv-core, so adding focus, element or text events later breaks no consumer.
- Events carry plain data in physical virtual-screen pixels plus a timestamp, never OS handles or COM pointers (ADR 0003's rule, kept for the core).
- Each event names its source (for example which caret API produced it), so per-app policy can tell sources apart. Fields identifying the foreground application are added when 4.3 decides what it needs.
- Delivery is fan-out: each consumer (tracking policy now, reader later) gets its own receiver. A consumer never blocks a source.
- Continuous values (pointer position) are sampled, not sent as events.

Which thread owns UIA, the fallback order and elevated windows are 4.3's decision, not this one.

### Upgrades

Timing B: after the refactor, before 3.1. wgpu 22 → 30 first (cv-magnifier only), then eframe and egui 0.29 → 0.36 (app only, needs Rust 1.95 or newer; you update the toolchain). Each is its own item with its own Verify list, so a regression points at one change. If 0.36 brings more panel churn than expected, the step may stop at 0.35 (Rust 1.92) and record why.

### Roadmap

2.2 is replaced by these items (in docs/ROADMAP.md, marked pending this ADR). Each must not change behaviour:

- 2.2 cv-platform-win: new crate absorbing cv-capture unchanged; DPI awareness and `RegisterHotKey` move in from app; `Action` and its effect on `AppState` move to cv-core with tests; app drops its `windows` dependency.
- 2.3 Portable renderer: gfx.rs takes raw display and window handles instead of `HWND`; the Win32 overlay (lib.rs window, loop, AppBar, clip, cursor, clean exit) and appbar.rs move to cv-platform-win; cv-render drops `windows` and is renamed cv-magnifier.
- 2.4 Seam traits: `PointerSource`, `OverlayHost`, `CaptureSource` and `CaptureError`; the capture loop moves from main.rs to cv-magnifier behind `CaptureSource`.
- 2.5 `Magnifier::tick`: the per-tick logic moves out of `on_timer` into cv-magnifier behind `OverlayHost` and `Layout`; unit tests with a fake host for enable and disable, mode change, panel size, monitor switch and uniform-write caching. If this is bigger than one step, the step stops and proposes sub-items.
- 2.6 wgpu 22 → 30, naga with it; `bench_modes_4k` before and after.
- 2.7 eframe and egui 0.29 → 0.36 (or 0.35, see Upgrades).

### How each item is checked as behaviour-preserving

- The gate (`cargo build`, `cargo clippy --workspace --all-targets` at 0 warnings, also with `--features app/tts`; `cargo test --workspace`). Test count only goes up.
- `cargo tree -i windows` lists only cv-platform-win and cv-tts once 2.3 is done.
- The same manual regression list in every Verify, from STATUS "Works": hotkeys on and off and zoom with the panel unfocused; fullscreen follow and the cursor circle on the pointer; second monitor follow; docked on each edge with the work area restored on toggle-off; all four colour filters and four interpolation modes at 10x and 20x; panel controls and settings restored on relaunch; exit by window close, Ctrl+C, console close and Task Manager kill leaves nothing behind; no `[dpi]` line; idle CPU no worse by eye.
- 2.6 adds `bench_modes_4k` numbers against the 1.9 numbers in STATUS.

## Consequences

- Easier: Phase 3 is backend-only work behind `Layout`; 3.5 deletes files in one crate. The per-tick behaviour gets unit tests for the first time. The caret source and tracking policy (Phase 4) have a place to go. A Linux backend later is a new crate implementing three traits plus hotkeys, not edits across the tree.
- Harder: five items of moves before any new feature; 2.5 is a real untangling of `on_timer` and the riskiest of them. One more indirection between the window and the tick. The trait shapes are designed from Windows only, so a Linux backend may still force changes (Wayland pointer and capture are the known unknowns). That is accepted; the deferred questions come back with Linux.
- The `windows`-only-in-the-backend rule needs keeping by hand; `cargo tree -i windows` is the check.
- eframe 0.36 needs a toolchain update on your machine and on any build machine. The tester's machine is unaffected (static CRT, 1.7).
- Revisit: when 3.1 reports whether DirectComposition presentation helps the overlay; when 4.3 decides caret sources and the core event fields; when 5.5 measures the CPU frame path; when Linux work starts (questions 4 to 6 of the old brief).
