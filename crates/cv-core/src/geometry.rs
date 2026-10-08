//! Pure view math: smoothing, crop and zoom, cursor mapping, panel sizing.
//! No platform types; the render loop feeds in plain numbers.

use crate::{DisplayMode, Edge, OutputInfo};

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

/// Convert a panel_size percentage (1-100) to pixels along the given screen dimension.
pub fn panel_pct_to_px(pct: u32, dim: i32) -> i32 {
    (dim * pct as i32 / 100).max(1)
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
pub fn compute_crop(
    cx: f32,
    cy: f32,
    win_w: u32,
    win_h: u32,
    zoom: f32,
    frame_w: f32,
    frame_h: f32,
) -> Crop {
    let src_w = (win_w as f32 / zoom).min(frame_w);
    let src_h = (win_h as f32 / zoom).min(frame_h);
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

/// Cursor position in output-window pixels, accounting for zoom and crop.
/// `(cx, cy)` must be in the same monitor-local space as the crop.
pub fn cursor_in_output(cx: f32, cy: f32, crop: &Crop, win_w: u32, win_h: u32) -> (u32, u32) {
    let x = ((cx - crop.src_x) / crop.src_w * win_w as f32).clamp(0.0, win_w as f32 - 1.0) as u32;
    let y = ((cy - crop.src_y) / crop.src_h * win_h as f32).clamp(0.0, win_h as f32 - 1.0) as u32;
    (x, y)
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
    fn crop_larger_window_than_frame_is_clamped_to_frame() {
        // Window bigger than frame at zoom 1: source never exceeds the frame.
        let c = compute_crop(100.0, 100.0, 3000, 2000, 1.0, 1920.0, 1080.0);
        assert_eq!((c.src_w, c.src_h), (1920.0, 1080.0));
        assert_eq!((c.src_x, c.src_y), (0.0, 0.0));
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
    fn cursor_centre_maps_to_window_centre() {
        let c = compute_crop(960.0, 540.0, 1920, 1080, 2.0, 1920.0, 1080.0);
        assert_eq!(cursor_in_output(960.0, 540.0, &c, 1920, 1080), (960, 540));
    }

    #[test]
    fn cursor_at_clamped_crop_edge_maps_to_window_edge() {
        let c = compute_crop(0.0, 0.0, 1920, 1080, 2.0, 1920.0, 1080.0);
        assert_eq!(cursor_in_output(0.0, 0.0, &c, 1920, 1080), (0, 0));
        let c = compute_crop(1920.0, 1080.0, 1920, 1080, 2.0, 1920.0, 1080.0);
        assert_eq!(
            cursor_in_output(1920.0, 1080.0, &c, 1920, 1080),
            (1919, 1079)
        );
    }

    #[test]
    fn cursor_off_crop_is_bounded_to_window() {
        let c = compute_crop(960.0, 540.0, 1920, 1080, 2.0, 1920.0, 1080.0);
        assert_eq!(cursor_in_output(-500.0, -500.0, &c, 1920, 1080), (0, 0));
        assert_eq!(
            cursor_in_output(9000.0, 9000.0, &c, 1920, 1080),
            (1919, 1079)
        );
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
    fn panel_px_is_percent_with_minimum_one() {
        assert_eq!(panel_pct_to_px(50, 1080), 540);
        assert_eq!(panel_pct_to_px(100, 1920), 1920);
        assert_eq!(panel_pct_to_px(1, 50), 1);
        assert_eq!(panel_pct_to_px(0, 1080), 1);
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
