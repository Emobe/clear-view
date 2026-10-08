# ROADMAP

Numbered items. Commands take the number: `/step 1.1`, `/adr 3.1`. Status lives in docs/STATUS.md, decisions in docs/adr/, reader stage detail in tts-plan.md. Each phase ends in a state that builds, runs and does not regress the one before. Items marked (ADR first) need an Accepted ADR before any code.

## How work proceeds

`/clear`, `/next`, then the command it names (`/adr` or `/step`). A step that needs an ADR stops itself and records why in docs/NEXT.md. You accept ADRs and merge branches; the agent does neither.

## Phase 0: Audit

0.1 Run `/audit`, then work through the "Needs your run" list in docs/STATUS.md by hand and record the results.

## Phase 1: Make the docs true

1.1 Move PLAN.md, PHASE2PLAN.md, handoff.md and shaders/magnify.hlsl to docs/archive/ (confirm magnify.hlsl is unused first).

## Phase 2: Foundation

2.1 Move pure logic out of the render loop into cv-core with unit tests: lerp, crop and zoom math, cursor-to-output mapping.
2.2 Settings persistence (JSON in the user config dir) for all AppState fields.
2.3 Per-mode TTS toggles from tts-plan.md (selection, caret, typing, AppReader, alongside hover).
2.4 Find the real cause of the unfocused-window TTS bug. Remove the 100 ms repaint thread in app.rs or justify it.
2.5 Stop taking the AppState write lock every frame in the egui panel: snapshot, edit a local copy, write back on change.
2.6 Make cargo clippy and cargo test the gate and record it in CLAUDE.md.

## Phase 3: Platform seams (Windows only)

3.1 (ADR first) ADR 0004 with Opus. Its output replaces 3.2 with numbered sub-items.
3.2 Refactor behind the traits ADR 0004 defines: capture source, window host, hotkeys, cursor source. Win32 moves into a Windows backend. Behaviour must not change.
3.3 Reader interfaces: AccessibilityBackend (UIA now; IA2 and AT-SPI2 later) and SpeechEngine (SAPI now; Speech Dispatcher later). Interfaces only.

## Phase 4: Windows v1

4.1 Fix the broken items from the audit (add sub-items as needed).
4.2 Zoom in and out hotkeys.
4.3 Multi-monitor behaviour decided and fixed.
4.4 Performance measured at your native resolution and at 4K; fix if unacceptable.
4.5 Reader stage 5: selection reading.
4.6 Reader stage 6: caret following.
4.7 Reader stage 7: typing echo.
4.8 UIAccess manifest, signing and installer.
4.9 Test on a clean Windows install.

Post-v1 unless you say otherwise: reader stage 8 (AppReader) and stage 9 (IA2).

## Phase 5: Linux

5.1 (ADR first) X11 backend.
5.2 X11 implementation.
5.3 (ADR first) Wayland strategy: portal ScreenCast capture as the common path, then per-compositor docking and hotkey routes. Prototype early whether a live magnifier can get the pointer position while its overlay passes input through.
5.4 Wayland implementation (split per compositor by the ADR).
5.5 (ADR first) Reader on Linux: AT-SPI2 and Speech Dispatcher.
5.6 Reader on Linux implementation.

## Open decisions

- Magnifier v1 before reader stages, or interleaved? The order above is magnifier first.
- Dependency upgrades (wgpu, eframe) before or after Phase 3. ADR 0004 covers this.
- Whether AppReader belongs in v1.
