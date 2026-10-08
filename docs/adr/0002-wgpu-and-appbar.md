# 0002: wgpu renderer and AppBar docking

Status: Accepted
Date: 2026-10-08 (recorded retroactively from CLAUDE.md and handoff history)

## Context
Phase 1 used GDI StretchBlt. Colour filters and better scaling need shaders. Docked mode needs to reserve screen space like ZoomText.

## Options
1. Keep GDI. No shader support.
2. wgpu with a fullscreen quad and a WGSL fragment shader; docking through SHAppBarMessage.
3. Direct D3D11 renderer (the original PLAN.md). Windows-only.

## Decision
Option 2. Colour filters, bilinear and bicubic, and the software cursor are all in shader.wgsl. Docked panel registers as an AppBar on any of four edges and shifts the work area. The overlay uses WS_EX_TRANSPARENT and WS_EX_NOACTIVATE and is excluded from its own capture with WDA_EXCLUDEFROMCAPTURE.

## Consequences
Rendering is portable (wgpu, WGSL); window management, AppBar and ClipCursor are not. That split is the subject of ADR 0004. wgpu 22 is old; upgrade timing is an open decision in docs/ROADMAP.md.
