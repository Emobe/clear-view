# PRODUCT

Draft 1, 2026-10-08. What Clearview is, who it's for, what v1 contains and what it doesn't. docs/ROADMAP.md is built from this file and docs/STATUS.md. If the roadmap and this file disagree, this file wins until it is changed.

## What it is

A free screen magnifier for Windows in the style of ZoomText, for people with low vision. It is built as a core that watches the system, with the magnifier on top. A screen reader can be added later on the same core without changing the magnifier.

## Who it's for

- Primary tester: a long-time ZoomText user on Windows 11, single monitor, usually at 10x.
- Target users: anyone with low vision doing everyday computing: web browsing, email, files, Word documents.
- Free and open. Selling only if it becomes necessary.

## Principles

1. High zoom is the normal case. 10x must be smooth and readable, not an edge case.
2. Never leave the machine broken. Work area, system cursor and cursor clip are restored on exit, on crash and on kill.
3. Real use decides. Get a usable build to the tester early, then ask what's missing, rather than guessing features.
4. Small steps. Each step is small enough to understand and review. Work runs through /next, /step and /adr.
5. No speech in v1. Speech is v1.1, built after the magnifier is solid (see "Speech (v1.1)").

## Architecture

Three layers:

- **Core**: watches the system and reports positions. v1: pointer position and caret position. Later: keyboard focus, element under the pointer, text.
- **Magnifier**: capture, rendering, tracking policy (what to follow and when), fullscreen and docked modes, hotkeys, settings UI.
- **Reader** (v1.1): consumes the same core events plus text, and speaks. See "Speech (v1.1)".

Per-app differences (Chrome vs Word vs File Explorer) are handled in the core, once, for both the magnifier and the reader.

Platform code stays behind a seam so Linux can be added later, but no Linux work happens before Windows v1. Crate names and trait boundaries are decided in an ADR.

## v1 (Windows)

### Must

1. **Fullscreen mode.**
2. **Docked mode, overlay style.** The panel sits on one screen edge on top of the desktop. The desktop is not resized and windows do not shrink. The mouse can move under the panel, and the panel shows that area magnified. As the pointer nears the panel, the unmagnified area pans smoothly to show the strip the panel covers, without moving real windows; clicks land where the pointer is drawn. The panel and the pan view are excluded from capture so they never magnify themselves. This replaces the current AppBar docking.
3. **Zoom 1x to 20x**, smooth at 10x and above.
4. **Hotkeys**: on/off, zoom in, zoom out. Ctrl+Alt+Shift+Z stays for on/off until the hotkey ADR decides the scheme.
5. **Tracking**: follow the mouse by default; follow the text caret while typing; go back to the mouse when it moves. Must work in Chrome, Edge, File Explorer, Word, Notepad and the tester's email app.
6. **Multiple monitors**: fullscreen and docked both work on any monitor, and the view follows the mouse between monitors.
7. **Colour filters** (done).
8. **Settings persistence** (done, 2.2).
9. **Clean exit** in every case (STATUS Finding 9).
10. **Performance** acceptable at the tester's resolution and at 4K. Thresholds set when first measured.

### Should (in v1 if cheap)

- Cursor enhancement: large pointer, colour (blue), thicker outline.

### Not in v1

- Speech of any kind (it is v1.1, see "Speech (v1.1)"). The cv-tts code is kept behind a build flag that is off by default, so it can't affect the magnifier.
- Keyboard focus tracking (Tab, arrows through menus). First item after v1.
- Lens, line and freeze windows.
- Linux.
- Scripting.

### Needed before public release, not before tester builds

- UIAccess manifest, signing and installer. Without UIAccess the magnifier can't follow into elevated windows; that's acceptable for testing.
- A test on a clean Windows install.

## Milestones

- **M1 Tester build 1**: fullscreen, mouse tracking, on/off and zoom hotkeys, settings, clean exit. Installed by hand on the tester's machine. Feedback collected before M2 is planned in detail.
- **M2 Docked overlay**: overlay docked mode with capture exclusion, all four edges.
- **M3 Caret tracking**: mouse and caret switching in the app list above.
- **M4 Multi-monitor and performance**: both modes on any monitor; measured at native and 4K.
- **M5 v1**: cursor enhancement, UIAccess, signing, installer, clean install test.

v1 is done when the tester uses Clearview instead of ZoomText for a full week of normal use.

- **M6 v1.1 (speech)**: the stages in "Speech (v1.1)", each ending in a tester build. Done when the tester uses Clearview's speech instead of ZoomText's for a full week.

## Speech (v1.1)

Speech is the release after v1. It does not change the v1 gate or the v1 list above. It is built in stages, each ending in a tester build, so the tester can use one stage while the next is built. The numbered plan is the "Speech track (v1.1)" in docs/ROADMAP.md (Phases 8 to 13).

### Stages

0. **Groundwork** (Phase 8): audit of the old reader code, the voice engine, the speech pipeline and the speech hotkeys decided in ADRs, then the engine, speech on/off and silence.
1. **Focus and program echo** (Phase 9): hear what has focus and which window or dialog you're in. Reading commands on hotkeys. Optionally the magnifier follows keyboard focus.
2. **Typing echo** (Phase 10): characters and words as you type, and what moving the caret speaks.
3. **Mouse echo** (Phase 11): the word, line or element under the pointer.
4. **Reading documents** (Phase 12): continuous reading and stepping by word, line, sentence and paragraph, with the magnifier following and highlighting the spoken word.
5. **Release** (Phase 13): speech on by default, checks in elevated windows, a clean install test, and the tester's week.

### Rules

- With speech switched off, the magnifier behaves exactly as before.
- Password fields are never spoken: no characters, no words, no length. "Can't tell" is treated as a password field.
- No UI Automation or MSAA call runs on the render thread. A hung app must not freeze the magnifier or stop speech from being silenced.
- The text that gets spoken is pure logic in the core with unit tests.
- Defaults follow what the tester actually uses (ROADMAP 8.3), not what ZoomText ships with.

### ADRs this needs

Voice engine (8.4), speech pipeline (8.5), speech hotkeys (8.6), focus events (9.3), typing echo (10.1), mouse echo (11.3) and reading (12.1).

### Later, not planned here

DocReader-style reformatted reading view, Background Reader, Recorder, OCR and camera, Braille, per-app scripting, Linux speech.

## ADRs this needs

- Platform seams and core boundaries (the planned ADR 0004), including when to upgrade wgpu 22 and eframe 0.29.
- Docked overlay: click-through topmost window plus capture exclusion. Needs a prototype first, because of a known 24H2 issue where an excluded overlay still triggers new frames.
- Caret position sources per app type (classic Win32, Chromium, Office). The riskiest item in v1.
- Hotkey scheme: modifier combinations via RegisterHotKey, or a Caps Lock modifier like ZoomText via a low-level keyboard hook.

## Later

Speech is no longer here: it is "Speech (v1.1)" above.

1. Keyboard focus tracking (ROADMAP 9.9 may pull it into the speech track).
2. Lens and other window types if testers ask for them.
3. Linux: X11, then Wayland.
4. Scripting: per-app scripts in the spirit of JAWS. Likely a separate project; the core's event API should not rule it out.

## Open questions

- Which email app the tester uses.
- Which ZoomText hotkeys the tester relies on (ask once M1 is in their hands).
- How the tester uses ZoomText's speech today: echo modes, voice, rate, hotkeys (ROADMAP 8.3).
- Voice engine and natural voices (ROADMAP 8.4).
- Whether anyone needs the old AppBar split once overlay docking works. Default: no, it's removed.
- Performance thresholds.
