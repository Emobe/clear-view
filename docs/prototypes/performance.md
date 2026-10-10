# Performance measurements (roadmap 5.5)

Tooling built by 5.5a. The runs and the thresholds are 5.5b (you, on your machine). Findings 4 and 17 and ADR 0001 ("revisit with numbers") are the reason for this file.

## Tools

| Tool | What it measures | Needs |
|---|---|---|
| `CV_PERF=1` | A `[perf]` line on stderr once a second, from inside the app (below) | A release build; set the variable before starting |
| `scripts/perf-sample.ps1` | CPU and GPU use of the running app, once a second, with mean, p95 and max | The app running; Windows with the "GPU Engine" counters (Task Manager's source) |
| `bench_readback_4k` | The capture copy at 3840x2160 (`CopyResource`, `Map`, row copy) | A D3D11 adapter; no 4K monitor |
| `bench_upload_4k` | Uploading one 3840x2160 frame to the GPU | A DX12 adapter; no 4K monitor |

Run the benches with the GPU idle:

```
cargo test --release -p cv-platform-win bench_readback_4k -- --ignored --nocapture
cargo test --release -p cv-magnifier bench_upload_4k -- --ignored --nocapture
```

### The `[perf]` line

```
[perf] cpu 23% | capture image 35/s ptr-only 0/s copy avg 4.1 ms max 6.0 | tick 60/s avg 2.1 ms max 4.0 | upload 35/s avg 1.9 ms | render 60/s
```

- `cpu`: the whole process, as a percentage of one core (it can pass 100). From `GetProcessTimes` ([docs](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes)).
- `capture image` and `ptr-only`: capture frames a second. A frame is pointer-only when `LastPresentTime` is 0 ([docs](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/ns-dxgi1_2-dxgi_outdupl_frame_info)). Both kinds are copied in full today (Finding 4). `copy` is the time of `CopyResource`, `Map` and the row copy for one frame, over both kinds; `max` is the larger of the two kinds' maxima.
- `tick`: calls of `Magnifier::tick` a second, enabled or not, and the time of each (mean and maximum).
- `upload`: frames written to the GPU texture a second, and the time of the `write_texture` call (the copy into wgpu's staging memory; the GPU copy happens at the next submit).
- `render`: presents a second.

Off, the counters cost one flag read per call and read no clock.

### `scripts/perf-sample.ps1`

```
.\scripts\perf-sample.ps1 -Seconds 20 -Label "fullscreen still 1440p" -Csv fs-still.csv
.\scripts\perf-sample.ps1 -Seconds 20 -Move -Label "fullscreen moving 1440p"
```

- CPU is `TotalProcessorTime` over the wall clock, % of one core.
- GPU is the "GPU Engine" `Utilization Percentage` counters for the app's PID, summed per engine type: `Gpu3D`, `GpuCopy`, `GpuOther`. The app has two D3D devices (the capture copy and wgpu) and both count. The compositor's own use is not part of it.
- `-Move` drives the pointer in a circle (radius 300 px) so "moving" is the same each time. It takes your mouse while it runs. Leave it off for "still" and keep your hands off the mouse.
- The first second is dropped; there is nothing to compare it with.
- The script was dry-run on other processes (CPU of a shell; GPU of `dwm`) to check the counter names and parsing. It has not been run against clear-view.

## Matrix to fill in (5.5b)

Release build, 10x zoom, `CV_PERF=1`, about 20 s per cell. Note the resolution and refresh rate of the monitor.

| Mode | Pointer | Desktop | Resolution | `[perf]` cpu | script CPU mean/p95 | Gpu3D mean/p95 | GpuCopy mean/p95 | image/s, ptr-only/s | Notes |
|---|---|---|---|---|---|---|---|---|---|
| Off | still | quiet | native | | | | | | |
| Off | moving | quiet | native | | | | | | |
| Fullscreen | still | quiet | native | | | | | | |
| Fullscreen | moving | quiet | native | | | | | | |
| Docked top 25% | still | quiet | native | | | | | | |
| Docked top 25% | moving | quiet | native | | | | | | |
| Fullscreen | still | video playing | native | | | | | | |
| Docked top 25% | moving | video playing | native | | | | | | |
| Fullscreen | still | quiet | 4K | | | | | | |
| Fullscreen | moving | quiet | 4K | | | | | | |
| Docked top 25% | moving | quiet | 4K | | | | | | |

4K rows need the desktop at 3840x2160 (NVIDIA DSR on the RTX 3070 would do it). If that is not possible, those rows stay empty and the 4K cost rests on the two benches below. "Off" is the magnifier toggled off with the app still running: capture and the tick timer keep going.

## Headless 4K numbers

Measured 2026-10-10 by the agent on the RTX 3070 machine, release build, 100 frames after 5 warm-up. The machine was in normal use, not confirmed idle, so treat them as an upper-ish bound and re-run when idle.

| Bench | Result |
|---|---|
| `bench_readback_4k`: `CopyResource` + `Map` + row copy of one 3840x2160 frame (33 MB) | avg 6.00 ms, max 6.79 ms |
| `bench_upload_4k`: `write_texture` call | avg 1.54 ms, max 2.12 ms |
| `bench_upload_4k`: upload until the GPU copy has finished | avg 2.98 ms |
| `bench_modes_4k` (earlier, wgpu 30, STATUS 2.6) | cleanEdge 1.49–1.78 ms; other modes 0.16–0.38 ms |

What the first two mean on their own: at 4K one captured frame takes about 6 ms of wall time to copy (part of that is waiting for the GPU copy into the staging texture, so it is not all CPU), and the render thread spends about 1.5 ms handing it to wgpu. A desktop that updates 60 times a second then keeps the capture thread busy for about 36% of the time. While the mouse moves, pointer-only frames came at 150–210 a second at 1440p (Finding 4); at 4K the same rate would need 0.9 to 1.3 s of copying per second, more than one thread can do. That is the case 5.6 is for. These are bench numbers, not the real-desktop runs the matrix asks for.

## Thresholds

Not set. They are decided with you after the matrix is filled in (5.5b), then recorded here and in PRODUCT.md's open questions.
