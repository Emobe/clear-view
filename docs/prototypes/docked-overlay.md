# Docked overlay prototype (roadmap 3.1)

Measured 2026-10-09. Input for the 3.2 ADR (docked overlay design, superseding the AppBar part of ADR 0002). The prototype code is on branch `step/3.1-overlay-prototype` (commit 73bdf0a). It is never merged.

## What was built

The current overlay window, unchanged: `WS_POPUP` with `WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_LAYERED`, `SetLayeredWindowAttributes` alpha 255, and `WDA_EXCLUDEFROMCAPTURE`. In docked mode it is placed at the monitor edge with `SetWindowPos`. There is no AppBar and no `ClipCursor`, so the work area does not change. `Magnifier::tick` is unchanged apart from one switch.

Instrumentation, logged once a second:

- **Capture** (`cv-platform-win/src/capture.rs`): frames split into image updates (`LastPresentTime` non-zero) and pointer-only updates. For image updates, `GetFrameDirtyRects` gives the changed areas. A frame whose dirty rects all lie inside the overlay rect is counted as "inside overlay only": a suspected extra frame caused by the overlay's own presents.
- **Render** (`cv-platform-win/src/overlay.rs`): ticks, presents, skipped presents, tick time, and CPU time of the whole process (`GetProcessTimes`).
- **At startup**: `IDXGIOutput2::SupportsOverlays` (multiplane overlay, MPO) for each output, and the wgpu DX12 presentation system.

Two switches, read at startup:

- `CV_PROTO_IDLE_SKIP=1`: present only when there is a new frame, new uniforms or a resize.
- `CV_PROTO_VISUAL=1`: wgpu `Dx12SwapchainKind::DxgiFromVisual` (a DirectComposition swapchain) instead of `DxgiFromHwnd`.

## Machine

- Windows 11 Pro 25H2, build 26200.9457.
- NVIDIA GeForce RTX 3070, driver 32.0.16.1088, 16 logical CPUs.
- Two monitors:
  - DISPLAY1 is the primary, 2560x1440 at 179 Hz.
  - DISPLAY2 is to its right at x = 2560 and 1920 wide.
- **Multiplane overlay support is false on both outputs.**

## Runs

Each run was a release build at 10x: about 15 s with the mouse still, then about 15 s moving the mouse. Docked runs used the top edge at 25% (2560x360). CPU is the whole process, as a percentage of one core; 100% is one full core out of 16.

The log lines were cut off at the console width (PowerShell wraps native stderr when it is piped through `Tee-Object`). The dirty-pixel and copy-time columns were lost, so the copy cost below is inferred from CPU time.

| Run | Overlay | Mouse still: image frames/s | Still: pointer-only/s | Still: presents/s | Still: CPU | Mouse moving: frames/s (pointer-only) | Moving: CPU |
|---|---|---|---|---|---|---|---|
| 1 Off | hidden | 34–36 | 0 | 0 | 9–20% | 197–249 (152–212) | 96–131% |
| 2 Fullscreen | 2560x1440 | 34–39 | 0 | 40–41 | 9–37%, mostly 13–22% | 46–114 (0) | 64–101% |
| 3 Docked | 2560x360 | 34–36 | 0 | 40–41 | 12–23% | 165–243 (72–212) | 108–173% |
| 4 Docked, idle skip | 2560x360 | 33–37 | 0 | 27–32 (9–14 skipped) | 9–32%, mostly 15–22% | toggled during this part, not comparable | — |
| 5 Docked, DirectComposition | 2560x360 | 33–35 | 0 | 40–41 | 11–29% | 149–233 (118–202) | 75–152% |
| 6 Docked, video under the panel | 1920x300 on DISPLAY2 | 81–182 while playing | 0–149 | 40–41 | 36–98% | — | — |

Image frames whose dirty rects lay only inside the overlay, with the mouse still:

- Run 3 docked: **0** every second.
- Run 4 idle skip: **0**.
- Run 5 DirectComposition: **0**.
- In every docked run, 2 frames per second had a dirty rect *touching* the panel area. Most likely a blinking caret in a window under the panel.
- In run 2 every image frame counts as "inside overlay", because the overlay covers the whole monitor. There the test is the frame rate against run 1 instead: 34–39/s against 34–36/s.

Tick time on the render thread was 1.9–2.5 ms on average with the mouse still and 2.9–3.8 ms while moving. Most of it is uploading a full frame each time a new one arrives.

Run 6 was toggled off and on while the pointer was on DISPLAY2, so the panel came back on that monitor (1920x300). That is the current `Layout` behaviour (STATUS Finding 10, roadmap 5.1 and 5.2), not something the prototype changed.

## Findings

### 1. The overlay's own presents cause no extra frames on this machine

With the mouse still, presenting 40–41 times a second added no image frames:

- docked: 34–36/s against 34–36/s with the magnifier off
- fullscreen: 34–39/s
- no frame had dirty rects only inside the panel

The same holds for the HWND swapchain and the DirectComposition swapchain.

This does **not** clear the 24H2 issue in general:

- That issue ([robmikh/Win32CaptureSample#83](https://github.com/robmikh/Win32CaptureSample/issues/83)) appeared only on outputs with multiplane overlay support. Neither output here has MPO.
- The reporter said it was fixed by KB5046617, the 2024-11-12 cumulative update for 24H2, [OS build 26100.2314](https://www.elevenforum.com/t/kb5046617-windows-11-cumulative-update-build-26100-2314-24h2-nov-12.30510/latest).
- So the tester's machine matters: its Windows build, and whether its display reports MPO. Neither is known yet.

### 2. Dropping frames whose dirty rects are inside the panel is not a safe fix

In run 6 a video played partly under the panel. 1 to 20 frames a second had dirty rects only inside the panel, and they were real: the video under the panel changed. A rule that discards such frames would freeze whatever is under the panel, and that is exactly what the panel has to show. If the 24H2 behaviour ever shows up, the fix has to be on the present side (present less often), not on the capture side.

### 3. Skipping idle presents saves little here

With `CV_PROTO_IDLE_SKIP=1`, presents dropped from 40–41 to 27–32 a second, but the CPU did not change measurably. Something on this desktop produces 33–37 image frames a second even with nothing visibly moving; the source was not identified. Each of those frames is uploaded and presented, so the skip only removes the ticks between them. It would matter more on a quiet desktop, or as the mitigation for finding 1 on an affected machine. It has a unit test on the prototype branch.

### 4. DirectComposition presentation works but changes nothing measured

`DxgiFromVisual` ran on the layered overlay window. It reported more alpha modes than `Opaque`. You said the overlay looked good, without singling out a run. It showed the same frame counts and CPU as `DxgiFromHwnd`. Unlike `DxgiFromHwnd`, it has no RenderDoc support ([wgpu docs](https://docs.rs/wgpu/30.0.1/wgpu/enum.Dx12SwapchainKind.html)). Nothing here needs transparency, so there is no reason found to switch.

### 5. The mouse goes under the panel

You moved the mouse under the docked panel freely; without `ClipCursor` there is no wall (Finding 12). The magnified view in the panel worked fine and followed the pointer under the panel, because `Magnifier::tick` is unchanged. The real desktop under the panel stays where it is ("the actual view underneath doesn't pan at all but that's to be expected"); only the magnified view moves.

You could click through the panel onto what is under it, as the docs say: [layered windows](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features) with `WS_EX_TRANSPARENT` pass mouse events to the windows underneath. In a second session you checked the rest and reported that all of it works; individual items were not itemised:

- right-clicks, scrolling and dragging on windows under the panel
- the panel never taking focus
- the taskbar being reachable under a bottom panel
- maximised windows keeping their full size
- Win+Shift+S not capturing the panel
- exit leaving nothing behind

### 6. Pointer-only frames cost about one CPU core while the mouse moves

This is not part of the 3.1 question, but it is the largest cost measured.

- With the mouse moving, 150–210 of each second's 200–250 frames are pointer-only: no desktop image change.
- `Capturer` still copies the whole 2560x1440 frame for each one: 14.7 MB a frame, or about 2.9 GB/s at 195 frames a second.
- The magnifier then uploads each copy as a new frame.
- Result: process CPU goes from about 15% of one core with the mouse still to 100–170% with it moving, in every mode where the system cursor is visible (off and docked).
- Fullscreen hides the system cursor, so it gets no pointer-only frames (0/s in run 2).
- Skipping the copy and the publish when `LastPresentTime` is 0 would remove this. It belongs with Finding 4 and roadmap 5.6. It is cheap enough to do earlier if you prefer.

## Recommendation for the 3.2 ADR

Your verdict after trying it (2026-10-09): "overlay is the way forward."

1. **Window.** Keep the current overlay window and styles: topmost, click-through layered window, no activation, `WDA_EXCLUDEFROMCAPTURE`. Place it with `SetWindowPos` at the monitor edge. No AppBar, no `ClipCursor`, no work-area change. This is a backend-only change inside `WinHost::apply_layout`, as ADR 0004 intended.
2. **Swapchain.** Stay on `DxgiFromHwnd`. DirectComposition gave no measured benefit.
3. **Extra frames.** Build no mitigation now. The ADR should require checking the tester's Windows build (26100.2314 or later) and MPO support before 3.6. If the issue does appear, present only on change (the idle skip measured here) rather than filtering captured frames (finding 2).
4. **Under the panel.** Keep following the pointer: the panel magnifies the area around the pointer while the real desktop stays put. You found this works. Still to decide: the system cursor is visible on top of the panel at its real position, so should it be hidden or drawn differently while it is under the panel?
5. **Panel size limits.** Not measured. The ADR should set them.
6. **Separately** (5.6 or earlier): stop copying pointer-only frames (finding 6).
