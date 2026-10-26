pub mod events;
pub mod focus;
pub mod geometry;
pub mod hotkeys;
pub mod speech;
pub mod tracking;
pub mod voice;
pub mod wording;

pub use events::{AppId, CaretSource, CoreEvent, EventHub};
pub use focus::{
    Checked, Control, DialogNode, FocusFilter, FocusSource, Password, Reading, Role, States, Value,
    WindowInfo,
};
pub use speech::{Kind, Level, SpeechOp, SpeechQueue, SpeechSettings, Utterance, utterances_for};
pub use tracking::{Following, Tracker};
pub use voice::{Engine, SpeechState, SpeechStatus, VoiceChoice, VoiceId, VoiceInfo};
pub use wording::Verbosity;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use voice::{NEUTRAL_MAX, NEUTRAL_RATE_NORMAL};

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

/// Docked panel size limits, as a percentage of the monitor dimension the panel spans
/// (ADR 0007).
pub const PANEL_SIZE_MIN: u32 = 10;
pub const PANEL_SIZE_MAX: u32 = 90;

/// Pointer size limits: output pixels per pixel of the cursor Windows draws (ADR 0010).
pub const POINTER_SIZE_MIN: f32 = 1.0;
pub const POINTER_SIZE_MAX: f32 = 6.0;
/// Widest pointer outline in output pixels. The shader's `MAX_OUTLINE` is the same number
/// (a test in cv-magnifier checks).
pub const POINTER_OUTLINE_MAX: f32 = 8.0;

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
    /// Panel size as a percentage of the relevant screen dimension
    /// (`PANEL_SIZE_MIN`–`PANEL_SIZE_MAX`). Ignored in Fullscreen mode.
    pub panel_size: u32,
    /// Docked with more than one monitor (roadmap 5.1, 5.2). Off: the panel stays on the
    /// primary monitor and shows whichever monitor the target is on. On: the panel moves to
    /// the target's monitor, same edge, `panel_size` percent of that monitor.
    pub panel_follows_monitor: bool,
    pub color_filter: ColorFilter,
    /// The magnified view's pointer (ADR 0010, roadmap 6.2c3). On: the real cursor shape. Off:
    /// the circle for every look except a hidden pointer. The other `pointer_*` fields only
    /// affect the shape.
    pub pointer_shape: bool,
    /// Output pixels per pixel of the cursor Windows draws
    /// (`POINTER_SIZE_MIN`–`POINTER_SIZE_MAX`).
    pub pointer_size: f32,
    /// Outline width in output pixels (0–`POINTER_OUTLINE_MAX`).
    pub pointer_outline: f32,
    /// Fill colour, sRGB bytes (what the shader writes to the surface).
    pub pointer_fill: [u8; 3],
    /// Outline colour, sRGB bytes.
    pub pointer_outline_colour: [u8; 3],
    /// Master TTS on/off switch.
    pub tts_enabled: bool,
    /// Speech volume 0–100 (ADR 0012).
    pub tts_volume: u32,
    /// Speech rate, the neutral 0–`NEUTRAL_MAX` (ADR 0012): 50 is the voice's normal rate. Each
    /// engine maps it onto its own range. A file from before 8.9 had the SAPI scale -10 to 10
    /// under the key `tts_rate`; the app's settings loader migrates it into this field.
    pub tts_rate_neutral: u32,
    /// The voice the user picked (roadmap 8.7d, ADR 0012): engine and the engine's own id. None
    /// until one is picked; a voice that is no longer installed stays saved so it comes back.
    pub tts_voice: Option<VoiceId>,
    /// How much is said about a focused control (roadmap 9.5). Old files load the default.
    pub tts_verbosity: Verbosity,
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
            panel_follows_monitor: false,
            color_filter: ColorFilter::None,
            pointer_shape: true,
            pointer_size: 2.0,
            pointer_outline: 2.0,
            pointer_fill: [26, 102, 255],
            pointer_outline_colour: [255, 255, 255],
            tts_enabled: false,
            tts_volume: 80,
            tts_rate_neutral: NEUTRAL_RATE_NORMAL,
            tts_voice: None,
            tts_verbosity: Verbosity::default(),
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
        if !self.pointer_size.is_finite() { self.pointer_size = d.pointer_size; }
        if !self.pointer_outline.is_finite() { self.pointer_outline = d.pointer_outline; }
        self.zoom = self.zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        self.smooth_speed = self.smooth_speed.clamp(0.01, 1.0);
        self.edge_threshold = self.edge_threshold.clamp(0.0, 1.0);
        self.panel_size = self.panel_size.clamp(PANEL_SIZE_MIN, PANEL_SIZE_MAX);
        self.pointer_size = self.pointer_size.clamp(POINTER_SIZE_MIN, POINTER_SIZE_MAX);
        self.pointer_outline = self.pointer_outline.clamp(0.0, POINTER_OUTLINE_MAX);
        self.tts_volume = self.tts_volume.min(100);
        self.tts_rate_neutral = self.tts_rate_neutral.min(NEUTRAL_MAX);
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

/// What a hotkey asks for. The platform backend maps keys to these; `AppState::apply` does them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Toggle,
    ZoomIn,
    ZoomOut,
    /// Stop speaking and clear the speech queue (ADR 0013). Changes no `AppState` field: the
    /// caller sends it to the speech thread. Bound to a lone Ctrl (`hotkeys::SilenceKey`, ADR
    /// 0014); nothing reads the key before roadmap 8.6b.
    Silence,
    /// Speech on or off (`tts_enabled`). In `hotkeys::DEFAULT_BINDINGS` as a speech binding;
    /// not registered before roadmap 8.6b.
    ToggleSpeech,
}

impl AppState {
    pub fn apply(&mut self, action: Action) {
        match action {
            Action::Toggle => self.enabled = !self.enabled,
            Action::ZoomIn => self.step_zoom(1),
            Action::ZoomOut => self.step_zoom(-1),
            Action::Silence => {}
            Action::ToggleSpeech => self.tts_enabled = !self.tts_enabled,
        }
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

/// A point in physical pixels on the virtual screen (all monitors, primary at the origin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenPoint {
    pub x: i32,
    pub y: i32,
}

/// A rectangle in physical pixels on the virtual screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

impl ScreenRect {
    /// Whether `p` lies inside. Left and top edges are inclusive, right and bottom exclusive,
    /// as in `geometry::output_at`.
    pub fn contains(&self, p: ScreenPoint) -> bool {
        let (x, y) = (i64::from(p.x), i64::from(p.y));
        let (left, top) = (i64::from(self.left), i64::from(self.top));
        x >= left
            && x < left + i64::from(self.width)
            && y >= top
            && y < top + i64::from(self.height)
    }
}

/// For logs: `(x, y)`.
impl std::fmt::Display for ScreenPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

/// For logs: `(left, top) WxH`.
impl std::fmt::Display for ScreenRect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {}) {}x{}", self.left, self.top, self.width, self.height)
    }
}

/// The pointer's shape as a coverage mask (ADR 0010): which pixels the pointer draws, with the
/// platform's AND/XOR/alpha rules already resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerShape {
    pub width: u32,
    pub height: u32,
    /// The pixel that sits on the pointer position, from the top-left of the mask.
    pub hotspot: (u32, u32),
    /// `width * height` bytes, row-major, top-down: 255 where the pointer is drawn, 0 elsewhere.
    pub mask: Vec<u8>,
}

/// What the pointer looks like right now (ADR 0010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointerLook {
    /// The system says there is no pointer to show (hidden, or suppressed by touch or pen).
    Hidden,
    /// The platform cannot say, for example a failed call. Callers fall back to a marker.
    Unknown,
    /// The same `Arc` comes back while the shape is unchanged, so callers compare with
    /// `Arc::ptr_eq` to skip re-uploads.
    Shape(Arc<PointerShape>),
}

/// Where the pointer is and what it looks like (ADR 0004, ADR 0010). Sampled when needed, not
/// sent as events.
pub trait PointerSource {
    /// `None` when the platform cannot say, for example while another desktop has input.
    fn position(&self) -> Option<ScreenPoint>;

    /// The pointer's shape, also while the system cursor is hidden. A backend with no way to
    /// read it answers `Unknown`.
    fn look(&mut self) -> PointerLook;
}

/// Information about a single DXGI output (monitor).
#[derive(Debug, Clone, PartialEq)]
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
            panel_follows_monitor: true,
            color_filter: ColorFilter::GreyscaleInverted,
            pointer_shape: false,
            pointer_size: 3.5,
            pointer_outline: 4.0,
            pointer_fill: [200, 30, 10],
            pointer_outline_colour: [0, 0, 0],
            tts_enabled: true,
            tts_volume: 55,
            tts_rate_neutral: 35,
            tts_voice: Some(VoiceId { engine: Engine::Sapi, id: "token".into() }),
            tts_verbosity: Verbosity::Full,
        }
    }

    #[test]
    fn a_file_without_a_voice_loads_none() {
        // Every settings file written before 8.7d.
        let s: AppState = serde_json::from_str(r#"{"tts_enabled": true}"#).unwrap();
        assert_eq!(s.tts_voice, None);
        assert!(s.tts_enabled);
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
        let s: AppState = serde_json::from_str(r#"{"zoom": 3.0, "tts_rate_neutral": 20}"#).unwrap();
        assert_eq!(s.zoom, 3.0);
        assert_eq!(s.tts_rate_neutral, 20);
        assert_eq!(s.panel_size, AppState::default().panel_size);
    }

    #[test]
    fn a_file_without_panel_follows_monitor_loads_it_off() {
        // Every settings file written before 5.2.
        let s: AppState = serde_json::from_str(r#"{"display_mode": {"Docked": "Top"}}"#).unwrap();
        assert!(!s.panel_follows_monitor);
        assert_eq!(s.display_mode, DisplayMode::Docked(Edge::Top));
    }

    #[test]
    fn a_file_without_pointer_settings_loads_the_defaults() {
        // Every settings file written before 6.2c3.
        let s: AppState = serde_json::from_str(r#"{"zoom": 3.0}"#).unwrap();
        let d = AppState::default();
        assert!(s.pointer_shape);
        assert_eq!(s.pointer_size, d.pointer_size);
        assert_eq!(s.pointer_outline, d.pointer_outline);
        assert_eq!(s.pointer_fill, d.pointer_fill);
        assert_eq!(s.pointer_outline_colour, d.pointer_outline_colour);
    }

    #[test]
    fn pointer_defaults_are_inside_their_ranges() {
        let d = AppState::default();
        assert!((POINTER_SIZE_MIN..=POINTER_SIZE_MAX).contains(&d.pointer_size));
        assert!((0.0..=POINTER_OUTLINE_MAX).contains(&d.pointer_outline));
        let mut s = d.clone();
        s.sanitize();
        assert_eq!(s, d);
    }

    #[test]
    fn sanitize_clamps_the_pointer_settings() {
        let mut s = AppState { pointer_size: 0.1, pointer_outline: -3.0, ..AppState::default() };
        s.sanitize();
        assert_eq!(s.pointer_size, POINTER_SIZE_MIN);
        assert_eq!(s.pointer_outline, 0.0);

        s.pointer_size = 50.0;
        s.pointer_outline = 50.0;
        s.sanitize();
        assert_eq!(s.pointer_size, POINTER_SIZE_MAX);
        assert_eq!(s.pointer_outline, POINTER_OUTLINE_MAX);

        let d = AppState::default();
        let mut s = AppState { pointer_size: f32::NAN, pointer_outline: f32::INFINITY, ..d.clone() };
        s.sanitize();
        assert_eq!(s.pointer_size, d.pointer_size);
        assert_eq!(s.pointer_outline, d.pointer_outline);
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
            tts_rate_neutral: 500,
            ..AppState::default()
        };
        s.sanitize();
        assert_eq!(s.zoom, 20.0);
        assert_eq!(s.smooth_speed, 0.01);
        assert_eq!(s.panel_size, PANEL_SIZE_MIN);
        assert_eq!(s.edge_threshold, 0.0);
        assert_eq!(s.tts_volume, 100);
        assert_eq!(s.tts_rate_neutral, 100);

        s.panel_size = 400;
        s.zoom = 0.2;
        s.edge_threshold = 3.0;
        s.sanitize();
        assert_eq!(s.panel_size, PANEL_SIZE_MAX);
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
    fn apply_toggle_flips_enabled_and_back() {
        let mut s = AppState::default();
        assert!(!s.enabled);
        s.apply(Action::Toggle);
        assert!(s.enabled);
        s.apply(Action::Toggle);
        assert!(!s.enabled);
    }

    #[test]
    fn apply_zoom_matches_step_zoom() {
        for z in [ZOOM_MIN, 2.0, ZOOM_COARSE_FROM, 4.3, 10.0, ZOOM_MAX] {
            let mut by_action = AppState { zoom: z, ..AppState::default() };
            let mut by_step = by_action.clone();
            by_action.apply(Action::ZoomIn);
            by_step.step_zoom(1);
            assert_eq!(by_action.zoom, by_step.zoom, "in from {z}");

            let mut by_action = AppState { zoom: z, ..AppState::default() };
            let mut by_step = by_action.clone();
            by_action.apply(Action::ZoomOut);
            by_step.step_zoom(-1);
            assert_eq!(by_action.zoom, by_step.zoom, "out from {z}");
        }
    }

    #[test]
    fn apply_zoom_clamps_at_both_ends() {
        let mut s = AppState { zoom: ZOOM_MAX, ..AppState::default() };
        s.apply(Action::ZoomIn);
        assert_eq!(s.zoom, ZOOM_MAX);
        s.zoom = ZOOM_MIN;
        s.apply(Action::ZoomOut);
        assert_eq!(s.zoom, ZOOM_MIN);
    }

    #[test]
    fn apply_touches_only_its_own_field() {
        let before = AppState { enabled: false, zoom: 4.5, ..non_default() };

        let mut s = before.clone();
        s.apply(Action::Toggle);
        assert_eq!(s, AppState { enabled: true, ..before.clone() });

        for a in [Action::ZoomIn, Action::ZoomOut] {
            let mut s = before.clone();
            s.apply(a);
            assert_ne!(s.zoom, before.zoom, "{a:?}");
            assert_eq!(s, AppState { zoom: s.zoom, ..before.clone() }, "{a:?}");
        }
    }

    #[test]
    fn apply_toggle_speech_flips_only_tts_enabled() {
        let before = non_default();
        let mut s = before.clone();
        s.apply(Action::ToggleSpeech);
        assert_eq!(s, AppState { tts_enabled: !before.tts_enabled, ..before.clone() });
        s.apply(Action::ToggleSpeech);
        assert_eq!(s, before);
    }

    #[test]
    fn apply_silence_changes_nothing() {
        let before = non_default();
        let mut s = before.clone();
        s.apply(Action::Silence);
        assert_eq!(s, before);
    }

    #[test]
    fn screen_rect_contains_is_inclusive_left_top_exclusive_right_bottom() {
        let r = ScreenRect { left: -100, top: 50, width: 200, height: 10 };
        let p = |x, y| ScreenPoint { x, y };
        assert!(r.contains(p(-100, 50)));
        assert!(r.contains(p(99, 59)));
        assert!(r.contains(p(0, 55)));
        assert!(!r.contains(p(100, 55)));
        assert!(!r.contains(p(0, 60)));
        assert!(!r.contains(p(-101, 55)));
        assert!(!r.contains(p(0, 49)));
    }

    #[test]
    fn empty_screen_rect_contains_nothing() {
        let r = ScreenRect { left: 0, top: 0, width: 0, height: 0 };
        assert!(!r.contains(ScreenPoint { x: 0, y: 0 }));
    }

    #[test]
    fn screen_point_and_rect_display_for_logs() {
        assert_eq!(ScreenPoint { x: -5, y: 7 }.to_string(), "(-5, 7)");
        let r = ScreenRect { left: 812, top: -64, width: 1240, height: 32 };
        assert_eq!(r.to_string(), "(812, -64) 1240x32");
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

    #[test]
    fn pointer_look_compares_by_shape_and_by_arc() {
        let shape = PointerShape { width: 2, height: 1, hotspot: (0, 0), mask: vec![255, 0] };
        let a = Arc::new(shape.clone());
        let b = Arc::new(shape);
        assert_eq!(PointerLook::Shape(a.clone()), PointerLook::Shape(b.clone()));
        assert!(Arc::ptr_eq(&a, &a.clone()));
        assert!(!Arc::ptr_eq(&a, &b));
        assert_ne!(PointerLook::Hidden, PointerLook::Unknown);
    }
}
