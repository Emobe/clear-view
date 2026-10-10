# 0009: Monitors connected or disconnected while running

Status: Proposed
Date: 2026-10-10

## Context

Roadmap 5.4. PRODUCT.md v1 Must 6: fullscreen and docked both work on any monitor, and the view follows the mouse between monitors. Today that holds only for the monitors present at startup. This ADR decides how a change in the set of monitors reaches the platform-neutral magnifier. That is a new call across the ADR 0004 seam, so it needs an ADR (NEXT.md, 5.4 notes).

### What the code does today (integration branch step/4.6-fullscreen-tracking, after 5.3)

- **Enumerated once.** app main.rs calls `platform::enumerate_outputs()` once, before any thread starts, and hands the `Vec<OutputInfo>` to `spawn_overlay`. `run_overlay` starts on `outputs.first()` and passes the list to `Magnifier::new`. Nothing enumerates again. `log_system_info` prints the `[system] output N` lines once, at startup.
- **What enumeration is.** `enumerate_outputs` (cv-platform-win/src/capture.rs) creates a D3D11 device on the default adapter and walks `EnumOutputs(0..)` until it fails. It keeps only outputs with `AttachedToDesktop` and records each one's DXGI index in `OutputInfo.idx`, so the indices can have gaps. It never returns an empty list: with no outputs it returns one made-up 1920x1080 output at index 0.
- **Who reads the list.** cv-magnifier `View` (magnifier.rs) holds `outputs`, `active` (the monitor being magnified) and `primary` (`geometry::primary_output`: the output containing (0, 0), else the start monitor). `follow_target` picks the output under the target with `geometry::output_at`. The docked panel sits on `primary`, or on `active` with `panel_follows_monitor`. Both the crop and the tracker's view size come from those `OutputInfo`s.
- **Capture is addressed by index.** `View::follow_target` writes `desired_output` (an `AtomicU32`) only when the active index changes. The capture loop (cv-magnifier/src/capture.rs) calls `switch_output(wanted)` when `wanted != current`. `Capturer::duplicate` (switch) calls `EnumOutputs(idx)` on the device it already has. `Capturer::recreate` (reconnect) builds a new device and then duplicates `self.output_idx`. After any frame error the loop retries `reconnect` every 250 ms (2.5a). For an output that no longer exists that never succeeds, and for an index that now names a different monitor it succeeds on the wrong one.
- **Layout is re-applied only on an index change.** `Applied` holds output indices (`Fullscreen { output }`, `Docked { monitor, .. }`). If a monitor keeps its index but changes rect (new resolution, rearranged or moved to (0, 0)), `want == applied` and the window is not moved.
- **A resolution change freezes the view.** `WgpuState::upload_frame` returns early when the frame size differs from the texture (gfx.rs), and the texture is resized only by `follow_target` on an index change. A monitor that changes resolution and keeps its index therefore shows its last frame until the pointer leaves and comes back.
- **The overlay window** is a top-level `WS_POPUP` window. `wnd_proc` handles `WM_CLOSE`, `WM_DESTROY`, `WM_NCHITTEST`, `WM_DPICHANGED` (ignored, 5.2/5.3) and `WM_TIMER`. It borrows no state; the message loop runs `Magnifier::tick` on `WM_TIMER`. Before 3.5 the AppBar `ABN_POSCHANGED` notification set a flag that the loop handled before the next tick (STATUS 2.5). ADR 0004 originally had `Magnifier::resized`, which the backend called when the OS moved the window. 3.5 removed it with the AppBar.

### External facts (checked 2026-10-10)

- `WM_DISPLAYCHANGE` "is sent to all windows when the display resolution has changed"; it "is only sent to top-level windows" ([Microsoft docs](https://learn.microsoft.com/en-us/windows/win32/gdi/wm-displaychange)). The `DuplicateOutput` docs name it as the mode change notification to wait for before calling `DuplicateOutput` again ([DuplicateOutput](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutput1-duplicateoutput)). Neither page says in so many words that it is sent when a monitor is plugged in or unplugged with no other resolution change. That has to be checked by hand.
- "`EnumOutputs` first returns the output on which the desktop primary is displayed. This output corresponds with an index of zero." It returns `DXGI_ERROR_NOT_FOUND` past the last output ([EnumOutputs](https://learn.microsoft.com/en-us/windows/win32/api/dxgi/nf-dxgi-idxgiadapter-enumoutputs)). So when the primary changes (the primary is unplugged, or the user picks another one), every index can change meaning.
- `AcquireNextFrame` returns `DXGI_ERROR_ACCESS_LOST` on a desktop switch, a mode change or a switch to or from a full-screen app. The app "must release the `IDXGIOutputDuplication` interface and create a new" one ([AcquireNextFrame](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutputduplication-acquirenextframe)). `DuplicateOutput` returns `DXGI_ERROR_UNSUPPORTED` in unsupported modes, and the app "can wait for system notification of desktop switches and mode changes" and try again. It returns `E_INVALIDARG` while the process already duplicates that output ([DuplicateOutput](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutput1-duplicateoutput)).
- `IDXGIFactory1::IsCurrent` returns FALSE "if a new adapter is becoming available or the current adapter is going away", telling the app to re-create the factory and re-enumerate ([IsCurrent](https://learn.microsoft.com/en-us/windows/win32/api/dxgi/nf-dxgi-idxgifactory1-iscurrent)). The `GetDesc1` docs say apps should re-create the factory when `IsCurrent` is FALSE and re-query "from the new factory's equivalent output" ([GetDesc1](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_6/nf-dxgi1_6-idxgioutput6-getdesc1)). So a factory, and the adapter from it, can describe outputs as they were when it was created.
- `D3D11CreateDevice` with a null adapter uses "the first adapter that is enumerated by `IDXGIFactory1::EnumAdapters`" ([D3D11CreateDevice](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-d3d11createdevice)). The docs don't say whether each call uses a new factory. `enumerate_outputs` and `recreate` each create a new device; `duplicate` reuses the old one. Whether the old device's adapter still lists the outputs as they were before a change is not known, and the step finds out.

### Not in scope

The egui settings window: Windows moves it off a removed monitor, and eframe/winit handle that. Adapters other than the primary (a monitor on a second GPU is not enumerated today, ADR 0001's CPU path). The made-up output for a machine with no displays stays as it is.

## Options

### Who notices the change, and how the magnifier hears it

1. **The backend tells the magnifier: `Magnifier::outputs_changed(Vec<OutputInfo>)`, called from the backend's loop.** `wnd_proc` handles `WM_DISPLAYCHANGE` by setting a flag (an atomic, like `OVERLAY_HWND`) and borrows nothing. The loop sees the flag before the next tick, calls `enumerate_outputs()` and, if the list differs from the last one, calls `magnifier.outputs_changed(list)`. This is the `ABN_POSCHANGED` flag pattern from 2.5 and the `Magnifier::resized` direction ADR 0004 already allowed (the backend calls the magnifier when the OS changes something, never from inside `apply_layout`). Cost: one new public method on `Magnifier`, no trait change. A Linux backend has to call it too. Unit tests call `View::outputs_changed` directly.
2. **The magnifier asks the host: `OverlayHost::outputs_changed(&mut self) -> Option<Vec<OutputInfo>>`, polled every tick.** The backend sets the same flag and re-enumerates inside the call. Cost: the same work, plus a second method on the trait that every host, the fake one included, must implement. It is polled 60 times a second for something that happens a few times a year. The pull model fits the tick, but ADR 0004 keeps `OverlayHost` to window placement, and outputs are a capture and topology fact, not a window one.
3. **A core event: `CoreEvent::OutputsChanged(Vec<OutputInfo>)` on the `EventHub`.** It fits ADR 0004's event rules (plain data, `#[non_exhaustive]`, fan-out), and the render thread already drains the hub every tick. Cost: something with a hub reference has to publish it, but `wnd_proc` holds no state, so it needs a static hub or a new watcher thread with its own hidden window. Every consumer (the tracker today, the reader later) sees the full list. `OutputInfo.idx` is a DXGI capture index, which makes the event Windows capture data rather than core data, and ADR 0008 kept core events to positions.
4. **The capture thread notices.** On `AccessLost` it re-enumerates and writes the list into new shared state (`Arc<RwLock<Vec<OutputInfo>>>` next to `desired_output`), which the magnifier reads each tick. Cost: no window message is needed, but it only hears about changes that break the duplication it holds. Plugging in a monitor that isn't being captured may not break it; that is undocumented. The capture loop would also need a backend enumerate function next to its factory, and the topology would live in a third place.

### How capture stays on the right monitor

A. **Keep the DXGI index as the address (ADR 0004), and have the magnifier re-send it after every change.** On `outputs_changed` the magnifier writes `desired_output` even if the number is unchanged. The backend makes `switch_output` duplicate through a current factory (check `IsCurrent`, or build a new device as `recreate` does). Cost: during the gap between the OS change and the next tick, a reconnect can land on the old index, which may now be another monitor. That shows the wrong monitor, or no new frame when the size differs, for about a tick.
B. **Address capture by a stable output id** (`DXGI_OUTPUT_DESC.DeviceName`, for example `\\.\DISPLAY2`) instead of an index. Cost: `CaptureSource::switch_output`, `desired_output` (no longer an atomic integer) and `OutputInfo` all change, and so does ADR 0004's statement that `idx` is the duplication's index. It closes the one-tick gap. Whether `DeviceName` stays the same across unplug and replug is undocumented.

## Decision

Recommendation: **option 1 with addressing A.** This extends ADR 0004 and supersedes nothing in it. The new call goes from the backend to the magnifier, like `Magnifier::resized` did, and keeps the re-entrancy rule. `OverlayHost` and `CaptureSource` stay as they are. Option 2 widens a trait for a rare event, option 3 puts capture indices in core events, and option 4 misses changes that don't break the active duplication. B fixes a gap of about one tick at the cost of the capture contract; revisit it only if the step's checks show a visible wrong-monitor flash.

### What the backend does (cv-platform-win)

- `wnd_proc`: on `WM_DISPLAYCHANGE`, set a "displays changed" flag (and return 0). No other work in `wnd_proc`.
- The loop: when the flag is set, clear it, run `enumerate_outputs()`, and if the list differs from the last one it passed, log the new `[system] output N` lines (the same format as at startup) and call `magnifier.outputs_changed(list)` before the next tick. A plug or unplug can send several messages while the topology settles, so the loop enumerates once more about 1 s after the last message and passes the list again only if it changed. The step picks the exact delay.
- `Capturer::duplicate` checks that its device's factory is current (`IDXGIFactory1::IsCurrent`) and builds a new device first when it is not, so a switch after a change never uses a stale output list. `recreate` already builds a new device.
- If `WM_DISPLAYCHANGE` turns out not to arrive for a plug or unplug that keeps the resolution (checked by hand in the step), the step stops and records it in NEXT.md. A slow re-enumeration timer or `WM_DEVICECHANGE` would then be a change to this ADR, not something to slip in.

### What the magnifier does (cv-magnifier, `View::outputs_changed`)

- Replace `outputs`. Recompute `primary` (the output at (0, 0), else the first in the list).
- Re-pick `active`: the output under the current target. If the target is on no monitor (its monitor was removed), use the primary and move the target and the smoothed position to the primary's centre. The view jumps there; it does not slide across the desktop.
- Always write `desired_output` with the new `active.idx`. Always recreate the frame texture for the new `active` size and clear `last_frame`. Together these also fix the resolution-change freeze above.
- Force the layout to be applied again on the next tick (reset `applied`), so the fullscreen window or the docked panel lands on the new rect even if no index changed. The system cursor state resets with it, as on any layout change.
- The tracker is not reset: a caret position stays a screen point. The next tick's view size comes from the new monitor.

### Unit tests (fake host and renderer, no hardware)

At least: a second monitor removed while active (falls back to the primary, layout re-applied, `desired_output` rewritten); the primary removed and another output becoming index 0 at (0, 0) (the docked panel moves, `desired_output` rewritten even though the number may repeat); a monitor added (reachable by `follow_target`, no layout change if the active one is unchanged); the active monitor changing resolution with the same index (texture recreated, layout re-applied); an unchanged list (nothing happens). Plus the backend's "only on change" comparison, if it is pure enough to test.

### Verify by hand (for the step; you cannot test right now)

Plug and unplug a second monitor in fullscreen and docked, with `panel_follows_monitor` on and off; unplug the primary; change the resolution of the active monitor; switch the primary in Settings; the `[system] output` lines reprinted each time; no freeze, no stale panel, clean exit afterwards; and whether `WM_DISPLAYCHANGE` arrived each time (a log line).

## Consequences

- Easier: one call carries every topology change (plug, unplug, primary change, resolution change, rearranging), and the magnifier reacts in one place with unit tests. The resolution-change freeze goes away as a side effect.
- Harder: the magnifier's public API grows by one method that every backend must call. A Linux backend needs its own display-change signal (RandR on X11, `wl_output` events on Wayland) feeding the same call.
- The index can mean a different monitor for about one tick after a change (addressing A). Accepted; revisit with option B if a flash is visible.
- `WM_DISPLAYCHANGE` arriving for a plain plug or unplug is undocumented and is the first thing the step checks.
- The roadmap is unchanged: 5.4 is one step, now pending this ADR.
