# 0013: Speech pipeline

Status: Proposed
Date: 2026-10-12

## Context

Roadmap 8.5, the second ADR of the speech track (v1.1, PRODUCT.md "Speech (v1.1)"). It decides the queue, priorities and interruption (what cuts off what), which thread talks to the engine, how speech receives events from the core, and the timeouts that keep a hung app from blocking speech. The roadmap says it supersedes ADR 0003 where they disagree. The engine is decided (ADR 0012, Accepted). The speech hotkeys and how a lone Ctrl is detected are 8.6's decision. What each kind of event says is decided by 9.5, 10.x, 11.3 and 12.1.

8.3 (how the tester uses ZoomText's speech) is on hold: the tester is unavailable and ZoomText can't be used for now. The kind table below is a starting point to adjust from tester feedback later. Nothing here waits on 8.3.

### Rules this must keep (ROADMAP "Rules for speech steps", PRODUCT.md)

- With speech switched off, the magnifier behaves exactly as before.
- No UI Automation or MSAA call runs on the render thread. A hung or slow app must not freeze the magnifier or stop speech from being silenced.
- The text that gets spoken is pure logic in cv-core with unit tests.
- Only cv-platform-win and cv-tts depend on `windows` (CLAUDE.md, ADR 0004).

### What exists

- cv-tts (crates/cv-tts/src/lib.rs, 275 lines, no tests, `tts` feature off by default) runs one loop that sleeps 250 ms, drains a private focus channel, re-reads `AppState`, calls UIA `ElementFromPoint` and then `ISpVoice::Speak` with `SPF_ASYNC | SPF_PURGEBEFORESPEAK`. Every new string cuts off the last. A UIA call that hangs in another app holds up the next `Speak` and any stop. That is conflict 2 in STATUS "Speech audit (8.1)". ADR 0012 deletes the UIA parts and adds a backend per engine with speak, stop, list voices, set voice, rate and volume, and "finished". It left the owning thread, the queue and the stop path to this ADR.
- ADR 0003 (Accepted, partly superseded) chose "one dedicated background thread owning all COM, UIA and SAPI state". ADR 0008 already moved UIA for the caret to the core caret thread (MTA, no window, no UIA handlers, `UIA_TIMEOUT_MS` 500 for connection and transaction, crates/cv-platform-win/src/caret/mod.rs:56). Where focus events read UIA is 9.3's decision.
- Core events: `CoreEvent` (`CaretMoved`, `CaretLost`, each with `at: Instant`) are published through cv-core's `EventHub` (events.rs): an unbounded `std::sync::mpsc` channel per subscriber, which never blocks the publisher (ADR 0004 fan-out rules). A subscriber can only block on `recv` or poll `try_recv`. There is no way to wake a thread that is waiting on Win32 handles.
- Wiring (crates/app/src/main.rs): the hotkey thread reports each press as a cv-core `Action` (`Toggle`, `ZoomIn`, `ZoomOut`); app applies it with `AppState::apply` and wakes the panel through `RepaintSlot`. The panel writes back only the fields it changed. With `--features tts`, main.rs spawns the TTS thread with a shutdown flag and joins it after the overlay and caret threads.

### External facts

- **SAPI.** `Speak` with `SPF_ASYNC` returns once the request is queued. Each queued call gets an incrementing stream number, and `SPF_PURGEBEFORESPEAK` with NULL text discards pending speech ([ISpVoice::Speak](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee125024(v=vs.85))). `SPEI_END_INPUT_STREAM` fires when "the input stream (text or audio) from a Speak or SpeakStream call has finished synthesizing to the output". `SPEI_WORD_BOUNDARY` gives each word's position ([SPEVENTENUM](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee431845(v=vs.85))). A voice sends no notifications until `SetInterest` names the events ([ISpEventSource::SetInterest](https://msdn.microsoft.com/en-us/library/ee450801(v=vs.85))). `SetNotifyWin32Event` gives a Win32 event handle to "wait using one of the various Win32 synchronization functions" ([SetNotifyWin32Event](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee450846(v=vs.85))), and `GetEvents` takes the queued events ([GetEvents](https://msdn.microsoft.com/en-us/library/ee450799(v=vs.85))).
- **OneCore.** `SynthesizeTextToStreamAsync` returns the whole stream for the app to play (ADR 0012). In the pinned `windows-future` 0.3.2 (Cargo.lock), `IAsyncOperation` has `join()`, which blocks until done, and `when(op)`, which calls `op` when the operation completes (src/join.rs, src/when.rs in the crate source). `IAsyncInfo.Cancel` "cancels the asynchronous operation" ([Cancel](https://learn.microsoft.com/en-us/uwp/api/windows.foundation.iasyncinfo.cancel)).
- **WASAPI** (our playback for OneCore, ADR 0012). `IAudioClient::Stop` "stops data from streaming through the client's connection with the audio engine" and freezes the stream position ([Stop](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-stop)). `Reset` on a stopped stream "flushes all pending data" ([Reset](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-reset)). With `AUDCLNT_STREAMFLAGS_EVENTCALLBACK`, `SetEventHandle` registers an event "that the system will signal each time a buffer becomes ready to be processed by the client" ([SetEventHandle](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-seteventhandle)). Microsoft's sample feeds the buffer in a loop on the client's own thread ([Rendering a Stream](https://learn.microsoft.com/en-us/windows/win32/coreaudio/rendering-a-stream)). So OneCore playback needs a thread that keeps the buffer fed. Stop takes effect at the engine, not after the queued audio drains.
- **UIA timeouts.** The connection timeout defaults to 2 s and the transaction timeout to 20 s, and a hung app can block a UIA call that long (cited in ADR 0008).
- **Priorities in another reader.** NVDA has three speech priorities: NORMAL; NEXT ("spoken after the next utterance of lower priority is complete"); and NOW ("spoken right now, interrupting low priority speech. After it is spoken, interrupted speech will resume") ([priorities.py](https://github.com/nvaccess/nvda/blob/master/source/speech/priorities.py)).

## Options

### Which thread talks to the engine

1. **Keep ADR 0003: one reader thread for UIA and speech.** Cost: a stop waits behind any UIA call in progress, up to the transaction timeout (20 s by default, 500 ms if lowered as the caret thread does). This breaks the silencing rule. It is also a second UIA client next to the caret thread, which 9.3 would have to reconcile.
2. **A speech thread that only talks to engines.** It lives in cv-tts, is MTA, makes no UIA or MSAA calls, and waits on Win32 event handles: commands, SAPI's notify event, WASAPI's buffer event and OneCore completion. The rules about what cuts off what are a pure queue in cv-core. Core events reach it through its own `EventHub` subscription, with a wake added to the hub. Cost: a small change to the hub, one Win32 wait loop, and a new shutdown path. Gains: nothing outside the thread can block a stop. The queue rules are unit-tested without Windows. It has the same shape as the magnifier: neutral logic in a tested type, with the loop in the backend (ADR 0004).
3. **Option 2 plus a separate neutral "reader" thread** that receives core events, turns them into utterances and hands them to the speech thread. Cost: one more thread and shutdown path, and a handoff on every utterance. Gains: the speech thread knows nothing about events, and the event loop needs no hub change and would be portable as is. The same portability comes from keeping the event-to-utterance logic in cv-core (option 2), so the extra thread buys little.
4. **No speech thread: call the engine from whichever thread wants to speak** (hotkey, panel, caret thread), behind a mutex. Cost: OneCore playback still needs a thread to feed WASAPI, so this saves nothing there. COM objects are used from several threads, and a stop from the hotkey thread waits on the mutex while another caller is inside a slow engine call. Rejected.

### What cuts off what

A. **Every new utterance cuts off everything** (the old plan and today's cv-tts). Cost: a pointer moving over the page cuts off a focus or dialog announcement half way through. A two-part announcement (a dialog's title, then its text) can't be queued.
B. **NVDA-style priorities with resume** (normal, next, now; interrupted speech resumes). Cost: resume needs a position inside an utterance, which SAPI and OneCore give in different ways (word boundaries). The tester has asked for none of it. Only continuous reading (12.x) obviously needs resume.
C. **Two levels and append batches, no resume.** Foreground speech (anything the user did: focus, typing, caret moves, commands, test voice) cuts off everything. Background speech (pointer echo) cuts off only other background speech and never interrupts foreground speech. A batch of strings queues in order behind its first item. Resume is left to 12.1. Cost: a pointer echo that arrives while a focus announcement is speaking is dropped, not queued. The table is a guess until the tester uses it.

## Decision

Recommendation: **option 2 with C**. It supersedes ADR 0003's threading decision ("one dedicated background thread owning all COM, UIA and SAPI state"). Speech gets its own thread, and UIA and MSAA belong to the core threads (ADR 0008, and 9.3 for focus). The rest of 0003 (MTA first, plain data over channels and never COM pointers, `CUIAutomation8`, `GetPhysicalCursorPos` for anything that asks about the pointer) is already carried by 0008 and STATUS "Speech audit (8.1)", so nothing of 0003 is left standing alone. Changing 0003's Status line is yours. This ADR extends ADR 0004's `EventHub` and contradicts no Accepted ADR.

### The speech thread (cv-tts)

- `cv_tts::spawn_speech(state, events)` starts one thread and returns a `SpeechHandle`. Any thread may call the handle, and no call blocks: each sends a plain-data request on an `mpsc` channel and signals the thread's command event (`SetEvent`).
  - `say(Utterance)` / `say_batch(Vec<Utterance>)`: speak.
  - `silence()`: stop speaking and clear the queue.
  - `wake()`: speech settings in `AppState` changed. Any thread that writes `tts_*` fields calls it, the way the panel uses `request_repaint`.
  - `shutdown()`: stop, wait up to 1 s, then leave the thread to end with the process (as `CaretHandle::shutdown` does).
- MTA COM first. The thread owns the SAPI voice, the OneCore synthesizer and the WASAPI client. No COM or WinRT object leaves it, and it makes no UIA or MSAA call. It doesn't read the pointer either; pointer and text sources are core threads.
- It waits in one Win32 wait (the step picks `WaitForMultipleObjects` or `MsgWaitForMultipleObjectsEx`) on:
  - the command event;
  - the core-event wake (below);
  - SAPI's notify handle (`SetNotifyWin32Event`, with `SetInterest` on `SPEI_END_INPUT_STREAM`);
  - the WASAPI buffer event (event-driven mode).
  It also waits with a timeout for the expiry rules below. OneCore synthesis uses `when`, never `join`. The callback only signals the thread with the operation's id, and the thread takes the result. So the thread never blocks on an engine, and a silence is handled on its next wake.
- The thread gives the engine **one utterance at a time**. The queue is ours, not SAPI's, so what cuts off what doesn't depend on the engine. The cost is one wake between queued utterances. Each utterance gets an id. A backend reports "finished" with that id (SAPI from the stream number of `SPEI_END_INPUT_STREAM`, OneCore when our playback reaches the end of the stream), and the queue ignores reports for ids it has already cut off.
- **Stop path**: SAPI `Speak(NULL, SPF_PURGEBEFORESPEAK)`. OneCore: cancel any synthesis in progress, then WASAPI `Stop` and `Reset`. Target: the audio stops within 100 ms of `silence()`. 8.8b checks it with timestamped `[tts]` log lines, the way `CV_TRACK_LOG` works.
- **Speech off.** With the `tts` feature off (the default until 13.1), none of this is built and nothing changes. With the feature on and `tts_enabled` false, the thread waits, creates no engine and opens no audio device. Requests and core events are dropped. Engines are created when speech is first turned on. Turning speech off is a silence, after which the WASAPI client is released.
- On every wake the thread re-reads the speech fields of `AppState` under a read lock (enabled, voice, rate, volume). A changed voice, rate or volume applies from the next utterance, and a change to enabled applies at once.

### The queue (cv-core, pure, unit-tested)

A new cv-core module `speech` has `Utterance { text, kind }`, `Kind` and `SpeechQueue`. The queue takes inputs (`push`, `push_batch`, `silence`, `finished(id)`, `tick(now)`) and returns what the backend must do (`Speak(id, text)`, `Stop`, nothing). It has no Windows types and no threads, so its rules are tested with made-up inputs.

| Kind | Level | Starting rule |
|---|---|---|
| `Command` (read title, repeat; 9.8) | foreground | cuts off everything |
| `Focus` (focus, window and dialog changes; 9.5, 9.6) | foreground | cuts off everything |
| `Typing` (10.x) | foreground | cuts off everything |
| `Caret` (caret moves; 10.4) | foreground | cuts off everything |
| `Test` (panel "Test voice"; 8.7d) | foreground | cuts off everything |
| `Pointer` (mouse echo; 11.x) | background | cuts off only background; dropped while foreground speech is speaking or queued; expires if not started within 1 s |
| `Reading` (continuous reading; 12.x) | background | any foreground speech, silence or new reading stops it; no resume until 12.1 decides |

- A **batch** cuts off what its first item's rule says, then the rest queue in order behind it: for example a dialog's title, then its text (9.6).
- **Silence** stops the current utterance and clears the queue, whatever is in it.
- **Late events.** After a silence, a core event whose `at` is earlier than the silence is dropped. A focus event that arrives late from a slow app can't speak after the user silenced speech.
- The queue holds at most 32 utterances, dropping the oldest. With these rules it rarely holds more than one batch.
- The numbers (1 s expiry, 32, 100 ms) are starting values. Steps may tune them with a measurement and say so in the PR. Changing a kind's rule is a change to this table, made in the step that needs it (9.5, 10.1, 11.3, 12.1). If a step contradicts the levels or adds resume, it needs a new ADR.

### How speech receives events and commands

- **Core events.** `EventHub` gets `subscribe_with_wake(wake)`, which returns a receiver as `subscribe` does and calls `wake()` after each send to it. cv-tts passes a closure that signals a second Win32 event, so the speech thread wakes on core events without polling. `wake` must not block, and a panicking or slow wake is a bug in the subscriber. Existing subscribers are unchanged, and ADR 0004's fan-out rules hold.
- On each wake the thread drains its receiver and turns each event into utterances with a pure cv-core function (`speech::utterances_for(&CoreEvent, &SpeechSettings) -> Vec<Utterance>`, tested). Today it returns nothing for every event: caret moves speak nothing until 10.4, and focus events don't exist until 9.4. Events carry the text they need (9.3 decides the fields). The speech thread never asks UIA for more.
- **Hotkeys.** Silence and speech on/off are new `Action` variants in cv-core (bindings from 8.6). app's hotkey closure sends silence to the `SpeechHandle`. Speech on/off goes through `AppState::apply` and then `wake()`. With the `tts` feature off these bindings are not registered.
- **Panel.** "Test voice" is `say(Utterance { kind: Test, … })`. A changed `tts_*` field calls `wake()` after `apply_changes`.

### Timeouts for hung apps

- The speech thread makes no UIA, MSAA or cross-process window call, so a hung app can't block it. A stop never waits for an app.
- Text for speech is read on core threads with lowered UIA timeouts: the caret thread's 500 ms today, and the same rule for whatever 9.3 adds. A read that times out produces no event, or a late one. Late events are handled by the silence rule above and by the queue's normal cut-off.
- **Engine timeouts.** A OneCore synthesis that hasn't completed after 5 s is cancelled, logged once (`[tts] synthesis timed out`) and treated as finished. SAPI `SPERR_DEVICE_BUSY` and WASAPI `AUDCLNT_E_DEVICE_INVALIDATED` drop the utterance with one log line. The next utterance opens the default device again (ADR 0012).
- **Shutdown.** app shuts the speech thread down after the overlay and caret threads, as it does the TTS thread today. It waits at most 1 s.

### Roadmap

8.8 is split (pending this ADR). 8.7d's "Test voice" speaks through the speech thread, so **8.8a and 8.8b come before 8.7d**:

- 8.8a cv-core `speech`: `Utterance`, `Kind`, `SpeechQueue` with the table above, batches, silence, expiry, the late-event rule, ids and stale "finished" reports; `utterances_for` (returns nothing yet); `EventHub::subscribe_with_wake`; new `Action` variants for silence and speech on/off. Unit tests. No Windows code.
- 8.8b cv-tts speech thread and `SpeechHandle` on the backends from 8.7b and 8.7c: the wait loop, lazy engines, finished reports, the stop path, engine timeouts, shutdown; app wiring behind `tts` (spawn, hub subscription, silence and on/off from the 8.6 bindings). Verify: a long sentence stops within about 100 ms of silence (log timestamps); speech on/off; the magnifier stays smooth while speaking; a run with speech off and one with the feature off changes nothing.

## Consequences

- Easier: silence can't be held up by any app. What cuts off what is one tested table in cv-core, and the later steps (9.5, 10.x, 11.3, 12.x) change rows, not threads. Engines can be swapped without touching the rules. A Linux backend later writes its own wait loop around the same queue.
- Harder: one more thread with COM, a Win32 wait over several handles and a shutdown path. Utterances go one at a time, so there is one wake between them. The hub gains a wake hook that subscribers must keep cheap. Two items (8.8a, 8.8b) move ahead of 8.7d.
- Unknown until built: the real stop latency of each engine (SAPI's purge and WASAPI's stop are documented, not measured here). Whether the gap between queued utterances can be heard. Whether pointer echo dropped behind foreground speech feels wrong to the tester.
- Revisit: when the tester uses speech (the kind table, the expiry); at 8.6 if the silence key must be detected somewhere other than the hotkey thread; at 9.3 (which fields events carry, and whether focus has its own thread); at 12.1 (resume, and splitting long text); if the 100 ms target is missed.
