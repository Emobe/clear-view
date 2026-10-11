//! Pure view math: smoothing, crop and zoom, cursor mapping, panel sizing.
//! No platform types; the render loop feeds in plain numbers.

use crate::{DisplayMode, Edge, OutputInfo, ScreenPoint, ScreenRect};

/// Frame-rate-independent lerp factor for one tick of `dt` seconds.
/// `smooth_speed` is the per-tick factor at a logical 60 Hz.
pub fn smooth_alpha(smooth_speed: f32, dt: f32) -> f32 {
    1.0_f32 - (1.0 - smooth_speed).powf(dt * 60.0)
}

/// Move `current` toward `target` by `alpha`.
pub fn lerp_toward(current: f32, target: f32, alpha: f32) -> f32 {
    current + (target - current) * alpha
}

/// Convert a virtual-screen coordinate to monitor-local (DXGI frame) space.
pub fn to_monitor_local(virtual_coord: f32, monitor_origin: i32) -> f32 {
    virtual_coord - monitor_origin as f32
}

/// The monitor whose rectangle contains the virtual-screen point, if any.
pub fn output_at(outputs: &[OutputInfo], x: i32, y: i32) -> Option<&OutputInfo> {
    outputs.iter().find(|o| {
        x >= o.left && x < o.left + o.width as i32 && y >= o.top && y < o.top + o.height as i32
    })
}

/// The primary monitor: the one containing the virtual-screen origin (0, 0)
/// ([The Virtual Screen](https://learn.microsoft.com/en-us/windows/win32/gdi/the-virtual-screen)).
/// `None` when no output does, for example when the primary is on another adapter.
pub fn primary_output(outputs: &[OutputInfo]) -> Option<&OutputInfo> {
    output_at(outputs, 0, 0)
}

/// Convert a panel_size percentage to pixels along the given screen dimension (at least 1).
pub fn panel_pct_to_px(pct: u32, dim: i32) -> i32 {
    (dim * pct as i32 / 100).max(1)
}

/// The docked panel on `edge` of `monitor`, `thickness` physical pixels across the edge
/// (clamped to the monitor), in virtual-screen pixels with the monitor origin included
/// (ADR 0007). Also the rect the system cursor is hidden inside.
pub fn docked_rect(monitor: &OutputInfo, edge: Edge, thickness: u32) -> ScreenRect {
    let (left, top, w, h) = (monitor.left, monitor.top, monitor.width, monitor.height);
    match edge {
        Edge::Top => ScreenRect { left, top, width: w, height: thickness.min(h) },
        Edge::Bottom => {
            let t = thickness.min(h);
            ScreenRect { left, top: top + (h - t) as i32, width: w, height: t }
        }
        Edge::Left => ScreenRect { left, top, width: thickness.min(w), height: h },
        Edge::Right => {
            let t = thickness.min(w);
            ScreenRect { left: left + (w - t) as i32, top, width: t, height: h }
        }
    }
}

/// The part of `monitor` the docked panel does not cover: the monitor rect minus `docked_rect`,
/// in virtual-screen pixels (ADR 0011). Empty (zero width or height) when the thickness covers
/// the whole monitor.
pub fn uncovered_rect(monitor: &OutputInfo, edge: Edge, thickness: u32) -> ScreenRect {
    let (left, top, w, h) = (monitor.left, monitor.top, monitor.width, monitor.height);
    match edge {
        Edge::Top => {
            let t = thickness.min(h);
            ScreenRect { left, top: top + t as i32, width: w, height: h - t }
        }
        Edge::Bottom => ScreenRect { left, top, width: w, height: h - thickness.min(h) },
        Edge::Left => {
            let t = thickness.min(w);
            ScreenRect { left: left + t as i32, top, width: w - t, height: h }
        }
        Edge::Right => ScreenRect { left, top, width: w - thickness.min(w), height: h },
    }
}

/// How far the unmagnified area is panned, in pixels, for a pointer at `pointer` (ADR 0011,
/// rule "pinned push"). A magnitude: the direction of the shift follows from `edge`.
///
/// With p the pointer's distance from the monitor edge the panel sits on (the edge row is 0),
/// T the thickness and m the margin, the offset is `clamp(T + m - p, 0, T)`: 0 while the pointer
/// is farther than T + m from that edge, then growing one pixel per pixel toward the panel, so
/// the pointer drawn at p + offset stays at T + m, until the whole strip is shown at p = m.
/// 0 when the pointer is not on `monitor`. The thickness is clamped to the monitor and the margin
/// to `0 ..= uncovered dimension - 1`, so while any part of the monitor is uncovered the offset
/// is 0 on the far row.
pub fn pan_offset(
    pointer: ScreenPoint,
    monitor: &OutputInfo,
    edge: Edge,
    thickness: u32,
    margin: f32,
) -> f32 {
    let on_monitor = ScreenRect {
        left: monitor.left,
        top: monitor.top,
        width: monitor.width,
        height: monitor.height,
    };
    if !on_monitor.contains(pointer) {
        return 0.0;
    }
    let x = i64::from(pointer.x) - i64::from(monitor.left);
    let y = i64::from(pointer.y) - i64::from(monitor.top);
    let (w, h) = (i64::from(monitor.width), i64::from(monitor.height));
    let (p, dim) = match edge {
        Edge::Top => (y, h),
        Edge::Bottom => (h - 1 - y, h),
        Edge::Left => (x, w),
        Edge::Right => (w - 1 - x, w),
    };
    let t = i64::from(thickness).min(dim);
    // `max` first: it turns a NaN margin into 0.
    let m = margin.max(0.0).min((dim - t - 1).max(0) as f32);
    (t as f32 + m - p as f32).clamp(0.0, t as f32)
}

/// Overlay window size for a display mode on a `sw` x `sh` monitor.
pub fn window_dims(mode: DisplayMode, panel_pct: u32, sw: i32, sh: i32) -> (u32, u32) {
    match mode {
        DisplayMode::Fullscreen => (sw as u32, sh as u32),
        DisplayMode::Docked(Edge::Top | Edge::Bottom) => {
            (sw as u32, panel_pct_to_px(panel_pct, sh) as u32)
        }
        DisplayMode::Docked(Edge::Left | Edge::Right) => {
            (panel_pct_to_px(panel_pct, sw) as u32, sh as u32)
        }
    }
}

/// The source rectangle, in frame pixels, shown in the output window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crop {
    pub src_x: f32,
    pub src_y: f32,
    pub src_w: f32,
    pub src_h: f32,
    pub frame_w: f32,
    pub frame_h: f32,
}

impl Crop {
    /// `[x, y, w, h]` as fractions of the frame, the layout the shader uniform expects.
    pub fn normalized(&self) -> [f32; 4] {
        [
            self.src_x / self.frame_w,
            self.src_y / self.frame_h,
            self.src_w / self.frame_w,
            self.src_h / self.frame_h,
        ]
    }
}

/// Crop centred on `(cx, cy)` (monitor-local frame pixels), clamped to the frame.
///
/// The crop keeps the window's aspect ratio. A window larger than the frame at this zoom (a
/// docked panel on a bigger monitor than the one it shows, roadmap 5.2) gets the largest crop
/// of that shape that fits, so the image is zoomed a little more rather than stretched.
pub fn compute_crop(
    cx: f32,
    cy: f32,
    win_w: u32,
    win_h: u32,
    zoom: f32,
    frame_w: f32,
    frame_h: f32,
) -> Crop {
    let (w, h) = (win_w as f32 / zoom, win_h as f32 / zoom);
    let fit = (frame_w / w).min(frame_h / h).min(1.0);
    // `min` again: rounding in `h * fit` must never leave the crop a hair larger than the frame
    // (the clamps below would then get min > max).
    let (src_w, src_h) = ((w * fit).min(frame_w), (h * fit).min(frame_h));
    let src_x = (cx - src_w * 0.5).clamp(0.0, frame_w - src_w);
    let src_y = (cy - src_h * 0.5).clamp(0.0, frame_h - src_h);
    Crop {
        src_x,
        src_y,
        src_w,
        src_h,
        frame_w,
        frame_h,
    }
}

/// Pointer position in output-window pixels, accounting for zoom and crop, or `None` when the
/// pointer is outside the crop (not shown in the window). `(px, py)` must be in the same
/// monitor-local space as the crop.
pub fn pointer_in_output(
    px: f32,
    py: f32,
    crop: &Crop,
    win_w: u32,
    win_h: u32,
) -> Option<(u32, u32)> {
    let x = (px - crop.src_x) / crop.src_w * win_w as f32;
    let y = (py - crop.src_y) / crop.src_h * win_h as f32;
    let inside = (0.0..win_w as f32).contains(&x) && (0.0..win_h as f32).contains(&y);
    inside.then_some((x as u32, y as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn out(idx: u32, left: i32, top: i32, width: u32, height: u32) -> OutputInfo {
        OutputInfo {
            idx,
            left,
            top,
            width,
            height,
        }
    }

    #[test]
    fn alpha_is_zero_with_no_elapsed_time() {
        assert!(approx(smooth_alpha(0.15, 0.0), 0.0));
    }

    #[test]
    fn alpha_equals_speed_at_one_logical_tick() {
        assert!(approx(smooth_alpha(0.15, 1.0 / 60.0), 0.15));
    }

    #[test]
    fn alpha_is_one_at_full_speed() {
        assert!(approx(smooth_alpha(1.0, 1.0 / 60.0), 1.0));
    }

    #[test]
    fn alpha_is_frame_rate_independent() {
        // One 2-tick step must land where two 1-tick steps land.
        let speed = 0.15;
        let one = lerp_toward(0.0, 100.0, smooth_alpha(speed, 2.0 / 60.0));
        let a = lerp_toward(0.0, 100.0, smooth_alpha(speed, 1.0 / 60.0));
        let two = lerp_toward(a, 100.0, smooth_alpha(speed, 1.0 / 60.0));
        assert!(approx(one, two));
    }

    #[test]
    fn lerp_endpoints_and_midpoint() {
        assert!(approx(lerp_toward(10.0, 20.0, 0.0), 10.0));
        assert!(approx(lerp_toward(10.0, 20.0, 1.0), 20.0));
        assert!(approx(lerp_toward(10.0, 20.0, 0.5), 15.0));
        assert!(approx(lerp_toward(20.0, 10.0, 0.5), 15.0));
    }

    #[test]
    fn crop_centred_at_zoom_two() {
        let c = compute_crop(960.0, 540.0, 1920, 1080, 2.0, 1920.0, 1080.0);
        assert_eq!(c.src_w, 960.0);
        assert_eq!(c.src_h, 540.0);
        assert_eq!(c.src_x, 480.0);
        assert_eq!(c.src_y, 270.0);
        assert_eq!(c.normalized(), [0.25, 0.25, 0.5, 0.5]);
    }

    #[test]
    fn crop_clamps_at_all_corners() {
        let tl = compute_crop(0.0, 0.0, 1920, 1080, 2.0, 1920.0, 1080.0);
        assert_eq!((tl.src_x, tl.src_y), (0.0, 0.0));
        let br = compute_crop(1920.0, 1080.0, 1920, 1080, 2.0, 1920.0, 1080.0);
        assert_eq!((br.src_x, br.src_y), (960.0, 540.0));
        let tr = compute_crop(1920.0, 0.0, 1920, 1080, 2.0, 1920.0, 1080.0);
        assert_eq!((tr.src_x, tr.src_y), (960.0, 0.0));
        let bl = compute_crop(0.0, 1080.0, 1920, 1080, 2.0, 1920.0, 1080.0);
        assert_eq!((bl.src_x, bl.src_y), (0.0, 540.0));
    }

    #[test]
    fn crop_at_zoom_one_is_the_full_frame() {
        let c = compute_crop(300.0, 700.0, 1920, 1080, 1.0, 1920.0, 1080.0);
        assert_eq!(c.normalized(), [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn crop_larger_window_than_frame_fits_the_frame_and_keeps_the_shape() {
        // Window bigger than frame at zoom 1: source never exceeds the frame, and keeps the
        // window's 3:2 shape (height is the tighter side: 1080 / 2000).
        let c = compute_crop(100.0, 100.0, 3000, 2000, 1.0, 1920.0, 1080.0);
        assert!((c.src_w - 1620.0).abs() < 0.01 && (c.src_h - 1080.0).abs() < 0.01, "{c:?}");
        assert!(c.src_h <= 1080.0 && c.src_w <= 1920.0, "{c:?}");
        assert_eq!((c.src_x, c.src_y), (0.0, 0.0));
    }

    #[test]
    fn crop_for_a_wider_panel_than_the_frame_keeps_the_panel_shape() {
        // A 2560x720 top panel showing a 1920x1080 monitor at 1x: 2560 is too wide, so the crop
        // is 1920x540, the panel's shape, not 1920x720 stretched.
        let c = compute_crop(960.0, 540.0, 2560, 720, 1.0, 1920.0, 1080.0);
        assert_eq!((c.src_w, c.src_h), (1920.0, 540.0));
        assert_eq!((c.src_x, c.src_y), (0.0, 270.0));
        // From about 1.34x it fits, and the zoom is exact again.
        let c = compute_crop(960.0, 540.0, 2560, 720, 2.0, 1920.0, 1080.0);
        assert_eq!((c.src_w, c.src_h), (1280.0, 360.0));
    }

    #[test]
    fn crop_docked_top_panel() {
        // 50% docked top on 1920x1080: window 1920x540, zoom 2 shows 960x270.
        let (w, h) = window_dims(DisplayMode::Docked(Edge::Top), 50, 1920, 1080);
        let c = compute_crop(960.0, 540.0, w, h, 2.0, 1920.0, 1080.0);
        assert_eq!((c.src_w, c.src_h), (960.0, 270.0));
        assert_eq!((c.src_x, c.src_y), (480.0, 405.0));
    }

    #[test]
    fn pointer_at_the_centre_maps_to_window_centre() {
        let c = compute_crop(960.0, 540.0, 1920, 1080, 2.0, 1920.0, 1080.0);
        assert_eq!(pointer_in_output(960.0, 540.0, &c, 1920, 1080), Some((960, 540)));
    }

    #[test]
    fn pointer_off_centre_maps_with_the_zoom() {
        // Crop 480..1440 x 270..810 at 2x.
        let c = compute_crop(960.0, 540.0, 1920, 1080, 2.0, 1920.0, 1080.0);
        assert_eq!(pointer_in_output(1000.0, 500.0, &c, 1920, 1080), Some((1040, 460)));
    }

    #[test]
    fn pointer_at_clamped_crop_corners_maps_to_window_corners() {
        let c = compute_crop(0.0, 0.0, 1920, 1080, 2.0, 1920.0, 1080.0);
        assert_eq!(pointer_in_output(0.0, 0.0, &c, 1920, 1080), Some((0, 0)));
        // Crop 960..1920 x 540..1080: the last monitor pixel is two output pixels in.
        let c = compute_crop(1920.0, 1080.0, 1920, 1080, 2.0, 1920.0, 1080.0);
        assert_eq!(pointer_in_output(1919.0, 1079.0, &c, 1920, 1080), Some((1918, 1078)));
    }

    #[test]
    fn pointer_outside_the_crop_is_not_shown() {
        // Crop 480..1440 x 270..810.
        let c = compute_crop(960.0, 540.0, 1920, 1080, 2.0, 1920.0, 1080.0);
        assert_eq!(pointer_in_output(479.0, 540.0, &c, 1920, 1080), None);
        assert_eq!(pointer_in_output(1440.0, 540.0, &c, 1920, 1080), None);
        assert_eq!(pointer_in_output(960.0, 269.0, &c, 1920, 1080), None);
        assert_eq!(pointer_in_output(960.0, 810.0, &c, 1920, 1080), None);
        assert_eq!(pointer_in_output(-500.0, -500.0, &c, 1920, 1080), None);
        assert_eq!(pointer_in_output(480.0, 270.0, &c, 1920, 1080), Some((0, 0)));
    }

    #[test]
    fn monitor_local_with_negative_origin() {
        assert!(approx(to_monitor_local(-1000.0, -1920), 920.0));
        assert!(approx(to_monitor_local(2000.0, 1920), 80.0));
    }

    #[test]
    fn output_at_finds_monitor_and_respects_bounds() {
        let outs = [out(0, 0, 0, 1920, 1080), out(1, -1280, 0, 1280, 1024)];
        assert_eq!(output_at(&outs, 10, 10).map(|o| o.idx), Some(0));
        assert_eq!(output_at(&outs, -10, 10).map(|o| o.idx), Some(1));
        // Right/bottom edges are exclusive; left/top are inclusive.
        assert_eq!(output_at(&outs, 1919, 1079).map(|o| o.idx), Some(0));
        assert!(output_at(&outs, 1920, 500).is_none());
        assert!(output_at(&outs, 500, 1080).is_none());
        assert_eq!(output_at(&outs, -1280, 0).map(|o| o.idx), Some(1));
        assert!(output_at(&outs, -1281, 0).is_none());
    }

    #[test]
    fn primary_is_the_output_at_the_origin_wherever_it_is_listed() {
        let outs = [out(0, -1280, 0, 1280, 1024), out(1, 0, 0, 1920, 1080)];
        assert_eq!(primary_output(&outs).map(|o| o.idx), Some(1));
        // Origin on the primary's top-left corner even with monitors above and left of it.
        let outs = [
            out(0, 1920, -500, 1280, 1024),
            out(1, 0, 0, 1920, 1080),
            out(2, -800, -600, 800, 600),
        ];
        assert_eq!(primary_output(&outs).map(|o| o.idx), Some(1));
    }

    #[test]
    fn no_output_at_the_origin_has_no_primary() {
        let outs = [out(0, 1920, 0, 1280, 1024)];
        assert!(primary_output(&outs).is_none());
        assert!(primary_output(&[]).is_none());
    }

    #[test]
    fn panel_px_is_percent_with_minimum_one() {
        assert_eq!(panel_pct_to_px(50, 1080), 540);
        assert_eq!(panel_pct_to_px(100, 1920), 1920);
        assert_eq!(panel_pct_to_px(1, 50), 1);
        assert_eq!(panel_pct_to_px(0, 1080), 1);
    }

    fn rect(left: i32, top: i32, width: u32, height: u32) -> ScreenRect {
        ScreenRect { left, top, width, height }
    }

    #[test]
    fn docked_rect_on_each_edge_of_the_primary() {
        let m = out(0, 0, 0, 1920, 1080);
        assert_eq!(docked_rect(&m, Edge::Top, 270), rect(0, 0, 1920, 270));
        assert_eq!(docked_rect(&m, Edge::Bottom, 270), rect(0, 810, 1920, 270));
        assert_eq!(docked_rect(&m, Edge::Left, 480), rect(0, 0, 480, 1080));
        assert_eq!(docked_rect(&m, Edge::Right, 480), rect(1440, 0, 480, 1080));
    }

    #[test]
    fn docked_rect_includes_the_monitor_origin() {
        let m = out(1, -1280, -200, 1280, 1024);
        assert_eq!(docked_rect(&m, Edge::Top, 100), rect(-1280, -200, 1280, 100));
        assert_eq!(docked_rect(&m, Edge::Bottom, 100), rect(-1280, 724, 1280, 100));
        assert_eq!(docked_rect(&m, Edge::Left, 100), rect(-1280, -200, 100, 1024));
        assert_eq!(docked_rect(&m, Edge::Right, 100), rect(-100, -200, 100, 1024));
    }

    #[test]
    fn docked_rect_is_clamped_to_the_monitor() {
        let m = out(0, 0, 0, 1920, 1080);
        assert_eq!(docked_rect(&m, Edge::Top, 5000), rect(0, 0, 1920, 1080));
        assert_eq!(docked_rect(&m, Edge::Bottom, 5000), rect(0, 0, 1920, 1080));
        assert_eq!(docked_rect(&m, Edge::Left, 5000), rect(0, 0, 1920, 1080));
        assert_eq!(docked_rect(&m, Edge::Right, 5000), rect(0, 0, 1920, 1080));
    }

    #[test]
    fn docked_rect_matches_window_dims() {
        let m = out(0, 0, 0, 1920, 1080);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            let dim = match edge {
                Edge::Top | Edge::Bottom => 1080,
                Edge::Left | Edge::Right => 1920,
            };
            let r = docked_rect(&m, edge, panel_pct_to_px(30, dim) as u32);
            assert_eq!(
                (r.width, r.height),
                window_dims(DisplayMode::Docked(edge), 30, 1920, 1080),
                "{edge:?}"
            );
        }
    }

    #[test]
    fn uncovered_rect_on_each_edge_of_the_primary() {
        let m = out(0, 0, 0, 1920, 1080);
        assert_eq!(uncovered_rect(&m, Edge::Top, 270), rect(0, 270, 1920, 810));
        assert_eq!(uncovered_rect(&m, Edge::Bottom, 270), rect(0, 0, 1920, 810));
        assert_eq!(uncovered_rect(&m, Edge::Left, 480), rect(480, 0, 1440, 1080));
        assert_eq!(uncovered_rect(&m, Edge::Right, 480), rect(0, 0, 1440, 1080));
    }

    #[test]
    fn uncovered_rect_includes_the_monitor_origin() {
        let m = out(1, -1280, -200, 1280, 1024);
        assert_eq!(uncovered_rect(&m, Edge::Top, 100), rect(-1280, -100, 1280, 924));
        assert_eq!(uncovered_rect(&m, Edge::Bottom, 100), rect(-1280, -200, 1280, 924));
        assert_eq!(uncovered_rect(&m, Edge::Left, 100), rect(-1180, -200, 1180, 1024));
        assert_eq!(uncovered_rect(&m, Edge::Right, 100), rect(-1280, -200, 1180, 1024));
    }

    #[test]
    fn uncovered_rect_is_empty_when_the_panel_covers_the_monitor() {
        let m = out(0, 0, 0, 1920, 1080);
        for edge in [Edge::Top, Edge::Bottom] {
            assert_eq!(uncovered_rect(&m, edge, 1080).height, 0, "{edge:?}");
            assert_eq!(uncovered_rect(&m, edge, 5000).height, 0, "{edge:?}");
        }
        for edge in [Edge::Left, Edge::Right] {
            assert_eq!(uncovered_rect(&m, edge, 1920).width, 0, "{edge:?}");
            assert_eq!(uncovered_rect(&m, edge, 5000).width, 0, "{edge:?}");
        }
    }

    #[test]
    fn uncovered_rect_and_docked_rect_tile_the_monitor() {
        let m = out(1, -1280, -200, 1280, 1024);
        for edge in EDGES {
            let (d, u) = (docked_rect(&m, edge, 300), uncovered_rect(&m, edge, 300));
            let area = |r: ScreenRect| u64::from(r.width) * u64::from(r.height);
            assert_eq!(area(d) + area(u), 1280 * 1024, "{edge:?}");
            // Each point of the monitor lies in exactly one of the two.
            for (x, y) in [(-1280, -200), (-1, 823), (-640, 312), (-1280, 823), (-1, -200)] {
                let p = ScreenPoint { x, y };
                assert_ne!(d.contains(p), u.contains(p), "{edge:?} at ({x}, {y})");
            }
        }
    }

    const EDGES: [Edge; 4] = [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right];

    /// `pan_offset` on a 1920x1080 monitor at the origin, a 300 px panel and a 100 px margin,
    /// for the pointer at distance `p` from the edge the panel sits on.
    fn pan_at(edge: Edge, p: i32) -> f32 {
        let m = out(0, 0, 0, 1920, 1080);
        let (x, y) = match edge {
            Edge::Top => (700, p),
            Edge::Bottom => (700, 1079 - p),
            Edge::Left => (p, 500),
            Edge::Right => (1919 - p, 500),
        };
        pan_offset(ScreenPoint { x, y }, &m, edge, 300, 100.0)
    }

    #[test]
    fn pan_offset_is_zero_beyond_the_margin() {
        for edge in EDGES {
            for p in [400, 401, 700, 1000] {
                assert_eq!(pan_at(edge, p), 0.0, "{edge:?} p={p}");
            }
        }
    }

    #[test]
    fn pan_offset_ramps_one_pixel_per_pixel() {
        for edge in EDGES {
            for (p, want) in [(399, 1.0), (350, 50.0), (300, 100.0), (200, 200.0), (101, 299.0)] {
                assert_eq!(pan_at(edge, p), want, "{edge:?} p={p}");
            }
        }
    }

    #[test]
    fn pan_offset_shows_the_whole_strip_in_the_last_margin() {
        for edge in EDGES {
            for p in [100, 50, 1, 0] {
                assert_eq!(pan_at(edge, p), 300.0, "{edge:?} p={p}");
            }
        }
    }

    #[test]
    fn pan_offset_pins_the_drawn_pointer_while_it_ramps() {
        // The pointer is drawn at p + offset; between m and T + m that stays at T + m.
        for edge in EDGES {
            for p in 100..=400 {
                assert_eq!(p as f32 + pan_at(edge, p), 400.0, "{edge:?} p={p}");
            }
        }
    }

    #[test]
    fn pan_offset_never_drops_as_the_pointer_nears_the_panel() {
        for edge in EDGES {
            let mut last = 0.0;
            for p in (0..=500).rev() {
                let now = pan_at(edge, p);
                assert!(now >= last, "{edge:?} p={p}: {now} < {last}");
                last = now;
            }
        }
    }

    #[test]
    fn pan_offset_is_zero_off_the_panels_monitor() {
        let m = out(0, 0, 0, 1920, 1080);
        for (x, y) in [(-1, 10), (1920, 10), (10, -1), (10, 1080), (-500, 5), (2500, 5)] {
            for edge in EDGES {
                let p = ScreenPoint { x, y };
                assert_eq!(pan_offset(p, &m, edge, 300, 100.0), 0.0, "{edge:?} ({x}, {y})");
            }
        }
    }

    #[test]
    fn pan_offset_uses_the_monitor_origin() {
        let m = out(1, -1280, -200, 1280, 1024);
        let off = |x, y, edge| pan_offset(ScreenPoint { x, y }, &m, edge, 300, 100.0);
        // T = 300, m = 100: a pointer 250 px from the panel's edge is panned 150.
        assert_eq!(off(-600, 50, Edge::Top), 150.0);
        assert_eq!(off(-1280 + 250, 0, Edge::Left), 150.0);
        // The last row or column of the monitor is p = 0: the whole strip.
        assert_eq!(off(-600, 823, Edge::Bottom), 300.0);
        assert_eq!(off(-1, 0, Edge::Right), 300.0);
        assert_eq!(off(-1 - 250, 0, Edge::Right), 150.0);
        // Another monitor's point is off this panel's monitor.
        assert_eq!(off(250, 250, Edge::Top), 0.0);
    }

    #[test]
    fn pan_offset_clamps_thickness_and_margin() {
        let m = out(0, 0, 0, 1920, 1080);
        let at = |y, thickness, margin| {
            pan_offset(ScreenPoint { x: 5, y }, &m, Edge::Top, thickness, margin)
        };
        // A panel as big as the monitor: the strip is the whole monitor (and no row is
        // uncovered, so there is nothing to pan; the formula still gives a value).
        assert_eq!(at(0, 5000, 100.0), 1080.0);
        assert_eq!(at(1079, 5000, 100.0), 1.0);
        // A margin past the uncovered dimension still reaches 0 on the far row.
        assert_eq!(at(1079, 300, 5000.0), 0.0);
        assert!(at(1078, 300, 5000.0) > 0.0);
        // No margin: the offset starts only under the panel.
        assert_eq!(at(300, 300, 0.0), 0.0);
        assert_eq!(at(299, 300, 0.0), 1.0);
        assert_eq!(at(0, 300, 0.0), 300.0);
        // A negative or NaN margin is no margin.
        assert_eq!(at(300, 300, -50.0), 0.0);
        assert_eq!(at(300, 300, f32::NAN), 0.0);
    }

    #[test]
    fn window_dims_per_mode() {
        assert_eq!(window_dims(DisplayMode::Fullscreen, 50, 1920, 1080), (1920, 1080));
        assert_eq!(window_dims(DisplayMode::Docked(Edge::Top), 50, 1920, 1080), (1920, 540));
        assert_eq!(window_dims(DisplayMode::Docked(Edge::Bottom), 25, 1920, 1080), (1920, 270));
        assert_eq!(window_dims(DisplayMode::Docked(Edge::Left), 50, 1920, 1080), (960, 1080));
        assert_eq!(window_dims(DisplayMode::Docked(Edge::Right), 25, 1920, 1080), (480, 1080));
    }
}
