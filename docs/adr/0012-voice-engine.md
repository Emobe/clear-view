# 0012: Voice engine

Status: Proposed
Date: 2026-10-12

## Context

Roadmap 8.4, the first ADR of the speech track (v1.1, PRODUCT.md "Speech (v1.1)"). It decides which Windows speech engine speaks, how voices are listed and chosen, the rate, volume and pitch ranges, what happens with no voices or a removed voice, the audio output device, whether the Windows natural (Narrator) voices can be used, and whether cv-tts is kept, rewritten or replaced. The speech pipeline (queue, interruption, which thread talks to the engine) is 8.5, not this ADR.

Not yet known: how the tester uses ZoomText's speech (8.3, yours, not written yet). Which voice and rate she uses decide the defaults, not the engine. If she relies on a ZoomText voice that is not registered with Windows, no option below gives her that voice (see Consequences).

### What exists

- cv-tts (crates/cv-tts/src/lib.rs, 275 lines, no tests) is behind the `tts` feature and off by default. Its engine part is SAPI 5: `CoCreateInstance(SpVoice)` on an MTA thread, `Speak` with `SPF_ASYNC | SPF_PURGEBEFORESPEAK`, `SetVolume` and `SetRate` only on change. The same loop holds UIA (`CUIAutomation8`, a focus handler, hover through `ElementFromPoint`) and speaks "clear-view ready" even with speech off. The 8.1 audit (STATUS "Speech audit (8.1)") lists ten conflicts with later decisions; the reusable parts are the SAPI call shapes.
- Settings: `tts_volume` (0 to 100) and `tts_rate` (-10 to 10) in cv-core `AppState`, both on the SAPI scale. No voice setting exists.
- cv-tts's `windows` features are `Win32_System_Com`, `Win32_Media_Speech` and `Win32_UI_Accessibility`. Only cv-platform-win and cv-tts may depend on `windows` (CLAUDE.md, ADR 0004).
- ADR 0003 (Accepted) chose SAPI as the engine and one thread owning COM, UIA and SAPI. ADR 0008 already took UIA for the caret onto the caret thread. ADR 0004 left the reader's `SpeechEngine` interface undecided.

### Voices on this machine (your dev PC, Windows 11 build 26200, read from the registry 2026-10-12)

- SAPI 5 (`HKLM\SOFTWARE\Microsoft\Speech\Voices\Tokens`): 5 voices. Hazel (en-GB), David and Zira (en-US), Haruka (ja-JP), Paulina (pl-PL).
- OneCore (`HKLM\SOFTWARE\Microsoft\Speech_OneCore\Voices\Tokens`): 12 voices. George, Hazel and Susan (en-GB), David, Mark and Zira (en-US), four ja-JP, two pl-PL. So for UK English, SAPI offers one voice and OneCore three.
- SAPI audio output token enumerators: `MMAudioOut` only.
- No natural voice package is installed (`Get-AppxPackage *Voice*` finds only `MicrosoftWindows.VoiceSignatureGeneratorPack`).

### SAPI 5 (Win32 `ISpVoice`)

- Docs are archived SAPI 5.4 pages; no current page was found (8.1 audit).
- `SetRate`: "Supported values range from -10 to 10"; "Voices do not all have the same default rate"; "The granularity of the rate is engine dependent" ([ISpVoice::SetRate](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee125019(v=vs.85))). `SetVolume` is 0 to 100 ([ISpVoice::SetVolume](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee125022(v=vs.85))). No pitch call; pitch is only through SAPI XML in the spoken text.
- `Speak` with `SPF_PURGEBEFORESPEAK` and NULL text discards pending speech ([ISpVoice::Speak](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee125024(v=vs.85))). SAPI plays the audio itself.
- `SetOutput` takes a stream, an audio device or an audio output token; "If pUnkOutput is NULL the default audio device will be used" ([ISpVoice::SetOutput](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee125017(v=vs.85))). `SetVoice` takes a voice token from the `SPCAT_VOICES` category ([ISpVoice::SetVoice](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee125021(v=vs.85)); [Object tokens and registry settings](https://msdn.microsoft.com/en-us/library/ms717036.aspx)).
- Any engine registered as a SAPI 5 voice works, including third-party ones.

### OneCore (WinRT `Windows.Media.SpeechSynthesis`)

- `SpeechSynthesizer` exists since Windows 10 10240. `AllVoices` lists installed voices, `DefaultVoice` is the system voice, `Voice` sets one. "Only Microsoft-signed voices installed on the system can be used to generate speech." `SynthesizeTextToStreamAsync` and `SynthesizeSsmlToStreamAsync` return an audio stream; the app plays it ([SpeechSynthesizer](https://learn.microsoft.com/en-us/uwp/api/windows.media.speechsynthesis.speechsynthesizer)). The call completes with the whole stream, so a long string is fully synthesised before any of it plays.
- `SpeechSynthesizerOptions` (since 1709, 16299) ([options](https://learn.microsoft.com/en-us/uwp/api/windows.media.speechsynthesis.speechsynthesizeroptions)):
  - `SpeakingRate`: "from 0.5 (half the default rate) to 6.0 (6x the default rate) … The default value is 1.0"; "Some voices have minimum speaking rates faster than 0.5 and maximum speaking rates slower than 6.0" ([SpeakingRate](https://learn.microsoft.com/en-us/uwp/api/windows.media.speechsynthesis.speechsynthesizeroptions.speakingrate)).
  - `AudioVolume`: 0.0 to 1.0, default 1.0 ([AudioVolume](https://learn.microsoft.com/en-us/uwp/api/windows.media.speechsynthesis.speechsynthesizeroptions.audiovolume)).
  - `AudioPitch`: 0.0 to 2.0, default 1.0, some voices narrower ([AudioPitch](https://learn.microsoft.com/en-us/uwp/api/windows.media.speechsynthesis.speechsynthesizeroptions.audiopitch)).
  - About 750 ms of silence is appended after each utterance by default; `AppendedSilence` and `PunctuationSilence` (since 1803) shorten it, and the docs name screen-reader descriptions as a case for that ([SpeakingRate remarks](https://learn.microsoft.com/en-us/uwp/api/windows.media.speechsynthesis.speechsynthesizeroptions.speakingrate), [AppendedSilence](https://learn.microsoft.com/en-us/uwp/api/windows.media.speechsynthesis.speechsynthesizeroptions.appendedsilence)).
  - `IncludeWordBoundaryMetadata` adds word timings to the stream, which 12.4 and 12.5 (follow and highlight the spoken word) need. SAPI has word-boundary events for the same job.
- The `windows` 0.62.2 crate has the bindings: features `Media_SpeechSynthesis`, `Storage_Streams`, `Foundation`; `SynthesizeTextToStreamAsync` returns `windows_future::IAsyncOperation<SpeechSynthesisStream>` and `AllVoices` a `windows_collections::IVectorView` (read from the crate source in the local registry). Playing the stream ourselves needs an audio API too, for example WASAPI (`Win32_Media_Audio`).
- NVDA's OneCore driver works this way from an unpackaged desktop app: a native helper calls `SpeechSynthesizer`, NVDA plays the audio with its own `WavePlayer` on the output device from its own settings, cancel stops feeding audio, and rate maps a percentage onto 0.5 to 1.5 (to 6.0 with "rate boost"). With no voices it raises "No voices available" ([oneCore.py](https://github.com/nvaccess/nvda/blob/master/source/synthDrivers/oneCore.py)).

### Natural (Narrator) voices

- Windows 11 22H2 added natural voices (Aria, Jenny, Guy, more locales later) for Narrator only. "As of 2024, no other third-party applications are able to use these voices in any way, shape or form" ([Wikipedia, Microsoft text-to-speech voices](https://en.wikipedia.org/wiki/Microsoft_text-to-speech_voices)). No Microsoft page found says otherwise.
- NaturalVoiceSAPIAdapter (MIT) wraps them as a SAPI 5 engine. Its README: "Microsoft hasn't yet allowed third-party apps to use the Narrator/Edge voices, and this can stop working at any time, for example, after a system update", and "This engine uses some encryption keys extracted from system files to use the voices, so it's more like a hack than a proper solution" ([NaturalVoiceSAPIAdapter](https://github.com/gexgd0419/NaturalVoiceSAPIAdapter)). Its other sources are Edge's online voices and Azure with a subscription key.

## Options

1. **SAPI 5 only.** Keep the engine part of cv-tts. Costs: least new code; the old call shapes and the `tts_rate` and `tts_volume` meanings stay. Fewer and older voices (5 against 12 here; one UK voice against three). Documentation is archived. Third-party SAPI voices work, including NaturalVoiceSAPIAdapter if a user installs it themselves. No pitch setting without XML in the text.
2. **OneCore only.** Replace the engine with `SpeechSynthesizer`. Costs: we write audio playback (WASAPI), stop and device handling ourselves; whole-utterance synthesis before playback adds latency on long text; only Microsoft-signed voices, so never a third-party voice; `tts_rate` changes meaning. Gains: more voices, current docs, rate, volume and pitch on documented ranges, word timings.
3. **Both, behind one engine interface, OneCore first.** One voice list with each voice tagged by engine; the default is OneCore's `DefaultVoice`, falling back to SAPI. Costs: everything in option 2, plus a SAPI backend (mostly the existing call shapes), a neutral rate and volume scale mapped to each engine, and two backends to verify. 8.7 grows into several steps.
4. **Natural voices through NaturalVoiceSAPIAdapter, bundled or required.** Costs: relies on extracted encryption keys, can break on any Windows update, Microsoft has not allowed it; not something a free public release can ship or depend on. The online sources need a network or an Azure key.

## Decision

Recommendation: **option 3**, built in the order below, with these rules.

### Engines

- **OneCore is the primary engine**, SAPI 5 the second. The voice list shows OneCore voices first, then SAPI voices, each labelled with its engine ("Hazel (OneCore)", "Hazel (SAPI)"), because the same names appear in both.
- **Natural voices are not used** (option 4 rejected). We do not bundle, install or require the adapter. If a user installs it, its voices appear in the SAPI list like any SAPI voice, unsupported. Revisit if `AllVoices` ever lists natural voices or Microsoft documents access for apps.

### Voices

- A voice is saved as its engine plus the engine's own id (the SAPI token id, the OneCore `VoiceInformation.Id`), not its display name.
- **No saved voice** (first run): OneCore `DefaultVoice`; if OneCore has none, the first SAPI voice in the Windows display language, else the first SAPI voice.
- **Saved voice missing** (uninstalled, or the other engine is gone): speak with the fallback above for this run, log one `[tts]` line, and show in the panel that the saved voice is missing. The saved setting is not overwritten until the user picks another voice, so a reinstalled voice comes back.
- **No voices at all**: speech cannot be turned on; the panel says so and names Settings > Time & language > Speech. Nothing else changes; the magnifier is unaffected.
- The list is read at start and when the panel's voice list is opened, not polled.

### Rate, volume, pitch

- **Rate** is one neutral setting, 0 to 100, with 50 the voice's normal rate. Each backend maps it onto its own range: SAPI -10 to 10 linearly; OneCore 0.5 to 1.0 below 50 and 1.0 to a maximum above. The maximum starts at 3.0 (OneCore allows up to 6.0, and NVDA caps at 1.5 without "rate boost"); 8.3 and the tester's ear decide it. The number means "position on this voice's range", not words per minute, which neither engine documents.
- **Volume** is 0 to 100: SAPI as is, OneCore divided by 100.
- **Pitch**: not a setting in 8.7. OneCore has `AudioPitch`; SAPI only has XML in the text. It is added later if 8.3 or the tester asks for it.
- OneCore's appended and punctuation silence are set to their shortest values for echo. Reading (12.1) may decide otherwise.
- The mapping functions and the voice-choice rules above are pure logic in cv-core with unit tests.
- `tts_rate` changes meaning. 8.9 migrates an old file: new = (old + 10) × 5, so 0 becomes 50.

### Audio output

- v1.1 speaks on the Windows default output device. No device setting until the tester asks for one.
- SAPI: `SetOutput(NULL)`. OneCore: we play the stream ourselves with WASAPI in shared mode on the default render endpoint. Stopping stops the audio client at once, not at the end of a buffer queue; the backend reports when playback ends, which continuous reading (12.2) needs.
- When the default device changes or disappears, the next utterance opens the new default. Whether SAPI follows a change while running is not documented; 8.7 checks it by hand.

### cv-tts

**Rewritten, not replaced.** The crate name, the `tts` feature (off by default until 13.1) and the rule that only it and cv-platform-win may depend on `windows` stay. Its contents change:

- Kept: the SAPI call shapes (MTA first, `SpVoice`, `Speak` async with purge, `SetVolume` and `SetRate` on change).
- Deleted: the UIA bootstrap, `FocusHandler`, `TtsMode`, the hover loop and the startup phrase. Focus and hover are the core's job (9.3, 11.3), and the startup phrase breaks the speech-off rule.
- Added: a backend per engine behind one interface: list voices, set voice, rate and volume, speak a string, stop, and report "finished". Which thread owns the backends, the queue and how stop reaches the engine while UIA hangs are 8.5's decision.
- New `windows` features, on cv-tts only: `Media_SpeechSynthesis`, `Storage_Streams`, `Foundation`, `Win32_Media_Audio`, plus whatever the async `get` needs. The interface is kept free of Windows types, so a Linux backend later (Speech Dispatcher) implements the same calls.

### Supersedes

This supersedes ADR 0003's engine choice (SAPI as the only engine) and its "Speech is always SPF_ASYNC with purge-before-speak" rule as a rule for all speech; that rule now applies to the SAPI backend only. 0003's threading (one thread owning COM, UIA and SAPI) is left to 8.5. Changing 0003's Status line is yours.

## Consequences

- Easier: three UK voices instead of one on a stock Windows 11; documented rate, volume and pitch ranges; word timings for following and highlighting the spoken word (12.4, 12.5); a third-party or adapter voice still works through SAPI for anyone who installs one.
- Harder: two backends to build and verify, and our own audio playback for OneCore. Whole-utterance synthesis means long text must be split before it is spoken (12.x); short echo strings are not affected. The neutral rate scale is approximate across engines and voices.
- If 8.3 finds the tester relies on a ZoomText voice that is not registered with Windows (ZoomText ships its own voices), neither engine offers it. Then the choice is a SAPI version of that voice, a Windows voice she accepts, or a new ADR for another engine.
- Revisit: when 8.3 is written (defaults, the rate maximum, pitch); when 8.5 decides the thread and stop path; if Microsoft opens the natural voices to apps; if the tester needs a device setting.

### Roadmap

8.7 is split (pending this ADR):

- 8.7a cv-core: the neutral rate and volume mapping for each engine and the voice-choice rules (no saved voice, missing voice, no voices), with unit tests. No Windows code.
- 8.7b cv-tts rewrite with the SAPI backend: speak, stop, list voices, set voice, rate and volume; the UIA parts and the startup phrase deleted. Run only through a dev example or test, not wired to the panel.
- 8.7c OneCore backend with WASAPI playback, and one voice list across both engines. Same dev check.
- 8.7d Panel controls with the `tts` feature on: speech on/off, voice (missing and no-voices states), rate, volume, "Test voice", which speaks and can be stopped. Verify: with the feature off, nothing changes.
