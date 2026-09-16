use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use crate::elements::{
    AppContext, Axis, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
    Vector2FExt,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, RectF, Vector2F};

/// How close to the end of the content counts as "at the end" when deciding
/// whether a following region keeps following.
const END_EPSILON: f32 = 0.5;

/// How fast a following region glides (points per second) before the glide's
/// duration is clamped. A jump of a few hundred points — a streamed answer —
/// takes its own time; a jump of the whole transcript is not allowed to take
/// longer than [`FOLLOW_EASE_MAX`].
const FOLLOW_EASE_SPEED: f32 = 3000.0;
/// The shortest and longest a follow glide runs. The floor keeps a small delta
/// visible as motion rather than as a step; the ceiling keeps a jump of
/// thousands of points from reading as a scroll.
const FOLLOW_EASE_MIN: f32 = 0.07;
const FOLLOW_EASE_MAX: f32 = 0.2;
/// How long a glide may sit with no frame before it counts as over. The frame
/// clock asks after a glide every frame, and a region that stopped being laid
/// out — its pane switched to another view — must not hold the clock at the
/// active rate forever.
const GLIDE_STALL: f32 = 1.0;

/// A follow glide in flight: the painted offset travelling from `from` to `to`
/// over `duration` seconds, eased out so it settles rather than stops.
#[derive(Clone, Copy, Debug)]
struct FollowEase {
    from: f32,
    to: f32,
    elapsed: f32,
    duration: f32,
}

/// Scroll offset plus the layout metrics needed to clamp it.
///
/// The tree is rebuilt every frame, so the offset cannot live on the element:
/// the caller owns one of these per scroll region, hands it to [`Scrollable`]
/// and keeps it across frames. Without one the region does not scroll (and is
/// not clipped), which is what the plain layout wrapper call sites rely on.
///
/// While a following region is pinned, content that arrives moves `offset` to
/// the new end at once — that is what the callers and the tests that hold a
/// scrollback position read — and the offset the region *paints* at chases it
/// with an ease ([`ScrollState::painted_offset`]), so a large arrival glides
/// into place instead of teleporting the transcript.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScrollState {
    offset: f32,
    content: f32,
    viewport: f32,
    /// Whether this region tails its content at all. Only
    /// [`ScrollState::following`] turns it on, so a plain scroller (the sidebar,
    /// the settings pane) never moves on its own.
    follow: bool,
    /// Whether the region is currently at the end of its content.
    pinned: bool,
    /// The offset the last paint used: `offset` chased by the follow ease.
    /// Equal to `offset` whenever no glide is in flight.
    painted: f32,
    /// The glide in flight, while a pinned region travels to newly arrived
    /// content.
    ease: Option<FollowEase>,
    /// Whether the region has been laid out at least once. A region that opens
    /// on content it has never measured (a pane mounting a long transcript)
    /// starts at the end instead of gliding there from the top.
    laid_out: bool,
    /// When this region was last laid out. The glide is driven by the frames
    /// that lay the region out, and the frame interval is only known here.
    last_layout: Option<Instant>,
}

impl ScrollState {
    /// A state that opens at the end of its content and follows the content as
    /// it grows — a transcript tailing a stream. Scrolling away from the end
    /// suspends following; scrolling back to the end resumes it.
    pub fn following() -> Self {
        Self {
            follow: true,
            pinned: true,
            ..Self::default()
        }
    }

    /// Current offset in points along the scroll axis.
    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// The offset to paint at this frame: the pin target chased by the follow
    /// ease, so content that arrives while following is glided to instead of
    /// teleported to. Equal to [`ScrollState::offset`] whenever no glide is in
    /// flight, and never past it: the glide reaches the end and stops there.
    pub fn painted_offset(&self) -> f32 {
        self.painted
    }

    /// Whether a follow glide is still in flight. The frame clock reads this to
    /// keep running at the active rate while the transcript is moving
    /// (`Element::wants_animation`); at the idle rate a glide would advance in
    /// 250ms steps and read as jank.
    pub fn is_animating(&self) -> bool {
        let Some(ease) = self.ease else {
            return false;
        };
        // A glide is driven by the frames that lay its region out, so one that
        // has had no frame for longer than it could possibly take is over: a
        // state nobody advances cannot hold the frame clock open.
        let since_last_frame = self
            .last_layout
            .map(|last| (Instant::now() - last).as_secs_f32())
            .unwrap_or(f32::INFINITY);
        since_last_frame <= ease.duration + GLIDE_STALL
    }

    /// Advance a follow glide by `dt` seconds. Frames drive this — the platform
    /// runs at 16ms while anything is animating — so the glide is time-based
    /// and settles within its own duration however fast the frames come.
    pub fn advance(&mut self, dt: f32) {
        let Some(ease) = self.ease.as_mut() else {
            return;
        };
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        ease.elapsed += dt;
        let progress = (ease.elapsed / ease.duration).clamp(0.0, 1.0);
        // Ease-out cubic: quick off the mark and settling at the end, monotone
        // over [0, 1], so the glide never passes the end it is aimed at.
        let travelled = 1.0 - (1.0 - progress).powi(3);
        self.painted = ease.from + (ease.to - ease.from) * travelled;
        if progress >= 1.0 {
            self.painted = ease.to;
            self.ease = None;
        }
    }

    /// Advance the glide to the frame's own clock, using the time since this
    /// region was last laid out.
    pub(crate) fn tick_at(&mut self, now: Instant) {
        let dt = self
            .last_layout
            .map(|last| (now - last).as_secs_f32())
            .unwrap_or(0.0);
        self.last_layout = Some(now);
        self.advance(dt);
    }

    /// Aim the glide at `target`, keeping the one already in flight when it is
    /// already going there. Re-aiming every frame would restart the curve and
    /// leave the end of a long jump creeping towards its target instead of
    /// arriving.
    fn aim_follow(&mut self, target: f32) {
        if self.ease.as_ref().is_some_and(|ease| ease.to == target) {
            return;
        }
        let distance = (target - self.painted).abs();
        let duration = (distance / FOLLOW_EASE_SPEED).clamp(FOLLOW_EASE_MIN, FOLLOW_EASE_MAX);
        self.ease = Some(FollowEase {
            from: self.painted,
            to: target,
            elapsed: 0.0,
            duration,
        });
    }

    /// How far the content can scroll before it runs out of viewport.
    pub fn max_offset(&self) -> f32 {
        (self.content - self.viewport).max(0.0)
    }

    /// How much room the region had at its last layout, along the scroll axis.
    ///
    /// A caller that draws only what fits — a file view standing the rest of a
    /// long body in as spacers — reads the room it has here, since the layout
    /// that measures it happens after the tree is built. Zero until the region
    /// has been laid out once.
    pub fn viewport(&self) -> f32 {
        self.viewport
    }

    /// Whether the region is currently pinned to the end of its content (a
    /// transcript in follow-the-stream mode).
    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    /// Scroll by `delta` points (positive scrolls the content up/left), clamped
    /// to the content extent. On a following region, leaving the end suspends
    /// following and reaching the end resumes it.
    pub fn scroll_by(&mut self, delta: f32) {
        let max = self.max_offset();
        self.offset = (self.offset + delta).clamp(0.0, max);
        // A wheel is the user's own hand: it lands where it lands, with no
        // glide in the way.
        self.painted = self.offset;
        self.ease = None;
        if self.follow {
            self.pinned = self.offset >= max - END_EPSILON;
        }
    }

    /// Return to the top/left of the content (a following region stops
    /// following, since the user asked for the start).
    pub fn reset(&mut self) {
        self.offset = 0.0;
        self.painted = 0.0;
        self.ease = None;
        self.pinned = false;
    }

    /// Record what the last layout measured and re-clamp the offset. A pinned
    /// region moves to the new end, so content that grows while the user is at
    /// the bottom is followed instead of pushing the bottom off screen; the
    /// offset it *paints* at glides there over a short glide, except on the
    /// region's first layout, which opens at the end it was pinned to.
    fn set_metrics(&mut self, content: f32, viewport: f32) {
        self.content = content;
        self.viewport = viewport;
        let max = self.max_offset();
        if self.pinned {
            if self.laid_out && (max - self.painted).abs() > END_EPSILON {
                self.aim_follow(max);
            } else {
                self.painted = max;
                self.ease = None;
            }
            self.offset = max;
        } else {
            self.offset = self.offset.clamp(0.0, max);
            self.painted = self.offset;
            self.ease = None;
        }
        self.laid_out = true;
    }
}

/// A scrollable region.
///
/// The child is laid out with unbounded space along the scroll axis and the
/// viewport itself fills the available space on that axis. With a
/// [`ScrollState`] attached the child is painted shifted by the offset and
/// clipped to the viewport, and wheel deltas over the viewport move it.
pub struct Scrollable {
    child: Box<dyn Element>,
    axis: Axis,
    state: Option<Rc<RefCell<ScrollState>>>,
    /// Whether content shorter than the viewport is flushed to the end of it
    /// instead of being drawn from the start. A shell's newest section and a
    /// transcript's newest message hug the input above them (warp-new), so a
    /// short conversation reads as content ending at the input rather than as
    /// content floating at the top of the pane.
    bottom_anchor: bool,
    /// The child's size from the last layout, which is what the flush above is
    /// measured against.
    content: Vector2F,
    /// Whether the pointer was over the viewport at the last paint. Wheel
    /// events carry no position, and the element instance survives from the
    /// paint to the event that follows it.
    pointer_over: bool,
    /// The pointer's last position, from the moves this region was told about.
    /// The last *paint* only knows where the pointer was when it painted, which
    /// can be a move behind: a wheel that arrives after the pointer moved into
    /// the region but before the next frame would otherwise be dropped.
    cursor: Option<Vector2F>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Scrollable {
    pub fn new(child: Box<dyn Element>, axis: Axis) -> Self {
        Self {
            child,
            axis,
            state: None,
            bottom_anchor: false,
            content: Vector2F::zero(),
            pointer_over: false,
            cursor: None,
            size: None,
            origin: None,
        }
    }

    /// Attach the caller-owned scroll state, turning this into a real scrolling
    /// viewport (offset + clip + wheel).
    pub fn with_state(mut self, state: Rc<RefCell<ScrollState>>) -> Self {
        self.state = Some(state);
        self
    }

    /// Flush content shorter than the viewport to the end of it. Only a region
    /// with a state has a viewport to flush against; without one this is inert.
    pub fn with_bottom_anchor(mut self, anchored: bool) -> Self {
        self.bottom_anchor = anchored;
        self
    }

    pub fn axis(&self) -> Axis {
        self.axis
    }

    fn viewport(&self) -> Vector2F {
        self.size.unwrap_or_else(Vector2F::zero)
    }

    /// The viewport's bounds at the last paint. Hit-testing outside the paint
    /// pass (a wheel) reads the same rect the paint clipped to.
    fn viewport_rect(&self) -> RectF {
        let origin = self
            .origin
            .map(|point| point.xy())
            .unwrap_or_else(Vector2F::zero);
        let viewport = self.viewport();
        rectf(origin.x, origin.y, viewport.x, viewport.y)
    }

    /// Whether the pointer is over this region: the position of the last move
    /// it saw, or the last paint's own reading when it has seen none.
    fn pointer_over(&self) -> bool {
        match self.cursor {
            Some(cursor) => crate::elements::interactive::contains(self.viewport_rect(), cursor),
            None => self.pointer_over,
        }
    }
}

impl Element for Scrollable {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let child_size = match self.axis {
            Axis::Vertical => self.child.layout(
                SizeConstraint::new(vec2f(0.0, 0.0), vec2f(constraint.max.x, f32::INFINITY)),
                ctx,
                app,
            ),
            Axis::Horizontal => self.child.layout(
                SizeConstraint::new(vec2f(0.0, 0.0), vec2f(f32::INFINITY, constraint.max.y)),
                ctx,
                app,
            ),
        };
        let viewport = match self.axis {
            Axis::Vertical => {
                if constraint.max.y.is_finite() {
                    vec2f(
                        child_size.x.min(constraint.max.x).max(constraint.min.x),
                        constraint.max.y,
                    )
                } else {
                    child_size
                }
            }
            Axis::Horizontal => {
                if constraint.max.x.is_finite() {
                    vec2f(
                        constraint.max.x,
                        child_size.y.min(constraint.max.y).max(constraint.min.y),
                    )
                } else {
                    child_size
                }
            }
        };
        if let Some(state) = self.state.as_ref() {
            let mut state = state.borrow_mut();
            state.set_metrics(child_size.along(self.axis), viewport.along(self.axis));
            // The glide a pinned region just started (or is in the middle of) is
            // frame-driven: the frame that measures the content is the one that
            // advances it, by the time that actually elapsed.
            state.tick_at(Instant::now());
        }
        self.content = child_size;
        self.size = Some(viewport);
        viewport
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if self.state.is_none() {
            // Plain layout wrapper: no viewport to clip to or scroll.
            self.child.paint(origin, ctx, app);
            return;
        }
        let viewport = self.viewport();
        let viewport_rect = rectf(origin.x, origin.y, viewport.x, viewport.y);
        self.pointer_over = ctx.hovered(viewport_rect);
        let offset = self
            .state
            .as_ref()
            .map(|state| state.borrow().painted_offset())
            .unwrap_or(0.0);
        // Content shorter than the viewport is flushed to its end: no offset
        // can express that (the scroll range is zero), so the child is painted
        // pushed down instead. Hit-testing follows the paint.
        let slack = if self.bottom_anchor {
            (viewport.along(self.axis) - self.content.along(self.axis)).max(0.0)
        } else {
            0.0
        };
        let child_origin = match self.axis {
            Axis::Vertical => vec2f(origin.x, origin.y - offset + slack),
            Axis::Horizontal => vec2f(origin.x - offset + slack, origin.y),
        };
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.clip_rect(viewport_rect);
        }
        self.child.paint(child_origin, ctx, app);
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.pop_clip();
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        if let DispatchedEvent::MouseMove { position } = event {
            // Wheel events carry no position, so the region hit-tests against
            // the last position it was told about rather than against where the
            // pointer was when the last paint ran.
            self.cursor = Some(*position);
        }
        if let DispatchedEvent::Scroll { delta } = event {
            let Some(state) = self.state.as_ref() else {
                return false;
            };
            if !self.pointer_over() {
                return false;
            }
            // The wheel reports how far the *content* should move, and positive
            // is right/down (winit's own contract, and AppKit's
            // `scrollingDeltaY` underneath it): a positive delta pulls the
            // content down, which walks the viewport back towards the start, so
            // the offset falls. Negating here is what makes a two-finger swipe
            // up scroll the transcript down on macOS, and the same sign gives
            // macOS' natural scrolling and a wheel the same direction.
            let delta = match self.axis {
                Axis::Vertical => delta.y,
                Axis::Horizontal => delta.x,
            };
            state.borrow_mut().scroll_by(-delta);
            return true;
        }
        // Hit-testing already sees the offset: the child was painted at
        // `origin - offset`, and element bounds come from the paint pass.
        self.child.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{Empty, Text};
    use crate::geometry::vec2f;

    #[test]
    fn vertical_scrollable_fills_viewport_height() {
        let app = AppContext::default();
        let mut scrollable = Scrollable::new(
            Empty::new().with_size(vec2f(100.0, 200.0)).finish(),
            Axis::Vertical,
        );
        let size = scrollable.layout(
            SizeConstraint::loose(vec2f(300.0, 300.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(
            size,
            vec2f(100.0, 300.0),
            "viewport should fill the scroll axis"
        );
    }

    #[test]
    fn scrollable_child_can_exceed_viewport() {
        let app = AppContext::default();
        let mut scrollable = Scrollable::new(
            Empty::new().with_size(vec2f(100.0, 800.0)).finish(),
            Axis::Vertical,
        );
        let size = scrollable.layout(
            SizeConstraint::loose(vec2f(300.0, 300.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(
            size,
            vec2f(100.0, 300.0),
            "viewport stays bounded even when content is taller"
        );
    }

    #[test]
    fn attached_state_records_the_scrollable_range() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(ScrollState::default()));
        let mut scrollable = Scrollable::new(
            Empty::new().with_size(vec2f(100.0, 800.0)).finish(),
            Axis::Vertical,
        )
        .with_state(Rc::clone(&state));
        scrollable.layout(
            SizeConstraint::loose(vec2f(300.0, 300.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(state.borrow().max_offset(), 500.0, "800 of content in 300");

        state.borrow_mut().scroll_by(1000.0);
        assert_eq!(state.borrow().offset(), 500.0, "clamped to the content");
        state.borrow_mut().scroll_by(-1000.0);
        assert_eq!(state.borrow().offset(), 0.0, "clamped at the top");
    }

    #[test]
    fn a_shrinking_viewport_re_clamps_the_offset() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(ScrollState::default()));
        let mut scrollable = Scrollable::new(
            Empty::new().with_size(vec2f(100.0, 400.0)).finish(),
            Axis::Vertical,
        )
        .with_state(Rc::clone(&state));
        scrollable.layout(
            SizeConstraint::loose(vec2f(300.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        state.borrow_mut().scroll_by(1000.0);
        assert_eq!(state.borrow().offset(), 300.0);
        scrollable.layout(
            SizeConstraint::loose(vec2f(300.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(
            state.borrow().offset(),
            0.0,
            "content now fits, so the offset resets"
        );
    }

    /// Lay out a fresh 100pt-tall region holding `content_height` of content,
    /// as the per-frame rebuild does, and record the metrics on `state`.
    fn layout_region(app: &AppContext, state: &Rc<RefCell<ScrollState>>, content_height: f32) {
        let mut scrollable = Scrollable::new(
            Empty::new()
                .with_size(vec2f(100.0, content_height))
                .finish(),
            Axis::Vertical,
        )
        .with_state(Rc::clone(state));
        scrollable.layout(
            SizeConstraint::loose(vec2f(300.0, 100.0)),
            &mut LayoutContext::default(),
            app,
        );
    }

    /// Short content in a bottom-anchored region is painted against the end of
    /// the viewport rather than against its start, and hit-testing follows it.
    #[test]
    fn a_bottom_anchored_region_flushes_short_content_to_the_end() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(ScrollState::following()));
        let mut scrollable = Scrollable::new(
            Text::new("only a line")
                .with_font_size(12.0)
                .finish(),
            Axis::Vertical,
        )
        .with_state(Rc::clone(&state))
        .with_bottom_anchor(true);
        let size = scrollable.layout(
            SizeConstraint::loose(vec2f(300.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size.y, 100.0, "the viewport fills the space it was given");

        let mut ctx = crate::elements::PaintContext::new(crate::render::Renderer::new());
        scrollable.paint(vec2f(0.0, 40.0), &mut ctx, &app);
        let commands = ctx.renderer.take().map(|r| r.commands().to_vec()).unwrap_or_default();
        let line = commands
            .iter()
            .find_map(|command| match command {
                crate::render::RenderCommand::DrawText { text, origin, .. }
                    if text == "only a line" =>
                {
                    Some(origin.y)
                }
                _ => None,
            })
            .expect("the content is drawn");
        assert!(
            line > 100.0,
            "the content sits at the end of the 40..140 viewport, not its start: {line}"
        );
    }

    #[test]
    fn a_following_region_opens_at_the_end_of_its_content() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(ScrollState::following()));
        layout_region(&app, &state, 400.0);
        assert_eq!(state.borrow().offset(), 300.0, "400 of content in 100");
        assert!(state.borrow().is_pinned());
        assert_eq!(
            state.borrow().painted_offset(),
            300.0,
            "a region that opens on content it has never measured starts at the end, without gliding there from the top"
        );
        assert!(!state.borrow().is_animating());
    }

    /// The frame rate's own step: the platform runs at 16ms while a scroll is
    /// animating.
    const FRAME: f32 = 0.016;

    /// Content that arrives while the region is following is glided to: the
    /// painted offset travels to the new end over a fraction of a second,
    /// monotonically, and stops exactly at the end.
    #[test]
    fn a_follow_glide_reaches_the_end_without_overshooting() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(ScrollState::following()));
        layout_region(&app, &state, 400.0);

        // A long result lands in one `chat:updated`: 2600 points of it.
        layout_region(&app, &state, 3000.0);
        let end = state.borrow().max_offset();
        assert_eq!(end, 2900.0, "3000 of content in a 100 tall viewport");
        assert_eq!(
            state.borrow().offset(),
            2900.0,
            "the pin target is the new end at once"
        );
        assert!(
            state.borrow().is_animating(),
            "the paint has not teleported to the new end"
        );
        assert!(state.borrow().painted_offset() < end);

        let mut previous = state.borrow().painted_offset();
        let mut frames = 0;
        while state.borrow().is_animating() {
            state.borrow_mut().advance(FRAME);
            let painted = state.borrow().painted_offset();
            assert!(
                painted >= previous,
                "the glide only ever moves towards the end: {previous} then {painted}"
            );
            assert!(
                painted <= end,
                "the glide must not overshoot the end: {painted} past {end}"
            );
            previous = painted;
            frames += 1;
            assert!(
                frames < 32,
                "the glide settles within a fraction of a second, still at {painted} after {}ms",
                frames * 16
            );
        }
        assert_eq!(previous, end, "and it arrives exactly at the end");
    }

    /// A glide in flight yields to the user: the wheel lands where it is put,
    /// with no glide between the hand and the view.
    #[test]
    fn a_wheel_cancels_a_follow_glide() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(ScrollState::following()));
        layout_region(&app, &state, 400.0);
        layout_region(&app, &state, 3000.0);
        assert!(state.borrow().is_animating());

        state.borrow_mut().scroll_by(-200.0);
        assert!(!state.borrow().is_animating(), "the glide is dropped");
        assert_eq!(state.borrow().offset(), 2700.0);
        assert_eq!(
            state.borrow().painted_offset(),
            2700.0,
            "the view follows the hand exactly"
        );

        layout_region(&app, &state, 3000.0);
        assert!(
            !state.borrow().is_animating(),
            "a held scrollback position does not glide"
        );
    }

    #[test]
    fn new_content_keeps_a_following_region_at_the_end() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(ScrollState::following()));
        layout_region(&app, &state, 400.0);
        layout_region(&app, &state, 900.0);
        assert_eq!(
            state.borrow().offset(),
            800.0,
            "content that arrives while pinned is followed"
        );
    }

    #[test]
    fn scrolling_up_holds_the_position_against_new_content() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(ScrollState::following()));
        layout_region(&app, &state, 400.0);
        state.borrow_mut().scroll_by(-200.0);
        assert_eq!(state.borrow().offset(), 100.0);
        assert!(!state.borrow().is_pinned(), "the user left the end");

        layout_region(&app, &state, 900.0);
        assert_eq!(
            state.borrow().offset(),
            100.0,
            "a scrollback position the user chose is held"
        );
    }

    #[test]
    fn scrolling_back_to_the_end_resumes_following() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(ScrollState::following()));
        layout_region(&app, &state, 400.0);
        state.borrow_mut().scroll_by(-200.0);
        state.borrow_mut().scroll_by(10_000.0);
        assert!(state.borrow().is_pinned(), "back at the end re-pins");

        layout_region(&app, &state, 900.0);
        assert_eq!(state.borrow().offset(), 800.0, "following resumed");
    }

    #[test]
    fn a_plain_state_never_follows_growing_content() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(ScrollState::default()));
        layout_region(&app, &state, 400.0);
        assert_eq!(
            state.borrow().offset(),
            0.0,
            "a plain region opens at the top"
        );
        state.borrow_mut().scroll_by(10_000.0);
        assert!(!state.borrow().is_pinned(), "a plain region never pins");
        layout_region(&app, &state, 900.0);
        assert_eq!(
            state.borrow().offset(),
            300.0,
            "growth does not move a plain region"
        );
    }

    #[test]
    fn wheel_over_the_viewport_scrolls_and_is_consumed() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(ScrollState::default()));
        let mut scrollable = Scrollable::new(
            Empty::new().with_size(vec2f(100.0, 400.0)).finish(),
            Axis::Vertical,
        )
        .with_state(Rc::clone(&state));
        scrollable.layout(
            SizeConstraint::loose(vec2f(300.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut ctx = crate::elements::PaintContext::new(crate::render::Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = vec2f(10.0, 10.0);
        scrollable.paint(vec2f(0.0, 0.0), &mut ctx, &app);

        // The delta is how far the *content* moves and positive is down
        // (winit's contract): pulling the content down walks back to the start,
        // so a positive delta leaves the region at the top, and a negative one
        // — the swipe that moves the content up — carries the offset down the
        // transcript.
        let mut wheel = |dy: f32| {
            let mut event_ctx = EventContext::default();
            scrollable.dispatch_event(
                &DispatchedEvent::Scroll {
                    delta: vec2f(0.0, dy),
                },
                &mut event_ctx,
                &app,
            )
        };
        assert!(wheel(30.0), "wheel inside the viewport scrolls the region");
        assert_eq!(
            state.borrow().offset(),
            0.0,
            "a delta that pulls the content down keeps the region at its start"
        );
        assert!(wheel(-30.0));
        assert_eq!(
            state.borrow().offset(),
            30.0,
            "and one that moves the content up carries the offset down the transcript"
        );
    }

    /// A wheel that arrives after the pointer moved into the region but before
    /// the next frame must scroll it: the last paint saw the pointer elsewhere,
    /// and the move is what says where it is now.
    #[test]
    fn a_wheel_after_the_pointer_moved_in_scrolls_the_region() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(ScrollState::following()));
        let mut scrollable = Scrollable::new(
            Empty::new().with_size(vec2f(100.0, 400.0)).finish(),
            Axis::Vertical,
        )
        .with_state(Rc::clone(&state));
        scrollable.layout(
            SizeConstraint::loose(vec2f(300.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        // The frame that painted ran with the pointer outside the transcript.
        let mut ctx = crate::elements::PaintContext::new(crate::render::Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = vec2f(500.0, 500.0);
        scrollable.paint(vec2f(0.0, 0.0), &mut ctx, &app);

        let mut event_ctx = EventContext::default();
        scrollable.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: vec2f(10.0, 10.0),
            },
            &mut event_ctx,
            &app,
        );
        let consumed = scrollable.dispatch_event(
            &DispatchedEvent::Scroll {
                delta: vec2f(0.0, 30.0),
            },
            &mut event_ctx,
            &app,
        );
        assert!(
            consumed,
            "the pointer is over the transcript now, so the wheel scrolls it"
        );
        assert_eq!(
            state.borrow().offset(),
            270.0,
            "the wheel walked the transcript 30 points back from its end"
        );
        assert!(!state.borrow().is_pinned(), "and the user left the end");
    }

    #[test]
    fn wheel_outside_the_viewport_is_ignored() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(ScrollState::default()));
        let mut scrollable = Scrollable::new(
            Empty::new().with_size(vec2f(100.0, 400.0)).finish(),
            Axis::Vertical,
        )
        .with_state(Rc::clone(&state));
        scrollable.layout(
            SizeConstraint::loose(vec2f(300.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut ctx = crate::elements::PaintContext::new(crate::render::Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = vec2f(500.0, 500.0);
        scrollable.paint(vec2f(0.0, 0.0), &mut ctx, &app);

        let mut event_ctx = EventContext::default();
        let consumed = scrollable.dispatch_event(
            &DispatchedEvent::Scroll {
                delta: vec2f(0.0, -30.0),
            },
            &mut event_ctx,
            &app,
        );
        assert!(!consumed, "the pointer is not over this region");
        assert_eq!(state.borrow().offset(), 0.0);
    }
}
