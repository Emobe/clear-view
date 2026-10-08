# 0005: Hotkey scheme

Status: Proposed
Date: 2026-10-09

## Context
Roadmap 1.2. PRODUCT.md v1 Must 4 needs on/off, zoom in and zoom out hotkeys, and says Ctrl+Alt+Shift+Z stays for on/off until this ADR decides the scheme. PRODUCT.md also asks which ZoomText hotkeys the tester relies on; that is to be asked once M1 is in their hands (Open questions).

The code today (crates/app/src/hotkey.rs):
- One binding: `RegisterHotKey(None, 1, MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT, VK_Z)` on a dedicated thread spawned from main.rs. On `WM_HOTKEY` it flips `AppState.enabled` and calls `wake()` so the panel repaints.
- The loop polls with `PeekMessageW` and a 10 ms sleep instead of blocking in `GetMessageW`.
- A failed registration only prints to stderr (STATUS Missing). Nothing tells the user in the settings window.
- Win-key combinations were tried and dropped: Win+= opened Windows Magnifier (STATUS Works, old 2.1).
- `AppState.zoom` is clamped to 1.0..=10.0 by `sanitize`; 1.4 widens it to 20 and picks the step size.

Both candidate APIs are in the `windows` 0.62 features already enabled in Cargo.toml (`Win32_UI_Input_KeyboardAndMouse` for `RegisterHotKey`, `Win32_UI_WindowsAndMessaging` for `SetWindowsHookExW`). Neither needs a new dependency.

External facts:
- `RegisterHotKey` with a NULL hWnd posts `WM_HOTKEY` to the calling thread's queue. It "typically" fails if the keystrokes are already registered for another hot key; `GetLastError` then gives the reason. Win-key shortcuts are reserved for the OS. F12 is reserved for the debugger. `MOD_NOREPEAT` stops auto-repeat from sending more notifications. ([RegisterHotKey](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey))
- The error for a taken combination is `ERROR_HOTKEY_ALREADY_REGISTERED`, 1409 (0x581). ([System error codes 1300-1699](https://learn.microsoft.com/en-us/windows/win32/debug/system-error-codes--1300-1699-))
- A `WH_KEYBOARD_LL` hook runs on the installing thread, which must have a message loop. Returning nonzero swallows the key. If the hook takes longer than `LowLevelHooksTimeout` (capped at 1000 ms since Windows 10 1709), Windows 7 and later remove it silently, and "there is no way for the application to know whether the hook is removed". Microsoft recommends a dedicated hook thread that hands work off and returns at once. ([LowLevelKeyboardProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc))
- Without UIAccess the app runs at medium integrity and cannot reach elevated UI ([Security considerations for assistive technologies](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-securityoverview)). Third-party reports say a medium-integrity low-level hook stops receiving keys while an elevated window is in the foreground ([example](https://en.ittrip.xyz/windows/detect-hook-blocked)). Microsoft does not document this for hooks, and it is unverified here. Whether `RegisterHotKey` hotkeys fire while an elevated window is in the foreground is also unverified.
- ZoomText uses Caps Lock as its modifier: Caps Lock+Up zooms in, Caps Lock+Down zooms out, Caps Lock+Enter toggles zoom and 1x, Caps Lock+Ctrl+Enter turns ZoomText on and off. With ZoomText running, Caps Lock is double-tapped to toggle capitals. ([Freedom Scientific: ZoomText hotkeys](https://www.freedomscientific.com/training/zoomtext/zoomtext-hotkeys/), [SAST: ZoomText hotkeys](https://sastltd.zohodesk.eu/portal/en/kb/articles/zoomtext-hotkeys))
- Windows Magnifier uses Win+Plus and Win+Minus for zoom, Win+Esc for off, and Ctrl+Alt with Minus, I, F, D, L, M, R, Space, arrows and the mouse wheel. ([Magnifier keyboard shortcuts](https://support.microsoft.com/en-gb/windows/magnifier-keyboard-shortcuts-and-touch-gestures-6d7c3095-75ec-258f-6f5b-3b0bc19a18e7))

## Options
1. **RegisterHotKey with fixed modifier combinations.** Extend the current approach to three bindings. Costs: a three-modifier chord is harder to press than ZoomText's two-key Caps Lock combinations, and the tester's muscle memory does not carry over. A combination another app has taken fails cleanly with 1409, so a conflict is detectable and reportable. Windows does all key matching. No code runs per keystroke, so a bug or a hang here cannot break typing.
2. **Low-level keyboard hook with a Caps Lock modifier, ZoomText style.** Costs: every keystroke on the machine passes through our process. The hook thread must never block, and a timeout removes the hook silently with no way to detect it. The current 10 ms polling loop would delay every keystroke. Caps Lock must be swallowed and its state managed (double-tap to toggle capitals); a bug could leave Caps Lock stuck or swallow keys, which breaks PRODUCT principle 2 ("never leave the machine broken"). There is no "already taken" signal; we would silently win or lose against other hooks, ZoomText included. Reported not to see keys while an elevated window is in the foreground until UIAccess (6.3). Security software may flag a global keyboard hook.
3. **Option 1 now, with the bindings as data, and a hook-based Caps Lock layer only if the tester asks for it.** The same as option 1 for M1, with the three actions (toggle, zoom in, zoom out) kept separate from the key combinations that trigger them, so a second trigger source can be added later without touching the actions. Cost: if the tester needs Caps Lock, a second ADR and more work come after M1.

## Decision
Recommendation: option 3.

PRODUCT principle 3 says real use decides, and the open question about which ZoomText keys the tester relies on is to be asked once M1 is in their hands. A keyboard hook is the riskiest way to deliver three hotkeys, and its main benefit (Caps Lock) is not known to be needed yet.

For 1.3:
- **API**: `RegisterHotKey` on the existing hotkey thread, NULL hWnd, one id per action. Every binding uses `MOD_NOREPEAT`: one press, one action. Holding a zoom key does not repeat; this is revisited from tester feedback.
- **Default bindings**:
  - On/off: Ctrl+Alt+Shift+Z (unchanged, verified working)
  - Zoom in: Ctrl+Alt+Shift+Up
  - Zoom out: Ctrl+Alt+Shift+Down

  Up and Down match the direction of ZoomText's Caps Lock+Up and Down. No Win key (the shell owns it), no F12 (reserved), and none of Windows Magnifier's Ctrl+Alt combinations without Shift.
- **Zoom step**: decided in 1.4, not here. 1.3 uses whatever step 1.4 records, or a placeholder that 1.4 replaces.
- **A binding that is already taken**: registration happens once, at startup. If it fails, that action has no hotkey for the session. The settings window shows one line per failed binding, naming the combination and saying another program is using it, and stderr logs the `GetLastError` code. There is no automatic fallback combination: TESTER.md (1.7) would then be wrong. There is no retry loop: the user closes the other program and restarts clear-view. The registration result is runtime-only and never saved to the settings file. 1.3 chooses how it reaches the panel, keeping in mind that a new `AppState` field must be added to the `apply_changes` list.
- **Not user-editable in v1**: bindings are constants in hotkey.rs. A key picker and saved bindings wait for tester feedback.
- **Message loop**: the hotkey thread blocks in `GetMessageW` instead of polling with `PeekMessageW` and a sleep.
- **Where the code lives**: hotkey.rs in the app crate, as now. The action list is Windows-free and can move behind the platform seam in 2.1/2.2. This ADR does not decide that boundary; ADR 0004 does.

This ADR supersedes nothing. No Accepted ADR covers hotkeys.

## Consequences
- 1.3 is small: two more `RegisterHotKey` calls, a dispatch on the id, the zoom change with a panel `wake()`, the failure message in the panel, and `GetMessageW`. The zoom hotkeys change `AppState.zoom`, so the settings saver persists them like a slider change.
- Three-modifier chords may be hard for the tester. 1.8 feedback decides whether to add a Caps Lock layer (a new ADR, likely a `WH_KEYBOARD_LL` hook on its own thread) or different combinations.
- Ctrl+Alt+Shift+Up and Down become unavailable to other apps while clear-view runs (for example VS Code's column selection). This is accepted for v1.
- Whether the hotkeys fire while an elevated window is in the foreground is unknown. 1.3 lists it in its Verify list, and UIAccess (6.3) is where it gets fixed if they don't.
- Revisit: after the 1.8 feedback; when ADR 0004 places hotkeys behind the platform seam; when 6.3 adds UIAccess.
