# 0003: Single TTS thread with UIA and SAPI

Status: Accepted
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
