# 0010: Cursor enhancement

Status: Accepted (by the owner's instruction, 2026-10-10)
Date: 2026-10-10

## Context

Roadmap 6.1. PRODUCT.md v1 "Should (in v1 if cheap)": cursor enhancement, meaning a large pointer, a colour (blue) and a thicker outline. ROADMAP 6.1 asks whether to draw our own enlarged pointer in the magnified view or to change the system cursor, and which size, colour and outline options to offer. ADR 0007 said to revisit this here, because the enhancement may replace the circle and change what is drawn under the docked panel.

### What the code does today

- **Captured image.** Nothing in the magnified view comes from the real pointer. The capture code (crates/cv-platform-win/src/capture.rs `acquire`) copies the desktop image and reads only `LastPresentTime` from `DXGI_OUTDUPL_FRAME_INFO`. It never reads `PointerPosition` and never calls `GetFramePointerShape`.
- **The circle.** shader.wgsl draws a fixed white circle, radius 10 output pixels, with a 2 px black ring. It sits at `cursor_x`/`cursor_y` (uniforms, 48 bytes) and is drawn after the colour filter, so inversion does not touch it. magnifier.rs `draw` places it at the real pointer with `geometry::pointer_in_output`, or not at all (`CURSOR_HIDDEN`) when the pointer is out of view. It is the same in fullscreen and docked. It has no settings. STATUS (1.6): "The circle radius is fixed in physical pixels, so it looks smaller at 150% (size, not alignment; roadmap 6.1/6.2)."
- **System cursor.** In fullscreen it is hidden by `MagShowSystemCursor(false)` in `WinHost::apply_layout`. In docked mode it is hidden only while the pointer is inside the panel (`OverlayHost::set_system_cursor`, ADR 0007). Teardown and the `OverlayHandle::shutdown` fallback show it again. So in fullscreen the circle is the only pointer on screen, and the real arrow, I-beam or hand is never seen.
- **Pointer.** `PointerSource::position` (cv-core) is read once a tick on the render thread. `WinHost` implements it with `GetCursorPos` (`CursorPointer`). Nothing reads the cursor's shape.
- **Renderer.** One bind group: uniforms, the frame texture and a sampler (shader.wgsl bindings 0 to 2). Uniform writes and frame uploads are skipped when unchanged (`last_uniforms`, `Arc::ptr_eq` on the frame).
- **`windows` features.** `Win32_UI_WindowsAndMessaging` and `Win32_Graphics_Gdi` are already in the workspace list. Those are the features `GetCursorInfo`, `GetIconInfo`, `GetDIBits` and `DeleteObject` need, so no new feature or crate is required.

### External facts

- **Desktop Duplication and the pointer.** "Either the mouse pointer is already drawn onto the desktop image … or the mouse pointer is separate from the desktop image … your client app must draw the mouse pointer shape onto the desktop image to accurately represent what the current user will see." Position comes with every frame. The shape comes from `GetFramePointerShape`, and only when it changes ([Desktop Duplication API](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api)). `PointerPosition.Position` "is valid only if the Visible member's value is set to TRUE" ([DXGI_OUTDUPL_POINTER_POSITION](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/ns-dxgi1_2-dxgi_outdupl_pointer_position)). Shapes come in three formats: monochrome (1 bpp AND mask followed by a 1 bpp XOR mask), colour (32 bpp ARGB) and masked colour, where the alpha byte selects replace (0) or XOR (0xFF) ([DXGI_OUTDUPL_POINTER_SHAPE_TYPE](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/ne-dxgi1_2-dxgi_outdupl_pointer_shape_type)). The shape call fails with `DXGI_ERROR_ACCESS_LOST` like the frame calls do ([GetFramePointerShape](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutputduplication-getframepointershape)). It works per output, on the capture thread.
- **The global cursor from user32.** `GetCursorInfo` gives `flags` (0 means "the cursor is hidden", `CURSOR_SHOWING`, and `CURSOR_SUPPRESSED` when touch or pen input is used), `hCursor` and the screen position ([CURSORINFO](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-cursorinfo)). `GetIconInfo` on that handle gives the hotspot plus `hbmMask` and `hbmColor`. For monochrome cursors `hbmMask` is twice the height (AND mask on top, XOR mask below) and `hbmColor` is NULL. Read the bits with `GetDIBits` ([ICONINFO](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-iconinfo)). "The calling application must manage these bitmaps and delete them with DeleteObject", and the call "does not participate in DPI virtualization" ([GetIconInfo](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-geticoninfo)).
- **Not documented anywhere checked:** whether `GetCursorInfo` and Desktop Duplication still report the cursor's shape and visibility while `MagShowSystemCursor(false)` hides it, and what either gives for an animated cursor (the busy cursor). `MagShowSystemCursor`'s page says only that it "shows or hides the system cursor", with no reference count ([MagShowSystemCursor](https://learn.microsoft.com/en-us/windows/win32/api/magnification/nf-magnification-magshowsystemcursor)). Fullscreen hides the cursor all the time, so the first question decides whether the real shape can be drawn at all.
- **Changing the system cursor.** `SetSystemCursor` "replaces the contents of the system cursor specified by the id parameter … and then destroys hcur". It works per cursor type (`OCR_NORMAL`, `OCR_IBEAM`, `OCR_HAND` and ten more) ([SetSystemCursor](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setsystemcursor)). `SystemParametersInfo(SPI_SETCURSORS)` "reloads the system cursors" ([SystemParametersInfo](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-systemparametersinfow)). The docs do not say how long a replacement lasts if the process dies without reloading.
- **Windows' own pointer settings.** Settings > Accessibility > Mouse pointer and touch has a pointer size from 1 to 15 and the styles White, Black, Inverted and Custom (any colour) ([pureinfotech](https://pureinfotech.com/change-mouse-pointer-color-size-windows-11/), [geekrewind](https://geekrewind.com/how-to-customize-mouse-pointer-size-and-color-in-windows-11-accessibility-settings/)). It is system-wide, it is the user's choice and it persists. It has no outline thickness.
- **ZoomText** (the tester's reference) offers pointer size and colour, a locator that marks the pointer's position, and since ZoomText 2024 an Inverted scheme and 15 pointer sizes instead of three ([Northeast Low Vision, ZoomText 2024 updates](https://nelowvision.com/wp-content/uploads/2022/06/Software-Updates-2024-ZoomText-Fusion.pdf), [Waubonsee ZoomText pointer enhancements](https://mymedia.waubonsee.edu/media/ZoomText+-+Pointer+Enhancements/1_8634gyi3)).

## Options

### Where the enhanced pointer comes from

1. **Our own arrow, drawn in the shader.** A fixed arrow shape (a signed-distance function or a small built-in mask) replaces the circle. Its size, fill colour and outline width come from uniforms. Cost: shader and uniform changes plus settings, with no new Windows call and no seam change. But it always looks like an arrow. Over text it does not become an I-beam, over a link it does not become a hand, and it gives no resize or busy shapes, so the user loses what the pointer is about to do. In fullscreen this is worse than today's circle: the circle claims nothing about the shape, while a wrong arrow does.

2. **The real cursor shape, recoloured and drawn by us.** The backend reads the current cursor shape and turns it into a platform-neutral coverage mask (one byte per pixel: drawn or not, with AND/XOR and alpha resolved) plus the hotspot. The magnifier uploads the mask as a second small texture only when it changes. The shader draws it at the pointer, scaled by the size setting, filled with the chosen colour and outlined by dilating the mask. Cost:
   - a new neutral type in cv-core and a shape method next to `PointerSource::position`, which ADR 0004 allows ("a separate source is allowed later"; "names may change in the steps, the split may not");
   - shape conversion for monochrome and colour cursors in cv-platform-win;
   - a second texture and binding, and uniforms that grow past 48 bytes (`uniform_size_matches_the_shader_struct` will check the new size);
   - new saved settings.

   The open question above decides whether this works while the cursor is hidden. The shape source has two choices:
   - **2a `GetCursorInfo` + `GetIconInfo` on the render thread**, read each tick next to `GetCursorPos`, and converted only when `hCursor` changes. It stays in `WinHost` and needs no `CaptureSource` change.
   - **2b Desktop Duplication's pointer shape on the capture thread.** It arrives only with frames of the active output, and it would have to cross the `CaptureSource` seam next to `Frame` (ADR 0004 names `CaptureSource` returning `Frame` as the line that would change). It also adds per-output bookkeeping that the duplication sample shows is fiddly (which output saw the pointer last).

3. **The real cursor drawn as Windows renders it, magnified, with no options of our own.** The user sets size and colour in Windows Settings, and we draw that shape in the magnified view (XOR pixels inverted against the frame). Cost: the same reading as option 2, plus the full colour and XOR handling in the shader. It offers nothing for the outline PRODUCT asks for, and the size can be set only system-wide, which also changes the unmagnified pointer.

4. **Change the system cursor** (`SetSystemCursor` with a large blue cursor per `OCR_*` type, restored with `SPI_SETCURSORS`). Cost:
   - it is a system-wide change that outlives a kill, against PRODUCT principle 2, and the docs do not say when Windows undoes it;
   - it overwrites the user's own Windows pointer settings while running;
   - it needs a cursor set for every shape and every size;
   - it does not reach the magnified view at all: fullscreen hides the system cursor, and the captured image does not contain it. To be seen there it would still need option 2's reading and drawing.

   Rejected.

### Size

a. **A multiple of the real cursor's own size, in output pixels, not tied to zoom.** At 1 the pointer is the size Windows draws it, which already includes Windows' pointer-size setting. Changing the zoom does not change the pointer, which is how the circle behaves today.
b. **Scaled with zoom**, as a magnified copy of the cursor would be. At 10x a 32 px cursor is 320 px and covers what it points at. A size multiplier on top would make it worse.

### The circle

i. **The enhanced pointer replaces the circle,** in fullscreen and inside the docked panel. If the shape cannot be read (a failure, or before the first read), the circle is drawn as today, so there is always a marker.
ii. **Keep the circle as well, as a locator around the pointer.** ZoomText has a separate locator. Nobody has asked for one, and a locator is a feature of its own.

## Decision

Recommendation: **source option 2 with 2a, size a, circle i.** Option 1 is the fallback if the probe (6.2a, 6.2b) shows that the shape cannot be read while `MagShowSystemCursor(false)` hides the cursor. That fallback needs no new ADR. This ADR supersedes nothing. It extends ADR 0004's pointer source and replaces the circle that ADR 0007 draws under the docked panel, as 0007 anticipated. It does not use the Magnification API for anything but the cursor hiding it already does.

- **Shape source (backend).** `WinHost` reads `GetCursorInfo` each tick on the render thread. When `hCursor` changes, it runs `GetIconInfo` and `GetDIBits` and converts the result to a coverage mask:
  - monochrome: a pixel is drawn unless AND = 1 and XOR = 0, so XOR (inverting) pixels count as drawn;
  - colour: alpha > 0.

  It deletes both bitmaps every time. It reports one of three things: hidden (`flags` 0 or `CURSOR_SUPPRESSED`), a shape, or unknown (a call failed). The same `Arc` is returned while `hCursor` is unchanged, so the magnifier skips re-uploads with `Arc::ptr_eq`, as it does for frames. Sketch (names may change in the step):

  ```rust
  // cv-core
  pub struct PointerShape { pub width: u32, pub height: u32, pub hotspot: (u32, u32), pub mask: Vec<u8> }
  pub enum PointerLook { Hidden, Unknown, Shape(Arc<PointerShape>) }
  pub trait PointerSource {
      fn position(&self) -> Option<ScreenPoint>;
      fn look(&mut self) -> PointerLook;
  }
  ```

- **Drawing (magnifier).** The mask goes into a second texture, recreated only when its size changes. The shader draws it after the colour filter (as the circle is drawn now), with its top-left at the pointer minus `hotspot × size` in output pixels, sampled with smooth edges. The fill colour covers every drawn pixel and an outline of the chosen width surrounds it. `Hidden` draws nothing. `Unknown` draws the circle. Out of view draws nothing, as today.
- **Where it shows.** The magnified view only: fullscreen, and inside the docked panel. Outside the docked panel the real system cursor stays visible at 1x (ADR 0007), and Windows' own pointer settings apply to it. TESTER.md says so.
- **Settings** (new `AppState` fields, saved, clamped in `sanitize`, added to the panel and to `apply_changes`):
  - pointer size (a multiple, size a);
  - fill colour, blue by default;
  - outline width in output pixels;
  - outline colour, a contrasting default.

  The ranges and the exact defaults are set in the step and tuned with you or the tester. Like panel size in ADR 0007, changing those numbers later needs no new ADR. There is no on/off switch: the pointer is always drawn, as the circle is now.
- **Display scaling.** The mask from `GetIconInfo` is the cursor at the size Windows draws it, so it scales with the display scale. This fixes the 1.6 note that the circle looks small at 150%. 6.2c's Verify checks this at 100% and 150%.
- **Roadmap.** 6.2 becomes 6.2a to 6.2c, measurement first, as Phase 4 did:
  - 6.2a (agent): a dev-only `pointer_probe` example. It hides the cursor with `MagShowSystemCursor(false)` for a few seconds, shows it again, and logs what `GetCursorInfo` and the duplication's `PointerPosition` and pointer shape report in both states, including over text, a link, a resize edge and a busy cursor.
  - 6.2b (you): run it and send the output.
  - 6.2c (agent): build this decision, or option 1 if 6.2b shows that nothing reports while hidden.

Why: the shape is information a low-vision user relies on (I-beam over text, hand over a link), and today fullscreen shows none of it, only a circle. Option 2 keeps that information and adds the size, colour and outline PRODUCT asks for. 2a keeps the whole change on the render thread, next to the pointer read that is already there, with no `CaptureSource` change. Option 3 gives no outline and pushes size into a system-wide setting. Option 4 breaks principle 2 and does not reach the magnified view. Size a over b, because a zoom-scaled pointer covers what it points at. The probe goes first because the one fact the decision depends on is undocumented.

## Consequences

- Easier: the fullscreen view shows what the pointer is (arrow, I-beam, hand, resize), large and coloured, and colour filters leave it alone. The circle's DPI size problem goes away. A Linux backend implements `look` or answers `Unknown` and keeps the circle.
- Harder: `PointerSource` grows a method every backend must implement. The renderer gets a second texture and binding, and the uniforms grow (the size test catches a mismatch). Cursor bitmaps must be deleted on every conversion, or GDI handles leak. There are more settings in the panel and in `apply_changes`. Animated cursors may show one frame only (6.2b says).
- Unknown until 6.2b: whether the shape can be read while the cursor is hidden (if not, option 1), animated cursors, and touch or pen input (`CURSOR_SUPPRESSED`).
- Revisit: after the tester uses it (whether size should follow zoom after all, and whether a locator is wanted); if 6.2b rules out 2a but shows 2b working, which is a `CaptureSource` change and needs a new ADR; with UIAccess (6.3 to 6.5), if elevated windows report the cursor differently.
