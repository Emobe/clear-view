# 0014: Speech hotkeys

Status: Accepted (you, 2026-10-12; set by the agent on your instruction)
Date: 2026-10-12

## Context

Roadmap 8.6, the third ADR of the speech track (v1.1, PRODUCT.md "Speech (v1.1)"). It revisits the hotkey scheme from 1.2 (ADR 0005) now that many more bindings are coming. It decides whether to add a Caps Lock modifier like ZoomText's, how a single Ctrl press silences speech without breaking Ctrl shortcuts in other apps, and the default bindings for every command in the track.

8.3 (how the tester uses ZoomText's speech, including "Ctrl to stop speech") is on hold: the tester is unavailable and ZoomText can't be used for now. The defaults below are placeholders, to be changed from tester feedback. Nothing here waits on 8.3.

### What exists

- **Hotkey thread** (crates/cv-platform-win/src/hotkey.rs, ADR 0005, moved there by ADR 0004): three `RegisterHotKey` bindings with a NULL hWnd, all `MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT`, kept as a `BINDINGS` constant (`Ctrl+Alt+Shift+Z` toggle, `+Up` zoom in, `+Down` zoom out). The thread blocks in `GetMessageW` and calls `on_action(Action)` for each `WM_HOTKEY`. A failed registration is logged with its `GetLastError` code and reported once to the panel through `on_failures`, with no fallback and no retry.
- **`Action`** (crates/cv-core/src/lib.rs:208): `Toggle`, `ZoomIn`, `ZoomOut`, applied by `AppState::apply`. app's closure (crates/app/src/main.rs:104-114) applies it under the write lock and wakes the panel.
- **ADR 0013** (Accepted) adds `Action` variants for silence and speech on/off in 8.8a. In 8.8b, app's hotkey closure sends silence to the `SpeechHandle` and applies speech on/off through `AppState::apply`. With the `tts` feature off, these bindings are not registered. 0013 asked 8.6 to say whether the silence key must be detected somewhere other than the hotkey thread.
- **Commands the track names**: speech on/off and silence (8.8); read window title, repeat focused item, read the whole active window or dialog (9.8); a hotkey to cycle mouse echo modes (11.4); continuous reading from the caret, stopped by the silence key (12.2), and stepping by word, line, sentence and paragraph (12.3). Typing echo (10.x) and verbosity (9.5) have settings, but the roadmap names no hotkey for them. Neither does speech rate (ADR 0012), though the panel has a rate control.
- **Features**: `Win32_UI_Input_KeyboardAndMouse` (RegisterHotKey) and `Win32_UI_WindowsAndMessaging` (`SetWindowsHookExW`) are enabled. The raw input functions (`RegisterRawInputDevices`, `GetRawInputData`) are in `Win32::UI::Input` of the pinned `windows` 0.62.2 (src/Windows/Win32/UI/Input/mod.rs in the crate source), behind the `Win32_UI_Input` feature, which is not enabled yet. That is a feature, not a new dependency.

### External facts

- **RegisterHotKey** (cited in ADR 0005): it reports a modifier combination plus one key, on key down. A combination that is already taken fails with 1409 (`ERROR_HOTKEY_ALREADY_REGISTERED`), so conflicts can be detected. Win-key combinations belong to the shell. ([RegisterHotKey](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey))
- **Low-level keyboard hook.** The hook "is called in the context of the thread that installed it ... the thread that installed the hook must have a message loop". Returning nonzero stops the key reaching the target window. If the hook times out, "on Windows 7 and later, the hook is silently removed without being called. There is no way for the application to know whether the hook is removed". The timeout is capped at 1000 ms since Windows 10 1709. Microsoft adds: "In most cases where the application needs to use low level hooks, it should monitor raw input instead. This is because raw input can asynchronously monitor mouse and keyboard messages that are targeted for other threads more effectively than low level hooks can." ([LowLevelKeyboardProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc))
- **Raw input.** An app registers the keyboard top-level collection (usage page 0x01, usage 0x06) and gets `WM_INPUT` messages. It "can receive data when it is in the foreground and when it is in the background (if registered with **RIDEV_INPUTSINK**)" ([Raw Input Overview](https://learn.microsoft.com/en-us/windows/win32/inputdev/about-raw-input)). `RIDEV_INPUTSINK` "enables the caller to receive the input even when the caller is not in the foreground. Note that **hwndTarget** must be specified" ([RAWINPUTDEVICE](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawinputdevice)). Raw input reports keys. It has no way to stop them reaching other apps, so it can't swallow a key. A message-only window (`HWND_MESSAGE` parent) as the target is a common pattern (for example [Panda3D's input device manager](https://www.panda3d.org/reference/cxx/winInputDeviceManager_8cxx_source.html)). Microsoft doesn't document it for raw input, so 8.6b checks it.
- **Elevated foreground windows.** WM_INPUT is reported to be subject to UIPI. Whether a medium-integrity app gets raw keyboard input while an elevated window has the foreground is not documented by Microsoft and is unverified here. The same is unverified for hooks and `RegisterHotKey` (ADR 0005). UIAccess (6.3, on hold) is where this is fixed.
- **ZoomText speech keys**: Caps Lock+Alt+Enter turns voice on and off, Caps Lock+Alt+Up and Down change the rate, Caps Lock+Alt+M and K cycle mouse and keyboard echo, and "press CTRL to momentarily stop speech" ([Freedom Scientific: Getting Started with ZoomText](https://www.freedomscientific.com/training/zoomtext/getting-started/), [SAST: ZoomText hotkeys](https://sastltd.zohodesk.eu/portal/en/kb/articles/zoomtext-hotkeys)). The magnifier keys use the same Caps Lock modifier (ADR 0005).
- **Narrator**: Caps Lock and Insert are both the Narrator key by default. "Press the Ctrl key to stop Narrator from reading". Caps Lock pressed twice quickly toggles capitals ([Narrator basics](https://support.microsoft.com/en-us/windows/chapter-2-narrator-basics-5ff4591e-7b6d-245e-c95d-ce83c0a1a8d4)).
- **NVDA**: Control stops speech, Shift pauses it. The NVDA key is Insert or Caps Lock ([NVDA commands quick reference](https://api.nvaccess.org/documentation/keyCommands.html)). In its source, NVDA traps the modifier in a low-level hook (`winInputHook`), passes a second press within the multi-press timeout through as the real key, and cancels speech **on key down** ([keyboardHandler.py](https://github.com/nvaccess/nvda/blob/master/source/keyboardHandler.py)).

## Options

### How Ctrl silences speech

A. **No lone Ctrl: silence is a chord** (for example Ctrl+Alt+Shift+X) through `RegisterHotKey`. No new mechanism. Cost: three keys to stop speech, against the one key that ZoomText, Narrator and NVDA all use. This misses what the roadmap item asks for.

B. **Raw input, observe only.** The hotkey thread gets a message-only window registered for raw keyboard input with `RIDEV_INPUTSINK`. It reads the virtual key and the up/down flag from each `WM_INPUT`, throws the rest away, and reports silence on the first key down of either Ctrl. Ctrl still reaches every app unchanged, because raw input can't swallow it, so Ctrl shortcuts can't break. There is no per-keystroke code that other apps wait on and no silent removal. Microsoft recommends raw input over hooks for monitoring. Cost: the `Win32_UI_Input` feature; a window and `WM_INPUT` handling on the hotkey thread; our process receives every keystroke as a message (we read only the key code). Whether it sees keys while an elevated window has the foreground is unknown.

C. **Low-level keyboard hook, observe only** (never returns nonzero). Same rule as B. Cost: every key on the machine waits for our hook procedure. If the thread stalls past the timeout, the hook is removed with no way to find out, and silence stops working without a sign. It also brings the hook risks ADR 0005 listed (security software, other hooks). Gains over B: the `LLKHF_INJECTED` flag. It is also the only base on which a Caps Lock layer (option 3 below) can be added later, so choosing it now would save a second mechanism then.

### The scheme for the other speech commands

1. **RegisterHotKey chords on Ctrl+Alt+Shift, as in ADR 0005.** Each command gets Ctrl+Alt+Shift plus one key. Cost: about ten more three-modifier chords. Each takes that combination from every other app while clear-view runs. Some will clash with app shortcuts (Word has many Ctrl+Alt+Shift combinations); the 1409 failure covers clashes with other programs that register hotkeys, but not shadowed app shortcuts. Gains: Windows does the key matching, a clash is detected and reported, and nothing runs per keystroke. Ctrl+Alt without Shift stays avoided: on many layouts it is AltGr, and it is Windows Magnifier's set (ADR 0005).
2. **Ctrl+Alt+Shift chords plus a layer key**: one chord (for example Ctrl+Alt+Shift+Space) arms a layer for about two seconds, during which single letters are registered as hotkeys with no modifiers and then unregistered. Cost: while the layer is armed, typed letters go to us, not the app. If unregistering fails, letters are stolen until it succeeds. It is also a second state machine in the hotkey thread. Gains: fewer clashes and easier keys. This is like ZoomText's layered keys, but without a hook.
3. **Caps Lock modifier through a low-level hook**, ZoomText style: Caps Lock+Alt+key for speech, Caps Lock+key for the magnifier. Caps Lock is swallowed, and a double press passes through to toggle capitals, as Narrator and NVDA do. Lone Ctrl comes from the same hook (C). Cost: every risk in ADR 0005 option 2. Every keystroke waits for our process. The hook is removed silently on a stall, and then Caps Lock works as normal again and every hotkey stops without a sign. A bug can swallow keys or leave Caps Lock stuck, against PRODUCT principle 2. Nothing signals a key already taken, so ZoomText or NVDA running alongside fights us. And it may not see keys in elevated windows. Gains: two-key combinations, the tester's muscle memory from ZoomText, and room for every later command without taking chords from apps.

## Decision

Recommendation: **B for silence, 1 for the other commands, no Caps Lock modifier yet.** The bindings become data in cv-core so a Caps Lock layer or a key picker can be added later without touching the actions. This extends ADR 0005. It supersedes only 0005's line "bindings are constants in hotkey.rs": the table moves to cv-core, and the backend maps it to virtual keys. Everything else in 0005 stands: `RegisterHotKey`, `MOD_NOREPEAT`, failure reporting, no automatic fallback, not user-editable. Changing 0005's Status line is yours. This answers ADR 0013's question: the silence key is detected on the hotkey thread, so app's closure sends it to the `SpeechHandle` the way 0013 planned.

Why: real use decides (PRODUCT principle 3), and nobody has asked for Caps Lock yet. The tester can't be asked now (8.3 is held). A hook is the riskiest way to read keys, and Microsoft says to monitor with raw input instead. Silence must work every time, so it should not depend on a hook that can vanish without a sign.

### Silence: a lone Ctrl, observed with raw input

- **Where**: the hotkey thread in cv-platform-win creates a message-only window and registers the keyboard collection (0x01, 0x06) with `RIDEV_INPUTSINK` and that window as the target. No `RIDEV_NOLEGACY`. The same `GetMessageW` loop handles `WM_HOTKEY` and dispatches `WM_INPUT` to the window. The window reads the key with `GetRawInputData` and calls `DefWindowProc`. It is built and registered only when app asks for speech keys, which app does only with the `tts` feature. With the feature off, the hotkey thread is exactly as today.
- **Rule**: the first key down of left or right Ctrl reports `Action::Silence`. Repeats while it is held do nothing until that key comes up. Ctrl is never swallowed. Ctrl+C, Ctrl+click and our own Ctrl+Alt+Shift chords also silence, because Ctrl goes down first. This matches NVDA, which cancels on key down, and it is the fastest path to ADR 0013's 100 ms target. Stopping on a lone tap instead (Ctrl up with no other key between) would spare Ctrl shortcuts, but silence would wait for the key to come up. That is a change to this rule, made from tester feedback.
- **Pure part**: the rule is a small state machine in cv-core (`hotkeys::SilenceKey`, fed neutral `(KeyId, down)` pairs, returns whether to silence), with unit tests: first down, repeats, left and right held together, up without down. The backend only turns `WM_INPUT` into those pairs.
- **Unknown until 8.6b runs**: whether AltGr (right Alt on the UK layout) shows up in raw input as a left Ctrl. If it does, 8.6b ignores a left Ctrl that comes with a right Alt in the same report sequence and says so in the PR. Also unknown: whether a message-only window gets `WM_INPUT` with `RIDEV_INPUTSINK`. If not, a hidden top-level window is used instead. And whether keys arrive while an elevated window has the foreground. An admin Notepad is in the Verify list. If keys don't arrive, it waits for UIAccess (6.3) like the other hotkeys.
- With speech off (`tts_enabled` false), silence still reaches the speech thread, which drops it (ADR 0013). Nothing else changes.

### The other commands: Ctrl+Alt+Shift chords

Every binding uses `MOD_NOREPEAT`, as in ADR 0005. Speech bindings are registered only with the `tts` feature. A command's binding is registered from the step that adds its `Action`, not before, so no chord is taken for a command that doesn't exist yet.

| Command | Default | Step that adds it | Notes |
|---|---|---|---|
| Magnifier on/off | Ctrl+Alt+Shift+Z | done (1.3) | unchanged |
| Zoom in / out | Ctrl+Alt+Shift+Up / Down | done (1.3) | unchanged |
| Silence | Ctrl, alone (raw input) | 8.6b with 8.8a's `Silence` | see above |
| Speech on/off | Ctrl+Alt+Shift+S | 8.6b with 8.8a's `SpeechToggle` | ZoomText uses Caps Lock+Alt+Enter |
| Read window title | Ctrl+Alt+Shift+T | 9.8 | NVDA uses NVDA+T |
| Repeat focused item | Ctrl+Alt+Shift+F | 9.8 | Ctrl+Alt+Shift+Tab is avoided: Ctrl+Alt+Tab is the shell's task switcher |
| Read the active window or dialog | Ctrl+Alt+Shift+W | 9.8 | NVDA uses NVDA+B |
| Cycle typing echo | Ctrl+Alt+Shift+K | 10.x, only if a step adds it | ZoomText's K |
| Cycle mouse echo (off, word, line) | Ctrl+Alt+Shift+M | 11.4 | ZoomText's M |
| Start or pause reading from the caret | Ctrl+Alt+Shift+R | 12.2 | 12.1 may change it |
| Step by word, line, sentence, paragraph | decided by 12.1 | 12.3 | 12.1 picks keys inside this scheme, or opens a new ADR if it needs a mode where plain keys step |
| Speech rate faster / slower | Ctrl+Alt+Shift+PageUp / PageDown | none yet | reserved. Up and Down are zoom. A roadmap item adds it if you want it |
| Cycle verbosity | Ctrl+Alt+Shift+V | none yet | reserved. 9.5 decides whether it gets a hotkey |

- A step that registers a binding checks the documented shortcuts of Word, Chrome, Edge and File Explorer for that combination. It may pick another key from the same Ctrl+Alt+Shift set and record why in its PR. That doesn't need a new ADR. Using another modifier set, a layer, or a hook does.
- TESTER.md lists the speech keys from 9.11, as the roadmap says.

### Bindings as data (cv-core)

- A new cv-core module `hotkeys`: neutral `Key` (letters, arrows, PageUp, PageDown and the keys in the table), `Chord { key }` with Ctrl+Alt+Shift implied, `Binding { chord, action, label, speech: bool }`, `DEFAULT_BINDINGS`, and `SilenceKey`. The tests check that no two bindings share a chord, every label matches its chord, and every speech binding is marked `speech`. A `Chord` has no modifier field yet, because every binding is Ctrl+Alt+Shift. A Caps Lock layer or a key picker would add one, in a new ADR.
- cv-platform-win maps `Key` to a virtual key and registers `DEFAULT_BINDINGS`, filtered by a `speech_keys: bool` that app passes to `spawn_hotkey_thread`. `TOGGLE_LABEL` comes from the table. The ids stay per binding as now.
- No `AppState` field and nothing saved. Bindings stay constants.

### Caps Lock: not now

No Caps Lock modifier and no low-level hook in this track for now. Revisit with a new ADR when any of these happens:
- 8.3 is raised and says the tester relies on Caps Lock+Alt keys;
- tester feedback after 9.11 says the chords are too hard;
- 10.1 chooses a low-level hook for typing echo anyway, which would make a Caps Lock layer cheaper.

### Roadmap

Two items under Phase 8, pending this ADR. They come after 8.8a, because 8.8a adds the `Silence` and `SpeechToggle` actions, and before 8.8b, which wires them to speech:

- 8.6a cv-core `hotkeys`: `Key`, `Chord`, `Binding`, `DEFAULT_BINDINGS` (the three magnifier bindings and speech on/off), `SilenceKey`, with unit tests. cv-platform-win registers from the table instead of its own `BINDINGS`, and the magnifier hotkeys behave as before. No raw input yet.
- 8.6b Raw input silence and the speech on/off chord in cv-platform-win, behind `speech_keys` (app passes true only with `tts`): message-only window, keyboard registration with `RIDEV_INPUTSINK`, `WM_INPUT` to `SilenceKey`, Ctrl+Alt+Shift+S registered; the `Win32_UI_Input` feature. app logs the two actions (one `[keys]` line each) and applies speech on/off; 8.8b sends them to the speech thread. Verify: a Ctrl tap logs silence once, and holding Ctrl logs once; Ctrl+C, Ctrl+V and Ctrl+click still work in Notepad and Brave; AltGr+4 (€) on the UK layout logs nothing, or the PR says why; an admin Notepad in the foreground (recorded, not fixed); a plain build, without `tts`, registers no raw input and behaves as before.

## Consequences

- Easier: silence is one key, as in ZoomText, Narrator and NVDA. Nothing we run sits in the path of other apps' keystrokes, and Ctrl shortcuts can't break. Every later command is one row in a tested table. A Linux backend later reads the same table. A Caps Lock layer or a key picker has a place to go.
- Harder: ten more three-modifier chords for the tester to learn, each taken from every other app while clear-view runs. Some will shadow app shortcuts (Word especially) without any failure being reported. Our own chords and every Ctrl shortcut also silence speech. The hotkey thread gains a window and `WM_INPUT` handling.
- Unknown until built: AltGr and raw input on the UK layout; a message-only window as the raw input target; keys while an elevated window has the foreground; whether silencing on Ctrl+C or Ctrl+click annoys the tester.
- Revisit: when 8.3 is raised, after 9.11 feedback, at 10.1 if it picks a hook, at 12.1 for stepping keys, and when 6.3 adds UIAccess.
