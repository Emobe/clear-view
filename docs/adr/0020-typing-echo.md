# 0020: Typing echo

Status: Accepted (you, 2026-11-03, "I Accept it"; set by the agent on your instruction)
Date: 2026-11-03

## Context

Roadmap 10.1, the first ADR of speech stage 2 (Phase 10, PRODUCT.md "Speech (v1.1)"). It decides where typed input comes from (a keyboard hook, UI Automation text events, or both); whether characters, words or both are spoken; what happens with shortcut keys, dead keys and the UK layout, and with Backspace and Delete; and how the password rule is enforced. Character echo is 10.2, word echo 10.3. What moving the caret speaks is 10.4 and is not decided here.

8.3 (how the tester uses ZoomText's speech, including which echo mode) is on hold. The defaults below are placeholders, to be changed from tester feedback. Nothing here waits on 8.3.

### Rules this must keep (ROADMAP "Rules for speech steps", PRODUCT.md)

- With speech switched off, the magnifier behaves exactly as before.
- Password fields are never spoken: no characters, no words, no length. "Can't tell" counts as a password field.
- No UI Automation or MSAA call runs on the render thread. A hung app must not freeze the magnifier or stop speech from being silenced.
- The text that gets spoken is pure logic in cv-core with unit tests.
- Only cv-platform-win and cv-tts depend on `windows`.
- PRODUCT principle 2: never leave the machine broken. Nothing we add may swallow, delay or change a key.

### Earlier decisions this touches

- **ADR 0005 and 0014** (Accepted). The hotkey thread in cv-platform-win already receives every keystroke on the machine. With `speech_keys` (app passes true only with `tts`) it owns a message-only window registered for raw keyboard input (usage page 0x01, usage 0x06) with `RIDEV_INPUTSINK` and no `RIDEV_NOLEGACY`. It reads each `WM_INPUT` with `GetRawInputData`, maps it to cv-core `(KeyId, down)` (`key_event`, crates/cv-platform-win/src/hotkey.rs:60) and feeds the lone-Ctrl `SilenceKey`. Raw input can't swallow a key. 0014 chose raw input over a low-level hook because Microsoft recommends it for monitoring and because a hook is removed silently on a stall. Its option B, which its Decision adopted, "reads the virtual key and the up/down flag from each `WM_INPUT`, throws the rest away", and its costs say "we read only the key code". Its Caps Lock section lists as a revisit trigger: "10.1 chooses a low-level hook for typing echo anyway, which would make a Caps Lock layer cheaper."
- **STATUS Finding 24** (resolved by 8.6b): raw input registration is one per process and device type. winit (under eframe) registers the keyboard when its event loop is created and took our registration. The hotkey thread now checks every 500 ms (`RAW_INPUT_CHECK_MS`) and restores it, logging `[hotkey] raw keyboard input had been taken over; restored`. Keys in that window of up to 500 ms, key-ups included, are not seen.
- **ADR 0015** (Accepted) and 0016 to 0019. The focus thread in cv-platform-win, spawned only with `tts`, hooks foreground and focus WinEvents with `WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS` (crates/cv-platform-win/src/focus/mod.rs:210), so it gets no event when one of our own windows takes the foreground or the focus. The callback stamps each event with `Instant::now()` (mod.rs:235), which is when the thread pumps it, not when it was raised: while a read blocks the thread (up to about 1 s in a hung app), events wait in its queue. It runs the pure cv-core `FocusFilter` (`begin_read`, `finish_read`; `SETTLE` 50 ms, `SETTLE_MAX` 150 ms; stale and duplicate readings dropped) and publishes `CoreEvent::FocusChanged` with a merged `Control` whose `states.password` is `Yes`, `No` or `CantTell`. `focus::merge` gives `No` only if one source says no and the other doesn't say yes. MSAA without `STATE_SYSTEM_PROTECTED` maps to `No` (crates/cv-platform-win/src/focus/map.rs:270). It reads only while `tts_enabled` is true (mod.rs:293) and never when the foreground window is ours. A hung or unreadable window is published with `control: None`. 0015 says: "If a later step needs a UIA handler (10.1 may), handlers go on one thread only, and that step's ADR says which." No thread adds one today. 9.9 (PR #119, open, not merged) widens "wanted" to include its follow-focus setting.
- **ADR 0013** (Accepted). `Kind::Typing` is foreground: it cuts off everything. A batch cuts off what its first item's rule says. Events older than a silence are dropped. The speech thread makes no UIA or MSAA call and turns events into utterances with pure cv-core code (`speech::utterances_for`, plus the stateful `FocusMemory` from 9.8a, crates/cv-core/src/speech.rs:292).
- **ADR 0004** (Accepted): one `#[non_exhaustive]` `CoreEvent` enum of plain data with a timestamp, fan-out delivery, no OS handles. The render thread's `Tracker::handle` matches every variant, so a new variant needs a new arm there.

### What 9.2 measured that matters here (docs/prototypes/focus-sources.md)

- **Notepad's text area** (class `RichEditD2DPT`): UIA type `Pane`, "no Value or Text pattern", MSAA role `client`. "Neither says 'edit'." So neither the role nor a text pattern tells us Notepad's text area is editable, and its text can't be read through UIA. You run Notepad as administrator every time (the same row).
- **Windows Terminal**: UIA `Text`, and the Text pattern holds the whole buffer, flagged `not-password` (Finding 25(h)). A password typed at an `ssh`, `sudo` or `runas` prompt goes into this same control, so its flag says `No`.
- **Search box** (Start): UIA `Edit` with a Value pattern whose value is the typed text. MSAA `editable`, same value.
- A Reddit password field in 9.1: UIA `password`, MSAA `protected`. No password field was in the 9.2 runs.
- Not measured: remote desktop, virtual machine and Citrix windows. Their content is one bitmap to both sources, so the focused control is the client window itself, and MSAA without a protected state gives `No` (map.rs:270) whatever is typed into the remote session.

### External facts

- **ToUnicodeEx** translates a virtual key and a 256-byte keyboard state into characters for a given layout (`dwhkl`). In `lpKeyState` "only the toggle bit of the CAPS LOCK key is relevant. The toggle state of the NUM LOCK and SCROLL LOCK keys is ignored." `wFlags` bit 2: "keyboard state is not changed (Windows 10, version 1607 and newer)". Return values: below 0, a dead key, and "the function has written a spacing version of the dead-key character" (for example U+00B4 ACUTE ACCENT, not U+0301); 0, no translation; above 0, that many UTF-16 units. Without bit 2, "it also changes the state of the kernel-mode keyboard buffer. This state-change affects dead keys, ligatures, Alt+Numeric keypad key entry" and "might also cause undesired side-effects if used in conjunction with TranslateMessage". Microsoft doesn't say whether that buffer belongs to the calling thread or is shared ([ToUnicodeEx](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-tounicodeex)).
- **Raw input** (cited in ADR 0014): `RIDEV_INPUTSINK` gets input while another window has the foreground. The header's `hDevice` is "a handle to the device generating the raw input data", and "can be zero if an input is received from a precision touchpad" ([RAWINPUTHEADER](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawinputheader)). Microsoft doesn't document whether `SendInput` keys (the on-screen keyboard, some remote tools) reach raw input, or with what `hDevice`, or whether a medium-integrity process gets raw keyboard input while an elevated window has the foreground. Both are unverified here, and 8.6b's admin Notepad result was not recorded.
- **Scan codes.** On a PC keyboard left Shift is make code 0x2A and right Shift 0x36, without an E0 prefix. An E0-prefixed 0x2A or 0x36 is a "fake" Shift that some keyboards send around navigation and numpad keys, and it is not a real Shift press. The right Ctrl and right Alt keys carry E0. (Scan code set 1: "e0 2a" is a fake LShift and "e0 36" a fake RShift, sent around the grey navigation keys "to fool old programs" ([Brouwer, Keyboard scancodes](https://aeb.win.tue.nl/linux/kbd/scancodes-1.html)). Raw input reports the make code and the E0 flag as separate fields; 10.2b1 confirms the fake Shift reports with `CV_KEYS_LOG`.)
- **UIA text events.** `AddTextEditTextChangedEventHandler` (Windows 8.1) "registers a method that handles programmatic text-edit events"; clients register per `TextEditChangeType`. Its remarks repeat that "a UI Automation client should not use multiple threads to add or remove event handlers" ([AddTextEditTextChangedEventHandler](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation3-addtextedittextchangedeventhandler)). The old reader plan noted that the older `TextChanged` event "does not describe what changed. You must diff against a cached previous text snapshot" (docs/archive/tts-plan.md, Stage 7).
- **ZoomText** (the tester's reader). Keyboard echo has four modes: No Echo, Keys, Words, Keys and Words. "Typed words are spoken upon completion when you type a space or punctuation character", and word echo "occurs only when a completed word appears on the screen". In Keys mode the user picks groups: printable keys; navigation keys (arrows, Home, End, Page Up, Page Down); editing keys (Backspace, Insert, Delete, Escape, Enter); function keys; modifier keys; locking keys "and their state"; other keys. Caps Lock+Alt+K cycles the mode. Its text editing table says Backspace "speaks the deleted character" and Delete "speaks the new character to the right of the cursor" ([ZoomText User Guide, English UK](https://support.freedomscientific.com/content/documents/manuals/ZoomText/ZoomText_User_Guide_English_UK.pdf), "Keyboard Echo", pp. 144–147, and the text reading keys table; [SAST: ZoomText/Fusion typing echo](https://sastltd.zohodesk.eu/portal/en/kb/articles/fusion-adjust-change-typing-echo)).
- **NVDA**. "Speak typed characters" defaults to "Only in edit controls" (others: Off, Always); "Speak typed words" defaults to Off; word echo is triggered by space, Enter or most punctuation ([NVDA keyboard settings, summarised](https://www.mintlify.com/nvaccess/nvda/configuration/keyboard-settings), via search; the page now returns 410). NVDA gets typed characters from an in-process helper that sees the app's own character messages. For UWP windows, where it "cannot inject in-process", it calls `ToUnicodeEx` itself, because there "wm_keydown / wm_char is never processed, thus NVDA will be the only one calling it" ([nvda PR #6631](https://github.com/nvaccess/nvda/pull/6631)). That is the TranslateMessage side-effect above, which bit 2 now avoids.

## Options

### Where typed input comes from

1. **Raw input keystrokes, translated by us.** Extend the hotkey thread's existing raw input: read the virtual key, scan code and flags of every report (not only Ctrl), keep our own key-down state from the reports, and translate key downs with `ToUnicodeEx` (bit 2 set, the foreground thread's layout from `GetKeyboardLayout(GetWindowThreadProcessId(GetForegroundWindow()))`). Costs: we now handle every key's code, not only Ctrl's, and turn letters into characters in our process (still observe-only: nothing is swallowed, delayed or changed). Our key state is rebuilt from reports, so Caps Lock's starting toggle state and keys held across a lost registration are guesses until corrected. Dead keys must be composed by us, because bit 2 stops Windows keeping the dead key. The raw report arrives at input time, possibly before the app has handled an earlier Tab, so the password gate needs a hold and a barrier (Decision), and echo comes at least 100 ms after the key. A key goes to whatever control has focus, and the control's password flag is all we can check: a terminal password prompt or a remote session says `No`, so those apps need a rule of their own. Doesn't see text that arrives without keys (paste, dictation, an IME), or know what Backspace and Delete removed. May not see keys while an elevated window is in front. Gains: no new mechanism, no new feature, no UIA handler, nothing in other apps' key path; the only option that can echo in Notepad, whose text UIA can't read.
2. **A low-level keyboard hook (`WH_KEYBOARD_LL`) in place of raw input.** The same translation and gate. Costs: every risk ADRs 0005 and 0014 listed. Every key on the machine waits for our hook. A stall past the timeout removes the hook silently, and with it typing echo and the silence key. Security software may flag it. Gains: `LLKHF_INJECTED` marks injected keys, and the hook runs before the app gets the key. It would also make a Caps Lock layer cheaper later (0014's revisit trigger). Neither gain is asked for, and the hook is subject to the same elevation question.
3. **UI Automation text events (and MSAA `EVENT_OBJECT_VALUECHANGE`), diffed.** Speak what the control says changed. Costs: the first UIA event handler in the process, so 0015's one-thread rule becomes live. `TextChanged` doesn't say what changed, so each event needs a text read and a diff; Terminal's text is its whole buffer, and documents are large. Notepad's text area has no Value or Text pattern (9.2), so there is no echo in Notepad at all. Which providers (Chromium, XAML, Win32 Edit, Word) raise which events is not measured. It can't tell typing from a programmatic change, so an address bar's autocomplete or a chat's incoming text would be echoed. Gains: it speaks only what appears on the screen, as ZoomText's words do, so a terminal password prompt that shows nothing speaks nothing and a masked field shows only bullets. It also gives the deleted text for Backspace and Delete from the diff, and pasted or dictated text.
4. **Option 1 for keys, plus option 3 later** for what keys can't give (the character Backspace or Delete removed when we didn't type it, pasted text, an on-screen check for terminals). Costs: both mechanisms and the merge between them. Nothing asks for the extra yet.
5. **An in-process hook (a DLL loaded into every GUI process, seeing `WM_CHAR`), as NVDA does.** Costs: a DLL built for both 32- and 64-bit, loaded into other apps, so a bug in it crashes them (PRODUCT principle 2); it doesn't load in UWP (NVDA's own fallback is `ToUnicodeEx`); security software flags injection. Rejected.

### Characters, words or both

a. **ZoomText's four modes**: Off, Characters, Words, Characters and words. One saved setting, a panel control, and the cycle hotkey 0014 reserved (Ctrl+Alt+Shift+K). Cost: four states to test.
b. **Two independent switches** (characters on/off, words on/off), as NVDA has. The same four combinations, but two controls, and no single cycle order.

### Where echo applies

i. **Any focused control that passes the password gate** (ZoomText's model; NVDA's "Always"). Cost: letters typed into lists (type-ahead), games and single-key web shortcuts are spoken.
ii. **Only editable controls** (NVDA's default). Cost: "editable" can't be told for Notepad's text area (UIA `Pane`, MSAA `client`, no patterns), the main text app, so a role rule would silence echo exactly where it matters. A caret-based rule (echo while the caret thread has a caret) needs the caret state on the thread that gates, and isn't measured for password fields.

## Decision

Recommendation: **option 1 (raw input keystrokes, translated by us), a (ZoomText's four modes), i (any control that passes the gate)**, with the gate on the focus thread, a list of apps where echo is always off (terminals, remote and virtual machine windows), and the echo logic pure in cv-core.

It extends ADR 0014: the hotkey thread's raw input now reads every key report, not only Ctrl's, keeps a key state and translates key downs to characters. That supersedes only 0014's "reads the virtual key and the up/down flag from each `WM_INPUT`, throws the rest away" (option B, adopted by its Decision) and "we read only the key code" (its costs). Everything else in 0014 stands, including observe-only, no `RIDEV_NOLEGACY`, the silence rule and the registration check. It extends ADR 0015 (the focus thread gains a stroke input, a gate and a forced read; no UIA handler is added) and ADR 0004 (one new `CoreEvent` variant). It uses ADR 0013's `Kind::Typing` row unchanged. No Caps Lock layer and no hook: 0014's revisit trigger doesn't fire. Changing 0014's Status line is yours.

Why: it is the only option that can echo in Notepad, it adds no new mechanism or dependency, and nothing in it can break a key. ZoomText's modes are what the tester knows (PRODUCT principle 3). Whether raw input sees keys with your elevated Notepad in front decides whether it works for your real use, so that is the first check in 10.2b1, with a stop condition.

### Keystrokes (cv-platform-win hotkey thread)

- Only with `speech_keys` (the `tts` feature), as today. With the feature off, no raw input exists and nothing changes.
- **Key state.** For every keyboard report the thread updates its own 256-byte down state: the left and right entries (`VK_LSHIFT`, `VK_RSHIFT`, `VK_LCONTROL`, `VK_RCONTROL`, `VK_LMENU`, `VK_RMENU`) and the generic ones (`VK_SHIFT`, `VK_CONTROL`, `VK_MENU`, down while either side is). Right Shift is its own make code (0x36), not E0. E0-prefixed Shift reports are fake and are ignored. Right Ctrl and right Alt are told by E0, as `key_event` already does for Ctrl. Ctrl still feeds `SilenceKey` as now. Caps Lock's toggle is read once at start with `GetKeyState(VK_CAPITAL)` and flipped on each Caps Lock down that follows an up (not on repeats). When the 500 ms check restores the registration (Finding 24), the down state is cleared and the modifiers are read again with `GetAsyncKeyState`, so a lost key-up can't leave Shift or Ctrl stuck. 10.2b1 checks whether `GetKeyState(VK_CAPITAL)` is right on a background thread, and how the "press Shift to turn off Caps Lock" setting shows up; it uses a better read if one exists.
- **Forwarded only while wanted, in two phases.** On a key down the thread first asks app's `wanted()` callback, an `AtomicBool` that app keeps equal to `tts_enabled && tts_typing_echo != Off` wherever it already wakes the speech and focus threads (no lock per key). Only if it is true does the thread translate the key and call `on_key(KeyStroke)`, which app forwards to the focus thread with `FocusWaker::typed`. `typed` pushes onto a bounded queue (64 strokes, dropping the oldest when full) and wakes the thread with a posted message. With speech or echo off, no key is translated and nothing is forwarded.
- **Translation.** One `ToUnicodeEx` per key down, with bit 2 set (Windows 10 1607 or later, which Windows 11 always is), the foreground thread's layout, and the state array above with Caps Lock's toggle. No call without bit 2, ever, so the foreground app's dead keys and Alt+numpad entry can't be disturbed.
- **Times.** Each stroke carries the `WM_INPUT` message time (`GetMessageTime`, milliseconds since boot) and an `Instant` taken on receipt. The gate compares the message time with each WinEvent's `dwmsEventTime`. 10.2b1 checks that both are on the same clock; if they aren't, it uses the `Instant`s and the barrier alone, and says so in the PR.
- What leaves the hotkey thread is a neutral cv-core `KeyStroke { at, time_ms, key }`, with `key` one of:
  - `Char(String)`: what `ToUnicodeEx` returned (more than one unit is allowed);
  - `Dead(char)`: the spacing accent it wrote for a dead key;
  - `Backspace`, `Delete`, `Enter`, `Tab`, `Escape`;
  - `Navigation`: arrows, Home, End, Page Up, Page Down (10.4 decides what they speak);
  - `Shortcut`: any key with the Windows key down, or left Alt, or right Alt without left Ctrl, or Ctrl without right Alt (AltGr arrives as left Ctrl plus right Alt; 0014). A left Ctrl plus right Alt key that translates to nothing is also `Shortcut`;
  - `Other`: function keys, Print Screen, Pause, the Application key.
  Shift, Ctrl, Alt, the Windows key and the locking keys pressed alone make no stroke, so a capital or AltGr doesn't end a word. Key repeat (a held key's repeated make reports) gives a stroke per repeat.
- `KeyStroke` and `TypedKey` implement `Debug` without the character. No log line ever carries one. `CV_KEYS_LOG` stays limited to the Ctrl and Alt reports it logs today.

### The gate (cv-core `typing::TypingGate`, run on the focus thread)

This is how the password rule is enforced. The gate is pure, with an injected clock, and unit-tested.

- **Inputs.** `stroke(KeyStroke)`; `winevent(at, time_ms)` for every foreground or focus WinEvent the thread receives (the same place it calls `FocusFilter::winevent`); `reading(started, app, password)` for every merged reading from the thread's read, before `FocusFilter::finish_read` drops any as duplicate or stale, with `started` the time of `begin_read` and `None` for no control; `foreground_is_ours(bool)`, from `GetForegroundWindow` and `GetWindowThreadProcessId` (no cross-process call), checked by the thread just before each release; `tick(now)`. Output: strokes to publish, in order, a request for a forced read, or nothing. The gate is cleared whenever "wanted" flips or the thread's `Session` is reset.
- **The barrier.** Every stroke other than `Char`, `Dead`, `Backspace` and `Delete` can move focus (Tab, Shift+Tab, Enter, Escape, navigation, shortcuts, Alt+Tab). After one, every later stroke is held until a reading arrives that started after a WinEvent whose `time_ms` is at or after the barrier key's. If no such WinEvent comes within `BARRIER` (500 ms), the gate asks the thread for a forced read (one read of the focused control, as on a focus event), and the held strokes wait for that reading.
- **A stroke is released only if all of these hold:**
  1. `HOLD` (100 ms) has passed since the stroke, and the thread has pumped its queued WinEvents just before the check.
  2. No WinEvent received so far has a `time_ms` at or after the stroke's `time_ms` that is newer than the reading in rule 4. If one has, every held stroke is dropped. This covers Tab then a fast first letter: the letter's raw report can arrive before the app has moved focus to the password field, and the focus event that the move raises is stamped later than the letter, however late it is delivered.
  3. No barrier is open.
  4. The newest reading started after the newest WinEvent received, and is younger than `READING_MAX` (2 s). An older reading makes the first stroke ask for a forced read, and it waits for that reading. This catches a field that changes between text and password, or a page that swaps the focused field, without a focus event.
  5. That reading has a control, its `states.password` is `No`, and its app is not on the no-echo list. `Yes`, `CantTell` and `None` (Task Manager, a hung window, nothing readable) drop the stroke.
  6. The foreground window is not ours (`WINEVENT_SKIPOWNPROCESS` means our own windows raise no event and no reading, so this is checked directly).
- **No-echo list** (cv-core `typing::NO_ECHO`, data, unit-tested). Apps where every stroke counts as `CantTell`, because a password typed there goes into a control that says `No`: terminals and consoles (Windows Terminal, conhost, OpenConsole, and anything with a console window class), remote desktop and virtual machine clients (mstsc, the Remote Desktop app, Hyper-V's vmconnect, VirtualBox, VMware, Citrix Workspace). It matches on program name and the foreground window class that the focus thread already reads. 10.2b1 confirms the names with `CV_FOCUS_LOG`. Focus echo in those apps is unchanged; only typing echo is off. Echo in a terminal comes back only with an on-screen check (option 4), in a new ADR.
- **A dropped stroke is gone.** It isn't counted, buffered or logged. A run of dropped strokes is followed by one `Break` (not one per stroke), so word echo never speaks the end of a word whose start was dropped, and nothing about the number of dropped keys can come out.
- Strokes held while a read is pending are released or dropped when the reading arrives. The gate holds at most 64 strokes and drops the oldest beyond that.
- The focus thread must be reading for the gate to work. "Wanted" stays `tts_enabled` (mod.rs:293); typing echo needs speech on anyway. Once 9.9 is merged, `|| follow focus` stays as 9.9 has it, and the gate is fed whenever the thread reads.
- Residual risk: a password field in an app not on the list that tells neither source it is one (a custom control, a game's login) reads as `No` and is echoed. 0015 trusts the same flag for a capped value read once at focus time; typing echo speaks every key, so this risk is larger. The list is how a newly found case is fixed: one row and a test.
- `BARRIER`, `HOLD`, `READING_MAX` and the list are starting values. A step may tune the numbers with a measurement and add rows, and says so in its PR. Removing a rule needs a new ADR.

### What the core reports

```rust
#[non_exhaustive]
pub enum CoreEvent {
    // .. existing variants unchanged
    /// A key typed into a control that passed the typing gate (ADR 0020). Never sent for a
    /// password field, a field that can't be told, an app on the no-echo list or our own windows.
    KeyTyped { at: Instant, app: AppId, key: TypedKey },
}

pub enum TypedKey {
    Char(String),
    Dead(char),
    Backspace,
    Delete,
    Enter,
    Tab,
    Escape,
    Navigation,
    /// A shortcut or another non-text key, or a run of dropped strokes: ends the word.
    Break,
}
```

`at` is the stroke's own receipt time, not the publish time. The focus thread publishes `Shortcut` and `Other` strokes as `Break`. `describe` gives the kind of key, never the character. The `Tracker` gets an arm that ignores it, with a test. Names may change in the step, the content may not.

### What gets spoken (cv-core, pure, in the speech path)

- A new `AppState` field `tts_typing_echo: TypingEchoMode` (`Off`, `Characters`, `Words`, `Both`), saved, default **`Characters`** (a placeholder until 8.3, NVDA's default; speech itself is off by default, so this only applies once the user turns speech on). The panel gets a selector in 10.2b2, and the Ctrl+Alt+Shift+K cycle binding (reserved by 0014) is added in 10.3.
- A stateful `typing::TypingEcho` in cv-core, kept by the speech thread next to `FocusMemory`, turns `KeyTyped` events into `Kind::Typing` utterances:
  - **Characters**: each `Char` spoken; space and punctuation by name (UK English names such as "full stop"), from a table in `wording`. Capitals are 10.2a's choice.
  - **Dead keys**: a `Dead(accent)` followed by a `Char(base)` is composed by a small table (the five common accents over vowels, c, n and y). A dead key followed by a space gives the accent alone. With no composition, both are spoken in order, which is what Windows types. The UK layout has no dead keys (AltGr gives accented letters directly, so they arrive as `Char`); UK Extended and US International do. 10.2a's tests cover both.
  - **Words** (10.3): the current word is built from `Char` strokes and spoken when a space, Enter, Tab or punctuation ends it. `Navigation`, `Break`, `Escape` and a `FocusChanged` clear it. A `CaretMoved` clears it too unless a `KeyTyped` with an `at` up to 150 ms before the caret move's `at` exists; that check waits `HOLD` after the caret move, so the stroke that caused it has had time to pass the gate. Word boundary rules are tested in cv-core.
  - **Both**: characters as above; when a word ends, the word replaces the boundary character's name (one utterance, so "space" doesn't cut off the word).
  - **Backspace**: the character it removed from the current word, if the word holds one; otherwise "backspace". **Delete**: "delete". **Enter**: "enter" in Characters and Both. Tab and Escape: nothing (focus echo speaks what Tab reaches).
  - **Break**: nothing. A Ctrl shortcut already silences speech (0014).
  - `Kind::Typing` is foreground, so each character cuts off the last (ADR 0013), which is what fast typing needs.
- Off: `KeyTyped` is never published (app's `wanted()` stops it at the hotkey thread).

### Speech off

- `--no-default-features`: no raw input, no focus thread, nothing changes.
- `tts` on and speech off (`tts_enabled` false): the hotkey thread handles every report a little more than today (it updates the key state, a few array writes per key) and reads one `AtomicBool` per key down. It translates nothing and forwards nothing. The focus thread reads nothing. The magnifier and the caret thread are untouched in every case.

### Roadmap

10.2 is split (pending this ADR):

- **10.2a** cv-core `typing`: `KeyStroke`, `TypedKey` (character-free `Debug`), `TypingGate` with an injected clock (hold, the message-time drop rule, the barrier and its forced read, `READING_MAX`, password `No` only, the no-echo list, foreground-is-ours, one `Break` per dropped run, the 64 cap, reset); `CoreEvent::KeyTyped`, `describe`, the `Tracker` arm; `tts_typing_echo: TypingEchoMode` with its default and persistence; `TypingEcho` for Characters (names table, dead-key composition, Backspace, Delete, Enter). Unit tests for each rule, including a password reading, a `CantTell` reading, Tab then a letter whose focus event is delivered after `HOLD`, a reading older than `READING_MAX`, a Terminal reading, our own foreground, and no reading. No Windows code.
- **10.2b1** cv-platform-win: first, with `CV_KEYS_LOG`, check that raw input sees keys while an elevated Notepad has the foreground. If it doesn't, stop and record it in STATUS Findings: typing echo can't work in your Notepad until UIAccess (6.3) or a new ADR, and you decide whether to go on. Then the key state (fake Shift, generic entries, Caps Lock, the resync on restore), `ToUnicodeEx` translation (bit 2 only, the foreground layout), the message-time check, `wanted()` and `on_key`, `FocusWaker::typed` with its bounded queue, the gate on the focus thread with the forced read and the foreground check, publishing `KeyTyped`, `[focus]` lines that count published and dropped strokes per window without characters, and the no-echo names confirmed. A dev check only: app logs `[keys] typed <kind>`. Nothing is spoken yet.
- **10.2b2** app and the speech thread: the `AtomicBool` kept with `tts_enabled` and `tts_typing_echo`, the speech thread's `TypingEcho`, the panel selector. Verify: Notepad (and an admin Notepad, per 10.2b1), Brave's address bar and a page's text box, and Settings search echo characters; a Brave login form and an app's password field speak nothing, including Tab from the user name straight into the password and typing at once; Terminal, an `ssh` or `runas` prompt and a remote desktop window speak nothing; AltGr+4 (€) and AltGr+E (é) on the UK layout; a dead key on UK Extended or US International; Caps Lock on and off, and Shift for a capital mid-word; Ctrl+C and Ctrl+S speak nothing; typing into the settings panel speaks nothing; Task Manager speaks nothing; speech off and `--no-default-features` as before.

10.3 (word echo) keeps its text and adds: Words and Both per this ADR, the word cleared by navigation keys, breaks, focus changes and the caret rule on stroke times, and the Ctrl+Alt+Shift+K cycle binding.

## Consequences

- Easier: typing echo can work wherever keys go, Notepad included, with no UIA handler and no new mechanism. Every rule (the gate, the list, the names, composition, words) is in tested cv-core code. A Linux backend feeds the same `KeyStroke`s from its own keyboard source. A password is never spoken where any source flags it, and with the drop rule nothing about it is kept.
- Harder: our process now turns every keystroke into a character while speech and echo are on (in memory only, never logged). Echo is at least `HOLD` (100 ms) behind the key. The first letters typed after a focus change, after Tab, Enter or a shortcut (until the barrier clears, up to `BARRIER` plus one read), and after `READING_MAX` of typing (one forced read) are not echoed, and neither is any key whose own effect moves focus: a letter that jumps a list to an item (type-ahead), or Enter that closes a dialog. Each `READING_MAX` of typing costs one focus read on the focus thread. No echo at all in terminals and remote sessions. Backspace outside the current word and Delete can't say what they removed, which ZoomText does. Letters typed into lists and games are echoed.
- Not covered: pasted text, dictation, IME composition, and possibly the on-screen keyboard (if `SendInput` keys don't reach raw input). Option 4 (UIA text events) is where those go if the tester needs them, in a new ADR, and the same on-screen check could bring echo back to terminals.
- Unknown until built: raw input with an elevated window in front (10.2b1's first check); whether `GetKeyState(VK_CAPITAL)` is right on a background thread; whether `GetMessageTime` and `dwmsEventTime` share a clock; how numpad keys arrive with Num Lock on (ToUnicodeEx ignores its toggle); whether 500 ms is enough for a slow app to raise its focus event after Tab (if not, the forced read still decides).
- Revisit: when 8.3 is raised (default mode, key groups, capitals); after 10.5 feedback (Backspace and Delete wording, echo in lists and games, terminals, `HOLD` and `BARRIER`); at 10.4, which uses `Navigation`; if the tester needs pasted or dictated text spoken (option 4); at 6.3 or 13.2 for elevated windows; whenever a password field is found that the gate passes (a row in the list, or a new rule).
