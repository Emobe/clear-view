# ROADMAP

Order of work from the current state. Status lives in docs/STATUS.md, decisions in docs/adr/, per-stage reader detail in tts-plan.md. Each phase ends in a state that builds, runs and does not regress the one before.

## How work proceeds

`/clear`, `/next`, then the command it names (`/adr` or `/step`). A step that needs an ADR stops itself and records why in docs/NEXT.md. You accept ADRs and merge branches; the agent does neither.

## Phase 0: Audit (no code changes)

Run `/audit` on the Windows machine. Output: a real docs/STATUS.md replacing the static draft. Delete branches that are dead ends.

## Phase 1: Make the docs true

- Replace CLAUDE.md with the revised one
- Delete or move to docs/archive/: PLAN.md, PHASE2PLAN.md, handoff.md, shaders/magnify.hlsl
- Add checkboxes to the stage headings in tts-plan.md and tick them from STATUS.md

## Phase 2: Foundation

Small, boring, and it pays off for every later phase.
- Move pure logic out of the render loop into cv-core with unit tests: lerp, crop and zoom math, cursor-to-output mapping. Most of the earlier cursor bugs lived here.
- Settings persistence (JSON in the user config dir) covering all AppState fields
- Per-mode TTS toggles from tts-plan.md
- Deal with findings 2 and 3 from STATUS.md
- `cargo clippy` and `cargo test` as the gate in `/step`

## Phase 3: Platform seams (still Windows only)

1. Write ADR 0004 with Opus from STATUS.md plus the ADR 0004 brief. Output: the trait boundaries and crate layout.
2. Refactor behind those traits with Windows as the only implementation: capture source, window host (fullscreen, docked, monitor, cursor clip), global hotkeys, cursor source. cv-render keeps wgpu and the shader; Win32 moves into a Windows backend crate.
3. Reader side: the planned AccessibilityBackend gets an AT-SPI2 slot, SpeechEngine gets a Speech Dispatcher slot. Interfaces only.
4. Done when the app behaves exactly as before. No feature work in this phase.

## Phase 4: Windows v1

Definition of done:
- Magnifier: audit gaps closed; zoom hotkeys; multi-monitor behaviour decided; 4K performance acceptable; settings persist
- Reader: Stage 5 (selection), 6 (caret following), 7 (typing echo) with per-mode toggles. AppReader (8) and IA2 (9) are post-v1 unless you say otherwise.
- UIAccess manifest, signing and an installer, so elevated windows and UAC prompts are magnified
- Tested on a clean Windows install

Order inside the phase: magnifier gaps first (small), then reader stages one `/step` at a time.

## Phase 5: Linux

One ADR per backend, X11 before Wayland.
- X11: capture, strut-based docking, key grabs
- Wayland: portal ScreenCast capture as the common path, then per-compositor docking and hotkey routes. Prototype early whether a live magnifier can get the pointer position while its overlay passes input through.
- Reader: AT-SPI2 plus Speech Dispatcher

## Open decisions

- Magnifier v1 before reader stages, or interleaved? Default above is magnifier first.
- Dependency upgrades (wgpu, eframe) before or after Phase 3
- Whether AppReader belongs in v1
