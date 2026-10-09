# clear-view: tester build 0.1.0

A screen magnifier. This is an early test version: fullscreen magnification, following the mouse.

## Starting it

Double-click `clear-view.exe`. Two windows open:

- the **settings window** ("clear-view settings"), which stays on top
- a black **console window** with text in it. Leave it open. Closing it quits clear-view.

The magnifier always starts **off**. Turn it on with the hotkey below or the On/Off button.

## Hotkeys

Hold **Ctrl + Alt + Shift** and press:

| Key | Does |
|---|---|
| **Z** | Magnifier on / off |
| **Up arrow** | Zoom in |
| **Down arrow** | Zoom out |

Zoom goes from 1x to 20x. Below 4x each press changes it by 0.5, from 4x up by 1.

## Settings window

- **Zoom** and **Follow speed** sliders
- **Display mode**: use **Fullscreen**. The docked modes (Top, Bottom, Left, Right) are old and being replaced; in them the mouse cannot reach the taskbar.
- **Colour filter**: none, inverted, greyscale, greyscale + inverted
- **Interpolation**: how the zoomed picture is smoothed. Bicubic is the default; try Sharp for crisper edges.

Settings are saved automatically and come back next time.

## Quitting

Close the settings window. The console window closes with it, and the mouse pointer and taskbar go back to normal.

If it ever stops responding: Ctrl + Shift + Esc opens Task Manager; end **clear-view.exe**.

## Things to tell us

Anything that's hard to read, hard to find, slow, jumpy, or missing compared with ZoomText. Which ZoomText hotkeys you miss.

---

## For whoever installs it

- Build with plain `cargo build --release`, with no `RUSTFLAGS` set (it would drop the static C runtime from `.cargo/config.toml`).
- Copy `target\release\clear-view.exe` and this file to any folder on the tester's machine. Nothing else is needed.
- Settings live in `%APPDATA%\clear-view\settings.json`. Delete it to go back to defaults.
- Not signed. Copied from a USB stick it normally runs straight away; if Windows SmartScreen appears, choose "More info", then "Run anyway".
