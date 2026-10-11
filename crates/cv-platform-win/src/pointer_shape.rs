//! The pointer's shape from GDI (ADR 0010, roadmap 6.2c1): `GetIconInfo` and `GetDIBits` on a
//! cursor handle, then a coverage mask. The conversion from bytes to mask is pure and tested.

use std::{ffi::c_void, mem::size_of, ptr};

use cv_core::PointerShape;
use windows::Win32::{
    Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC,
        GetDIBits, GetObjectW, HBITMAP, HDC, HGDIOBJ, ReleaseDC,
    },
    UI::WindowsAndMessaging::{GetIconInfo, HCURSOR, HICON, ICONINFO},
};

/// Reads the shape of `cursor` and says whether it was monochrome or colour. The error text is
/// for the `[pointer]` log line.
pub(crate) fn read_shape(cursor: HCURSOR) -> Result<(PointerShape, &'static str), String> {
    let mut raw = ICONINFO::default();
    unsafe { GetIconInfo(HICON(cursor.0), &mut raw) }
        .map_err(|e| format!("GetIconInfo failed: {e}"))?;
    // Deletes both bitmaps on every path out of here.
    let _bitmaps = IconBitmaps(raw);
    let dc = ScreenDc::get().ok_or("GetDC failed")?;

    let (mask_w, mask_h) = bitmap_size(raw.hbmMask).ok_or("GetObjectW failed on the mask bitmap")?;
    let mask = bitmap_bgra(dc.0, raw.hbmMask, mask_w, mask_h)
        .ok_or("GetDIBits failed on the mask bitmap")?;
    let hotspot = (raw.xHotspot, raw.yHotspot);

    let (kind, width, height, coverage) = if raw.hbmColor.is_invalid() {
        let height = mask_h / 2;
        ("monochrome", mask_w, height, mask_from_monochrome(&mask, mask_w, mask_h))
    } else {
        let (w, h) = bitmap_size(raw.hbmColor).ok_or("GetObjectW failed on the colour bitmap")?;
        let color = bitmap_bgra(dc.0, raw.hbmColor, w, h)
            .ok_or("GetDIBits failed on the colour bitmap")?;
        // The AND mask only matters when no pixel has alpha, and then it must match in size.
        let and = (mask_w == w && mask_h == h).then_some(mask.as_slice());
        ("colour", w, h, mask_from_colour(&color, and))
    };
    let mask = coverage.ok_or("the mask bitmap does not fit the cursor")?;
    Ok((PointerShape { width, height, hotspot, mask }, kind))
}

/// A monochrome cursor: `bits` is the mask bitmap as 32 bpp BGRA, `width` x `height` with the
/// AND mask on the top half and the XOR mask below, so the cursor is half as tall. A set bit
/// reads as white (non-zero). A pixel is drawn unless AND = 1 and XOR = 0 (that one leaves the
/// screen alone); XOR pixels, which invert the screen, count as drawn.
///
/// `None` when `height` is odd or `bits` is the wrong length.
pub(crate) fn mask_from_monochrome(bits: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    if height == 0 || !height.is_multiple_of(2) || bits.len() != (width * height * 4) as usize {
        return None;
    }
    let half = (height / 2) as usize;
    let width = width as usize;
    let mut mask = Vec::with_capacity(width * half);
    for y in 0..half {
        for x in 0..width {
            let and = bits[(y * width + x) * 4] != 0;
            let xor = bits[((y + half) * width + x) * 4] != 0;
            mask.push(if and && !xor { 0 } else { 255 });
        }
    }
    Some(mask)
}

/// A colour cursor: `color` is the colour bitmap as 32 bpp BGRA. A pixel is drawn where alpha is
/// above zero. A cursor with no alpha at all (an old-style colour cursor) is drawn where the AND
/// mask is 0; `and` is that mask as 32 bpp BGRA of the same size.
///
/// `None` when `color` is not whole pixels, or there is no alpha and no matching AND mask.
pub(crate) fn mask_from_colour(color: &[u8], and: Option<&[u8]>) -> Option<Vec<u8>> {
    if color.is_empty() || !color.len().is_multiple_of(4) {
        return None;
    }
    let alpha = color.iter().skip(3).step_by(4);
    if alpha.clone().any(|a| *a > 0) {
        return Some(alpha.map(|a| if *a > 0 { 255 } else { 0 }).collect());
    }
    let and = and.filter(|a| a.len() == color.len())?;
    Some(and.iter().step_by(4).map(|b| if *b == 0 { 255 } else { 0 }).collect())
}

/// Owns the two bitmaps `GetIconInfo` hands out: "the calling application must manage these
/// bitmaps and delete them with DeleteObject".
struct IconBitmaps(ICONINFO);

impl Drop for IconBitmaps {
    fn drop(&mut self) {
        for bitmap in [self.0.hbmMask, self.0.hbmColor] {
            if !bitmap.is_invalid() {
                let _ = unsafe { DeleteObject(HGDIOBJ(bitmap.0)) };
            }
        }
    }
}

/// The screen's device context, released on drop.
struct ScreenDc(HDC);

impl ScreenDc {
    fn get() -> Option<Self> {
        let dc = unsafe { GetDC(None) };
        (!dc.is_invalid()).then_some(Self(dc))
    }
}

impl Drop for ScreenDc {
    fn drop(&mut self) {
        unsafe { ReleaseDC(None, self.0) };
    }
}

fn bitmap_size(bitmap: HBITMAP) -> Option<(u32, u32)> {
    let mut bm = BITMAP::default();
    let got = unsafe {
        GetObjectW(
            HGDIOBJ(bitmap.0),
            size_of::<BITMAP>() as i32,
            Some(ptr::from_mut(&mut bm).cast::<c_void>()),
        )
    };
    (got != 0 && bm.bmWidth > 0 && bm.bmHeight > 0).then_some((bm.bmWidth as u32, bm.bmHeight as u32))
}

/// The bitmap as top-down 32 bpp BGRA bytes, or `None` if `GetDIBits` copied nothing. The bitmap
/// must not be selected into a device context; the ones `GetIconInfo` makes are not.
fn bitmap_bgra(dc: HDC, bitmap: HBITMAP, w: u32, h: u32) -> Option<Vec<u8>> {
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bytes = vec![0u8; (w * h * 4) as usize];
    let lines = unsafe {
        GetDIBits(dc, bitmap, 0, h, Some(bytes.as_mut_ptr().cast::<c_void>()), &mut info, DIB_RGB_COLORS)
    };
    (lines != 0).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 32 bpp BGRA bytes for a row-major list of bits: set = white, clear = black.
    fn bits(rows: &[&str]) -> Vec<u8> {
        rows.iter()
            .flat_map(|r| r.chars())
            .flat_map(|c| if c == '1' { [255, 255, 255, 0] } else { [0, 0, 0, 0] })
            .collect()
    }

    #[test]
    fn monochrome_draws_everything_but_the_untouched_pixels() {
        // A 3x1 cursor, AND row on top and XOR row below. Per column (AND, XOR): x0 = (0, 0)
        // black, x1 = (1, 1) inverts the screen, x2 = (1, 0) leaves the screen alone.
        let mask = mask_from_monochrome(&bits(&["011", "010"]), 3, 2).unwrap();
        assert_eq!(mask, vec![255, 255, 0]);
    }

    #[test]
    fn monochrome_white_pixel_counts_as_drawn() {
        // AND = 0, XOR = 1: a white pixel.
        let mask = mask_from_monochrome(&bits(&["0", "1"]), 1, 2).unwrap();
        assert_eq!(mask, vec![255]);
    }

    #[test]
    fn monochrome_is_half_the_mask_height() {
        let mask = mask_from_monochrome(&bits(&["00", "00", "11", "00", "00", "11"]), 2, 6);
        assert_eq!(mask.map(|m| m.len()), Some(2 * 3));
    }

    #[test]
    fn monochrome_rejects_an_odd_height_and_a_short_buffer() {
        assert_eq!(mask_from_monochrome(&bits(&["0", "0", "0"]), 1, 3), None);
        assert_eq!(mask_from_monochrome(&bits(&["0", "0"]), 1, 4), None);
        assert_eq!(mask_from_monochrome(&[], 0, 0), None);
    }

    #[test]
    fn colour_is_drawn_where_alpha_is_above_zero() {
        let color = [10, 20, 30, 0, 10, 20, 30, 1, 10, 20, 30, 255];
        assert_eq!(mask_from_colour(&color, None), Some(vec![0, 255, 255]));
    }

    #[test]
    fn colour_alpha_wins_over_the_and_mask() {
        let color = [0, 0, 0, 255, 0, 0, 0, 0];
        let and = bits(&["10"]);
        assert_eq!(mask_from_colour(&color, Some(&and)), Some(vec![255, 0]));
    }

    #[test]
    fn colour_without_any_alpha_uses_the_and_mask() {
        // Alpha is all zero, so the AND mask decides: 0 is drawn, 1 is not.
        let color = [9, 9, 9, 0, 9, 9, 9, 0];
        let and = bits(&["01"]);
        assert_eq!(mask_from_colour(&color, Some(&and)), Some(vec![255, 0]));
    }

    #[test]
    fn colour_without_alpha_needs_a_matching_and_mask() {
        let color = [9, 9, 9, 0, 9, 9, 9, 0];
        assert_eq!(mask_from_colour(&color, None), None);
        assert_eq!(mask_from_colour(&color, Some(&bits(&["0"]))), None);
    }

    #[test]
    fn colour_rejects_partial_pixels_and_nothing() {
        assert_eq!(mask_from_colour(&[0, 0, 0], None), None);
        assert_eq!(mask_from_colour(&[], None), None);
    }
}
