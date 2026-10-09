pub mod geometry;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Raw BGRA8 pixel data, row-major, top-down.
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum ColorFilter {
    #[default]
    None,
    Inverted,
    Greyscale,
    GreyscaleInverted,
}

impl ColorFilter {
    pub fn as_u32(self) -> u32 {
        match self {
            Self::None              => 0,
            Self::Inverted          => 1,
            Self::Greyscale         => 2,
            Self::GreyscaleInverted => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum Interpolation {
    Bilinear,
    /// Default: chosen by you after comparing the three modes at 10x and 20x (1.5).
    #[default]
    Bicubic,
    /// Sharp bilinear: texels drawn as flat blocks, edges blended over about 1 output pixel (ADR 0006).
    Sharp,
    /// cleanEdge edge smoothing: texels drawn as blocks with diagonal edges cut in (ADR 0006, 1.9).
    CleanEdge,
}

impl Interpolation {
    pub fn as_u32(self) -> u32 {
        match self {
            Self::Bilinear  => 0,
            Self::Bicubic   => 1,
            Self::Sharp     => 2,
            Self::CleanEdge => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum DisplayMode {
    #[default]
    Fullscreen,
    Docked(Edge),
}

pub const ZOOM_MIN: f32 = 1.0;
pub const ZOOM_MAX: f32 = 20.0;
/// Hotkey zoom step below `ZOOM_COARSE_FROM`.
pub const ZOOM_STEP_FINE: f32 = 0.5;
/// Hotkey zoom step from `ZOOM_COARSE_FROM` up.
pub const ZOOM_STEP_COARSE: f32 = 1.0;
/// Zoom at which the hotkey step changes from fine to coarse.
pub const ZOOM_COARSE_FROM: f32 = 4.0;

/// Persisted to settings.json except `enabled`, which always starts false.
/// `#[serde(default)]` lets a file written by an older version load: missing fields take defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    /// Not persisted: the magnifier always starts off.
    #[serde(skip)]
    pub enabled: bool,
    /// Magnification factor (`ZOOM_MIN`–`ZOOM_MAX`).
    pub zoom: f32,
    /// Lerp speed per logical 60 Hz tick (0.01–1.0).
    pub smooth_speed: f32,
    pub interpolation: Interpolation,
    /// cleanEdge colour similarity threshold (0–1): RGB distance at or below which two texels
    /// count as the same shape. Only used by `Interpolation::CleanEdge`.
    pub edge_threshold: f32,
    pub display_mode: DisplayMode,
    /// Panel size as a percentage of the relevant screen dimension (1–100). Ignored in Fullscreen mode.
    pub panel_size: u32,
    pub color_filter: ColorFilter,
    /// Master TTS on/off switch.
    pub tts_enabled: bool,
    /// Speak the name of the element under the cursor.
    pub tts_hover_enabled: bool,
    /// SAPI volume 0–100.
    pub tts_volume: u32,
    /// SAPI rate -10 to 10.
    pub tts_rate: i32,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            enabled: false,
            zoom: 2.0,
            smooth_speed: 0.15,
            interpolation: Interpolation::default(),
            edge_threshold: 0.1,
            display_mode: DisplayMode::Fullscreen,
            panel_size: 50,
            color_filter: ColorFilter::None,
            tts_enabled: false,
            tts_hover_enabled: true,
            tts_volume: 80,
            tts_rate: 0,
        }
    }
}

impl AppState {
    /// Repair values loaded from disk: clamp to the documented ranges and
    /// replace non-finite floats with the default.
    pub fn sanitize(&mut self) {
        let d = Self::default();
        if !self.zoom.is_finite() { self.zoom = d.zoom; }
        if !self.smooth_speed.is_finite() { self.smooth_speed = d.smooth_speed; }
        if !self.edge_threshold.is_finite() { self.edge_threshold = d.edge_threshold; }
        self.zoom = self.zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        self.smooth_speed = self.smooth_speed.clamp(0.01, 1.0);
        self.edge_threshold = self.edge_threshold.clamp(0.0, 1.0);
        self.panel_size = self.panel_size.clamp(1, 100);
        self.tts_volume = self.tts_volume.min(100);
        self.tts_rate = self.tts_rate.clamp(-10, 10);
    }

    /// Moves zoom one step in (`direction` > 0) or out (< 0), clamped to the zoom range
    /// and rounded to the slider's 0.1 grid so repeated presses do not drift.
    ///
    /// The step is `ZOOM_STEP_FINE` below `ZOOM_COARSE_FROM` and `ZOOM_STEP_COARSE` from there
    /// up. Stepping out from exactly `ZOOM_COARSE_FROM` uses the fine step, so in and out
    /// presses retrace the same values.
    pub fn step_zoom(&mut self, direction: i32) {
        let dir = direction.signum() as f32;
        let coarse = if dir > 0.0 { self.zoom >= ZOOM_COARSE_FROM } else { self.zoom > ZOOM_COARSE_FROM };
        let step = if coarse { ZOOM_STEP_COARSE } else { ZOOM_STEP_FINE };
        let z = self.zoom + step * dir;
        self.zoom = ((z * 10.0).round() / 10.0).clamp(ZOOM_MIN, ZOOM_MAX);
    }
}

pub type SharedState = Arc<RwLock<AppState>>;
pub type FrameState = Arc<Mutex<Option<Arc<Frame>>>>;

pub fn new_shared() -> SharedState {
    shared_from(AppState::default())
}

pub fn shared_from(state: AppState) -> SharedState {
    Arc::new(RwLock::new(state))
}

/// Information about a single DXGI output (monitor).
#[derive(Clone)]
pub struct OutputInfo {
    /// Zero-based DXGI output index on the primary adapter.
    pub idx: u32,
    /// Left edge in virtual screen coordinates.
    pub left: i32,
    /// Top edge in virtual screen coordinates.
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn non_default() -> AppState {
        AppState {
            enabled: true,
            zoom: 4.5,
            smooth_speed: 0.4,
            interpolation: Interpolation::CleanEdge,
            edge_threshold: 0.25,
            display_mode: DisplayMode::Docked(Edge::Left),
            panel_size: 30,
            color_filter: ColorFilter::GreyscaleInverted,
            tts_enabled: true,
            tts_hover_enabled: false,
            tts_volume: 55,
            tts_rate: -3,
        }
    }

    #[test]
    fn round_trip_keeps_everything_but_enabled() {
        let before = non_default();
        let json = serde_json::to_string(&before).unwrap();
        let after: AppState = serde_json::from_str(&json).unwrap();
        assert!(!after.enabled);
        assert_eq!(after, AppState { enabled: false, ..before });
    }

    #[test]
    fn enabled_is_never_written() {
        let json = serde_json::to_string(&non_default()).unwrap();
        // Match the exact key: "tts_enabled" legitimately appears in the file.
        assert!(!json.contains("\"enabled\""), "{json}");
    }

    #[test]
    fn enabled_in_file_is_ignored() {
        let s: AppState = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
        assert!(!s.enabled);
    }

    #[test]
    fn empty_object_gives_defaults() {
        let s: AppState = serde_json::from_str("{}").unwrap();
        assert_eq!(s, AppState::default());
    }

    #[test]
    fn partial_file_keeps_given_fields() {
        let s: AppState = serde_json::from_str(r#"{"zoom": 3.0, "tts_rate": 2}"#).unwrap();
        assert_eq!(s.zoom, 3.0);
        assert_eq!(s.tts_rate, 2);
        assert_eq!(s.panel_size, AppState::default().panel_size);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(serde_json::from_str::<AppState>("not json").is_err());
        assert!(serde_json::from_str::<AppState>(r#"{"zoom": "big"}"#).is_err());
        assert!(serde_json::from_str::<AppState>(r#"{"color_filter": "Sepia"}"#).is_err());
    }

    #[test]
    fn every_enum_variant_round_trips() {
        for f in [ColorFilter::None, ColorFilter::Inverted, ColorFilter::Greyscale, ColorFilter::GreyscaleInverted] {
            let j = serde_json::to_string(&f).unwrap();
            assert_eq!(serde_json::from_str::<ColorFilter>(&j).unwrap(), f);
        }
        for i in [Interpolation::Bilinear, Interpolation::Bicubic, Interpolation::Sharp, Interpolation::CleanEdge] {
            let j = serde_json::to_string(&i).unwrap();
            assert_eq!(serde_json::from_str::<Interpolation>(&j).unwrap(), i);
        }
        for m in [
            DisplayMode::Fullscreen,
            DisplayMode::Docked(Edge::Top),
            DisplayMode::Docked(Edge::Bottom),
            DisplayMode::Docked(Edge::Left),
            DisplayMode::Docked(Edge::Right),
        ] {
            let j = serde_json::to_string(&m).unwrap();
            assert_eq!(serde_json::from_str::<DisplayMode>(&j).unwrap(), m);
        }
    }

    #[test]
    fn sanitize_clamps_out_of_range() {
        let mut s = AppState {
            zoom: 99.0,
            smooth_speed: 0.0,
            panel_size: 0,
            edge_threshold: -0.5,
            tts_volume: 500,
            tts_rate: -50,
            ..AppState::default()
        };
        s.sanitize();
        assert_eq!(s.zoom, 20.0);
        assert_eq!(s.smooth_speed, 0.01);
        assert_eq!(s.panel_size, 1);
        assert_eq!(s.edge_threshold, 0.0);
        assert_eq!(s.tts_volume, 100);
        assert_eq!(s.tts_rate, -10);

        s.panel_size = 400;
        s.zoom = 0.2;
        s.edge_threshold = 3.0;
        s.sanitize();
        assert_eq!(s.panel_size, 100);
        assert_eq!(s.zoom, 1.0);
        assert_eq!(s.edge_threshold, 1.0);
    }

    #[test]
    fn step_zoom_moves_one_step() {
        let mut s = AppState { zoom: 2.0, ..AppState::default() };
        s.step_zoom(1);
        assert_eq!(s.zoom, 2.0 + ZOOM_STEP_FINE);
        s.step_zoom(-1);
        assert_eq!(s.zoom, 2.0);

        s.zoom = 10.0;
        s.step_zoom(1);
        assert_eq!(s.zoom, 10.0 + ZOOM_STEP_COARSE);
        s.step_zoom(-1);
        assert_eq!(s.zoom, 10.0);
    }

    #[test]
    fn step_zoom_changes_size_at_the_boundary_and_retraces() {
        let mut s = AppState { zoom: ZOOM_MIN, ..AppState::default() };
        let mut up = vec![s.zoom];
        while s.zoom < ZOOM_MAX {
            s.step_zoom(1);
            up.push(s.zoom);
        }
        // 1.0, 1.5 ... 4.0 (fine), then 5.0 ... 20.0 (coarse).
        assert_eq!(up.len(), 1 + 6 + 16);
        assert_eq!(up[6], ZOOM_COARSE_FROM);
        assert_eq!(up[7], ZOOM_COARSE_FROM + ZOOM_STEP_COARSE);

        let mut down = vec![s.zoom];
        while s.zoom > ZOOM_MIN {
            s.step_zoom(-1);
            down.push(s.zoom);
        }
        down.reverse();
        assert_eq!(up, down);
    }

    #[test]
    fn step_zoom_off_whole_number_above_the_boundary_stays_on_grid() {
        let mut s = AppState { zoom: 4.3, ..AppState::default() };
        s.step_zoom(1);
        assert_eq!(s.zoom, 5.3);
        s.zoom = 4.3;
        s.step_zoom(-1);
        assert_eq!(s.zoom, 3.3);
    }

    #[test]
    fn step_zoom_clamps_at_both_ends() {
        let mut s = AppState { zoom: ZOOM_MAX - 0.1, ..AppState::default() };
        s.step_zoom(1);
        assert_eq!(s.zoom, ZOOM_MAX);
        s.step_zoom(1);
        assert_eq!(s.zoom, ZOOM_MAX);

        s.zoom = ZOOM_MIN + 0.1;
        s.step_zoom(-1);
        assert_eq!(s.zoom, ZOOM_MIN);
        s.step_zoom(-1);
        assert_eq!(s.zoom, ZOOM_MIN);
    }

    #[test]
    fn step_zoom_stays_on_the_slider_grid() {
        // An off-grid value (hand-edited file) is snapped to 0.1 by the first step.
        let mut s = AppState { zoom: 2.04, ..AppState::default() };
        s.step_zoom(1);
        assert_eq!(s.zoom, 2.5);
        for _ in 0..40 {
            s.step_zoom(1);
            s.step_zoom(-1);
        }
        assert_eq!(s.zoom, 2.5);
    }

    #[test]
    fn step_zoom_zero_direction_does_not_move() {
        let mut s = AppState { zoom: 3.0, ..AppState::default() };
        s.step_zoom(0);
        assert_eq!(s.zoom, 3.0);
    }

    #[test]
    fn sanitize_replaces_non_finite_floats() {
        let mut s = AppState {
            zoom: f32::NAN,
            smooth_speed: f32::INFINITY,
            edge_threshold: f32::NAN,
            ..AppState::default()
        };
        s.sanitize();
        assert_eq!(s.zoom, AppState::default().zoom);
        assert_eq!(s.smooth_speed, AppState::default().smooth_speed);
        assert_eq!(s.edge_threshold, AppState::default().edge_threshold);
    }

    #[test]
    fn sanitize_keeps_zoom_between_old_and_new_maximum() {
        for z in [1.0, 6.0, 10.0, 10.5, 15.0, 20.0] {
            let mut s = AppState { zoom: z, ..AppState::default() };
            s.sanitize();
            assert_eq!(s.zoom, z);
        }
    }

    #[test]
    fn sanitize_leaves_valid_state_alone() {
        let mut s = non_default();
        let before = s.clone();
        s.sanitize();
        assert_eq!(s, before);
    }
}
