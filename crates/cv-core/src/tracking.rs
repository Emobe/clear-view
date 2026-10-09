//! Tracking policy (roadmap 4.5): what the view follows, the pointer or the caret.
//! Pure logic over the pointer sample and [`CoreEvent`]s; the magnifier eases toward the
//! returned target with `geometry::lerp_toward`, so a switch between targets is smoothed there.
//!
//! - The pointer is followed by default.
//! - `CaretMoved` switches to the caret, but only once the pointer has been still for
//!   [`POINTER_QUIET`], so dragging a selection or clicking into a box never fights the mouse.
//! - While on the caret, the pointer moving more than [`POINTER_RETURN_PX`] from where it rested
//!   switches back to the pointer. A caret event re-anchors that rest point only while the
//!   pointer is still, so bumps while typing don't add up but a slow, deliberate move does.
//! - The view holds while the caret stays inside the middle of the view ([`CARET_MARGIN`] off
//!   each side) and re-centres on it when it leaves, per axis.
//! - `CaretLost` holds the view where it is until the pointer moves or a new caret arrives.

use std::time::{Duration, Instant};

use crate::events::{CaretSource, CoreEvent};
use crate::{ScreenPoint, ScreenRect};

/// How long the pointer must be still before a caret event takes the view.
pub const POINTER_QUIET: Duration = Duration::from_millis(250);
/// Pointer movement up to this many physical pixels (either axis) is jitter, not motion.
pub const POINTER_JITTER_PX: i32 = 2;
/// While on the caret, the pointer moving farther than this (physical pixels, either axis) from
/// where it rested switches back to the pointer.
pub const POINTER_RETURN_PX: i32 = 24;
/// Fraction of the view on each side outside the middle area. The view holds while the caret is
/// inside the middle area. 0.5 always centres on the caret.
pub const CARET_MARGIN: f32 = 0.2;

/// What the view follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Following {
    Pointer,
    Caret,
}

#[derive(Debug, Clone, Copy, Default)]
enum Mode {
    #[default]
    Pointer,
    Caret {
        /// Where the pointer rested when caret tracking began, or at the last caret event that
        /// came while it was still.
        anchor: ScreenPoint,
        /// Virtual-screen point the view is centred on.
        target: (f32, f32),
    },
}

/// The tracking policy. One per view; call [`update`](Self::update) once per tick.
#[derive(Debug, Default)]
pub struct Tracker {
    mode: Mode,
    /// Where the pointer was when it last counted as moving. `None` before the first sample.
    rest: Option<ScreenPoint>,
    /// When the pointer last moved more than [`POINTER_JITTER_PX`] from `rest`. `None`: never.
    moved_at: Option<Instant>,
}

impl Tracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// One tick. `pointer` is this tick's sample, `events` the core events drained this tick in
    /// order, `view` the size of the magnified area in screen pixels (window size / zoom).
    /// Returns the virtual-screen point the view should ease toward.
    ///
    /// The pointer is looked at before the events, so when both move in one tick the pointer
    /// wins.
    pub fn update(
        &mut self,
        now: Instant,
        pointer: ScreenPoint,
        events: impl IntoIterator<Item = CoreEvent>,
        view: (f32, f32),
    ) -> (f32, f32) {
        self.observe_pointer(now, pointer);
        if let Mode::Caret { anchor, .. } = self.mode
            && distance(pointer, anchor) > POINTER_RETURN_PX
        {
            self.mode = Mode::Pointer;
        }
        for event in events {
            self.handle(&event, now, pointer, view);
        }
        match self.mode {
            Mode::Pointer => point(pointer),
            Mode::Caret { target, .. } => target,
        }
    }

    pub fn following(&self) -> Following {
        match self.mode {
            Mode::Pointer => Following::Pointer,
            Mode::Caret { .. } => Following::Caret,
        }
    }

    fn observe_pointer(&mut self, now: Instant, pointer: ScreenPoint) {
        match self.rest {
            Some(rest) if distance(pointer, rest) <= POINTER_JITTER_PX => {}
            Some(_) => {
                self.rest = Some(pointer);
                self.moved_at = Some(now);
            }
            None => self.rest = Some(pointer),
        }
    }

    fn pointer_still(&self, now: Instant) -> bool {
        self.moved_at.is_none_or(|t| now.saturating_duration_since(t) >= POINTER_QUIET)
    }

    fn handle(&mut self, event: &CoreEvent, now: Instant, pointer: ScreenPoint, view: (f32, f32)) {
        match event {
            CoreEvent::CaretMoved { rect, source, .. } => {
                let still = self.pointer_still(now);
                self.mode = match self.mode {
                    // The view is on the pointer, so the caret is measured from there.
                    Mode::Pointer if still => Mode::Caret {
                        anchor: pointer,
                        target: caret_target(point(pointer), *rect, *source, view, CARET_MARGIN),
                    },
                    Mode::Pointer => Mode::Pointer,
                    Mode::Caret { anchor, target } => Mode::Caret {
                        anchor: if still { pointer } else { anchor },
                        target: caret_target(target, *rect, *source, view, CARET_MARGIN),
                    },
                };
            }
            // Hold: the view stays where the caret was until the pointer moves or a caret
            // comes back.
            CoreEvent::CaretLost { .. } => {}
        }
    }
}

/// Where to centre the view for a caret `rect`, given the current centre.
fn caret_target(
    current: (f32, f32),
    rect: ScreenRect,
    source: CaretSource,
    view: (f32, f32),
    margin: f32,
) -> (f32, f32) {
    match source {
        // A whole control, not a caret: show it, or its start when it is too big.
        CaretSource::FocusRect => (
            show_span(rect.left, rect.width, view.0, margin),
            show_span(rect.top, rect.height, view.1, margin),
        ),
        CaretSource::Msaa | CaretSource::Uia | CaretSource::Gui => (
            keep_in_middle(current.0, rect.left as f32 + rect.width as f32 / 2.0, view.0, margin),
            keep_in_middle(current.1, rect.top as f32 + rect.height as f32 / 2.0, view.1, margin),
        ),
    }
}

/// One axis: keep the centre while `caret` is inside the middle area of a view of `size`
/// centred on `current`, else centre on `caret`.
fn keep_in_middle(current: f32, caret: f32, size: f32, margin: f32) -> f32 {
    let half_middle = (size * (0.5 - margin)).max(0.0);
    if (caret - current).abs() <= half_middle { current } else { caret }
}

/// One axis: centre on a span that fits inside the middle area; otherwise put its start
/// (left or top, where text starts) at the start of the middle area.
fn show_span(start: i32, len: u32, size: f32, margin: f32) -> f32 {
    let middle = (size * (1.0 - 2.0 * margin)).max(0.0);
    if len as f32 <= middle {
        start as f32 + len as f32 / 2.0
    } else {
        start as f32 + size * (0.5 - margin).max(0.0)
    }
}

/// The larger of the x and y distances.
fn distance(a: ScreenPoint, b: ScreenPoint) -> i32 {
    let dx = (i64::from(a.x) - i64::from(b.x)).abs();
    let dy = (i64::from(a.y) - i64::from(b.y)).abs();
    dx.max(dy).min(i64::from(i32::MAX)) as i32
}

fn point(p: ScreenPoint) -> (f32, f32) {
    (p.x as f32, p.y as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::AppId;

    /// 2560x1440 at 10x. The middle area reaches 76.8 px either side in x and 43.2 px in y.
    const VIEW: (f32, f32) = (256.0, 144.0);

    fn approx(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    fn app() -> AppId {
        AppId { pid: 7, exe: "notepad.exe".into() }
    }

    fn p(x: i32, y: i32) -> ScreenPoint {
        ScreenPoint { x, y }
    }

    /// A 2x20 caret centred on (x, y).
    fn caret(x: i32, y: i32) -> CoreEvent {
        CoreEvent::CaretMoved {
            at: Instant::now(),
            rect: ScreenRect { left: x - 1, top: y - 10, width: 2, height: 20 },
            source: CaretSource::Msaa,
            app: app(),
        }
    }

    fn focus(left: i32, top: i32, width: u32, height: u32) -> CoreEvent {
        CoreEvent::CaretMoved {
            at: Instant::now(),
            rect: ScreenRect { left, top, width, height },
            source: CaretSource::FocusRect,
            app: app(),
        }
    }

    fn lost() -> CoreEvent {
        CoreEvent::CaretLost { at: Instant::now(), app: app() }
    }

    struct Rig {
        tracker: Tracker,
        now: Instant,
    }

    impl Rig {
        /// A tracker that has seen the pointer resting at (1000, 500).
        fn new() -> Self {
            let mut rig = Self { tracker: Tracker::new(), now: Instant::now() };
            rig.tick(0, p(1000, 500), []);
            rig
        }

        /// Advances `ms` and runs one update.
        fn tick(
            &mut self,
            ms: u64,
            pointer: ScreenPoint,
            events: impl IntoIterator<Item = CoreEvent>,
        ) -> (f32, f32) {
            self.now += Duration::from_millis(ms);
            self.tracker.update(self.now, pointer, events, VIEW)
        }

        /// From the rest at (1000, 500), onto a caret far to the right: the view centres on
        /// (1500, 500).
        fn on_far_caret(&mut self) {
            let t = self.tick(300, p(1000, 500), [caret(1500, 500)]);
            assert_eq!(self.tracker.following(), Following::Caret);
            assert!(approx(t, (1500.0, 500.0)), "{t:?}");
        }

        fn following(&self) -> Following {
            self.tracker.following()
        }
    }

    #[test]
    fn starts_on_the_pointer() {
        let mut rig = Rig::new();
        assert_eq!(rig.following(), Following::Pointer);
        assert!(approx(rig.tick(16, p(1000, 500), []), (1000.0, 500.0)));
    }

    #[test]
    fn a_caret_while_the_pointer_is_still_takes_the_view() {
        let mut rig = Rig::new();
        rig.on_far_caret();
    }

    #[test]
    fn a_caret_on_first_sight_takes_the_view() {
        // No pointer motion seen yet counts as still.
        let mut tracker = Tracker::new();
        tracker.update(Instant::now(), p(0, 0), [caret(900, 0)], VIEW);
        assert_eq!(tracker.following(), Following::Caret);
    }

    #[test]
    fn a_caret_inside_the_middle_of_the_pointer_view_does_not_move_it() {
        let mut rig = Rig::new();
        let t = rig.tick(300, p(1000, 500), [caret(1070, 540)]);
        assert_eq!(rig.following(), Following::Caret);
        assert!(approx(t, (1000.0, 500.0)), "{t:?}");
    }

    #[test]
    fn a_caret_soon_after_pointer_motion_is_ignored() {
        let mut rig = Rig::new();
        rig.tick(100, p(1100, 500), []);
        let t = rig.tick(249, p(1100, 500), [caret(1500, 500)]);
        assert_eq!(rig.following(), Following::Pointer);
        assert!(approx(t, (1100.0, 500.0)));
        // Once the pointer has been still long enough, the next caret event takes the view.
        rig.tick(1, p(1100, 500), [caret(1500, 500)]);
        assert_eq!(rig.following(), Following::Caret);
    }

    #[test]
    fn jitter_is_not_motion() {
        let mut rig = Rig::new();
        rig.tick(100, p(1002, 498), []);
        rig.tick(16, p(1000, 500), [caret(1500, 500)]);
        assert_eq!(rig.following(), Following::Caret);
    }

    #[test]
    fn creeping_past_the_jitter_is_motion() {
        let mut rig = Rig::new();
        for x in 1001..=1003 {
            rig.tick(16, p(x, 500), []);
        }
        rig.tick(16, p(1003, 500), [caret(1500, 500)]);
        assert_eq!(rig.following(), Following::Pointer);
    }

    #[test]
    fn the_pointer_takes_the_view_back_past_the_threshold() {
        let mut rig = Rig::new();
        rig.on_far_caret();
        let t = rig.tick(16, p(1000 + POINTER_RETURN_PX, 500), []);
        assert_eq!(rig.following(), Following::Caret);
        assert!(approx(t, (1500.0, 500.0)));
        let t = rig.tick(16, p(1000, 500 - POINTER_RETURN_PX - 1), []);
        assert_eq!(rig.following(), Following::Pointer);
        assert!(approx(t, (1000.0, 475.0)), "{t:?}");
    }

    #[test]
    fn a_bump_then_stillness_is_re_anchored_by_the_next_caret() {
        let mut rig = Rig::new();
        rig.on_far_caret();
        rig.tick(16, p(1020, 500), []); // bumped while typing
        rig.tick(300, p(1020, 500), [caret(1510, 500)]); // still again: re-anchored at 1020
        rig.tick(16, p(1040, 500), []); // 40 from the first rest, 20 from the new one
        assert_eq!(rig.following(), Following::Caret);
    }

    #[test]
    fn a_slow_move_while_carets_arrive_is_not_re_anchored() {
        let mut rig = Rig::new();
        rig.on_far_caret();
        let mut x = 1000;
        while x <= 1000 + POINTER_RETURN_PX {
            assert_eq!(rig.following(), Following::Caret, "at {x}");
            x += 5;
            rig.tick(50, p(x, 500), [caret(1500 + x - 1000, 500)]);
        }
        assert_eq!(rig.following(), Following::Pointer);
    }

    #[test]
    fn the_pointer_wins_when_both_move_in_one_tick() {
        let mut rig = Rig::new();
        rig.on_far_caret();
        let t = rig.tick(16, p(1030, 500), [caret(1600, 500)]);
        assert_eq!(rig.following(), Following::Pointer);
        assert!(approx(t, (1030.0, 500.0)));
    }

    #[test]
    fn typing_holds_the_view_until_the_caret_leaves_the_middle() {
        let mut rig = Rig::new();
        rig.on_far_caret();
        // Right by 76 px and down by 43: inside the middle area.
        let t = rig.tick(100, p(1000, 500), [caret(1576, 543)]);
        assert!(approx(t, (1500.0, 500.0)), "{t:?}");
        // Past it on x only: x re-centres, y holds.
        let t = rig.tick(100, p(1000, 500), [caret(1580, 540)]);
        assert!(approx(t, (1580.0, 500.0)), "{t:?}");
        // A new line far down: y re-centres.
        let t = rig.tick(100, p(1000, 500), [caret(1580, 560)]);
        assert!(approx(t, (1580.0, 560.0)), "{t:?}");
    }

    #[test]
    fn a_margin_of_one_half_always_centres() {
        assert_eq!(keep_in_middle(100.0, 101.0, 256.0, 0.5), 101.0);
        assert_eq!(keep_in_middle(100.0, 99.0, 256.0, 0.5), 99.0);
        assert_eq!(keep_in_middle(100.0, 101.0, 256.0, CARET_MARGIN), 100.0);
    }

    #[test]
    fn a_focus_rect_that_fits_is_centred() {
        let mut rig = Rig::new();
        let t = rig.tick(300, p(1000, 500), [focus(2000, 800, 100, 30)]);
        assert_eq!(rig.following(), Following::Caret);
        assert!(approx(t, (2050.0, 815.0)), "{t:?}");
    }

    #[test]
    fn a_focus_rect_too_big_shows_its_start() {
        let mut rig = Rig::new();
        // Wider than the 153.6 px middle: the left edge goes 76.8 px left of centre.
        let t = rig.tick(300, p(1000, 500), [focus(2000, 800, 1000, 30)]);
        assert!(approx(t, (2076.8, 815.0)), "{t:?}");
        // Taller than the 86.4 px middle: the top edge goes 43.2 px above centre.
        let t = rig.tick(16, p(1000, 500), [focus(2000, 800, 50, 500)]);
        assert!(approx(t, (2025.0, 843.2)), "{t:?}");
    }

    #[test]
    fn a_focus_rect_ignores_the_current_view() {
        // Inside the pointer view's middle, but still centred on: it is a control, not a caret.
        let mut rig = Rig::new();
        let t = rig.tick(300, p(1000, 500), [focus(1000, 500, 40, 20)]);
        assert!(approx(t, (1020.0, 510.0)), "{t:?}");
    }

    #[test]
    fn a_lost_caret_holds_the_view_until_the_pointer_moves() {
        let mut rig = Rig::new();
        rig.on_far_caret();
        let t = rig.tick(16, p(1000, 500), [lost()]);
        assert_eq!(rig.following(), Following::Caret);
        assert!(approx(t, (1500.0, 500.0)));
        let t = rig.tick(500, p(1000, 500), []);
        assert!(approx(t, (1500.0, 500.0)));
        let t = rig.tick(16, p(1100, 500), []);
        assert_eq!(rig.following(), Following::Pointer);
        assert!(approx(t, (1100.0, 500.0)));
    }

    #[test]
    fn a_lost_caret_on_the_pointer_changes_nothing() {
        let mut rig = Rig::new();
        let t = rig.tick(300, p(1000, 500), [lost()]);
        assert_eq!(rig.following(), Following::Pointer);
        assert!(approx(t, (1000.0, 500.0)));
    }

    #[test]
    fn moved_then_lost_in_one_tick_holds_on_the_caret() {
        let mut rig = Rig::new();
        let t = rig.tick(300, p(1000, 500), [caret(1500, 500), lost()]);
        assert_eq!(rig.following(), Following::Caret);
        assert!(approx(t, (1500.0, 500.0)));
    }

    #[test]
    fn back_on_the_pointer_a_later_caret_takes_the_view_again() {
        let mut rig = Rig::new();
        rig.on_far_caret();
        rig.tick(16, p(1200, 500), []);
        assert_eq!(rig.following(), Following::Pointer);
        let t = rig.tick(300, p(1200, 500), [caret(1700, 500)]);
        assert_eq!(rig.following(), Following::Caret);
        assert!(approx(t, (1700.0, 500.0)));
    }

    #[test]
    fn a_caret_on_a_monitor_left_of_the_primary_is_followed() {
        let mut rig = Rig::new();
        let t = rig.tick(300, p(1000, 500), [caret(-1500, 300)]);
        assert!(approx(t, (-1500.0, 300.0)), "{t:?}");
        // And the return threshold works across negative coordinates.
        let mut tracker = Tracker::new();
        let now = Instant::now();
        tracker.update(now, p(-1000, -200), [caret(-1500, 300)], VIEW);
        tracker.update(now, p(-1000 - POINTER_RETURN_PX - 1, -200), [], VIEW);
        assert_eq!(tracker.following(), Following::Pointer);
    }
}
