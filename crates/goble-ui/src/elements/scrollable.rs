use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Axis, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
    Vector2FExt,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, Vector2F};

/// How close to the end of the content counts as "at the end" when deciding
/// whether a following region keeps following.
const END_EPSILON: f32 = 0.5;

/// Scroll offset plus the layout metrics needed to clamp it.
///
/// The tree is rebuilt every frame, so the offset cannot live on the element:
/// the caller owns one of these per scroll region, hands it to [`Scrollable`]
/// and keeps it across frames. Without one the region does not scroll (and is
/// not clipped), which is what the plain layout wrapper call sites rely on.
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

    /// How far the content can scroll before it runs out of viewport.
    pub fn max_offset(&self) -> f32 {
        (self.content - self.viewport).max(0.0)
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
        if self.follow {
            self.pinned = self.offset >= max - END_EPSILON;
        }
    }

    /// Return to the top/left of the content (a following region stops
    /// following, since the user asked for the start).
    pub fn reset(&mut self) {
        self.offset = 0.0;
        self.pinned = false;
    }

    /// Record what the last layout measured and re-clamp the offset. A pinned
    /// region moves to the new end, so content that grows while the user is at
    /// the bottom is followed instead of pushing the bottom off screen.
    fn set_metrics(&mut self, content: f32, viewport: f32) {
        self.content = content;
        self.viewport = viewport;
        if self.pinned {
            self.offset = self.max_offset();
        } else {
            let max = self.max_offset();
            self.offset = self.offset.clamp(0.0, max);
        }
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
    /// Whether the pointer was over the viewport at the last paint. Wheel
    /// events carry no position, and the element instance survives from the
    /// paint to the event that follows it.
    pointer_over: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Scrollable {
    pub fn new(child: Box<dyn Element>, axis: Axis) -> Self {
        Self {
            child,
            axis,
            state: None,
            pointer_over: false,
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

    pub fn axis(&self) -> Axis {
        self.axis
    }

    fn offset(&self) -> f32 {
        self.state
            .as_ref()
            .map(|state| state.borrow().offset())
            .unwrap_or(0.0)
    }

    fn viewport(&self) -> Vector2F {
        self.size.unwrap_or_else(Vector2F::zero)
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
            state
                .borrow_mut()
                .set_metrics(child_size.along(self.axis), viewport.along(self.axis));
        }
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
        let offset = self.offset();
        let child_origin = match self.axis {
            Axis::Vertical => vec2f(origin.x, origin.y - offset),
            Axis::Horizontal => vec2f(origin.x - offset, origin.y),
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
        if let DispatchedEvent::Scroll { delta } = event {
            let Some(state) = self.state.as_ref() else {
                return false;
            };
            if !self.pointer_over {
                return false;
            }
            // Wheel down (positive y) moves the content up, i.e. the offset
            // grows along the axis.
            let delta = match self.axis {
                Axis::Vertical => delta.y,
                Axis::Horizontal => delta.x,
            };
            state.borrow_mut().scroll_by(delta);
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
    use crate::elements::Empty;
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

    #[test]
    fn a_following_region_opens_at_the_end_of_its_content() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(ScrollState::following()));
        layout_region(&app, &state, 400.0);
        assert_eq!(state.borrow().offset(), 300.0, "400 of content in 100");
        assert!(state.borrow().is_pinned());
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

        let mut event_ctx = EventContext::default();
        let consumed = scrollable.dispatch_event(
            &DispatchedEvent::Scroll {
                delta: vec2f(0.0, 30.0),
            },
            &mut event_ctx,
            &app,
        );
        assert!(consumed, "wheel inside the viewport scrolls the region");
        assert_eq!(state.borrow().offset(), 30.0);
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
                delta: vec2f(0.0, 30.0),
            },
            &mut event_ctx,
            &app,
        );
        assert!(!consumed, "the pointer is not over this region");
        assert_eq!(state.borrow().offset(), 0.0);
    }
}
