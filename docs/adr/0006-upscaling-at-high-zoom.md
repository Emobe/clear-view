# 0006: Upscaling filter for readable text at 10x to 20x

Status: Proposed
Date: 2026-10-09

## Context

Roadmap 1.5. You judged 20x "quite blurry but not terrible" and asked for smoothing or sharpening (STATUS Finding 14). PRODUCT principle 1: high zoom is the normal case, and 10x must be smooth and readable. The tester usually runs ZoomText at 10x.

### What the code does today

- The overlay draws one fullscreen quad in a single render pass (cv-render/src/gfx.rs `render`). The fragment shader (cv-render/src/shader.wgsl `fs`) maps the panel UV into the crop rectangle and samples the whole captured frame texture.
- `interp_mode` 0 is bilinear: `textureSample` with a Linear, ClampToEdge sampler (gfx.rs). `interp_mode` 1 is Catmull-Rom bicubic: 16 `textureLoad` reads per output pixel.
- Uniforms are 32 bytes (crop 16, colour mode 4, interp mode 4, cursor 8), written by gfx.rs `write_uniforms` only when something changed. The output window size is not a uniform.
- `Interpolation` (cv-core/src/lib.rs) has `Bilinear` (default) and `Bicubic`. Panel radio buttons are in app/src/app.rs; `interpolation` is already in `apply_changes`. It is persisted in settings.json via serde, so a new variant is a new string value; old files still load.
- The crop origin is fractional: smooth follow lerps the view centre (`geometry::lerp_toward`) and `compute_crop` keeps the sub-pixel position. So the source grid moves by fractions of a texel every frame, and any filter must stay stable under sub-texel motion or text shimmers while following.
- The captured frame is the full monitor (DXGI Desktop Duplication, ADR 0001). At zoom z the crop is `output / z` source pixels: at 10x on a 3840x2160 output it is 384x216 texels, at 20x 192x108. Any per-texel pre-pass on the crop gets cheaper as zoom goes up.
- The gate does not check WGSL; wgpu validates the shader when the pipeline is created at runtime. naga 22.1.0 with `wgsl-in` is already compiled as part of wgpu 22.1.0's default `wgsl` feature (`cargo tree -i naga -e features`). `naga::front::wgsl::parse_str` and `naga::valid::Validator::new(flags, caps).validate(&module)` exist in that version ([parse_str](https://docs.rs/naga/22.1.0/naga/front/wgsl/fn.parse_str.html), [Validator](https://docs.rs/naga/22.1.0/naga/valid/struct.Validator.html), [features](https://docs.rs/crate/naga/22.1.0/features)).

### Why bilinear and bicubic stay blurry at 20x

Both are linear filters with a support of 2 or 4 source texels. A hard glyph edge in the source becomes a ramp about 1 texel wide in source space, which is about z output pixels wide on screen: about 20 px at 20x. Catmull-Rom shortens the ramp a little and adds a small overshoot; it does not remove it. Lanczos and unsharp masking have the same problem: they sharpen at the scale of the source texel, not the output pixel.

### What other magnifiers do

- **Windows Magnifier**: one switch, "Smooth edges of images and text", described by Microsoft as smooth ("soft and less blocky") or sharp ("clear but more defined"); registry value `UseBitmapSmoothing` ([Microsoft support](https://support.microsoft.com/help/11542/windows-use-magnifier), [ElevenForum](https://www.elevenforum.com/t/enable-or-disable-smooth-edges-of-images-and-text-with-magnifier-in-windows-11.44117/latest)). That is the same trade-off as bilinear against nearest-neighbour. Comparison only; we do not build on the Magnification API (your decision, NEXT.md).
- **ZoomText / Fusion**: two technologies. *Geometric smoothing* is "a medium quality smoothing algorithm" that works in most applications with low overhead, but "some artefacts may still be visible, especially at higher magnification powers". *xFont* is high quality but "has to be supported by the application": currently Microsoft 365 build 2307 and later. Vispero's distributor states that "at present (August 2026) all major browsers support Geometric font smoothing only", and that smoothing "momentarily disappears during text scrolling" and returns once scrolling stops ([expectations](https://sastltd.zohodesk.eu/portal/en/kb/articles/zoomtext-font-smoothing-expectations), [roadmap](https://sastltd.zohodesk.eu/portal/en/kb/articles/zoomtext-font-smoothing-problems-and-development-roadmap), [scrolling](https://sastltd.zohodesk.eu/portal/en/kb/articles/zoomtext-font-smoothing-enhancements-disappear-during-scrolling)). So in Chrome, Edge and most email apps the tester today sees an image-based edge smoother, not re-rendered text.
- **SuperNova**: *True Fonts* re-render text, listed for Microsoft 365, Office 2013 to 2021, the Windows desktop, File Explorer and SuperNova's own panel ([Dolphin, SuperNova 22](https://www.yourdolphin.com/news?id=529), [Office True Fonts](https://kb.yourdolphin.com/knowledge/office-truefonts)). Other apps get bitmap smoothing.

How xFont and True Fonts get the text is not documented publicly. Both are limited to a list of applications, and ZoomText says it works "with partners" to add browsers, so it needs per-application cooperation or interception of the app's text drawing.

### ClearType

ClearType draws text with the red, green and blue sub-pixels set separately ([Microsoft ClearType overview](https://learn.microsoft.com/en-us/typography/cleartype/)). Magnified, those pixels show as coloured fringes on glyph edges. Every option below magnifies them; edge-directed filters that compare colours may treat a fringe as a separate shape. Whether the tester's apps render with ClearType is not known.

### Candidate filters researched

From the libretro slang-shaders collection (read with `gh api`; licences from the file headers):

- **Sharp bilinear**: each texel is drawn as a flat block, and the edge between two blocks is blended over about 1 output pixel. The texel-to-output-pixel ratio comes from `fwidth` of the texel coordinate or from the zoom factor. It uses one hardware bilinear sample per pixel. Sub-texel motion moves the 1 px blend, so it stays smooth while following, where nearest-neighbour would shimmer. WGSL derivatives such as `fwidth` must be in uniform control flow; branching on a uniform-buffer value keeps it uniform (WGSL spec, checked in /step 1.5 Phase A).
- **cleanEdge** (torcado, MIT, `edge-smoothing/cleanEdge`): a single-pass edge-directed filter at any scale, made for pixel art. It has a colour similarity threshold, and its comment warns that a higher threshold "creates some artifacting". It is untested on anti-aliased text.
- **Super-xBR** (Hyllian, MIT, `edge-smoothing/xbr/super-xbr.slangp`): 6 passes (luma, three xBR passes that double the size, a Jinc2 resample to the viewport, deblur). It needs fixed-ratio intermediate render targets.
- **ScaleFX** (Sp00kyFox, MIT): 5 passes plus a reference pass, fixed 3x output, and "will only consist of colours present in the original". The remaining 3x to 20x step would still need a separate filter.

## Options

1. **Keep bilinear and Catmull-Rom; you pick the default.** Cost: almost nothing; only the default may change. It does not answer Finding 14: at 20x both stay blurry, about 20 px edge ramps.

2. **Add sharp bilinear as a third mode.** Cost: about 15 lines of WGSL in `fs`, a `Sharp` variant in `Interpolation` with `as_u32` 2, a third radio button, and a serde test. No new uniform, pipeline change or dependency. Glyph edges are crisp at any zoom, but curves and diagonals become staircases of z x z blocks, the "blocky" end of Windows Magnifier's switch. ClearType fringes become solid coloured blocks. Cost per pixel is lower than bicubic.

3. **Add a single-pass edge-directed mode (port cleanEdge).** Cost: port the 384-line GLSL include to WGSL, a fourth mode, MIT attribution, and more texel reads per output pixel than bicubic, so it needs measuring at 4K. It keeps the current single-pass pipeline. Rounded edges, closest to ZoomText's geometric smoothing in idea. Risk: it was made for flat-colour pixel art; on anti-aliased or ClearType text the similarity threshold may give artefacts or no gain. This is unknown until it is tried.

4. **Multi-pass edge-directed pipeline (Super-xBR 2x or 4x on the crop, then a final resample).** Cost: restructure gfx.rs into several render passes with intermediate textures sized to the integer-aligned crop plus margin, recreated when zoom or monitor changes; the final pass takes the fractional offset; port 4 to 6 shaders; MIT attribution. Best published edge quality of the candidates. Work per frame scales with the crop, which is small at high zoom (about 1536x864 at 4x pre-scale for 10x on a 4K output). This is the largest change and touches the render structure that ADR 0004 (2.1) will draw boundaries around.

Not an option for v1: **text re-rendering (xFont / True Fonts style).** It needs, per application, either cooperation from the vendor or interception of the app's text drawing (GDI, DirectWrite, Chromium's own rasteriser), plus font, size and position for every run of text. Even ZoomText ships it only for Microsoft 365, and in browsers it falls back to geometric smoothing. It conflicts with PRODUCT "small steps" and has no place in the v1 Must list. It can come back after v1 alongside the reader, which needs the same per-app text access.

Not an option in code: Windows Magnifier and the Magnification API smoothing (your decision).

## Decision

Recommendation: **option 2 now, option 3 as a later, separate item, option 4 only if option 3 fails.**

- /step 1.5 adds sharp bilinear as a third `Interpolation` mode next to bilinear and bicubic. You compare all three at 10x and 20x, in fullscreen, while reading and while smooth-following, and pick the default. The step also covers the jitter work already in roadmap 1.5; that is not part of this decision.
- The same step adds a cv-render test that parses and validates shader.wgsl with naga 22.1 (`wgsl-in`) as a dev-dependency. It adds no new crate to the build (naga 22.1.0 with `wgsl-in` is already compiled via wgpu), and every new shader mode is then checked by `cargo test` instead of at runtime.
- New item 1.9 (pending this ADR) ports cleanEdge as an edge-smoothing mode after the tester's first feedback (1.8). If your 1.5 comparison says sharp bilinear is not good enough to give the tester, run 1.9 before 1.7 instead; the order is your call.
- If cleanEdge does not hold up on anti-aliased text, the multi-pass pipeline (option 4) gets its own ADR, after ADR 0004 has set the render boundaries.

Why: option 2 is the smallest change that actually removes the 20 px blur, it is readable at the high zoom the tester uses, and it gives a clear comparison: soft (bilinear or bicubic) against crisp (sharp). Options 3 and 4 match what ZoomText users see in browsers more closely, but their quality on real anti-aliased text is unknown. PRODUCT principle 3 says real use should decide that, not a guess made now.

## Consequences

- Easier: the fix is one shader branch and one enum variant, with no pipeline or uniform change. Shader mistakes fail `cargo test` instead of crashing the overlay at startup.
- Harder: three modes to keep in step across WGSL, `Interpolation`, the panel and serde. The `as_u32` mapping and the comment at the top of shader.wgsl must agree. The naga test pins naga to the wgpu version: when wgpu is upgraded (decided in ADR 0004), naga moves with it.
- Sharp text is blocky on curves at 20x. If you or the tester find that worse than blur, the default stays bilinear or bicubic and 1.9 becomes urgent.
- ClearType fringes show in every mode. If they turn out to hurt readability, a later item can test a fringe-suppression step (for example, desaturating pixels near a luminance edge) or tell the tester to turn ClearType off.
- Revisit after 1.8 (tester feedback) and when 5.5 measures GPU cost at 4K, which must include the chosen default and, if built, the 1.9 mode.
- Text re-rendering is recorded as out of v1. It comes back, if at all, with the reader after v1.
