//! The per-tick magnifier logic (ADR 0004). The backend owns the window, its thread and its
//! timer, and calls `Magnifier::tick` on each tick. The magnifier says what the window should
//! be (`Layout`); the backend makes it so.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Instant,
};

use cv_core::{
    AppState, DisplayMode, Edge, Frame, FrameState, OutputInfo, ScreenPoint, ScreenRect,
    SharedState,
    geometry::{self, panel_pct_to_px, window_dims},
};

use crate::{
    gfx::WgpuState,
    host::{Layout, OverlayHost},
};

/// The magnifier for one overlay window: smoothing, the active monitor, the applied layout and
/// the GPU upload and uniform caches.
pub struct Magnifier {
    view: View<WgpuState>,
}

impl Magnifier {
    /// Creates the wgpu surface on `host`'s window, sized to `start`, the monitor the window
    /// starts on.
    ///
    /// # Safety
    /// The window behind `host.raw_handles()` must stay valid until this `Magnifier` is dropped.
    pub unsafe fn new(
        host: &impl OverlayHost,
        start: OutputInfo,
        outputs: Vec<OutputInfo>,
        app_state: SharedState,
        frame_state: FrameState,
        desired_output: Arc<AtomicU32>,
    ) -> Self {
        let (display, window) = host.raw_handles();
        // SAFETY: the caller keeps the window valid for the life of `self`.
        let wgpu = unsafe {
            WgpuState::new(display, window, start.width, start.height, start.width, start.height)
        };
        Self {
            view: View::new(
                wgpu,
                start,
                outputs,
                app_state,
                frame_state,
                desired_output,
                Instant::now(),
            ),
        }
    }

    /// One tick: follow the pointer, apply any layout change through `host`, then draw.
    pub fn tick(&mut self, host: &mut impl OverlayHost, now: Instant) {
        self.view.tick(host, now);
    }

    /// The OS moved or resized the window outside `apply_layout` (the AppBar `ABN_POSCHANGED`
    /// path today). Must not be called from inside a `tick`.
    pub fn resized(&mut self, width: u32, height: u32) {
        self.view.resized(width, height);
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
    fn write_uniforms(&mut self, uniforms: &Uniforms);
    /// Draws and presents. False when the surface is lost or outdated.
    fn render(&mut self) -> bool;
}

/// Everything the shader's uniform buffer holds, compared to skip redundant writes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Uniforms {
    /// `[x, y, w, h]` as fractions of the frame.
    pub crop: [f32; 4],
    pub color_mode: u32,
    pub interp_mode: u32,
    /// Software cursor in output window pixels.
    pub cursor_x: u32,
    pub cursor_y: u32,
    pub edge_threshold: f32,
}

/// What was last applied to the window, as the magnifier decides when to apply again.
/// Docked compares edge and panel percentage, not the monitor, so a monitor switch while docked
/// leaves the panel where it is (STATUS Finding 10, roadmap 5.2).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Applied {
    Hidden,
    Fullscreen { output: u32 },
    Docked { edge: Edge, panel_pct: u32 },
}

struct View<R> {
    renderer: R,
    app_state: SharedState,
    frame_state: FrameState,
    /// All monitors, enumerated at startup (virtual-screen coordinates).
    outputs: Vec<OutputInfo>,
    /// The monitor being magnified.
    active: OutputInfo,
    /// Written here on a monitor switch, read by the capture loop.
    desired_output: Arc<AtomicU32>,
    /// Last pointer position read. Kept when a read fails (lock screen, UAC prompt), so the
    /// view stays put instead of drifting to (0, 0) (STATUS Finding 15).
    pointer: ScreenPoint,
    smooth_x: f32,
    smooth_y: f32,
    /// Time of the last enabled tick; smoothing measures `dt` from it.
    last_tick: Instant,
    applied: Applied,
    /// The docked panel's rect as last applied, `None` unless docked. Kept from the layout
    /// rather than recomputed, because a monitor switch while docked does not move the panel.
    panel: Option<ScreenRect>,
    /// True while `set_system_cursor(false)` is in force because the pointer is inside the
    /// panel. `apply_layout` resets the cursor, so this is cleared with every layout change.
    cursor_hidden_in_panel: bool,
    last_frame: Option<Arc<Frame>>,
    last_uniforms: Option<Uniforms>,
}

impl<R: Renderer> View<R> {
    fn new(
        renderer: R,
        start: OutputInfo,
        outputs: Vec<OutputInfo>,
        app_state: SharedState,
        frame_state: FrameState,
        desired_output: Arc<AtomicU32>,
        now: Instant,
    ) -> Self {
        let centre = ScreenPoint {
            x: start.left + start.width as i32 / 2,
            y: start.top + start.height as i32 / 2,
        };
        Self {
            renderer,
            app_state,
            frame_state,
            outputs,
            smooth_x: start.width as f32 / 2.0 + start.left as f32,
            smooth_y: start.height as f32 / 2.0 + start.top as f32,
            active: start,
            desired_output,
            pointer: centre,
            last_tick: now,
            applied: Applied::Hidden,
            panel: None,
            cursor_hidden_in_panel: false,
            last_frame: None,
            last_uniforms: None,
        }
    }

    fn tick(&mut self, host: &mut impl OverlayHost, now: Instant) {
        let state = self.app_state.read().clone();
        let Ok(frame) = self.frame_state.lock().map(|f| f.clone()) else {
            return;
        };

        if state.enabled {
            if let Some(p) = host.position() {
                self.pointer = p;
            }
            self.follow_pointer();
        }

        let want = match (state.enabled, state.display_mode) {
            (false, _) => Applied::Hidden,
            (true, DisplayMode::Fullscreen) => Applied::Fullscreen { output: self.active.idx },
            (true, DisplayMode::Docked(edge)) => {
                Applied::Docked { edge, panel_pct: state.panel_size }
            }
        };
        if want != self.applied {
            let layout = self.layout_for(want);
            let (w, h) = host.apply_layout(&layout);
            if want != Applied::Hidden {
                self.renderer.resize(w, h);
            }
            self.applied = want;
            self.panel = match &layout {
                Layout::Docked { monitor, edge, thickness } => {
                    Some(geometry::docked_rect(monitor, *edge, *thickness))
                }
                Layout::Hidden | Layout::Fullscreen { .. } => None,
            };
            self.cursor_hidden_in_panel = false;
        }

        if state.enabled {
            self.update_system_cursor(host);
            self.draw(&state, frame, now);
        }
    }

    /// While docked, hides the system cursor when the pointer is inside the panel and shows it
    /// when it leaves (ADR 0007); the circle marks the pointer in the magnified view. Calls the
    /// host only on a change. Fullscreen hiding is `apply_layout`'s job.
    fn update_system_cursor(&mut self, host: &mut impl OverlayHost) {
        let Some(panel) = self.panel else {
            return;
        };
        let inside = panel.contains(self.pointer);
        if inside != self.cursor_hidden_in_panel {
            host.set_system_cursor(!inside);
            self.cursor_hidden_in_panel = inside;
        }
    }

    fn resized(&mut self, width: u32, height: u32) {
        self.renderer.resize(width, height);
    }

    /// Switches the active monitor to the one under the pointer, if that changed.
    fn follow_pointer(&mut self) {
        let Some(target) = geometry::output_at(&self.outputs, self.pointer.x, self.pointer.y) else {
            return;
        };
        if target.idx == self.active.idx {
            return;
        }
        let target = target.clone();
        self.desired_output.store(target.idx, Ordering::Relaxed);
        self.renderer.recreate_frame_texture(target.width, target.height);
        self.last_frame = None; // the new texture needs the frame again
        self.active = target;
    }

    fn layout_for(&self, applied: Applied) -> Layout {
        let monitor = self.active.clone();
        match applied {
            Applied::Hidden => Layout::Hidden,
            Applied::Fullscreen { .. } => Layout::Fullscreen { monitor },
            Applied::Docked { edge, panel_pct } => {
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
    fn draw(&mut self, state: &AppState, frame: Option<Arc<Frame>>, now: Instant) {
        // Frame-rate-independent lerp toward the pointer.
        let dt = now.saturating_duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;
        let alpha = geometry::smooth_alpha(state.smooth_speed, dt);
        self.smooth_x = geometry::lerp_toward(self.smooth_x, self.pointer.x as f32, alpha);
        self.smooth_y = geometry::lerp_toward(self.smooth_y, self.pointer.y as f32, alpha);

        // Monitor-local coordinates: the DXGI frame origin is the monitor's top-left.
        let cx = geometry::to_monitor_local(self.smooth_x, self.active.left);
        let cy = geometry::to_monitor_local(self.smooth_y, self.active.top);

        let (win_w, win_h) = window_dims(
            state.display_mode,
            state.panel_size,
            self.active.width as i32,
            self.active.height as i32,
        );
        let (fw, fh) = self.renderer.frame_size();
        let crop = geometry::compute_crop(cx, cy, win_w, win_h, state.zoom, fw as f32, fh as f32);

        // Upload only when the capture thread has produced a new Arc<Frame>.
        if let Some(frame) = &frame {
            let new_frame = self.last_frame.as_ref().is_none_or(|last| !Arc::ptr_eq(last, frame));
            if new_frame {
                self.renderer.upload_frame(&frame.data, frame.width, frame.height);
                self.last_frame = Some(Arc::clone(frame));
            }
        }

        let (cursor_x, cursor_y) = geometry::cursor_in_output(cx, cy, &crop, win_w, win_h);
        let uniforms = Uniforms {
            crop: crop.normalized(),
            color_mode: state.color_filter.as_u32(),
            interp_mode: state.interpolation.as_u32(),
            cursor_x,
            cursor_y,
            edge_threshold: state.edge_threshold,
        };
        if self.last_uniforms != Some(uniforms) {
            self.renderer.write_uniforms(&uniforms);
            self.last_uniforms = Some(uniforms);
        }

        if !self.renderer.render() {
            // Surface lost or outdated: reconfigure to recover.
            self.renderer.resize(win_w, win_h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_core::{ColorFilter, Interpolation, PointerSource};
    use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
    use std::{
        sync::Mutex,
        time::Duration,
    };

    #[derive(Debug, Clone, PartialEq)]
    enum Gpu {
        Resize(u32, u32),
        Recreate(u32, u32),
        Upload(u32, u32),
        Uniforms(Uniforms),
        Render,
    }

    struct FakeRenderer {
        frame: (u32, u32),
        render_ok: bool,
        calls: Vec<Gpu>,
    }

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
        fn write_uniforms(&mut self, uniforms: &Uniforms) {
            self.calls.push(Gpu::Uniforms(*uniforms));
        }
        fn render(&mut self) -> bool {
            self.calls.push(Gpu::Render);
            self.render_ok
        }
    }

    /// Records each layout and system cursor call, and answers with the size the window would
    /// get.
    struct FakeHost {
        pointer: Option<ScreenPoint>,
        layouts: Vec<Layout>,
        cursor: Vec<bool>,
    }

    impl PointerSource for FakeHost {
        fn position(&self) -> Option<ScreenPoint> {
            self.pointer
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

        fn raw_handles(&self) -> (RawDisplayHandle, RawWindowHandle) {
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
        now: Instant,
    }

    impl Rig {
        fn new() -> Self {
            let state = cv_core::new_shared();
            let frames: FrameState = Arc::new(Mutex::new(None));
            let desired = Arc::new(AtomicU32::new(0));
            let now = Instant::now();
            let renderer = FakeRenderer { frame: (1920, 1080), render_ok: true, calls: Vec::new() };
            let view = View::new(
                renderer,
                primary(),
                vec![primary(), second()],
                state.clone(),
                frames.clone(),
                desired.clone(),
                now,
            );
            let host = FakeHost {
                pointer: Some(ScreenPoint { x: 960, y: 540 }),
                layouts: Vec::new(),
                cursor: Vec::new(),
            };
            Self { view, host, state, frames, desired, now }
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

    #[test]
    fn docked_monitor_switch_moves_capture_but_not_the_panel() {
        let mut rig = Rig::new();
        rig.set(|s| {
            s.display_mode = DisplayMode::Docked(Edge::Top);
            s.enabled = true;
        });
        rig.step();

        rig.point_at(2000, 100);
        let (layouts, gpu) = rig.step();
        assert_eq!(rig.desired.load(Ordering::Relaxed), 1);
        assert!(layouts.is_empty());
        assert_eq!(gpu.first(), Some(&Gpu::Recreate(1280, 1024)));
        assert_eq!(count(&gpu, |c| matches!(c, Gpu::Resize(..))), 0);
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
    fn resized_reaches_the_renderer() {
        let mut rig = Rig::new();
        rig.view.resized(800, 600);
        assert_eq!(rig.view.renderer.calls, [Gpu::Resize(800, 600)]);
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
}
