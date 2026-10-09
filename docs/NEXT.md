# NEXT

The handoff between sessions. Written by /audit, /step and /adr. Read by /next. Context is cleared between commands, so anything the next session needs must be here or in docs/STATUS.md. Keep it short.

State: blocked-on-adr
Item: 1.5 Readability at 10x and above
Branch: none. No code written.
Decision needed: which upscaling method (or methods) the shader uses so text is readable at 10x to 20x (STATUS Finding 14), and which is the default.
ADR: docs/adr/0006-upscaling-at-high-zoom.md (Proposed). Recommends adding sharp bilinear as a third mode in 1.5, plus a naga 22.1 dev-dependency test that validates shader.wgsl; edge smoothing (cleanEdge) becomes new item 1.9. /step 1.5 resumes after you set ADR 0006 to Accepted. If you change the decision, update the 1.5 and 1.9 text in ROADMAP.md to match.

Not an option in code: the native Windows Magnifier and the Magnification API (`MagSetImageScalingCallback`, Mag window smoothing and the like). Your decision: it is broken and we don't build on it. Researching how it looks and where it falls short is fine, as a comparison.

Options seen in /step 1.5 Phase A (not researched yet; the ADR should research them, including what ZoomText, SuperNova and Windows Magnifier actually do):
- Keep bilinear and Catmull-Rom bicubic, pick the better as default. Both are linear filters: at 20x each edge becomes a ramp about 20 output px wide, so neither removes the blur.
- Sharp bilinear: each source pixel drawn as a block, edges blended over about 1 output px, scale from `fwidth` in the shader. One pass, no new uniform or AppState field. Crisp but blocky.
- Sharper linear filters (Lanczos, unsharp mask after bicubic). Limited at 20x for the same reason as above.
- Edge-directed upscalers (xBR / Super-xBR, ScaleFX style). Rounded, smooth edges; multi-pass with intermediate textures, so a change to the render pipeline's structure. Cost at 4K unmeasured.
- Text re-rendering (ZoomText xFont style). Probably out of reach; the ADR should say what it needs and why it is or isn't in v1.

Where to resume:
- Interpolation lives in crates/cv-render/src/shader.wgsl (`interp_mode`: 0 bilinear, 1 Catmull-Rom via `textureLoad`), sampler in gfx.rs (Linear, ClampToEdge), uniforms are 32 bytes written in gfx.rs `write_uniforms`.
- `Interpolation` enum in cv-core/src/lib.rs, default Bilinear; panel radio buttons in app/src/app.rs; `interpolation` is already in `apply_changes`.
- Docs already checked in Phase A: WGSL spec (`textureSampleLevel` needs no uniform control flow; `fwidth` is a derivative builtin), wgpu 22.1.0 `PresentMode::Fifo` (`get_current_texture` blocks when the queue is full), Microsoft SetTimer (10 ms minimum, no accuracy promise).
- Jitter at high zoom is part of 1.5 but not of this ADR unless research says otherwise. Candidates from the code, unconfirmed: the 16 ms `SetTimer` (cv-render/src/lib.rs:292) against 60 Hz vsync, `dt` measured at timer time not present time, about 60 fps cap on faster monitors.
- The gate does not validate WGSL (wgpu validates at runtime). A naga 22.1.0 dev-dependency test would; it is a new direct dependency, so the ADR may decide it.
- After the ADR is Accepted, /step 1.5 builds the chosen option(s); you then compare at 10x and 20x and pick the default.

Still true:
- `cargo build` and `cargo clippy --workspace --all-targets` are at 0 warnings, also with `--features app/tts`. Keep them there. Docs-only steps skip the gate.
- The panel repaints only on input or when another thread calls `request_repaint()` through `app::RepaintSlot`. The panel writes back only fields it changed (app.rs `apply_changes`); a new `AppState` field the panel edits must be added to that macro list.
- Finding 12 (mouse cannot reach the taskbar) is docked-only and goes with roadmap 3.5. Finding 13 is ignored by your decision.
- Work in roadmap order. ADR 0004 (2.1) is not due until Phase 2.
- Old 2.3 (TTS toggles) is dropped; its patch is in docs/archive/. Do not bring it back.
