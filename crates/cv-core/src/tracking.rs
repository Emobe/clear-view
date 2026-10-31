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
//! - A `FocusRect` event that comes while the pointer is still moving, as when clicking into a
//!   box, is kept for [`PENDING_FOR`] and takes the view once the pointer has been still for
//!   [`POINTER_QUIET`]. The source sends it once and never repeats it, so dropping it would lose
//!   it for good (Explorer address bar, roadmap 4.8a).
//! - While on the pointer, a `FocusRect` with the pointer inside it is a click into that box:
//!   the view is already where the user clicked, so it stays on the pointer. Showing the box's
//!   start instead pulled the view away from a click on the right of a wide box (roadmap 4.8a,
//!   your choice 2026-10-11). Keyboard focus (Ctrl+L, Tab) with the pointer elsewhere, or while
//!   on the caret, still moves the view to the box.

use std::sync::OnceLock;
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
/// How long a `FocusRect` event that came while the pointer was moving waits for the pointer to
/// settle. After this the control is no longer what the user just went to.
pub const PENDING_FOR: Duration = Duration::from_secs(1);

/// True when `CV_TRACK_LOG=1`: the caret thread logs every event it publishes and the magnifier
/// logs every event it drains and every tracking decision (roadmap 4.8a diagnostic). Read once.
pub fn log_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("CV_TRACK_LOG").is_ok_and(|v| v == "1"))
}

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

/// A `FocusRect` event held back until the pointer is still.
#[derive(Debug, Clone, Copy)]
struct Pending {
    rect: ScreenRect,
    /// When the event arrived.
    since: Instant,
    /// Where the pointer was then. Moving past [`POINTER_RETURN_PX`] from here drops the event.
    pointer: ScreenPoint,
}

/// The tracking policy. One per view; call [`update`](Self::update) once per tick.
#[derive(Debug, Default)]
pub struct Tracker {
    mode: Mode,
    /// Where the pointer was when it last counted as moving. `None` before the first sample.
    rest: Option<ScreenPoint>,
    /// When the pointer last moved more than [`POINTER_JITTER_PX`] from `rest`. `None`: never.
    moved_at: Option<Instant>,
    pending: Option<Pending>,
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
        self.apply_pending(now, pointer, view);
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

    /// When the `FocusRect` held back for the pointer to settle arrived; `None` when none is
    /// held. For the `CV_TRACK_LOG` lines.
    pub fn pending_since(&self) -> Option<Instant> {
        self.pending.map(|p| p.since)
    }

    /// Whether the pointer has been still for [`POINTER_QUIET`] as of the last
    /// [`update`](Self::update), measured at `now`.
    pub fn pointer_still(&self, now: Instant) -> bool {
        self.moved_at.is_none_or(|t| now.saturating_duration_since(t) >= POINTER_QUIET)
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

    /// Gives a held `FocusRect` the view once the pointer is still, or drops it when it is stale
    /// or the pointer has gone elsewhere.
    fn apply_pending(&mut self, now: Instant, pointer: ScreenPoint, view: (f32, f32)) {
        let Some(pending) = self.pending else { return };
        if now.saturating_duration_since(pending.since) > PENDING_FOR
            || distance(pointer, pending.pointer) > POINTER_RETURN_PX
            || pending.rect.contains(pointer)
            || !matches!(self.mode, Mode::Pointer)
        {
            self.pending = None;
        } else if self.pointer_still(now) {
            self.pending = None;
            self.mode = Mode::Caret {
                anchor: pointer,
                target: caret_target(
                    point(pointer),
                    pending.rect,
                    CaretSource::FocusRect,
                    view,
                    CARET_MARGIN,
                ),
            };
        }
    }

    fn handle(&mut self, event: &CoreEvent, now: Instant, pointer: ScreenPoint, view: (f32, f32)) {
        match event {
            CoreEvent::CaretMoved { rect, source, .. } => {
                let still = self.pointer_still(now);
                let clicked = *source == CaretSource::FocusRect && rect.contains(pointer);
                // The newest event is the one that counts.
                self.pending = (!still
                    && !clicked
                    && *source == CaretSource::FocusRect
                    && matches!(self.mode, Mode::Pointer))
                .then_some(Pending { rect: *rect, since: now, pointer });
                self.mode = match self.mode {
                    // Clicked into a box: the view is already on the click.
                    Mode::Pointer if clicked => Mode::Pointer,
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
            CoreEvent::CaretLost { .. } => self.pending = None,
            // The view does not follow focus until 9.9 (ADR 0015).
            CoreEvent::FocusChanged { .. } => {}
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

    fn focus_changed() -> CoreEvent {
        use crate::focus::{Control, FocusSource, Role, WindowInfo};
        let mut control = Control::new("OK", Role::Button, FocusSource::Both);
        control.rect = Some(ScreenRect { left: 1800, top: 900, width: 80, height: 24 });
        CoreEvent::FocusChanged {
            at: Instant::now(),
            app: app(),
            window: WindowInfo::new("Untitled"),
            window_changed: true,
            dialog_changed: false,
            control: Some(control),
        }
    }

    #[test]
    fn a_focus_event_changes_neither_the_mode_nor_the_target() {
        // On the pointer.
        let mut rig = Rig::new();
        let t = rig.tick(300, p(1000, 500), [focus_changed()]);
        assert_eq!(rig.following(), Following::Pointer);
        assert!(approx(t, (1000.0, 500.0)), "{t:?}");

        // On a caret.
        let mut rig = Rig::new();
        rig.on_far_caret();
        let t = rig.tick(300, p(1000, 500), [focus_changed()]);
        assert_eq!(rig.following(), Following::Caret);
        assert!(approx(t, (1500.0, 500.0)), "{t:?}");
    }

    #[test]
    fn a_focus_event_does_not_cancel_a_pending_focus_rect() {
        let mut with = Rig::new();
        let mut without = Rig::new();
        // The pointer moved a moment ago, so the focus rectangle waits (pending).
        with.tick(100, p(1100, 500), [focus(1500, 480, 200, 40)]);
        without.tick(100, p(1100, 500), [focus(1500, 480, 200, 40)]);
        let a = with.tick(20, p(1100, 500), [focus_changed()]);
        let b = without.tick(20, p(1100, 500), []);
        assert_eq!(with.following(), without.following());
        assert!(approx(a, b), "{a:?} {b:?}");
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
        let t = rig.tick(300, p(1000, 500), [focus(1010, 495, 40, 20)]);
        assert!(approx(t, (1030.0, 505.0)), "{t:?}");
    }

    #[test]
    fn a_focus_rect_under_the_resting_pointer_leaves_the_view_on_the_pointer() {
        // Clicked on the right of a box wider than the view: no jump to its left edge.
        let mut rig = Rig::new();
        let t = rig.tick(300, p(1000, 500), [focus(600, 490, 442, 32)]);
        assert_eq!(rig.following(), Following::Pointer);
        assert!(approx(t, (1000.0, 500.0)), "{t:?}");
        // Typing in the box keeps it there; a caret elsewhere later still takes the view.
        rig.tick(300, p(1000, 500), [focus(600, 490, 442, 33)]);
        assert_eq!(rig.following(), Following::Pointer);
        rig.tick(300, p(1000, 500), [caret(1500, 500)]);
        assert_eq!(rig.following(), Following::Caret);
    }

    #[test]
    fn a_focus_rect_under_the_moving_pointer_is_not_held() {
        let mut rig = Rig::new();
        rig.tick(100, p(1100, 500), []); // moving to click
        rig.tick(20, p(1100, 500), [focus(1000, 490, 442, 32)]);
        assert_eq!(rig.tracker.pending_since(), None);
        rig.tick(400, p(1100, 500), []);
        assert_eq!(rig.following(), Following::Pointer);
    }

    #[test]
    fn a_held_focus_rect_is_dropped_when_the_pointer_settles_inside_it() {
        let mut rig = Rig::new();
        rig.tick(100, p(1100, 500), []);
        rig.tick(20, p(1100, 500), [focus(1110, 490, 400, 32)]);
        assert!(rig.tracker.pending_since().is_some());
        rig.tick(16, p(1115, 500), []); // drifts into the box
        rig.tick(300, p(1115, 500), []);
        assert_eq!(rig.following(), Following::Pointer);
        assert_eq!(rig.tracker.pending_since(), None);
    }

    #[test]
    fn a_focus_rect_with_the_pointer_far_away_still_takes_the_view() {
        // Ctrl+L with the mouse parked elsewhere: the box's start is shown.
        let mut rig = Rig::new();
        let t = rig.tick(300, p(1000, 500), [focus(1241, 203, 442, 32)]);
        assert_eq!(rig.following(), Following::Caret);
        assert!(approx(t, (1241.0 + 76.8, 219.0)), "{t:?}");
    }

    #[test]
    fn on_the_caret_a_focus_rect_under_the_pointer_still_takes_the_view() {
        // Typing in Notepad with the mouse resting over the address bar, then Ctrl+L.
        let mut rig = Rig::new();
        rig.on_far_caret();
        let t = rig.tick(300, p(1000, 500), [focus(900, 490, 442, 32)]);
        assert_eq!(rig.following(), Following::Caret);
        assert!(approx(t, (976.8, 506.0)), "{t:?}");
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
    fn a_focus_rect_during_pointer_motion_takes_the_view_once_the_pointer_settles() {
        let mut rig = Rig::new();
        rig.tick(100, p(1100, 500), []); // moving to click
        let t = rig.tick(20, p(1100, 500), [focus(2000, 800, 100, 30)]);
        assert_eq!(rig.following(), Following::Pointer);
        assert!(approx(t, (1100.0, 500.0)));
        rig.tick(200, p(1100, 500), []);
        assert_eq!(rig.following(), Following::Pointer);
        let t = rig.tick(50, p(1100, 500), []);
        assert_eq!(rig.following(), Following::Caret);
        assert!(approx(t, (2050.0, 815.0)), "{t:?}");
        // Applied once: the pointer going back is not undone by it.
        rig.tick(16, p(1100, 600), []);
        assert_eq!(rig.following(), Following::Pointer);
        rig.tick(600, p(1100, 600), []);
        assert_eq!(rig.following(), Following::Pointer);
    }

    #[test]
    fn a_held_focus_rect_expires() {
        let mut rig = Rig::new();
        rig.tick(100, p(1100, 500), []);
        rig.tick(20, p(1100, 500), [focus(2000, 800, 100, 30)]);
        // The pointer keeps twitching within the return distance, so it never settles.
        for i in 0..70 {
            rig.tick(16, p(1100 + (i % 2) * 5, 500), []);
        }
        rig.tick(300, p(1100, 500), []);
        assert_eq!(rig.following(), Following::Pointer);
    }

    #[test]
    fn a_held_focus_rect_is_dropped_when_the_pointer_goes_elsewhere() {
        let mut rig = Rig::new();
        rig.tick(100, p(1100, 500), []);
        rig.tick(20, p(1100, 500), [focus(2000, 800, 100, 30)]);
        rig.tick(100, p(1100 + POINTER_RETURN_PX + 1, 500), []);
        rig.tick(400, p(1100 + POINTER_RETURN_PX + 1, 500), []);
        assert_eq!(rig.following(), Following::Pointer);
    }

    #[test]
    fn a_held_focus_rect_is_replaced_by_a_newer_event_and_cleared_by_caret_lost() {
        let mut rig = Rig::new();
        rig.tick(100, p(1100, 500), []);
        rig.tick(20, p(1100, 500), [focus(2000, 800, 100, 30)]);
        rig.tick(20, p(1100, 500), [focus(3000, 800, 100, 30)]);
        let t = rig.tick(300, p(1100, 500), []);
        assert!(approx(t, (3050.0, 815.0)), "{t:?}");

        let mut rig = Rig::new();
        rig.tick(100, p(1100, 500), []);
        rig.tick(20, p(1100, 500), [focus(2000, 800, 100, 30)]);
        rig.tick(20, p(1100, 500), [lost()]);
        rig.tick(300, p(1100, 500), []);
        assert_eq!(rig.following(), Following::Pointer);
    }

    #[test]
    fn pending_since_and_pointer_still_report_the_held_focus_rect() {
        let mut rig = Rig::new();
        assert!(rig.tracker.pointer_still(rig.now));
        assert_eq!(rig.tracker.pending_since(), None);
        rig.tick(100, p(1100, 500), []);
        assert!(!rig.tracker.pointer_still(rig.now));
        rig.tick(20, p(1100, 500), [focus(2000, 800, 100, 30)]);
        assert_eq!(rig.tracker.pending_since(), Some(rig.now));
        rig.tick(300, p(1100, 500), []);
        assert!(rig.tracker.pointer_still(rig.now));
        assert_eq!(rig.tracker.pending_since(), None);
        assert_eq!(rig.following(), Following::Caret);
    }

    #[test]
    fn a_real_caret_during_pointer_motion_is_still_dropped() {
        let mut rig = Rig::new();
        rig.tick(100, p(1100, 500), []);
        rig.tick(20, p(1100, 500), [caret(1500, 500)]);
        rig.tick(400, p(1100, 500), []);
        assert_eq!(rig.following(), Following::Pointer);
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
