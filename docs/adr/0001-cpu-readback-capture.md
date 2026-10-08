# 0001: CPU readback for capture

Status: Accepted
Date: 2026-10-08 (recorded retroactively from CLAUDE.md)

## Context
DXGI Desktop Duplication produces a D3D11 texture. The renderer is wgpu on a separate thread.

## Options
1. Share the D3D11 device and immediate context with the renderer. Tried; the immediate context is single-threaded and frames never rendered.
2. Copy to a staging texture and read back to a CPU buffer, then upload to wgpu each frame.
3. Zero-copy DXGI to wgpu texture interop. Not implemented.

## Decision
Option 2. Capture thread copies each frame into `Arc<Frame>` (BGRA8) stored in a shared slot. The renderer skips the upload when the Arc is unchanged.

## Consequences
Simple and robust. Costs a full-frame copy per frame, so 4K performance is unmeasured. Revisit with numbers from the audit before attempting option 3.
