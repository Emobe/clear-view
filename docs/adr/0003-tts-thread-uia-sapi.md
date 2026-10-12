# 0003: Single TTS thread with UIA and SAPI

Status: Accepted; partly superseded: by 0008 (only the TTS thread adds or removes UIA handlers) and by 0012 (SAPI as the only engine, and "always SPF_ASYNC with purge-before-speak" for all speech, which now applies to the SAPI backend only), and by 0013 (the threading rule: one thread owning COM, UIA and SAPI; speech now has its own thread that makes no UIA or MSAA call). Nothing of this ADR stands alone any more: what carries over (MTA first, plain data over channels, `CUIAutomation8`) is in 0008 and 0013.
Date: 2026-10-08 (recorded retroactively from tts-plan.md)

## Context
ZoomText-style reading needs element and text information from the OS and a speech engine.

## Options
1. UIA calls from the render or egui thread. Rejected: UIA threads must not own windows.
2. One dedicated background thread owning all COM, UIA and SAPI state.
3. OCR of the captured frame. Reserved as a future backend, not built.

## Decision
Option 2. MTA COM initialised first on the thread. Only that thread adds or removes UIA handlers. Handlers send plain data over a channel, never COM pointers. Speech is always SPF_ASYNC with purge-before-speak. GetPhysicalCursorPos for hover. CUIAutomation8, not the unnumbered class.

## Consequences
Matches the dead-ends list in tts-plan.md. UIA and SAPI are Windows-only, so Linux needs an AT-SPI2 backend and a Speech Dispatcher engine behind the AccessibilityBackend and SpeechEngine interfaces planned in tts-plan.md. IA2 for Firefox and LibreOffice stays an additive branch.
