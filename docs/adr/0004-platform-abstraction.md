# 0004: Platform abstraction for Windows, X11 and Wayland

Status: Proposed
Date: 2026-10-08

This file is the brief for the Opus planning session. Fill in Options, Decision and Consequences from it.

## Context
The goal is Windows first, then X11 and Wayland. Today cv-core and the wgpu parts (gfx.rs, shader.wgsl) are portable. Everything else is Win32: capture (DXGI), window and docking (AppBar, ClipCursor, monitor switching, cursor hiding), hotkeys (RegisterHotKey), cursor position, reader (UIA, SAPI).

Known Linux constraints to design for, from research:
- X11: capture, strut docking and key grabs are available to ordinary clients.
- Wayland: no single capture path. The ScreenCast portal over PipeWire works across GNOME, KDE and wlroots; zwlr_screencopy is wlroots-only and absent on GNOME. Global hotkeys are restricted for ordinary clients. Docking and overlay support differ per compositor (layer-shell on wlroots, not on GNOME).
- Reader: AT-SPI2 over D-Bus and Speech Dispatcher replace UIA and SAPI.

## Questions the ADR must answer
1. Trait boundaries: capture source, window host (fullscreen, docked, per-monitor), hotkeys, cursor source. What does each return and who owns threads?
2. Crate layout: where does Win32 go, where does wgpu stay, how do platform crates get selected (cfg, features, separate binaries)?
3. Frame transport: keep `Arc<Frame>` CPU frames across all platforms, or allow a GPU-handle variant later?
4. Wayland strategy: portal-only baseline plus compositor-specific enhancements, or target specific compositors first?
5. Cursor position on Wayland while the overlay passes input through: what is the fallback if it is not available?
6. Reader: shape of AccessibilityBackend and SpeechEngine so UIA, IA2 and AT-SPI2 all fit without touching the state machine.
7. Dependency baseline: upgrade wgpu and eframe before this refactor or after?
8. How the refactor is verified as behaviour-preserving on Windows (tests and manual checklist from STATUS.md).

## Options
TBD

## Decision
TBD

## Consequences
TBD
