# Text smoothing: notes to come back to

Collected 2026-10-09 while testing 1.9 (cleanEdge). The redrawn-text parts are parked until after v1 (ADR 0006). The pixel-filter parts can come back sooner, through an ADR (see "Next").

## What 1.9 showed

- Smooth edges (cleanEdge, ADR 0006 option 3) works and does not lag, but in your words it "just makes things rounder". It is still not as clean as ZoomText.
- Why: cleanEdge was made for pixel art, where edges are hard lines between flat colours. It finds those lines and cuts corners along them. Screen text is anti-aliased: glyph edges are grey in-between pixels, and cleanEdge treats each grey as one more colour to round off, not as information about where the edge is.
- Offscreen check during the build: hard-edged shapes became clean polygons and diagonals, but an anti-aliased circle kept its blocky grey ring.
- Cost at 3840x2160 on an RTX 3070: 0.9 to 1.5 ms per frame, about 4.5x Bicubic (PR #23).

## ZoomText xFont

What Vispero's UK distributor documents (links below):

- xFont redraws text, not the whole screen. It replaces the text as the app draws it with text drawn from the font at full size. Pictures, icons and everything else stay enlarged pixels.
- It worked by "direct font replacement": injecting into each app. It used to work in more places, including Internet Explorer (retired June 2022).
- Browsers were hardened against code injection (a security measure aimed at malware), which made font replacement "impossible to implement" in Chrome, Edge and Firefox.
- It also broke in Office, and ZoomText disabled it partway through the 2022 release cycle.
- It came back only for Microsoft 365: Office build 2307 or later with ZoomText 2024 (December 2023 release), on by default from 2024.2403.
- As of August 2026 all major browsers get "geometric smoothing" only, ZoomText's pixel filter. Vispero says it is working "with partners" to bring xFont back to browsers, with no dates.
- When the original date xFont was introduced: not found.

From your own use: it looked much cleaner in certain things, but it kept breaking, and in your experience there was no fallback: it just stopped working. Vispero's documents describe geometric smoothing as the fallback where xFont can't go, so what you saw may have been xFont failing in apps where it was meant to work. Not confirmed.

Sources:
- [Loss of font smoothing moving from Internet Explorer](https://sastltd.zohodesk.eu/portal/en/kb/articles/zoomtext-loss-of-font-smoothing-moving-from-internet-explorer)
- [Font smoothing problems and development roadmap](https://sastltd.zohodesk.eu/portal/en/kb/articles/zoomtext-font-smoothing-problems-and-development-roadmap)
- [New font smoothing support in 2024 with Microsoft Office](https://sastltd.zohodesk.eu/portal/en/kb/articles/zoomtext-new-font-smoothing-support-in-2024-with-microsoft-office)
- [Enable the new xFont smoothing in version 2024](https://sastltd.zohodesk.eu/portal/en/kb/articles/zoomtext-fusion-enable-the-new-xfont-smoothing-in-version-2024)
- [Font smoothing levels and expectations](https://sastltd.zohodesk.eu/portal/en/kb/articles/zoomtext-font-smoothing-expectations)

## Redrawn text for clear-view (after v1)

- Needs, for everything visible: the text, font, size, colour and exact position. Then draw it at the zoomed size and keep it in sync with scrolling, typing and animation.
- Injection is the route that browsers closed, so don't plan on it. A candidate is Windows UI Automation, which reports text and on-screen rectangles in many apps. The parked reader already uses UI Automation. How accurate and complete it is per app is unknown.
- Lesson from ZoomText: always keep a working pixel filter underneath, so a failure falls back to it instead of breaking.
- Pairs with the reader after v1: both need "what text is on screen, and where". Out of v1 per ADR 0006.

## Pixel filters to try next

1. **Contour sharpening (first choice).**
   - The grey edge pixels tell where the edge really is: 30% grey means the glyph covers about 30% of that pixel.
   - The shader blends them smoothly (as Bicubic does), then snaps to text or background colour at the halfway point, with about 1 output pixel of softening.
   - The two local colours come from the neighbouring pixels, so coloured text works. Areas that are not two-tone (photos) fall back to Bicubic.
   - One pass, about the cost of Bicubic.
   - Related to Valve's technique for crisp magnified text ([Green, SIGGRAPH 2007](https://cdn.akamai.steamstatic.com/apps/valve/2007/SIGGRAPH2007_AlphaTestedMagnification.pdf)).
   - Risks: grey edge pixels are a rougher guide than Valve's distance fields, so thin strokes and small serifs may break up or blob. ClearType fringes interfere.
2. **ClearType fringe suppression.** Turn coloured fringes grey before smoothing. First check whether turning ClearType off helps at all.
3. **Super-xBR multi-pass (ADR 0006 option 4).** Pixel-art family like cleanEdge. Probably smoother but still a bit soft, and it is the largest change.
4. **Upscalers trained on line art (Anime4K-style).** Built for 2–4x, heavier, untested on desktop text.

## What you want

The enhancements should be things you can toggle on and off: a settings section of their own, not only one more interpolation choice.

## Comparing by eye

- A screenshot of the magnifier does not work: the overlay is excluded from capture, so it shows the unzoomed desktop.
- What works: an ordinary unzoomed screenshot of text (browser, email, Word), run offscreen through the real shader in every mode at 10x and 20x and saved side by side. Plus a photo or screenshot of ZoomText on the same text as the target.
- Still open: which app you compared ZoomText in, and whether ClearType is on.

## Next

Contour sharpening is not one of ADR 0006's options, and that ADR names Super-xBR as the next step if cleanEdge fails. The toggles design also has to say how enhancements combine with the interpolation modes, in the shader and in settings. Both need an ADR before any code.
