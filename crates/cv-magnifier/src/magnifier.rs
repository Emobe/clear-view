//! The per-tick magnifier logic (ADR 0004). The backend owns the window, its thread and its
//! timer, and calls `Magnifier::tick` on each tick. The magnifier says what the window should
//! be (`Layout`); the backend makes it so.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc::Receiver,
    },
    time::Instant,
};

use cv_core::{
    AppState, ColorFilter, CoreEvent, DisplayMode, Edge, Following, Frame, FrameState,
    Interpolation, OutputInfo, PointerLook, PointerShape, ScreenPoint, ScreenRect, SharedState,
    Tracker,
    geometry::{self, panel_pct_to_px, window_dims},
    tracking,
};

use crate::{
    gfx::WgpuState,
    host::{Layout, OverlayHost},
    perf,
};

/// The magnifier for one overlay window: smoothing, the active monitor, the applied layout and
/// the GPU upload and uniform caches.
pub struct Magnifier {
    view: View<WgpuState>,
}

impl Magnifier {
    /// Creates the wgpu surface on `host`'s window, sized to `start`, the monitor the window
    /// starts on. `events` is this view's subscription to the core events (ADR 0008); the tick
    /// drains it.
    ///
    /// # Safety
    /// The windows behind `host.raw_handles()` and `host.pan_handles()` must stay valid until
    /// this `Magnifier` is dropped.
    pub unsafe fn new(
        host: &impl OverlayHost,
        start: OutputInfo,
        outputs: Vec<OutputInfo>,
        app_state: SharedState,
        frame_state: FrameState,
        desired_output: Arc<AtomicU32>,
        events: Receiver<CoreEvent>,
    ) -> Self {
        let (display, window) = host.raw_handles();
        let pan = host.pan_handles();
        // SAFETY: the caller keeps both windows valid for the life of `self`.
        let wgpu = unsafe {
            WgpuState::new(
                display, window, pan, start.width, start.height, start.width, start.height,
            )
        };
        Self {
            view: View::new(wgpu, start, outputs, app_state, frame_state, desired_output, events),
        }
    }

    /// One tick: drain the core events, pick the target (pointer or caret, by the tracking
    /// policy), apply any layout change through `host`, then draw.
    pub fn tick(&mut self, host: &mut impl OverlayHost, now: Instant) {
        let timer = perf::start();
        self.view.tick(host, now);
        perf::ticked(timer);
    }

    /// The set of monitors changed (plugged, unplugged, new resolution, another primary): the
    /// backend calls this between ticks with the list it just read (ADR 0009). Never call it
    /// from inside `OverlayHost::apply_layout`. A list equal to the current one does nothing.
    pub fn outputs_changed(&mut self, outputs: Vec<OutputInfo>) {
        self.view.outputs_changed(outputs);
    }
}

/// What the tick needs from the GPU. `WgpuState` in the app, a recording fake in tests.
pub(crate) trait Renderer {
    /// Size of the frame texture.
    fn frame_size(&self) -> (u32, u32);
    /// Reconfigures the surface for a new window size.
    fn resize(&mut self, width: u32, height: u32);
    /// Replaces the frame texture with one of a new size (monitor switch).
    fn recreate_frame_texture(&mut self, width: u32, height: u32);
    fn upload_frame(&mut self, data: &[u8], width: u32, height: u32);
    /// Replaces the pointer mask (one byte per pixel, `width * height` bytes) the shader draws
    /// for [`POINTER_SHAPE`]. Called only for a new shape.
    fn upload_pointer_mask(&mut self, data: &[u8], width: u32, height: u32);
    fn write_uniforms(&mut self, uniforms: &Uniforms);
    /// Draws and presents. False when the surface is lost or outdated.
    fn render(&mut self) -> bool;
    /// Whether the pan window has a surface (ADR 0011). Without one the magnifier never pans.
    fn pan_available(&self) -> bool;
    /// Reconfigures the pan surface for a new pan window size.
    fn resize_pan(&mut self, width: u32, height: u32);
    /// The pan surface's own uniforms; the frame and pointer mask are shared with the panel.
    fn write_pan_uniforms(&mut self, uniforms: &Uniforms);
    /// Draws and presents the pan window. False when its surface is lost or outdated.
    fn render_pan(&mut self) -> bool;
}

/// Everything the shader's uniform buffer holds, compared to skip redundant writes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Uniforms {
    /// `[x, y, w, h]` as fractions of the frame.
    pub crop: [f32; 4],
    pub color_mode: u32,
    pub interp_mode: u32,
    /// The pointer in output window pixels; [`CURSOR_HIDDEN`] on both draws nothing.
    pub cursor_x: u32,
    pub cursor_y: u32,
    pub edge_threshold: f32,
    pub pointer: PointerDraw,
}

/// How the shader draws the pointer (ADR 0010).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PointerDraw {
    /// [`POINTER_NONE`], [`POINTER_CIRCLE`] or [`POINTER_SHAPE`].
    pub kind: u32,
    /// Size of the mask in the pointer texture; zero unless `kind` is [`POINTER_SHAPE`].
    pub mask: (u32, u32),
    pub hotspot: (u32, u32),
    /// Output pixels per mask pixel.
    pub size: f32,
    /// Outline width in output pixels.
    pub outline: f32,
    /// Straight RGBA, 0–1.
    pub fill: [f32; 4],
    pub outline_colour: [f32; 4],
}

/// Draw nothing at the pointer.
pub(crate) const POINTER_NONE: u32 = 0;
/// The circle, for a pointer whose shape is unknown.
pub(crate) const POINTER_CIRCLE: u32 = 1;
/// The shape in the pointer mask texture.
pub(crate) const POINTER_SHAPE: u32 = 2;

/// Largest mask side the magnifier uploads. Real cursors are far smaller; a bigger answer is
/// treated as unreadable (the circle) rather than risking a texture the device refuses.
const MAX_MASK_SIDE: u32 = 512;

impl PointerDraw {
    /// A marker with no mask: `kind` is [`POINTER_NONE`] or [`POINTER_CIRCLE`]. The circle has
    /// a fixed look, so size, outline and colours are zero: changing a pointer setting while
    /// it shows must not change the uniforms.
    pub(crate) fn marker(kind: u32) -> Self {
        Self {
            kind,
            mask: (0, 0),
            hotspot: (0, 0),
            size: 0.0,
            outline: 0.0,
            fill: [0.0; 4],
            outline_colour: [0.0; 4],
        }
    }
}

/// An sRGB byte colour as the opaque straight RGBA the shader takes.
fn rgba(rgb: [u8; 3]) -> [f32; 4] {
    [rgb[0] as f32 / 255.0, rgb[1] as f32 / 255.0, rgb[2] as f32 / 255.0, 1.0]
}

/// Whether the mask can be drawn: a size the device takes and as many bytes as pixels.
fn usable(shape: &PointerShape) -> bool {
    let (w, h) = (shape.width, shape.height);
    (1..=MAX_MASK_SIDE).contains(&w)
        && (1..=MAX_MASK_SIDE).contains(&h)
        && shape.mask.len() == w as usize * h as usize
}

/// Cursor position that draws nothing: the pointer is outside the magnified area. Far outside
/// any window, and the shader tests for it.
pub(crate) const CURSOR_HIDDEN: u32 = u32::MAX;

/// The margin of the pan rule (ADR 0011), as a fraction of the uncovered dimension: how far
/// short of the panel the pointer starts to push the image. Tuned by hand (roadmap 7.3).
const PAN_MARGIN: f32 = 0.1;

/// The pan offset's ease rate: the fraction of the remaining distance covered per 60 Hz tick,
/// through `geometry::smooth_alpha`. Its own constant, not `smooth_speed`, which users set for
/// the magnified view. Tuned by hand (roadmap 7.3).
const PAN_EASE: f32 = 0.4;

/// A docked panel as applied: the monitor, edge and thickness the pan is computed from.
#[derive(Debug, Clone)]
struct Dock {
    monitor: OutputInfo,
    edge: Edge,
    thickness: u32,
}

impl Dock {
    /// The part of the monitor the panel does not cover, which the pan window sits on; `None`
    /// when the panel covers the monitor.
    fn uncovered(&self) -> Option<ScreenRect> {
        let rect = geometry::uncovered_rect(&self.monitor, self.edge, self.thickness);
        (rect.width > 0 && rect.height > 0).then_some(rect)
    }
}

/// What the pan window draws at a whole-pixel `offset` (ADR 0011): the frame at 1x, shifted away
/// from the panel by `shift`, so the strip the panel covers comes into view.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PanView {
    /// The pan window's size, the uncovered rect's.
    window: (u32, u32),
    /// The frame pixel (monitor-local) at the window's top-left corner.
    src: (i32, i32),
    /// The window's top-left corner, monitor-local.
    origin: (i32, i32),
    /// How far the image moves on screen: down for a top panel, up for a bottom one, right for
    /// a left one, left for a right one.
    shift: (i32, i32),
}

impl PanView {
    /// `None` when the panel covers the whole monitor.
    fn new(dock: &Dock, offset: u32) -> Option<Self> {
        let rect = dock.uncovered()?;
        let origin = (rect.left - dock.monitor.left, rect.top - dock.monitor.top);
        let d = offset as i32;
        let shift = match dock.edge {
            Edge::Top => (0, d),
            Edge::Bottom => (0, -d),
            Edge::Left => (d, 0),
            Edge::Right => (-d, 0),
        };
        Some(Self {
            window: (rect.width, rect.height),
            src: (origin.0 - shift.0, origin.1 - shift.1),
            origin,
            shift,
        })
    }

    /// The pointer in pan window pixels, drawn where the content under it now is, or `None`
    /// when that is outside the window (under the panel, for a tick while the offset eases).
    fn pointer(&self, pointer: ScreenPoint, monitor: &OutputInfo) -> Option<(u32, u32)> {
        let x = pointer.x - monitor.left - self.origin.0 + self.shift.0;
        let y = pointer.y - monitor.top - self.origin.1 + self.shift.1;
        let inside = (0..self.window.0 as i32).contains(&x) && (0..self.window.1 as i32).contains(&y);
        inside.then_some((x as u32, y as u32))
    }
}

/// What was last applied to the window, as the magnifier decides when to apply again.
/// Docked names the monitor the panel is on: the primary, or with `panel_follows_monitor` the
/// active one, so only then does a monitor switch move the panel (roadmap 5.2).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Applied {
    Hidden,
    /// The monitors changed, so whatever the window is must be applied again (ADR 0009). Never
    /// a wanted state: the next tick always differs from it, including when disabled.
    Stale,
    Fullscreen { output: u32 },
    Docked { edge: Edge, panel_pct: u32, monitor: u32 },
}

struct View<R> {
    renderer: R,
    app_state: SharedState,
    frame_state: FrameState,
    /// All monitors (virtual-screen coordinates): at startup, then as the backend reports changes
    /// through `outputs_changed`.
    outputs: Vec<OutputInfo>,
    /// The monitor being magnified.
    active: OutputInfo,
    /// Where the docked panel sits unless it follows the target (roadmap 5.2): the output at
    /// (0, 0), or the start monitor if none is.
    primary: OutputInfo,
    /// Written here on a monitor switch, read by the capture loop.
    desired_output: Arc<AtomicU32>,
    /// Last pointer position read. Kept when a read fails (lock screen, UAC prompt), so the
    /// view stays put instead of drifting to (0, 0) (STATUS Finding 15).
    pointer: ScreenPoint,
    /// This view's core event subscription, drained every tick (ADR 0008).
    events: Receiver<CoreEvent>,
    /// The tracking policy (roadmap 4.5). Runs while enabled, in fullscreen and docked, and
    /// keeps its state across a mode switch; reset while disabled, so the view starts on the
    /// pointer when it comes back.
    tracker: Tracker,
    /// The virtual-screen point the view eases toward: the pointer, or the caret.
    target: (f32, f32),
    smooth_x: f32,
    smooth_y: f32,
    /// Time of the last enabled tick; smoothing measures `dt` from it.
    last_tick: Instant,
    applied: Applied,
    /// The docked panel's rect as last applied, `None` unless docked. Kept from the layout
    /// rather than recomputed, because a monitor switch while docked need not move the panel.
    panel: Option<ScreenRect>,
    /// The docked panel as last applied, `None` unless docked. `panel` is its rect.
    dock: Option<Dock>,
    /// True while `set_system_cursor(false)` is in force because the pointer is inside the
    /// panel or the pan window shows. `apply_layout` resets the cursor, so this is cleared with
    /// every layout change.
    cursor_hidden: bool,
    /// The pan offset the view has eased to, in pixels (ADR 0011); the window draws it rounded.
    pan_offset: f32,
    /// True while the pan window is shown. `apply_layout` hides it, so this is cleared with
    /// every layout change.
    pan_shown: bool,
    last_pan_uniforms: Option<Uniforms>,
    last_frame: Option<Arc<Frame>>,
    last_uniforms: Option<Uniforms>,
    /// What the pointer looked like at the last enabled tick (ADR 0010). `Unknown` until the
    /// first read, so the circle shows.
    look: PointerLook,
    /// The shape whose mask is in the renderer's pointer texture.
    last_mask: Option<Arc<PointerShape>>,
}

impl<R: Renderer> View<R> {
    fn new(
        renderer: R,
        start: OutputInfo,
        outputs: Vec<OutputInfo>,
        app_state: SharedState,
        frame_state: FrameState,
        desired_output: Arc<AtomicU32>,
        events: Receiver<CoreEvent>,
    ) -> Self {
        let centre = ScreenPoint {
            x: start.left + start.width as i32 / 2,
            y: start.top + start.height as i32 / 2,
        };
        let smooth_x = start.width as f32 / 2.0 + start.left as f32;
        let smooth_y = start.height as f32 / 2.0 + start.top as f32;
        let primary = geometry::primary_output(&outputs).cloned().unwrap_or_else(|| start.clone());
        Self {
            primary,
            renderer,
            app_state,
            frame_state,
            outputs,
            smooth_x,
            smooth_y,
            active: start,
            desired_output,
            pointer: centre,
            events,
            tracker: Tracker::new(),
            target: (smooth_x, smooth_y),
            last_tick: Instant::now(),
            applied: Applied::Hidden,
            panel: None,
            dock: None,
            cursor_hidden: false,
            pan_offset: 0.0,
            pan_shown: false,
            last_pan_uniforms: None,
            last_frame: None,
            last_uniforms: None,
            look: PointerLook::Unknown,
            last_mask: None,
        }
    }

    fn tick(&mut self, host: &mut impl OverlayHost, now: Instant) {
        // Every tick, enabled or not, so the channel never grows (ADR 0008).
        let events: Vec<CoreEvent> = self.events.try_iter().collect();
        let state = self.app_state.read().clone();
        let Ok(frame) = self.frame_state.lock().map(|f| f.clone()) else {
            return;
        };

        if state.enabled {
            if let Some(p) = host.position() {
                self.pointer = p;
            }
            // Read every enabled tick so the host notices a shape change; `draw` shows it.
            self.look = host.look();
            self.target = self.track(&state, now, events);
            self.follow_target();
        } else {
            self.tracker = Tracker::new();
        }

        let want = match (state.enabled, state.display_mode) {
            (false, _) => Applied::Hidden,
            (true, DisplayMode::Fullscreen) => Applied::Fullscreen { output: self.active.idx },
            (true, DisplayMode::Docked(edge)) => Applied::Docked {
                edge,
                panel_pct: state.panel_size,
                monitor: self.panel_monitor(&state).idx,
            },
        };
        if want != self.applied {
            let layout = self.layout_for(want, &state);
            let (w, h) = host.apply_layout(&layout);
            if want != Applied::Hidden {
                self.renderer.resize(w, h);
            }
            self.applied = want;
            self.dock = match &layout {
                Layout::Docked { monitor, edge, thickness } => {
                    Some(Dock { monitor: monitor.clone(), edge: *edge, thickness: *thickness })
                }
                Layout::Hidden | Layout::Fullscreen { .. } => None,
            };
            self.panel = self
                .dock
                .as_ref()
                .map(|d| geometry::docked_rect(&d.monitor, d.edge, d.thickness));
            // `apply_layout` hid the pan window and showed the system cursor (fullscreen
            // excepted, which has no pan); the pan starts again from nothing.
            self.cursor_hidden = false;
            self.pan_shown = false;
            self.pan_offset = 0.0;
            self.last_pan_uniforms = None;
            if let Some(uncovered) = self.dock.as_ref().and_then(Dock::uncovered) {
                self.renderer.resize_pan(uncovered.width, uncovered.height);
            }
        }

        if state.enabled {
            self.draw(host, &state, frame, now);
            self.update_system_cursor(host);
        }
    }

    /// While docked, hides the system cursor when the pointer is inside the panel (ADR 0007) or
    /// the pan window shows (ADR 0011), and shows it when neither holds; the pointer is drawn
    /// in the magnified view and in the pan window. Calls the host only on a change. Fullscreen
    /// hiding is `apply_layout`'s job.
    fn update_system_cursor(&mut self, host: &mut impl OverlayHost) {
        let Some(panel) = self.panel else {
            return;
        };
        let hidden = panel.contains(self.pointer) || self.pan_shown;
        if hidden != self.cursor_hidden {
            host.set_system_cursor(!hidden);
            self.cursor_hidden = hidden;
        }
    }

    /// The point the view eases toward this tick: the tracking policy over the pointer and
    /// `events`, in both modes (roadmap 4.6, 4.7). The view is the window divided by the zoom,
    /// the same size `draw` crops, so a docked panel keeps the caret inside its own middle area.
    fn track(&mut self, state: &AppState, now: Instant, events: Vec<CoreEvent>) -> (f32, f32) {
        let (win_w, win_h) = self.window_size(state);
        let view = (win_w as f32 / state.zoom, win_h as f32 / state.zoom);
        if !tracking::log_enabled() {
            return self.tracker.update(now, self.pointer, events, view);
        }
        // The `CV_TRACK_LOG` diagnostic (roadmap 4.8a): every event drained and every decision.
        let before = self.snap(self.target);
        let lines: Vec<String> = events.iter().map(CoreEvent::describe).collect();
        let target = self.tracker.update(now, self.pointer, events, view);
        // The tracker reads the pointer before the events, so this is what they were judged by.
        let pointer = if self.tracker.pointer_still(now) { "still" } else { "moving" };
        for line in lines {
            eprintln!(
                "[track] event {line}; pointer {} {pointer}, was following {:?}",
                self.pointer, before.following
            );
        }
        for line in track_changes(before, self.snap(target), now, self.pointer, view) {
            eprintln!("{line}");
        }
        target
    }

    fn snap(&self, target: (f32, f32)) -> TrackSnap {
        TrackSnap {
            following: self.tracker.following(),
            pending: self.tracker.pending_since(),
            target,
        }
    }

    /// The window size the view is drawn into: the active monitor in fullscreen, or the docked
    /// panel on the monitor it sits on, which need not be the one magnified (STATUS Finding 18).
    /// `layout_for` places the panel on the same monitor, so the two always agree.
    fn window_size(&self, state: &AppState) -> (u32, u32) {
        let monitor = match state.display_mode {
            DisplayMode::Fullscreen => &self.active,
            DisplayMode::Docked(_) => self.panel_monitor(state),
        };
        window_dims(
            state.display_mode,
            state.panel_size,
            monitor.width as i32,
            monitor.height as i32,
        )
    }

    /// The monitor the docked panel sits on (roadmap 5.2): the primary, or the one being
    /// magnified when the panel follows the target.
    fn panel_monitor(&self, state: &AppState) -> &OutputInfo {
        if state.panel_follows_monitor { &self.active } else { &self.primary }
    }

    /// Switches the active monitor to the one under the target, if that changed. A target off
    /// every monitor keeps the current one.
    fn follow_target(&mut self) {
        let (x, y) = (self.target.0.floor() as i32, self.target.1.floor() as i32);
        let Some(output) = geometry::output_at(&self.outputs, x, y) else {
            return;
        };
        if output.idx == self.active.idx {
            return;
        }
        let output = output.clone();
        self.desired_output.store(output.idx, Ordering::Relaxed);
        self.renderer.recreate_frame_texture(output.width, output.height);
        self.last_frame = None; // the new texture needs the frame again
        self.active = output;
    }

    /// Takes a new monitor list (ADR 0009). The active monitor is the one under the target; if
    /// the target is on none (its monitor was removed) the view jumps to the primary's centre.
    /// Capture is told the active index even when it is unchanged, the frame texture is made
    /// again for the active size (this also covers a resolution change that keeps the index),
    /// and the layout is applied again on the next tick if the active or primary monitor changed. A caret position stays a screen point,
    /// so the tracker is kept unless the target was lost.
    fn outputs_changed(&mut self, outputs: Vec<OutputInfo>) {
        if outputs.is_empty() || outputs == self.outputs {
            return;
        }
        let primary = geometry::primary_output(&outputs)
            .or(outputs.first())
            .cloned()
            .expect("the list is not empty");
        self.outputs = outputs;

        let (x, y) = (self.target.0.floor() as i32, self.target.1.floor() as i32);
        let active = match geometry::output_at(&self.outputs, x, y) {
            Some(output) => output.clone(),
            None => {
                let output = self.primary.clone();
                let centre = (
                    output.left as f32 + output.width as f32 / 2.0,
                    output.top as f32 + output.height as f32 / 2.0,
                );
                self.target = centre;
                self.smooth_x = centre.0;
                self.smooth_y = centre.1;
                // A caret kept from the removed monitor would pull the view back off screen.
                self.tracker = Tracker::new();
                output
            }
        };
        self.desired_output.store(active.idx, Ordering::Relaxed);
        self.renderer.recreate_frame_texture(active.width, active.height);
        self.last_frame = None; // the new texture needs the frame again
        // The window only needs placing again if a monitor it is placed on moved or changed
        // size. `Applied` names indices, which cannot tell.
        let moved = active != self.active || primary != self.primary;
        self.active = active;
        self.primary = primary;
        if moved {
            self.applied = Applied::Stale;
        }
    }

    fn layout_for(&self, applied: Applied, state: &AppState) -> Layout {
        match applied {
            Applied::Hidden | Applied::Stale => Layout::Hidden,
            Applied::Fullscreen { .. } => Layout::Fullscreen { monitor: self.active.clone() },
            Applied::Docked { edge, panel_pct, .. } => {
                let monitor = self.panel_monitor(state).clone();
                let dim = match edge {
                    Edge::Top | Edge::Bottom => monitor.height,
                    Edge::Left | Edge::Right => monitor.width,
                };
                let thickness = panel_pct_to_px(panel_pct, dim as i32) as u32;
                Layout::Docked { monitor, edge, thickness }
            }
        }
    }

    /// Lerp, crop, upload, uniforms, present.
    fn draw(
        &mut self,
        host: &mut impl OverlayHost,
        state: &AppState,
        frame: Option<Arc<Frame>>,
        now: Instant,
    ) {
        // Frame-rate-independent lerp toward the target, which also smooths a switch between
        // the pointer and the caret.
        let dt = now.saturating_duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;
        let alpha = geometry::smooth_alpha(state.smooth_speed, dt);
        self.smooth_x = geometry::lerp_toward(self.smooth_x, self.target.0, alpha);
        self.smooth_y = geometry::lerp_toward(self.smooth_y, self.target.1, alpha);

        // Monitor-local coordinates: the DXGI frame origin is the monitor's top-left.
        let cx = geometry::to_monitor_local(self.smooth_x, self.active.left);
        let cy = geometry::to_monitor_local(self.smooth_y, self.active.top);

        let (win_w, win_h) = self.window_size(state);
        let (fw, fh) = self.renderer.frame_size();
        let crop = geometry::compute_crop(cx, cy, win_w, win_h, state.zoom, fw as f32, fh as f32);

        // Upload only when the capture thread has produced a new Arc<Frame>.
        if let Some(frame) = &frame {
            let new_frame = self.last_frame.as_ref().is_none_or(|last| !Arc::ptr_eq(last, frame));
            if new_frame {
                let timer = perf::start();
                self.renderer.upload_frame(&frame.data, frame.width, frame.height);
                perf::uploaded(timer);
                self.last_frame = Some(Arc::clone(frame));
            }
        }

        // The pointer marks the real pointer, wherever the view is; none when it is out of view.
        let px = geometry::to_monitor_local(self.pointer.x as f32, self.active.left);
        let py = geometry::to_monitor_local(self.pointer.y as f32, self.active.top);
        let (cursor_x, cursor_y) = geometry::pointer_in_output(px, py, &crop, win_w, win_h)
            .unwrap_or((CURSOR_HIDDEN, CURSOR_HIDDEN));
        let pointer = self.pointer_draw(state);
        let uniforms = Uniforms {
            crop: crop.normalized(),
            color_mode: state.color_filter.as_u32(),
            interp_mode: state.interpolation.as_u32(),
            cursor_x,
            cursor_y,
            edge_threshold: state.edge_threshold,
            pointer,
        };
        if self.last_uniforms != Some(uniforms) {
            self.renderer.write_uniforms(&uniforms);
            self.last_uniforms = Some(uniforms);
        }

        if self.renderer.render() {
            perf::rendered();
        } else {
            // Surface lost or outdated: reconfigure to recover.
            self.renderer.resize(win_w, win_h);
        }

        self.update_pan(host, dt, pointer);
    }

    /// The offset the pan is easing toward, in pixels (ADR 0011), or `None` when it must not pan:
    /// not docked, nothing uncovered, no pan surface, no frame yet, or the captured monitor is
    /// not the one the panel is on (a caret on another monitor, or the panel staying on the
    /// primary), so the frame texture does not hold what lies under the pan window.
    fn pan_target(&self) -> Option<f32> {
        let dock = self.dock.as_ref()?;
        let uncovered = dock.uncovered()?;
        let monitor = &dock.monitor;
        if !self.renderer.pan_available()
            || self.last_frame.is_none()
            || monitor.idx != self.active.idx
            || self.renderer.frame_size() != (monitor.width, monitor.height)
        {
            return None;
        }
        let across = match dock.edge {
            Edge::Top | Edge::Bottom => uncovered.height,
            Edge::Left | Edge::Right => uncovered.width,
        };
        let margin = PAN_MARGIN * across as f32;
        Some(geometry::pan_offset(self.pointer, monitor, dock.edge, dock.thickness, margin))
    }

    /// Eases the pan offset toward the real pointer's target, draws the pan window and shows or
    /// hides it (ADR 0011). It shows while the whole-pixel offset is at least one; the surface is
    /// drawn before the window is first shown, so no stale frame appears. `panel_pointer` is the
    /// pointer as the panel draws it; the pan window draws the same shape at 1x.
    fn update_pan(&mut self, host: &mut impl OverlayHost, dt: f32, panel_pointer: PointerDraw) {
        let dock = self.dock.clone();
        let (Some(target), Some(dock)) = (self.pan_target(), dock) else {
            // Must not pan: hide at once and start the next pan from nothing.
            self.pan_offset = 0.0;
            self.hide_pan(host);
            return;
        };
        if (target - self.pan_offset).abs() < 0.5 {
            self.pan_offset = target;
        } else {
            let alpha = geometry::smooth_alpha(PAN_EASE, dt);
            self.pan_offset = geometry::lerp_toward(self.pan_offset, target, alpha);
        }
        let offset = self.pan_offset.round() as u32;
        let Some(view) = PanView::new(&dock, offset).filter(|_| offset > 0) else {
            // Rounds to nothing. The eased value is kept, so a small target can still grow.
            self.hide_pan(host);
            return;
        };

        // The frame at 1x, shifted by the offset, with no colour filter: the unmagnified area is
        // never filtered. The pointer is the real one plus the offset.
        let (fw, fh) = self.renderer.frame_size();
        let (fw, fh) = (fw as f32, fh as f32);
        let (cursor_x, cursor_y) = view
            .pointer(self.pointer, &dock.monitor)
            .unwrap_or((CURSOR_HIDDEN, CURSOR_HIDDEN));
        let pointer = match panel_pointer.kind {
            POINTER_SHAPE => PointerDraw { size: 1.0, ..panel_pointer },
            _ => panel_pointer,
        };
        let uniforms = Uniforms {
            crop: [
                view.src.0 as f32 / fw,
                view.src.1 as f32 / fh,
                view.window.0 as f32 / fw,
                view.window.1 as f32 / fh,
            ],
            color_mode: ColorFilter::None.as_u32(),
            interp_mode: Interpolation::Bilinear.as_u32(),
            cursor_x,
            cursor_y,
            edge_threshold: 0.0,
            pointer,
        };
        if self.last_pan_uniforms != Some(uniforms) {
            self.renderer.write_pan_uniforms(&uniforms);
            self.last_pan_uniforms = Some(uniforms);
        }
        if !self.renderer.render_pan() {
            // Surface lost or outdated: reconfigure to recover; not shown until it draws.
            self.renderer.resize_pan(view.window.0, view.window.1);
            return;
        }
        if !self.pan_shown {
            host.set_pan_visible(true);
            self.pan_shown = true;
        }
    }

    /// Hides the pan window if it is shown.
    fn hide_pan(&mut self, host: &mut impl OverlayHost) {
        if self.pan_shown {
            host.set_pan_visible(false);
            self.pan_shown = false;
        }
    }

    /// How to draw the pointer for the current look (ADR 0010): nothing when hidden, the circle
    /// when the shape is unknown or unusable or the `pointer_shape` setting is off, else the
    /// shape with the size, outline and colours of `state`. A mask is uploaded only when the
    /// shape is a new `Arc`, and never while the setting is off.
    fn pointer_draw(&mut self, state: &AppState) -> PointerDraw {
        match self.look.clone() {
            PointerLook::Hidden => PointerDraw::marker(POINTER_NONE),
            PointerLook::Shape(shape) if state.pointer_shape && usable(&shape) => {
                let new = self.last_mask.as_ref().is_none_or(|last| !Arc::ptr_eq(last, &shape));
                if new {
                    self.renderer.upload_pointer_mask(&shape.mask, shape.width, shape.height);
                    self.last_mask = Some(Arc::clone(&shape));
                }
                PointerDraw {
                    kind: POINTER_SHAPE,
                    mask: (shape.width, shape.height),
                    hotspot: shape.hotspot,
                    size: state.pointer_size,
                    outline: state.pointer_outline,
                    fill: rgba(state.pointer_fill),
                    outline_colour: rgba(state.pointer_outline_colour),
                }
            }
            PointerLook::Unknown | PointerLook::Shape(_) => PointerDraw::marker(POINTER_CIRCLE),
        }
    }
}

/// The tracker before or after one update, for the `CV_TRACK_LOG` lines.
#[derive(Debug, Clone, Copy)]
struct TrackSnap {
    following: Following,
    /// When the held `FocusRect` arrived, if one is held.
    pending: Option<Instant>,
    target: (f32, f32),
}

/// The `[track]` lines for what one update decided: a focus rect held, replaced, applied or
/// dropped, and a switch between pointer and caret or a new caret target.
fn track_changes(
    before: TrackSnap,
    after: TrackSnap,
    now: Instant,
    pointer: ScreenPoint,
    view: (f32, f32),
) -> Vec<String> {
    let ms = |since: Instant| now.saturating_duration_since(since).as_millis();
    let mut lines = Vec::new();
    let became_caret = before.following == Following::Pointer && after.following == Following::Caret;
    match (before.pending, after.pending) {
        (None, Some(_)) => lines.push("[track] focus rect held (pointer moving)".to_string()),
        (Some(a), Some(b)) if a != b => lines.push("[track] held focus rect replaced".to_string()),
        (Some(a), None) if became_caret => {
            lines.push(format!("[track] held focus rect applied after {} ms", ms(a)));
        }
        (Some(a), None) => {
            lines.push(format!("[track] held focus rect dropped after {} ms, pointer {pointer}", ms(a)));
        }
        _ => {}
    }
    let (x, y) = after.target;
    match (before.following, after.following) {
        (Following::Pointer, Following::Caret) => lines.push(format!(
            "[track] pointer -> caret, target ({x:.0}, {y:.0}), view {:.0}x{:.0}, pointer {pointer}",
            view.0, view.1
        )),
        (Following::Caret, Following::Pointer) => {
            lines.push(format!("[track] caret -> pointer at {pointer}"));
        }
        (Following::Caret, Following::Caret) if before.target != after.target => {
            lines.push(format!("[track] caret target ({x:.0}, {y:.0})"));
        }
        _ => {}
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_core::{
        AppId, CaretSource, ColorFilter, Interpolation, PointerLook, PointerShape, PointerSource,
    };
    use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
    use std::{
        sync::{
            Mutex,
            mpsc::{Sender, channel},
        },
        time::Duration,
    };

    #[derive(Debug, Clone, PartialEq)]
    enum Gpu {
        Resize(u32, u32),
        Recreate(u32, u32),
        Upload(u32, u32),
        /// A pointer mask of this size.
        Mask(u32, u32),
        Uniforms(Uniforms),
        Render,
    }

    struct FakeRenderer {
        frame: (u32, u32),
        render_ok: bool,
        calls: Vec<Gpu>,
        /// Whether there is a pan surface.
        pan_available: bool,
        pan_render_ok: bool,
        /// The pan surface's calls, apart from `calls` so the panel's tests stay exact.
        pan_calls: Vec<PanGpu>,
        /// `render_pan` and `set_pan_visible`, in the order they happened.
        trace: Trace,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum PanGpu {
        Resize(u32, u32),
        Uniforms(Uniforms),
        Render,
    }

    type Trace = Arc<Mutex<Vec<&'static str>>>;

    impl Renderer for FakeRenderer {
        fn frame_size(&self) -> (u32, u32) {
            self.frame
        }
        fn resize(&mut self, width: u32, height: u32) {
            self.calls.push(Gpu::Resize(width, height));
        }
        fn recreate_frame_texture(&mut self, width: u32, height: u32) {
            self.frame = (width, height);
            self.calls.push(Gpu::Recreate(width, height));
        }
        fn upload_frame(&mut self, _data: &[u8], width: u32, height: u32) {
            self.calls.push(Gpu::Upload(width, height));
        }
        fn upload_pointer_mask(&mut self, data: &[u8], width: u32, height: u32) {
            assert_eq!(data.len(), (width * height) as usize);
            self.calls.push(Gpu::Mask(width, height));
        }
        fn write_uniforms(&mut self, uniforms: &Uniforms) {
            self.calls.push(Gpu::Uniforms(*uniforms));
        }
        fn render(&mut self) -> bool {
            self.calls.push(Gpu::Render);
            self.render_ok
        }
        fn pan_available(&self) -> bool {
            self.pan_available
        }
        fn resize_pan(&mut self, width: u32, height: u32) {
            self.pan_calls.push(PanGpu::Resize(width, height));
        }
        fn write_pan_uniforms(&mut self, uniforms: &Uniforms) {
            self.pan_calls.push(PanGpu::Uniforms(*uniforms));
        }
        fn render_pan(&mut self) -> bool {
            self.pan_calls.push(PanGpu::Render);
            self.trace.lock().unwrap().push("render");
            self.pan_render_ok
        }
    }

    /// Records each layout and system cursor call, and answers with the size the window would
    /// get.
    struct FakeHost {
        pointer: Option<ScreenPoint>,
        /// What `look` answers.
        look: PointerLook,
        /// How many times `look` was called.
        looks: u32,
        layouts: Vec<Layout>,
        cursor: Vec<bool>,
        /// Each `set_pan_visible` call.
        pan: Vec<bool>,
        trace: Trace,
    }

    impl PointerSource for FakeHost {
        fn position(&self) -> Option<ScreenPoint> {
            self.pointer
        }

        fn look(&mut self) -> PointerLook {
            self.looks += 1;
            self.look.clone()
        }
    }

    impl OverlayHost for FakeHost {
        fn apply_layout(&mut self, layout: &Layout) -> (u32, u32) {
            self.layouts.push(layout.clone());
            match layout {
                Layout::Hidden => (0, 0),
                Layout::Fullscreen { monitor } => (monitor.width, monitor.height),
                Layout::Docked { monitor, edge: Edge::Top | Edge::Bottom, thickness } => {
                    (monitor.width, *thickness)
                }
                Layout::Docked { monitor, edge: Edge::Left | Edge::Right, thickness } => {
                    (*thickness, monitor.height)
                }
            }
        }

        fn set_system_cursor(&mut self, visible: bool) {
            self.cursor.push(visible);
        }

        fn set_pan_visible(&mut self, visible: bool) {
            self.pan.push(visible);
            self.trace.lock().unwrap().push(if visible { "show" } else { "hide" });
        }

        fn raw_handles(&self) -> (RawDisplayHandle, RawWindowHandle) {
            unreachable!("View never creates a surface")
        }

        fn pan_handles(&self) -> (RawDisplayHandle, RawWindowHandle) {
            unreachable!("View never creates a surface")
        }
    }

    fn primary() -> OutputInfo {
        OutputInfo { idx: 0, left: 0, top: 0, width: 1920, height: 1080 }
    }

    /// To the right of the primary, a different size.
    fn second() -> OutputInfo {
        OutputInfo { idx: 1, left: 1920, top: 0, width: 1280, height: 1024 }
    }

    const TICK: Duration = Duration::from_millis(16);

    struct Rig {
        view: View<FakeRenderer>,
        host: FakeHost,
        state: SharedState,
        frames: FrameState,
        desired: Arc<AtomicU32>,
        /// The core event channel the view drains.
        events: Sender<CoreEvent>,
        now: Instant,
    }

    impl Rig {
        /// Starts on the primary, with the second monitor to its right.
        fn new() -> Self {
            Self::with(primary(), vec![primary(), second()])
        }

        fn with(start: OutputInfo, outputs: Vec<OutputInfo>) -> Self {
            let state = cv_core::new_shared();
            let frames: FrameState = Arc::new(Mutex::new(None));
            let desired = Arc::new(AtomicU32::new(start.idx));
            let (events, rx) = channel();
            let frame = (start.width, start.height);
            let trace: Trace = Arc::default();
            let renderer = FakeRenderer {
                frame,
                render_ok: true,
                calls: Vec::new(),
                pan_available: true,
                pan_render_ok: true,
                pan_calls: Vec::new(),
                trace: trace.clone(),
            };
            let view = View::new(
                renderer,
                start,
                outputs,
                state.clone(),
                frames.clone(),
                desired.clone(),
                rx,
            );
            // Test time starts at the view's own start, so the first tick's `dt` is one TICK.
            let now = view.last_tick;
            let host = FakeHost {
                pointer: Some(ScreenPoint { x: 960, y: 540 }),
                look: PointerLook::Unknown,
                looks: 0,
                layouts: Vec::new(),
                cursor: Vec::new(),
                pan: Vec::new(),
                trace,
            };
            Self { view, host, state, frames, desired, events, now }
        }

        /// Sends a 2x20 msaa caret centred on (x, y), drained by the next tick.
        fn caret_at(&self, x: i32, y: i32) {
            let event = CoreEvent::CaretMoved {
                at: self.now,
                rect: ScreenRect { left: x - 1, top: y - 10, width: 2, height: 20 },
                source: CaretSource::Msaa,
                app: AppId { pid: 1, exe: "notepad.exe".into() },
            };
            self.events.send(event).unwrap();
        }

        /// True when the view has drained every event sent.
        fn events_drained(&self) -> bool {
            self.view.events.try_recv().is_err()
        }

        fn smooth(&self) -> (f32, f32) {
            (self.view.smooth_x, self.view.smooth_y)
        }

        /// The cursor in the last uniform write of `gpu`.
        fn cursor_written(gpu: &[Gpu]) -> Option<(u32, u32)> {
            gpu.iter().rev().find_map(|c| match c {
                Gpu::Uniforms(u) => Some((u.cursor_x, u.cursor_y)),
                _ => None,
            })
        }

        fn set(&self, f: impl FnOnce(&mut AppState)) {
            f(&mut self.state.write());
        }

        fn point_at(&mut self, x: i32, y: i32) {
            self.host.pointer = Some(ScreenPoint { x, y });
        }

        fn tick(&mut self) {
            self.now += TICK;
            self.view.tick(&mut self.host, self.now);
        }

        /// Ticks and returns the layouts applied and the GPU calls made during it.
        fn step(&mut self) -> (Vec<Layout>, Vec<Gpu>) {
            self.tick();
            (std::mem::take(&mut self.host.layouts), std::mem::take(&mut self.view.renderer.calls))
        }

        /// Ticks and returns the system cursor calls made during it.
        fn cursor_step(&mut self) -> Vec<bool> {
            self.tick();
            std::mem::take(&mut self.host.cursor)
        }

        /// Docked on the top edge at 25% (1920x270 on the primary), enabled, one tick taken.
        fn docked_top(&mut self) {
            self.docked_on(Edge::Top);
        }

        /// Docked on `edge` at 25% of the primary, enabled, one tick taken.
        fn docked_on(&mut self, edge: Edge) {
            self.set(|s| {
                s.display_mode = DisplayMode::Docked(edge);
                s.panel_size = 25;
                s.enabled = true;
            });
            self.tick();
            self.host.layouts.clear();
            self.host.cursor.clear();
        }

        fn publish_frame(&self) -> Arc<Frame> {
            let (width, height) = self.view.renderer.frame;
            let frame = Arc::new(Frame { width, height, data: Vec::new() });
            *self.frames.lock().unwrap() = Some(Arc::clone(&frame));
            frame
        }
    }

    fn docked(monitor: OutputInfo, edge: Edge, thickness: u32) -> Layout {
        Layout::Docked { monitor, edge, thickness }
    }

    fn count(calls: &[Gpu], f: impl Fn(&Gpu) -> bool) -> usize {
        calls.iter().filter(|c| f(c)).count()
    }

    fn uniform_writes(calls: &[Gpu]) -> usize {
        count(calls, |c| matches!(c, Gpu::Uniforms(_)))
    }

    #[test]
    fn starts_hidden_and_does_nothing_while_disabled() {
        let mut rig = Rig::new();
        for _ in 0..3 {
            assert_eq!(rig.step(), (vec![], vec![]));
        }
    }

    #[test]
    fn enabling_fullscreen_shows_the_monitor_and_draws() {
        let mut rig = Rig::new();
        rig.set(|s| s.enabled = true);
        let (layouts, gpu) = rig.step();
        assert_eq!(layouts, [Layout::Fullscreen { monitor: primary() }]);
        assert_eq!(gpu.first(), Some(&Gpu::Resize(1920, 1080)));
        assert_eq!(gpu.last(), Some(&Gpu::Render));

        // Nothing changed: no layout, no resize, still draws.
        let (layouts, gpu) = rig.step();
        assert!(layouts.is_empty());
        assert_eq!(gpu, [Gpu::Render]);
    }

    #[test]
    fn enabling_docked_reserves_the_panel() {
        let mut rig = Rig::new();
        rig.set(|s| {
            s.display_mode = DisplayMode::Docked(Edge::Top);
            s.panel_size = 50;
            s.enabled = true;
        });
        let (layouts, gpu) = rig.step();
        assert_eq!(layouts, [docked(primary(), Edge::Top, 540)]);
        assert_eq!(gpu.first(), Some(&Gpu::Resize(1920, 540)));
        assert_eq!(gpu.last(), Some(&Gpu::Render));
    }

    #[test]
    fn disabling_hides_once_and_stops_drawing() {
        let mut rig = Rig::new();
        rig.set(|s| s.enabled = true);
        rig.step();
        rig.set(|s| s.enabled = false);
        assert_eq!(rig.step(), (vec![Layout::Hidden], vec![]));
        assert_eq!(rig.step(), (vec![], vec![]));
    }

    #[test]
    fn toggling_back_on_shows_again() {
        let mut rig = Rig::new();
        rig.set(|s| s.enabled = true);
        rig.step();
        rig.set(|s| s.enabled = false);
        rig.step();
        rig.set(|s| s.enabled = true);
        let (layouts, _) = rig.step();
        assert_eq!(layouts, [Layout::Fullscreen { monitor: primary() }]);
    }

    #[test]
    fn mode_change_while_enabled_reapplies() {
        let mut rig = Rig::new();
        rig.set(|s| {
            s.panel_size = 50;
            s.enabled = true;
        });
        rig.step();

        rig.set(|s| s.display_mode = DisplayMode::Docked(Edge::Left));
        let (layouts, gpu) = rig.step();
        assert_eq!(layouts, [docked(primary(), Edge::Left, 960)]);
        assert_eq!(gpu.first(), Some(&Gpu::Resize(960, 1080)));

        rig.set(|s| s.display_mode = DisplayMode::Docked(Edge::Right));
        assert_eq!(rig.step().0, [docked(primary(), Edge::Right, 960)]);

        rig.set(|s| s.display_mode = DisplayMode::Fullscreen);
        assert_eq!(rig.step().0, [Layout::Fullscreen { monitor: primary() }]);
    }

    #[test]
    fn changes_while_disabled_wait_for_enable() {
        let mut rig = Rig::new();
        rig.set(|s| {
            s.display_mode = DisplayMode::Docked(Edge::Bottom);
            s.panel_size = 25;
        });
        assert_eq!(rig.step(), (vec![], vec![]));

        rig.set(|s| s.enabled = true);
        assert_eq!(rig.step().0, [docked(primary(), Edge::Bottom, 270)]);
    }

    #[test]
    fn panel_size_reapplies_only_when_docked() {
        let mut rig = Rig::new();
        rig.set(|s| s.enabled = true);
        rig.step();
        rig.set(|s| s.panel_size = 30);
        assert!(rig.step().0.is_empty());

        rig.set(|s| s.display_mode = DisplayMode::Docked(Edge::Top));
        assert_eq!(rig.step().0, [docked(primary(), Edge::Top, 324)]);
        rig.set(|s| s.panel_size = 40);
        let (layouts, gpu) = rig.step();
        assert_eq!(layouts, [docked(primary(), Edge::Top, 432)]);
        assert_eq!(gpu.first(), Some(&Gpu::Resize(1920, 432)));
    }

    #[test]
    fn fullscreen_follows_the_pointer_to_another_monitor() {
        let mut rig = Rig::new();
        rig.set(|s| s.enabled = true);
        rig.step();

        rig.point_at(2000, 100);
        let (layouts, gpu) = rig.step();
        assert_eq!(rig.desired.load(Ordering::Relaxed), 1);
        assert_eq!(layouts, [Layout::Fullscreen { monitor: second() }]);
        assert_eq!(gpu[..2], [Gpu::Recreate(1280, 1024), Gpu::Resize(1280, 1024)]);

        rig.point_at(100, 100);
        let (layouts, _) = rig.step();
        assert_eq!(rig.desired.load(Ordering::Relaxed), 0);
        assert_eq!(layouts, [Layout::Fullscreen { monitor: primary() }]);
    }

    /// The crop in the last uniform write of `gpu`.
    fn crop_written(gpu: &[Gpu]) -> Option<[f32; 4]> {
        gpu.iter().rev().find_map(|c| match c {
            Gpu::Uniforms(u) => Some(u.crop),
            _ => None,
        })
    }

    /// Docked on the top edge at the default 50%, snapping to the target, at 2x, one tick
    /// taken with the pointer at (960, 540) on the primary.
    fn docked_top_half(follows: bool) -> Rig {
        let mut rig = Rig::new();
        rig.set(|s| {
            s.display_mode = DisplayMode::Docked(Edge::Top);
            s.panel_follows_monitor = follows;
            s.smooth_speed = 1.0;
            s.enabled = true;
        });
        let (layouts, _) = rig.step();
        assert_eq!(layouts, [docked(primary(), Edge::Top, 540)]);
        rig.host.cursor.clear();
        rig
    }

    #[test]
    fn docked_monitor_switch_keeps_the_panel_on_the_primary_and_sizes_the_view_for_it() {
        let mut rig = docked_top_half(false);
        rig.point_at(2000, 100);
        let (layouts, gpu) = rig.step();
        assert_eq!(rig.desired.load(Ordering::Relaxed), 1);
        assert!(layouts.is_empty());
        assert_eq!(gpu.first(), Some(&Gpu::Recreate(1280, 1024)));
        assert_eq!(count(&gpu, |c| matches!(c, Gpu::Resize(..))), 0);
        // The 1920x540 panel on the primary at 2x shows 960x270 of the second monitor's
        // 1280x1024 frame, not the 640x256 a panel on the second monitor would (Finding 18).
        assert_eq!(crop_written(&gpu), Some([0.0, 0.0, 0.75, 270.0 / 1024.0]));
    }

    #[test]
    fn enabling_docked_on_the_second_monitor_puts_the_panel_on_the_primary() {
        let mut rig = Rig::new();
        rig.point_at(2000, 100);
        rig.set(|s| {
            s.display_mode = DisplayMode::Docked(Edge::Top);
            s.enabled = true;
        });
        let (layouts, _) = rig.step();
        assert_eq!(rig.desired.load(Ordering::Relaxed), 1);
        assert_eq!(layouts, [docked(primary(), Edge::Top, 540)]);
    }

    #[test]
    fn enabling_docked_on_the_second_monitor_with_the_setting_puts_the_panel_there() {
        let mut rig = Rig::new();
        rig.point_at(2000, 100);
        rig.set(|s| {
            s.display_mode = DisplayMode::Docked(Edge::Left);
            s.panel_follows_monitor = true;
            s.enabled = true;
        });
        let (layouts, gpu) = rig.step();
        // 50% of the second monitor's 1280 px width.
        assert_eq!(layouts, [docked(second(), Edge::Left, 640)]);
        assert!(gpu.contains(&Gpu::Resize(640, 1024)));
    }

    #[test]
    fn with_the_setting_the_panel_moves_with_the_target_and_back() {
        let mut rig = docked_top_half(true);
        rig.point_at(2000, 100);
        let (layouts, gpu) = rig.step();
        assert_eq!(rig.desired.load(Ordering::Relaxed), 1);
        assert_eq!(layouts, [docked(second(), Edge::Top, 512)]);
        assert_eq!(gpu[..2], [Gpu::Recreate(1280, 1024), Gpu::Resize(1280, 512)]);
        // A 1280x512 panel at 2x: 640x256 of the 1280x1024 frame.
        assert_eq!(crop_written(&gpu), Some([0.0, 0.0, 0.5, 0.25]));

        rig.point_at(100, 700);
        let (layouts, gpu) = rig.step();
        assert_eq!(rig.desired.load(Ordering::Relaxed), 0);
        assert_eq!(layouts, [docked(primary(), Edge::Top, 540)]);
        assert_eq!(gpu[..2], [Gpu::Recreate(1920, 1080), Gpu::Resize(1920, 540)]);
    }

    #[test]
    fn with_the_setting_a_caret_on_the_other_monitor_moves_the_panel() {
        let mut rig = docked_top_half(true);
        rig.caret_at(2500, 500);
        let (layouts, _) = rig.step();
        assert_eq!(layouts, [docked(second(), Edge::Top, 512)]);
        // Re-centred across only: at 2x, 500 is inside the middle of the view's height.
        assert_eq!(rig.smooth().0, 2500.0);
    }

    #[test]
    fn changing_the_setting_while_docked_moves_the_panel_at_once() {
        let mut rig = docked_top_half(false);
        rig.point_at(2000, 100);
        rig.step();

        rig.set(|s| s.panel_follows_monitor = true);
        assert_eq!(rig.step().0, [docked(second(), Edge::Top, 512)]);
        rig.set(|s| s.panel_follows_monitor = false);
        assert_eq!(rig.step().0, [docked(primary(), Edge::Top, 540)]);
    }

    #[test]
    fn the_setting_changes_nothing_on_one_monitor_or_in_fullscreen() {
        let mut rig = docked_top_half(false);
        rig.set(|s| s.panel_follows_monitor = true); // active is the primary: same panel
        assert!(rig.step().0.is_empty());

        rig.set(|s| s.display_mode = DisplayMode::Fullscreen);
        rig.step();
        rig.set(|s| s.panel_follows_monitor = false);
        assert!(rig.step().0.is_empty());
    }

    #[test]
    fn the_cursor_hides_inside_the_panel_after_it_moves() {
        let mut rig = docked_top_half(true);
        // (960, 540) is the first row below the primary's 540 px panel.
        assert!(rig.cursor_step().is_empty());
        // Inside the second monitor's 512 px panel, which the same tick puts there.
        rig.point_at(2000, 100);
        assert_eq!(rig.cursor_step(), [false]);
        let moved = ScreenRect { left: 1920, top: 0, width: 1280, height: 512 };
        assert_eq!(rig.view.panel, Some(moved));
        rig.point_at(2000, 600);
        assert_eq!(rig.cursor_step(), [true]);
    }

    #[test]
    fn the_primary_is_the_monitor_at_the_origin_not_the_start_one() {
        // Starts on the second monitor, listed first; the pointer stays on it.
        let mut rig = Rig::with(second(), vec![second(), primary()]);
        rig.point_at(2000, 100);
        rig.set(|s| {
            s.display_mode = DisplayMode::Docked(Edge::Bottom);
            s.panel_size = 25;
            s.enabled = true;
        });
        let (layouts, _) = rig.step();
        assert_eq!(rig.desired.load(Ordering::Relaxed), 1);
        assert_eq!(layouts, [docked(primary(), Edge::Bottom, 270)]);
    }

    #[test]
    fn with_no_monitor_at_the_origin_the_panel_stays_on_the_start_monitor() {
        let a = OutputInfo { idx: 0, left: 100, top: 0, width: 1920, height: 1080 };
        let b = OutputInfo { idx: 1, left: 2020, top: 0, width: 1280, height: 1024 };
        let mut rig = Rig::with(a.clone(), vec![a.clone(), b]);
        rig.point_at(2100, 100);
        rig.set(|s| {
            s.display_mode = DisplayMode::Docked(Edge::Top);
            s.enabled = true;
        });
        let (layouts, _) = rig.step();
        assert_eq!(rig.desired.load(Ordering::Relaxed), 1);
        assert_eq!(layouts, [docked(a, Edge::Top, 540)]);
    }

    #[test]
    fn pointer_off_every_monitor_keeps_the_monitor() {
        let mut rig = Rig::new();
        rig.set(|s| s.enabled = true);
        rig.step();
        rig.point_at(-5000, -5000);
        let (layouts, gpu) = rig.step();
        assert!(layouts.is_empty());
        assert_eq!(count(&gpu, |c| matches!(c, Gpu::Recreate(..))), 0);
        assert_eq!(rig.desired.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn pointer_is_not_followed_while_disabled() {
        let mut rig = Rig::new();
        rig.point_at(2000, 100);
        assert_eq!(rig.step(), (vec![], vec![]));
        assert_eq!(rig.desired.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_frame_is_uploaded_once_until_a_new_one_arrives() {
        let mut rig = Rig::new();
        rig.set(|s| s.enabled = true);
        rig.publish_frame();
        let uploads = |gpu: &[Gpu]| count(gpu, |c| matches!(c, Gpu::Upload(..)));
        assert_eq!(uploads(&rig.step().1), 1);
        assert_eq!(uploads(&rig.step().1), 0);
        rig.publish_frame();
        assert_eq!(uploads(&rig.step().1), 1);
    }

    #[test]
    fn monitor_switch_uploads_the_current_frame_again() {
        let mut rig = Rig::new();
        rig.set(|s| s.enabled = true);
        rig.publish_frame();
        rig.step();
        rig.point_at(2000, 100);
        let (_, gpu) = rig.step();
        assert_eq!(count(&gpu, |c| matches!(c, Gpu::Upload(..))), 1);
    }

    #[test]
    fn uniforms_are_written_only_when_something_changes() {
        let mut rig = Rig::new();
        rig.set(|s| {
            s.smooth_speed = 1.0; // snap to the pointer, so the view settles in one tick
            s.enabled = true;
        });
        assert_eq!(uniform_writes(&rig.step().1), 1);
        assert_eq!(uniform_writes(&rig.step().1), 0);
        assert_eq!(uniform_writes(&rig.step().1), 0);

        let changes: [&dyn Fn(&mut AppState); 4] = [
            &|s| s.color_filter = ColorFilter::Inverted,
            &|s| s.interpolation = Interpolation::Sharp,
            &|s| s.edge_threshold = 0.5,
            &|s| s.zoom = 4.0,
        ];
        for change in changes {
            rig.set(change);
            assert_eq!(uniform_writes(&rig.step().1), 1);
            assert_eq!(uniform_writes(&rig.step().1), 0);
        }

        rig.point_at(700, 300);
        assert_eq!(uniform_writes(&rig.step().1), 1);
        assert_eq!(uniform_writes(&rig.step().1), 0);
    }

    #[test]
    fn uniforms_carry_the_settings_and_cursor() {
        let mut rig = Rig::new();
        rig.set(|s| {
            s.smooth_speed = 1.0;
            s.zoom = 2.0;
            s.color_filter = ColorFilter::Greyscale;
            s.interpolation = Interpolation::CleanEdge;
            s.edge_threshold = 0.3;
            s.enabled = true;
        });
        let (_, gpu) = rig.step();
        let written = gpu.iter().find_map(|c| match c {
            Gpu::Uniforms(u) => Some(*u),
            _ => None,
        });
        assert_eq!(
            written,
            Some(Uniforms {
                crop: [0.25, 0.25, 0.5, 0.5],
                color_mode: ColorFilter::Greyscale.as_u32(),
                interp_mode: Interpolation::CleanEdge.as_u32(),
                cursor_x: 960,
                cursor_y: 540,
                edge_threshold: 0.3,
                pointer: PointerDraw::marker(POINTER_CIRCLE),
            })
        );
    }

    #[test]
    fn smoothing_uses_the_tick_time() {
        let mut rig = Rig::new();
        rig.set(|s| {
            s.smooth_speed = 0.15;
            s.enabled = true;
        });
        rig.point_at(1060, 540);
        rig.tick();
        let alpha = geometry::smooth_alpha(0.15, TICK.as_secs_f32());
        assert!((rig.view.smooth_x - (960.0 + 100.0 * alpha)).abs() < 1e-3);
        assert_eq!(rig.view.smooth_y, 540.0);

        // No time passed: no movement.
        let x = rig.view.smooth_x;
        rig.view.tick(&mut rig.host, rig.now);
        assert_eq!(rig.view.smooth_x, x);
    }

    #[test]
    fn failed_render_reconfigures_the_surface() {
        let mut rig = Rig::new();
        rig.view.renderer.render_ok = false;
        rig.set(|s| {
            s.display_mode = DisplayMode::Docked(Edge::Top);
            s.panel_size = 50;
            s.enabled = true;
        });
        rig.step();
        let (_, gpu) = rig.step();
        assert_eq!(gpu, [Gpu::Render, Gpu::Resize(1920, 540)]);
    }

    #[test]
    fn the_pointer_look_is_read_once_per_enabled_tick() {
        let mut rig = Rig::new();
        rig.tick();
        rig.tick();
        assert_eq!(rig.host.looks, 0, "not while disabled");

        rig.set(|s| s.enabled = true);
        rig.tick();
        rig.tick();
        rig.tick();
        assert_eq!(rig.host.looks, 3);

        rig.set(|s| s.enabled = false);
        rig.tick();
        assert_eq!(rig.host.looks, 3);
    }

    fn arrow() -> Arc<PointerShape> {
        Arc::new(PointerShape { width: 2, height: 2, hotspot: (1, 0), mask: vec![255, 0, 255, 255] })
    }

    /// The pointer part of the last uniform write of `gpu`.
    fn pointer_written(gpu: &[Gpu]) -> Option<PointerDraw> {
        gpu.iter().rev().find_map(|c| match c {
            Gpu::Uniforms(u) => Some(u.pointer),
            _ => None,
        })
    }

    fn masks(gpu: &[Gpu]) -> usize {
        count(gpu, |c| matches!(c, Gpu::Mask(..)))
    }

    #[test]
    fn the_circle_shows_until_a_shape_is_known() {
        let mut rig = Rig::new();
        rig.set(|s| s.enabled = true);
        let (_, gpu) = rig.step();
        assert_eq!(pointer_written(&gpu), Some(PointerDraw::marker(POINTER_CIRCLE)));
        assert_eq!(masks(&gpu), 0);
    }

    #[test]
    fn a_hidden_pointer_draws_nothing() {
        let mut rig = Rig::new();
        rig.host.look = PointerLook::Hidden;
        rig.set(|s| s.enabled = true);
        let (_, gpu) = rig.step();
        assert_eq!(pointer_written(&gpu), Some(PointerDraw::marker(POINTER_NONE)));
        assert_eq!(masks(&gpu), 0);
    }

    #[test]
    fn a_shape_is_drawn_with_the_default_settings() {
        let mut rig = Rig::new();
        rig.host.look = PointerLook::Shape(arrow());
        rig.set(|s| s.enabled = true);
        let (_, gpu) = rig.step();
        assert_eq!(masks(&gpu), 1);
        assert_eq!(
            pointer_written(&gpu),
            Some(PointerDraw {
                kind: POINTER_SHAPE,
                mask: (2, 2),
                hotspot: (1, 0),
                size: 2.0,
                outline: 2.0,
                fill: [26.0 / 255.0, 102.0 / 255.0, 1.0, 1.0],
                outline_colour: [1.0, 1.0, 1.0, 1.0],
            })
        );
    }

    #[test]
    fn the_pointer_settings_reach_the_shader_without_a_new_upload() {
        let mut rig = Rig::new();
        rig.host.look = PointerLook::Shape(arrow());
        rig.set(|s| s.enabled = true);
        assert_eq!(masks(&rig.step().1), 1);

        rig.set(|s| {
            s.pointer_size = 4.5;
            s.pointer_outline = 6.0;
            s.pointer_fill = [255, 0, 51];
            s.pointer_outline_colour = [0, 51, 102];
        });
        let (_, gpu) = rig.step();
        assert_eq!(masks(&gpu), 0);
        let p = pointer_written(&gpu).expect("uniforms rewritten");
        assert_eq!(p.kind, POINTER_SHAPE);
        assert_eq!((p.size, p.outline), (4.5, 6.0));
        assert_eq!(p.fill, [1.0, 0.0, 51.0 / 255.0, 1.0]);
        assert_eq!(p.outline_colour, [0.0, 51.0 / 255.0, 102.0 / 255.0, 1.0]);
    }

    #[test]
    fn with_the_shape_setting_off_the_circle_shows_and_no_mask_is_uploaded() {
        let mut rig = Rig::new();
        let shape = arrow();
        rig.host.look = PointerLook::Shape(Arc::clone(&shape));
        rig.set(|s| {
            s.pointer_shape = false;
            s.enabled = true;
        });
        let (_, gpu) = rig.step();
        assert_eq!(masks(&gpu), 0);
        assert_eq!(pointer_written(&gpu), Some(PointerDraw::marker(POINTER_CIRCLE)));

        // The circle has a fixed look: a size change writes nothing new.
        rig.set(|s| s.pointer_size = 5.0);
        let (_, gpu) = rig.step();
        assert_eq!(pointer_written(&gpu), None);

        // A hidden pointer still draws nothing.
        rig.host.look = PointerLook::Hidden;
        let (_, gpu) = rig.step();
        assert_eq!(pointer_written(&gpu), Some(PointerDraw::marker(POINTER_NONE)));

        // Back on: the shape returns, uploaded once because the circle never got it.
        rig.host.look = PointerLook::Shape(shape);
        rig.set(|s| s.pointer_shape = true);
        let (_, gpu) = rig.step();
        assert_eq!(masks(&gpu), 1);
        assert_eq!(pointer_written(&gpu).map(|p| p.kind), Some(POINTER_SHAPE));
    }

    #[test]
    fn switching_the_setting_off_and_on_does_not_upload_the_same_shape_again() {
        let mut rig = Rig::new();
        let shape = arrow();
        rig.host.look = PointerLook::Shape(shape);
        rig.set(|s| s.enabled = true);
        assert_eq!(masks(&rig.step().1), 1);
        rig.set(|s| s.pointer_shape = false);
        assert_eq!(masks(&rig.step().1), 0);
        rig.set(|s| s.pointer_shape = true);
        let (_, gpu) = rig.step();
        assert_eq!(masks(&gpu), 0);
        assert_eq!(pointer_written(&gpu).map(|p| p.kind), Some(POINTER_SHAPE));
    }

    #[test]
    fn the_mask_is_uploaded_once_per_shape() {
        let mut rig = Rig::new();
        let first = arrow();
        rig.host.look = PointerLook::Shape(Arc::clone(&first));
        rig.set(|s| s.enabled = true);
        rig.publish_frame();
        assert_eq!(masks(&rig.step().1), 1);

        // The same Arc, with the pointer moving: no upload.
        rig.point_at(700, 300);
        assert_eq!(masks(&rig.step().1), 0);
        rig.point_at(710, 305);
        assert_eq!(masks(&rig.step().1), 0);

        // A new Arc, even with equal contents, is a new shape; so is one of another size.
        let same_bytes = Arc::new((*first).clone());
        rig.host.look = PointerLook::Shape(same_bytes);
        assert_eq!(masks(&rig.step().1), 1);
        let big = Arc::new(PointerShape { width: 3, height: 3, hotspot: (0, 0), mask: vec![255; 9] });
        rig.host.look = PointerLook::Shape(big);
        let (_, gpu) = rig.step();
        assert!(gpu.contains(&Gpu::Mask(3, 3)));
        assert_eq!(pointer_written(&gpu).map(|p| p.mask), Some((3, 3)));
    }

    #[test]
    fn going_hidden_and_back_does_not_upload_the_same_shape_again() {
        let mut rig = Rig::new();
        let shape = arrow();
        rig.host.look = PointerLook::Shape(Arc::clone(&shape));
        rig.set(|s| s.enabled = true);
        assert_eq!(masks(&rig.step().1), 1);
        rig.host.look = PointerLook::Hidden;
        assert_eq!(masks(&rig.step().1), 0);
        rig.host.look = PointerLook::Shape(shape);
        let (_, gpu) = rig.step();
        assert_eq!(masks(&gpu), 0);
        assert_eq!(pointer_written(&gpu).map(|p| p.kind), Some(POINTER_SHAPE));
    }

    #[test]
    fn an_unusable_shape_falls_back_to_the_circle() {
        let wrong_length = PointerShape { width: 2, height: 2, hotspot: (0, 0), mask: vec![255; 3] };
        let empty = PointerShape { width: 0, height: 0, hotspot: (0, 0), mask: Vec::new() };
        let huge = PointerShape {
            width: MAX_MASK_SIDE + 1,
            height: 1,
            hotspot: (0, 0),
            mask: vec![255; MAX_MASK_SIDE as usize + 1],
        };
        for shape in [wrong_length, empty, huge] {
            let mut rig = Rig::new();
            rig.host.look = PointerLook::Shape(Arc::new(shape));
            rig.set(|s| s.enabled = true);
            let (_, gpu) = rig.step();
            assert_eq!(masks(&gpu), 0);
            assert_eq!(pointer_written(&gpu), Some(PointerDraw::marker(POINTER_CIRCLE)));
        }
    }

    #[test]
    fn the_shape_is_drawn_in_docked_mode_too() {
        let mut rig = Rig::new();
        rig.host.look = PointerLook::Shape(arrow());
        rig.set(|s| {
            s.display_mode = DisplayMode::Docked(Edge::Top);
            s.panel_size = 25;
            s.enabled = true;
        });
        let (_, gpu) = rig.step();
        assert_eq!(masks(&gpu), 1);
        assert_eq!(pointer_written(&gpu).map(|p| p.kind), Some(POINTER_SHAPE));
    }

    #[test]
    fn unreadable_pointer_keeps_the_last_position() {
        let mut rig = Rig::new();
        rig.set(|s| {
            s.smooth_speed = 1.0;
            s.enabled = true;
        });
        rig.point_at(500, 400);
        rig.step();
        rig.host.pointer = None;
        let (_, gpu) = rig.step();
        assert_eq!((rig.view.smooth_x, rig.view.smooth_y), (500.0, 400.0));
        assert_eq!(uniform_writes(&gpu), 0);
    }

    #[test]
    fn docked_hides_the_cursor_inside_the_panel_and_shows_it_outside() {
        let mut rig = Rig::new();
        rig.docked_top(); // pointer at (960, 540): below the 270 px panel
        assert!(rig.cursor_step().is_empty());

        rig.point_at(100, 100);
        assert_eq!(rig.cursor_step(), [false]);
        assert!(rig.cursor_step().is_empty());
        rig.point_at(1919, 269); // last pixel inside
        assert!(rig.cursor_step().is_empty());

        rig.point_at(1919, 270); // first row below
        assert_eq!(rig.cursor_step(), [true]);
        assert!(rig.cursor_step().is_empty());
    }

    #[test]
    fn every_other_edge_hides_the_cursor_inside_its_panel_and_shows_it_outside() {
        // 25% of 1920x1080: bottom rows 810..1080, left columns 0..480, right columns 1440..1920.
        // Each: two pixels on the inner boundary of the panel, then the first one outside.
        let cases = [
            (Edge::Bottom, [(0, 810), (1919, 1079)], (0, 809)),
            (Edge::Left, [(0, 0), (479, 1079)], (480, 0)),
            (Edge::Right, [(1440, 0), (1919, 1079)], (1439, 1079)),
        ];
        for (edge, inside, outside) in cases {
            let mut rig = Rig::new();
            rig.docked_on(edge); // pointer at (960, 540): outside every one of these panels
            assert!(rig.cursor_step().is_empty(), "{edge:?}");

            rig.point_at(inside[0].0, inside[0].1);
            assert_eq!(rig.cursor_step(), [false], "{edge:?} enter");
            rig.point_at(inside[1].0, inside[1].1);
            assert!(rig.cursor_step().is_empty(), "{edge:?} still inside");

            rig.point_at(outside.0, outside.1);
            assert_eq!(rig.cursor_step(), [true], "{edge:?} leave");
            assert!(rig.cursor_step().is_empty(), "{edge:?} stays shown");
        }
    }

    #[test]
    fn switching_edges_with_the_pointer_inside_both_panels_hides_it_again() {
        let mut rig = Rig::new();
        rig.docked_on(Edge::Left);
        rig.point_at(100, 100); // inside the left and the top panel at 25%
        assert_eq!(rig.cursor_step(), [false]);

        // The new layout shows the cursor, so the magnifier hides it again in the same tick.
        rig.set(|s| s.display_mode = DisplayMode::Docked(Edge::Top));
        assert_eq!(rig.cursor_step(), [false]);
        assert_eq!(rig.host.layouts, [docked(primary(), Edge::Top, 270)]);
    }

    #[test]
    fn enabling_docked_with_the_pointer_inside_hides_it_in_the_same_tick() {
        let mut rig = Rig::new();
        rig.point_at(100, 100);
        rig.set(|s| {
            s.display_mode = DisplayMode::Docked(Edge::Top);
            s.panel_size = 25;
            s.enabled = true;
        });
        rig.tick();
        assert_eq!(rig.host.layouts, [docked(primary(), Edge::Top, 270)]);
        assert_eq!(std::mem::take(&mut rig.host.cursor), [false]);
        // Hidden after the layout, so the next tick has nothing to undo.
        assert!(rig.cursor_step().is_empty());
    }

    #[test]
    fn fullscreen_and_disabled_never_set_the_cursor() {
        let mut rig = Rig::new();
        rig.set(|s| {
            s.display_mode = DisplayMode::Docked(Edge::Top);
            s.panel_size = 25;
        });
        rig.point_at(100, 100);
        assert!(rig.cursor_step().is_empty()); // disabled

        rig.set(|s| {
            s.display_mode = DisplayMode::Fullscreen;
            s.enabled = true;
        });
        for (x, y) in [(100, 100), (960, 540), (100, 100)] {
            rig.point_at(x, y);
            assert!(rig.cursor_step().is_empty());
        }
    }

    #[test]
    fn a_layout_change_inside_the_panel_hides_the_cursor_again() {
        let mut rig = Rig::new();
        rig.docked_top();
        rig.point_at(100, 100);
        assert_eq!(rig.cursor_step(), [false]);

        // Toggle off: `apply_layout(Hidden)` shows the cursor; the magnifier says nothing.
        rig.set(|s| s.enabled = false);
        let (layouts, _) = rig.step();
        assert_eq!(layouts, [Layout::Hidden]);
        assert!(std::mem::take(&mut rig.host.cursor).is_empty());

        // Back on: the layout showed it again, so it is hidden again.
        rig.set(|s| s.enabled = true);
        assert_eq!(rig.cursor_step(), [false]);

        // Fullscreen: `apply_layout` hides it; back to docked: hidden again by the magnifier.
        rig.set(|s| s.display_mode = DisplayMode::Fullscreen);
        assert!(rig.cursor_step().is_empty());
        rig.set(|s| s.display_mode = DisplayMode::Docked(Edge::Top));
        assert_eq!(rig.cursor_step(), [false]);

        // Panel size change re-applies the layout.
        rig.set(|s| s.panel_size = 30);
        assert_eq!(rig.cursor_step(), [false]);
    }

    #[test]
    fn docked_hit_test_uses_the_applied_panel_after_a_monitor_switch() {
        let mut rig = Rig::new();
        rig.docked_top();
        // Top of the second monitor: inside a panel there, but the panel stayed on the primary.
        rig.point_at(2000, 100);
        assert!(rig.cursor_step().is_empty());
        assert_eq!(rig.desired.load(Ordering::Relaxed), 1);

        rig.point_at(100, 100);
        assert_eq!(rig.cursor_step(), [false]);
    }

    // The pan window (ADR 0011, roadmap 7.2c). A docked panel at 25% of the 1920x1080 primary is
    // 270 px thick on Top and Bottom and 480 px on Left and Right; the margin is a tenth of what
    // is left uncovered (81 px and 144 px).

    /// Ticks until the eased pan offset has had time to settle.
    const SETTLE: usize = 30;

    impl Rig {
        /// A frame published, then docked on `edge` at 25% of the primary and one tick taken, so
        /// the frame is uploaded; the pan calls so far are forgotten.
        fn pan_rig(edge: Edge) -> Self {
            let mut rig = Rig::new();
            rig.publish_frame();
            rig.docked_on(edge);
            rig.host.pan.clear();
            rig.host.cursor.clear();
            rig.view.renderer.pan_calls.clear();
            rig.view.trace().clear();
            rig
        }

        fn settle(&mut self) {
            for _ in 0..SETTLE {
                self.tick();
            }
        }

        fn pan_uniforms(&self) -> Option<Uniforms> {
            self.view.renderer.pan_calls.iter().rev().find_map(|c| match c {
                PanGpu::Uniforms(u) => Some(*u),
                _ => None,
            })
        }

        fn pan_renders(&self) -> usize {
            self.view.renderer.pan_calls.iter().filter(|c| **c == PanGpu::Render).count()
        }
    }

    impl View<FakeRenderer> {
        fn trace(&self) -> std::sync::MutexGuard<'_, Vec<&'static str>> {
            self.renderer.trace.lock().unwrap()
        }
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn the_pan_window_stays_hidden_while_the_pointer_is_far_from_the_panel() {
        let mut rig = Rig::pan_rig(Edge::Top);
        rig.point_at(100, 900);
        rig.settle();
        rig.point_at(100, 351); // the margin's edge: the offset is still 0
        rig.settle();
        assert!(rig.host.pan.is_empty());
        assert!(rig.view.renderer.pan_calls.is_empty());
        assert!(rig.host.cursor.is_empty());
    }

    #[test]
    fn the_pan_surface_is_drawn_before_the_window_is_shown() {
        let mut rig = Rig::pan_rig(Edge::Top);
        rig.point_at(100, 200);
        rig.tick();
        assert_eq!(rig.host.pan, [true]);
        assert_eq!(*rig.view.trace(), ["render", "show"]);
        // Eased, not jumped: partway to the 151 px target.
        assert!(rig.view.pan_offset > 1.0 && rig.view.pan_offset < 151.0);
    }

    #[test]
    fn the_offset_settles_on_a_whole_pixel_and_stays_there() {
        let mut rig = Rig::pan_rig(Edge::Top);
        rig.point_at(100, 200);
        rig.settle();
        assert_eq!(rig.host.pan, [true]);
        assert!(approx(rig.view.pan_offset, 151.0));
        // The uniforms are written only when something changed.
        rig.view.renderer.pan_calls.clear();
        for _ in 0..3 {
            rig.tick();
        }
        assert_eq!(rig.pan_renders(), 3);
        assert_eq!(rig.pan_uniforms(), None);
    }

    #[test]
    fn a_target_under_a_pixel_still_grows_into_one() {
        let mut rig = Rig::pan_rig(Edge::Top);
        rig.point_at(100, 350); // the target is 1 px
        rig.settle();
        assert_eq!(rig.host.pan, [true]);
        let u = rig.pan_uniforms().unwrap();
        assert!(approx(u.crop[1], 269.0 / 1080.0));
    }

    #[test]
    fn the_pan_window_draws_the_frame_shifted_and_the_pointer_displaced_on_each_edge() {
        // (edge, pointer, window size, source corner in the frame, pointer in the window)
        let cases = [
            // Offset 151: 270 + 81 - 200. The strip rows 0..151 come in below the panel.
            (Edge::Top, (960, 200), (1920, 810), (0, 119), (960, 81)),
            // The pointer is 200 px from the bottom edge row; the image moves up by 151.
            (Edge::Bottom, (960, 879), (1920, 810), (0, 151), (960, 728)),
            // Offset 324: 480 + 144 - 300.
            (Edge::Left, (300, 500), (1440, 1080), (156, 0), (144, 500)),
            (Edge::Right, (1619, 500), (1440, 1080), (324, 0), (1295, 500)),
        ];
        for (edge, (x, y), (w, h), (sx, sy), (cx, cy)) in cases {
            let mut rig = Rig::pan_rig(edge);
            rig.point_at(x, y);
            rig.settle();
            assert_eq!(rig.host.pan, [true], "{edge:?}");
            let u = rig.pan_uniforms().unwrap();
            let want = [sx as f32 / 1920.0, sy as f32 / 1080.0, w as f32 / 1920.0, h as f32 / 1080.0];
            for (got, want) in u.crop.iter().zip(want) {
                assert!(approx(*got, want), "{edge:?}: crop {:?}, want {want:?}", u.crop);
            }
            assert_eq!((u.cursor_x, u.cursor_y), (cx, cy), "{edge:?}");
            assert_eq!((u.color_mode, u.interp_mode), (0, 0), "{edge:?}: no filter, bilinear");
        }
    }

    #[test]
    fn the_pan_pointer_is_hidden_while_it_would_fall_under_the_panel() {
        // A top panel and an offset still easing in: the displaced pointer is above the window.
        let dock = Dock { monitor: primary(), edge: Edge::Top, thickness: 270 };
        let view = PanView::new(&dock, 10).unwrap();
        assert_eq!(view.pointer(ScreenPoint { x: 100, y: 100 }, &primary()), None);
        assert_eq!(view.pointer(ScreenPoint { x: 100, y: 300 }, &primary()), Some((100, 40)));
    }

    #[test]
    fn the_pan_window_hides_once_the_offset_eases_back_to_nothing() {
        let mut rig = Rig::pan_rig(Edge::Top);
        rig.point_at(100, 200);
        rig.settle();
        rig.point_at(100, 900);
        rig.tick();
        assert_eq!(rig.host.pan, [true], "still easing back after one tick");
        rig.settle();
        assert_eq!(rig.host.pan, [true, false]);
    }

    #[test]
    fn a_layout_change_clears_the_shown_flag_so_the_pan_is_shown_again() {
        let mut rig = Rig::pan_rig(Edge::Top);
        rig.point_at(100, 200);
        rig.settle();
        assert_eq!(rig.host.pan, [true]);
        // 30%: 324 px thick, target 324 + 75.6 - 200. `apply_layout` hid the window.
        rig.set(|s| s.panel_size = 30);
        rig.view.renderer.pan_calls.clear();
        rig.view.trace().clear();
        rig.tick();
        assert_eq!(rig.view.renderer.pan_calls[0], PanGpu::Resize(1920, 756));
        assert_eq!(rig.host.pan, [true, true]);
        assert_eq!(*rig.view.trace(), ["render", "show"]);
    }

    #[test]
    fn the_pan_window_hides_at_once_when_the_panel_monitor_is_not_the_one_captured() {
        let mut rig = Rig::pan_rig(Edge::Top);
        rig.point_at(100, 200);
        rig.settle();
        assert_eq!(rig.host.pan, [true]);
        // The pointer stays still beside the panel while the view goes to a caret on the second
        // monitor (the panel stays on the primary): the frame texture is the second monitor's.
        rig.caret_at(2500, 500);
        rig.tick();
        assert_eq!(rig.desired.load(Ordering::Relaxed), 1);
        assert_eq!(rig.host.pan, [true, false]);
        assert_eq!(rig.view.pan_offset, 0.0);
        rig.settle();
        assert_eq!(rig.host.pan, [true, false]);
    }

    #[test]
    fn a_panel_that_follows_the_monitor_pans_on_the_second_monitor() {
        let mut rig = Rig::pan_rig(Edge::Top);
        rig.set(|s| s.panel_follows_monitor = true);
        // 25% of 1024 is 256 thick; 768 uncovered, margin 76.8: 256 + 76.8 - 200.
        rig.point_at(2000, 200);
        rig.settle();
        assert_eq!(rig.host.pan, [true]);
        let u = rig.pan_uniforms().unwrap();
        // The target 132.8 is drawn as 133: source row 256 - 133.
        let want = [0.0, 123.0 / 1024.0, 1.0, 768.0 / 1024.0];
        for (got, want) in u.crop.iter().zip(want) {
            assert!(approx(*got, want), "crop {:?}", u.crop);
        }
        // Window pixels: 2000 - 1920 = 80 across, 200 - 256 + 133 = 77 down.
        assert_eq!((u.cursor_x, u.cursor_y), (80, 77));
    }

    #[test]
    fn the_pan_window_is_not_shown_without_a_frame() {
        let mut rig = Rig::new();
        rig.docked_top();
        rig.point_at(100, 200);
        rig.settle();
        assert!(rig.host.pan.is_empty());
        assert!(rig.view.renderer.pan_calls.iter().all(|c| matches!(c, PanGpu::Resize(..))));
    }

    #[test]
    fn the_pan_window_is_not_shown_without_a_pan_surface() {
        let mut rig = Rig::pan_rig(Edge::Top);
        rig.view.renderer.pan_available = false;
        rig.point_at(100, 300); // below the 270 px panel, where the pan would start
        rig.settle();
        assert!(rig.host.pan.is_empty());
        assert!(rig.host.cursor.is_empty());
        assert!(rig.view.renderer.pan_calls.is_empty());
    }

    #[test]
    fn a_pan_surface_that_fails_to_draw_is_reconfigured_and_not_shown() {
        let mut rig = Rig::pan_rig(Edge::Top);
        rig.view.renderer.pan_render_ok = false;
        rig.point_at(100, 200);
        rig.tick();
        assert!(rig.host.pan.is_empty());
        assert_eq!(rig.view.renderer.pan_calls.last(), Some(&PanGpu::Resize(1920, 810)));
        rig.view.renderer.pan_render_ok = true;
        rig.tick();
        assert_eq!(rig.host.pan, [true]);
    }

    #[test]
    fn the_pan_window_is_never_used_in_fullscreen() {
        let mut rig = Rig::new();
        rig.publish_frame();
        rig.set(|s| s.enabled = true);
        for (x, y) in [(100, 5), (960, 540), (1900, 1070)] {
            rig.point_at(x, y);
            rig.settle();
        }
        assert!(rig.host.pan.is_empty());
        assert!(rig.view.renderer.pan_calls.is_empty());
    }

    #[test]
    fn a_thin_uncovered_strip_still_pans() {
        let mut rig = Rig::new();
        rig.publish_frame();
        rig.set(|s| {
            s.display_mode = DisplayMode::Docked(Edge::Top);
            s.panel_size = cv_core::PANEL_SIZE_MAX;
            s.enabled = true;
        });
        // 90%: still 108 px uncovered, so it pans; the pointer must be in the strip below.
        rig.point_at(100, 980);
        rig.settle();
        assert_eq!(rig.host.pan, [true]);
    }

    #[test]
    fn the_system_cursor_is_hidden_while_the_pan_window_shows() {
        let mut rig = Rig::pan_rig(Edge::Top);
        // Below the 270 px panel, so Windows' cursor would stay visible without the pan.
        rig.point_at(100, 300);
        rig.tick();
        assert_eq!(rig.host.pan, [true]);
        assert_eq!(rig.host.cursor, [false]);
        rig.point_at(100, 900);
        rig.settle();
        assert_eq!(rig.host.pan, [true, false]);
        assert_eq!(rig.host.cursor, [false, true]);
    }

    #[test]
    fn the_system_cursor_stays_hidden_when_the_pointer_goes_from_the_pan_into_the_panel() {
        let mut rig = Rig::pan_rig(Edge::Top);
        rig.point_at(100, 300);
        rig.settle();
        assert_eq!(rig.host.cursor, [false]);
        // Into the panel: the cursor stays hidden for the panel's sake, and the pan holds full.
        rig.point_at(100, 100);
        rig.settle();
        assert_eq!(rig.host.cursor, [false]);
        assert!(approx(rig.view.pan_offset, 251.0));
    }

    #[test]
    fn the_pan_pointer_is_the_same_shape_at_1x_in_the_same_colours() {
        let mut rig = Rig::pan_rig(Edge::Top);
        rig.set(|s| {
            s.pointer_size = 3.0;
            s.pointer_fill = [10, 20, 30];
        });
        rig.host.look = PointerLook::Shape(arrow());
        rig.point_at(100, 200);
        rig.settle();
        let panel = rig.view.last_uniforms.unwrap().pointer;
        let pan = rig.pan_uniforms().unwrap().pointer;
        assert_eq!((panel.kind, panel.size), (POINTER_SHAPE, 3.0));
        assert_eq!((pan.kind, pan.size), (POINTER_SHAPE, 1.0));
        assert_eq!(pan.fill, panel.fill);
        assert_eq!(pan.fill, rgba([10, 20, 30]));
        assert_eq!((pan.mask, pan.hotspot, pan.outline), (panel.mask, panel.hotspot, panel.outline));

        // A hidden pointer draws nothing in either.
        rig.host.look = PointerLook::Hidden;
        rig.tick();
        assert_eq!(rig.pan_uniforms().unwrap().pointer.kind, POINTER_NONE);
    }

    #[test]
    fn the_pan_window_shows_again_after_the_monitors_change() {
        let mut rig = Rig::pan_rig(Edge::Top);
        rig.point_at(100, 200);
        rig.settle();
        assert_eq!(rig.host.pan, [true]);
        // The primary gets taller: the panel is placed again, and so is the pan window.
        let taller = OutputInfo { idx: 0, left: 0, top: 0, width: 1920, height: 1200 };
        rig.view.outputs_changed(vec![taller, second()]);
        rig.settle();
        assert_eq!(rig.host.pan, [true, true]);
        assert!(rig.view.renderer.pan_calls.contains(&PanGpu::Resize(1920, 900)));
    }

    #[test]
    fn turning_the_magnifier_off_ends_the_pan() {
        let mut rig = Rig::pan_rig(Edge::Top);
        rig.point_at(100, 200);
        rig.settle();
        rig.set(|s| s.enabled = false);
        rig.tick();
        // `apply_layout(Hidden)` hides the window itself; the magnifier does not call again.
        assert_eq!(rig.host.layouts.last(), Some(&Layout::Hidden));
        assert_eq!(rig.host.pan, [true]);
        assert!(!rig.view.pan_shown);
        rig.set(|s| s.enabled = true);
        rig.tick();
        assert_eq!(rig.host.pan, [true, true]);
    }

    #[test]
    fn unreadable_pointer_inside_the_panel_keeps_the_cursor_hidden() {
        let mut rig = Rig::new();
        rig.docked_top();
        rig.point_at(100, 100);
        assert_eq!(rig.cursor_step(), [false]);
        rig.host.pointer = None;
        assert!(rig.cursor_step().is_empty());
    }

    #[test]
    fn unreadable_pointer_from_the_start_stays_centred() {
        let mut rig = Rig::new();
        rig.host.pointer = None;
        rig.set(|s| {
            s.smooth_speed = 1.0;
            s.enabled = true;
        });
        rig.step();
        assert_eq!((rig.view.smooth_x, rig.view.smooth_y), (960.0, 540.0));
        assert_eq!(rig.desired.load(Ordering::Relaxed), 0);
    }

    /// Fullscreen at 10x (a 192x108 view on the primary), snapping to the target, one tick
    /// taken with the pointer resting at (960, 540).
    fn fullscreen_at_10x() -> Rig {
        let mut rig = Rig::new();
        rig.set(|s| {
            s.smooth_speed = 1.0;
            s.zoom = 10.0;
            s.enabled = true;
        });
        let (_, gpu) = rig.step();
        assert_eq!(Rig::cursor_written(&gpu), Some((960, 540)));
        rig
    }

    #[test]
    fn fullscreen_follows_a_caret_while_the_pointer_is_still() {
        let mut rig = fullscreen_at_10x();
        rig.caret_at(1500, 300);
        let (layouts, gpu) = rig.step();
        assert!(rig.events_drained());
        assert!(layouts.is_empty());
        assert_eq!(rig.smooth(), (1500.0, 300.0));
        // The pointer at (960, 540) is outside the view now: no circle.
        assert_eq!(Rig::cursor_written(&gpu), Some((CURSOR_HIDDEN, CURSOR_HIDDEN)));
    }

    #[test]
    fn the_circle_stays_on_the_pointer_while_the_view_is_on_the_caret() {
        let mut rig = Rig::new();
        rig.set(|s| {
            s.smooth_speed = 1.0;
            s.zoom = 2.0;
            s.enabled = true;
        });
        rig.step();
        // 340 px right is past the middle of a 960 px view: the view centres on the caret.
        rig.caret_at(1300, 540);
        let (_, gpu) = rig.step();
        assert_eq!(rig.smooth(), (1300.0, 540.0));
        // Crop 820..1780 x 270..810 at 2x: the pointer at (960, 540) is 280 px in.
        assert_eq!(Rig::cursor_written(&gpu), Some((280, 540)));
    }

    #[test]
    fn moving_the_pointer_takes_the_view_back_from_the_caret() {
        let mut rig = fullscreen_at_10x();
        rig.caret_at(1500, 300);
        rig.step();

        // A bump below the return threshold keeps the caret.
        rig.point_at(970, 540);
        rig.step();
        assert_eq!(rig.smooth(), (1500.0, 300.0));

        rig.point_at(990, 540);
        let (_, gpu) = rig.step();
        assert_eq!(rig.smooth(), (990.0, 540.0));
        assert_eq!(Rig::cursor_written(&gpu), Some((960, 540)));
    }

    #[test]
    fn a_caret_on_the_other_monitor_moves_capture_there() {
        let mut rig = fullscreen_at_10x();
        rig.caret_at(2500, 500);
        let (layouts, gpu) = rig.step();
        assert_eq!(rig.desired.load(Ordering::Relaxed), 1);
        assert_eq!(layouts, [Layout::Fullscreen { monitor: second() }]);
        assert_eq!(gpu[..2], [Gpu::Recreate(1280, 1024), Gpu::Resize(1280, 1024)]);
        assert_eq!(rig.smooth(), (2500.0, 500.0));

        // Moving the mouse brings the view, and capture, back to it.
        rig.point_at(900, 540);
        let (layouts, _) = rig.step();
        assert_eq!(rig.desired.load(Ordering::Relaxed), 0);
        assert_eq!(layouts, [Layout::Fullscreen { monitor: primary() }]);
        assert_eq!(rig.smooth(), (900.0, 540.0));
    }

    /// Docked on the top edge at 25% and 10x (a 1920x270 panel, a 192x27 view), snapping to the
    /// target, the pointer resting at (960, 540) below the panel.
    fn docked_at_10x() -> Rig {
        let mut rig = Rig::new();
        rig.docked_top();
        rig.set(|s| {
            s.smooth_speed = 1.0;
            s.zoom = 10.0;
        });
        rig.step();
        rig
    }

    #[test]
    fn docked_follows_a_caret_and_the_pointer_takes_it_back() {
        let mut rig = docked_at_10x();
        rig.caret_at(1500, 300);
        let (layouts, gpu) = rig.step();
        assert!(rig.events_drained());
        assert!(layouts.is_empty());
        assert_eq!(rig.smooth(), (1500.0, 300.0));
        assert_eq!(Rig::cursor_written(&gpu), Some((CURSOR_HIDDEN, CURSOR_HIDDEN)));

        rig.point_at(970, 540); // a bump keeps the caret
        rig.step();
        assert_eq!(rig.smooth(), (1500.0, 300.0));
        rig.point_at(990, 540);
        rig.step();
        assert_eq!(rig.smooth(), (990.0, 540.0));
    }

    #[test]
    fn the_docked_view_keeps_the_caret_inside_the_panel_not_the_monitor() {
        // 15 px down is inside the middle of a 108 px fullscreen view at 10x, but past the
        // middle of the 27 px docked one.
        let mut full = fullscreen_at_10x();
        full.caret_at(1500, 300);
        full.step();
        full.caret_at(1500, 315);
        full.step();
        assert_eq!(full.smooth(), (1500.0, 300.0));

        let mut docked = docked_at_10x();
        docked.caret_at(1500, 300);
        docked.step();
        docked.caret_at(1500, 315);
        docked.step();
        assert_eq!(docked.smooth(), (1500.0, 315.0));
    }

    #[test]
    fn a_docked_caret_on_the_other_monitor_moves_capture_but_not_the_panel() {
        let mut rig = docked_at_10x();
        rig.caret_at(2500, 500);
        let (layouts, gpu) = rig.step();
        assert_eq!(rig.desired.load(Ordering::Relaxed), 1);
        assert!(layouts.is_empty());
        assert_eq!(gpu.first(), Some(&Gpu::Recreate(1280, 1024)));
        assert_eq!(count(&gpu, |c| matches!(c, Gpu::Resize(..))), 0);
        assert_eq!(rig.smooth(), (2500.0, 500.0));
    }

    #[test]
    fn docked_cursor_hiding_follows_the_pointer_while_the_view_is_on_the_caret() {
        let mut rig = docked_at_10x();
        rig.host.cursor.clear();
        rig.point_at(100, 100); // inside the panel
        assert_eq!(rig.cursor_step(), [false]);
        // Still for longer than the quiet time, so a caret can take the view.
        for _ in 0..20 {
            rig.caret_at(1500, 700);
            assert!(rig.cursor_step().is_empty());
        }
        assert_eq!(rig.view.tracker.following(), cv_core::Following::Caret);
        assert_eq!(rig.smooth(), (1500.0, 700.0));
        assert!(rig.cursor_step().is_empty()); // still inside: still hidden

        rig.point_at(100, 300); // out of the panel, and past the return threshold
        assert_eq!(rig.cursor_step(), [true]);
        assert_eq!(rig.smooth(), (100.0, 300.0));
    }

    #[test]
    fn switching_modes_keeps_the_view_on_the_caret() {
        let mut rig = fullscreen_at_10x();
        rig.caret_at(1500, 300);
        rig.step();

        rig.set(|s| s.display_mode = DisplayMode::Docked(Edge::Top));
        let (layouts, _) = rig.step();
        assert_eq!(layouts, [docked(primary(), Edge::Top, 540)]); // the default 50%
        assert_eq!(rig.smooth(), (1500.0, 300.0));

        rig.set(|s| s.display_mode = DisplayMode::Fullscreen);
        rig.step();
        assert_eq!(rig.smooth(), (1500.0, 300.0));
    }

    #[test]
    fn disabled_drains_carets_and_coming_back_starts_on_the_pointer() {
        let mut rig = fullscreen_at_10x();
        rig.caret_at(1500, 300);
        rig.step();
        assert_eq!(rig.smooth(), (1500.0, 300.0));

        rig.set(|s| s.enabled = false);
        rig.caret_at(1600, 300);
        rig.step();
        assert!(rig.events_drained());

        rig.set(|s| s.enabled = true);
        rig.step();
        assert_eq!(rig.smooth(), (960.0, 540.0));
    }

    // ── Monitors connected or disconnected (ADR 0009) ───────────────────────────

    #[test]
    fn the_active_monitor_removed_falls_back_to_the_primary() {
        let mut rig = Rig::new();
        rig.set(|s| s.enabled = true);
        rig.step();
        rig.point_at(2000, 100);
        rig.step();
        assert_eq!(rig.desired.load(Ordering::Relaxed), 1);

        rig.view.outputs_changed(vec![primary()]);
        // The view jumps to the primary's centre; it does not slide there.
        assert_eq!(rig.smooth(), (960.0, 540.0));
        assert_eq!(rig.desired.load(Ordering::Relaxed), 0);
        // Windows moves the pointer onto the monitor that is left.
        rig.point_at(100, 100);
        let (layouts, gpu) = rig.step();
        assert_eq!(layouts, [Layout::Fullscreen { monitor: primary() }]);
        assert_eq!(gpu[0], Gpu::Recreate(1920, 1080));
        assert!(gpu.contains(&Gpu::Resize(1920, 1080)));
        assert_eq!(rig.desired.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_caret_left_on_the_removed_monitor_does_not_pull_the_view_off_screen() {
        let mut rig = Rig::new();
        rig.set(|s| s.enabled = true);
        rig.step();
        rig.caret_at(2000, 100);
        for _ in 0..30 {
            rig.tick();
        }
        assert_eq!(rig.view.active.idx, 1, "the caret took the view to the second monitor");

        rig.view.outputs_changed(vec![primary()]);
        rig.tick();
        assert_eq!(rig.view.target, (960.0, 540.0), "back on the pointer");
        assert_eq!(rig.view.active.idx, 0);
    }

    #[test]
    fn the_primary_removed_moves_the_docked_panel_and_rewrites_the_index() {
        let mut rig = Rig::new();
        rig.set(|s| {
            s.display_mode = DisplayMode::Docked(Edge::Top);
            s.enabled = true;
        });
        let (layouts, _) = rig.step();
        assert_eq!(layouts, [docked(primary(), Edge::Top, 540)]);

        // The second monitor becomes output 0 at (0, 0), the same number as before.
        let left = OutputInfo { idx: 0, left: 0, top: 0, width: 1280, height: 1024 };
        rig.desired.store(7, Ordering::Relaxed);
        rig.view.outputs_changed(vec![left.clone()]);
        assert_eq!(rig.desired.load(Ordering::Relaxed), 0);
        let (layouts, gpu) = rig.step();
        assert_eq!(layouts, [docked(left, Edge::Top, 512)]);
        assert_eq!(gpu[0], Gpu::Recreate(1280, 1024));
    }

    #[test]
    fn a_monitor_added_is_reachable_and_changes_no_layout() {
        let mut rig = Rig::with(primary(), vec![primary()]);
        rig.set(|s| s.enabled = true);
        rig.step();
        rig.point_at(2000, 100);
        let (layouts, _) = rig.step();
        assert!(layouts.is_empty(), "no monitor there yet");
        assert_eq!(rig.view.active.idx, 0);

        // The pointer comes back before the monitor appears.
        rig.point_at(1900, 100);
        rig.step();
        rig.view.outputs_changed(vec![primary(), second()]);
        let (layouts, gpu) = rig.step();
        assert!(layouts.is_empty(), "the active monitor is unchanged: {layouts:?}");
        assert_eq!(rig.view.active.idx, 0);
        assert_eq!(rig.view.outputs.len(), 2);
        assert_eq!(count(&gpu, |c| matches!(c, Gpu::Resize(..))), 0);

        // The view follows the pointer onto the new monitor as onto any other.
        rig.point_at(2000, 100);
        let (layouts, _) = rig.step();
        assert_eq!(layouts, [Layout::Fullscreen { monitor: second() }]);
        assert_eq!(rig.desired.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn the_active_monitor_changing_resolution_places_the_window_and_uploads_again() {
        let mut rig = Rig::new();
        rig.set(|s| s.enabled = true);
        rig.step();
        rig.publish_frame();
        rig.step();

        let smaller = OutputInfo { idx: 0, left: 0, top: 0, width: 1280, height: 720 };
        rig.view.outputs_changed(vec![smaller.clone(), second()]);
        let (layouts, gpu) = rig.step();
        assert_eq!(layouts, [Layout::Fullscreen { monitor: smaller }]);
        assert_eq!(gpu[0], Gpu::Recreate(1280, 720));
        assert!(gpu.contains(&Gpu::Resize(1280, 720)));

        // The capture thread's next frame has the new size, and the texture takes it.
        rig.publish_frame();
        let (_, gpu) = rig.step();
        assert_eq!(count(&gpu, |c| matches!(c, Gpu::Upload(1280, 720))), 1);
    }

    #[test]
    fn an_unchanged_list_does_nothing() {
        let mut rig = Rig::new();
        rig.set(|s| s.enabled = true);
        rig.step();
        rig.desired.store(5, Ordering::Relaxed);
        rig.view.outputs_changed(vec![primary(), second()]);
        assert_eq!(rig.desired.load(Ordering::Relaxed), 5);
        assert!(rig.view.renderer.calls.is_empty(), "nothing reached the renderer");
        let (layouts, gpu) = rig.step();
        assert!(layouts.is_empty());
        assert_eq!(count(&gpu, |c| matches!(c, Gpu::Recreate(..))), 0);
    }

    #[test]
    fn a_change_while_the_window_is_shown_then_disabled_still_hides_it() {
        let mut rig = Rig::new();
        rig.set(|s| s.enabled = true);
        rig.step();

        let smaller = OutputInfo { idx: 0, left: 0, top: 0, width: 1280, height: 720 };
        rig.view.outputs_changed(vec![smaller.clone(), second()]);
        rig.set(|s| s.enabled = false);
        let (layouts, _) = rig.step();
        assert_eq!(layouts, [Layout::Hidden]);

        rig.set(|s| s.enabled = true);
        let (layouts, _) = rig.step();
        assert_eq!(layouts, [Layout::Fullscreen { monitor: smaller }]);
    }

    fn snap(following: Following, pending: Option<Instant>, target: (f32, f32)) -> TrackSnap {
        TrackSnap { following, pending, target }
    }

    #[test]
    fn track_changes_names_a_held_focus_rect_and_its_outcome() {
        let t0 = Instant::now();
        let now = t0 + Duration::from_millis(270);
        let pointer = ScreenPoint { x: 1100, y: 500 };
        let view = (256.0, 144.0);
        let on_pointer = snap(Following::Pointer, None, (1100.0, 500.0));
        let held = snap(Following::Pointer, Some(t0), (1100.0, 500.0));

        let lines = track_changes(on_pointer, held, now, pointer, view);
        assert_eq!(lines, ["[track] focus rect held (pointer moving)"]);

        let applied = snap(Following::Caret, None, (2076.8, 815.0));
        let lines = track_changes(held, applied, now, pointer, view);
        assert_eq!(
            lines,
            [
                "[track] held focus rect applied after 270 ms",
                "[track] pointer -> caret, target (2077, 815), view 256x144, pointer (1100, 500)",
            ]
        );

        let lines = track_changes(held, on_pointer, now, pointer, view);
        assert_eq!(lines, ["[track] held focus rect dropped after 270 ms, pointer (1100, 500)"]);
    }

    #[test]
    fn track_changes_names_a_return_to_the_pointer_and_a_new_caret_target() {
        let now = Instant::now();
        let pointer = ScreenPoint { x: 1030, y: 500 };
        let caret = snap(Following::Caret, None, (1500.0, 500.0));
        let moved = snap(Following::Caret, None, (1580.0, 500.0));
        let back = snap(Following::Pointer, None, (1030.0, 500.0));
        let view = (256.0, 144.0);

        assert_eq!(track_changes(caret, moved, now, pointer, view), ["[track] caret target (1580, 500)"]);
        assert_eq!(track_changes(caret, back, now, pointer, view), ["[track] caret -> pointer at (1030, 500)"]);
        assert!(track_changes(caret, caret, now, pointer, view).is_empty());
    }
}
