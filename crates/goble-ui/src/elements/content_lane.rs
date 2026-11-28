//! A centred, aspect-ratio width "content lane" for a whole content area.
//!
//! The desktop shell has a lot of width on a large screen, and a conversation
//! (or a shell) stretched across all of it reads badly. This element lays its
//! child out in a centred column whose width follows the available width — an
//! aspect ratio rather than a fixed cap — and paints **nothing** in the side
//! gutters: the lane's edge is invisible until the user grabs it.
//!
//! The width model is modelled on DeepSeek Harness's conversation column:
//!
//! - `column` is the available width the lane is given by layout;
//! - a margin budget of [`CONTENT_LANE_MARGIN_BUDGET`] (88 each side) is always
//!   kept free, so the content is never wider than `column - 176`;
//! - on a narrow area (`column < 856`) the lane is **inactive**: the content is
//!   the full column, with no gutters and no handles — today's small-screen
//!   behaviour;
//! - with a user preference (a drag) the content is `clamp(pref, 640, column -
//!   176)`; the preference wins over the ratio, so a resize keeps the width the
//!   user chose (still clamped into the window);
//! - without one the content is `min(clamp(column * 0.64, 680, 920), column -
//!   176)`: 64% of the column, clamped — which is what keeps the ratio on a
//!   window resize.
//!
//! The width is recomputed from the layout constraint every frame, so resizing
//! the window keeps the ratio live; the element itself remembers nothing
//! between frames (the tree is rebuilt every frame), which is why the
//! preference and the drag state live in app state and are handed in.
//!
//! The edges are grabbable: an invisible 10 px strip per side, its inner edge
//! 24 px outside the content column, starts a drag; the pointer turns into a
//! horizontal-resize cursor over it, and while hovering or dragging a short 2 px
//! line marks the handle (`Muted` on hover, `Accent` while dragging). The
//! 24 px gap is a safe zone, so the strip is never against the pane, sidebar or
//! window edge. When the gutter cannot fit the gap, the strip and its line are
//! not drawn at all.
//!
//! The drag is symmetric about the centre: moving the right edge outward by
//! `dx` widens the centred content by `2 * dx`, and the left edge is mirrored.
//! The element reports the pointer; the host owns the width preference and
//! commits it through [`resolve_content_width`], the one definition of the
//! width axis.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::contains;
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::{DispatchedEvent, BUTTON_PRIMARY};
use crate::geometry::{rectf, vec2f, RectF, Vector2F};
use crate::platform::window::CursorKind;
use crate::theme::ColorToken;

/// The width kept free on both sides together: 88 logical px each side. The
/// content column is never wider than `column - 176`.
pub const CONTENT_LANE_MARGIN_BUDGET: f32 = 176.0;
/// The narrowest available column the lane draws gutters in. Below it the lane
/// is a plain full-width pass-through (today's small-screen behaviour).
pub const CONTENT_LANE_MIN_ACTIVE_COLUMN: f32 = 856.0;
/// The narrowest the content column may be dragged to.
pub const CONTENT_LANE_MIN_CONTENT_WIDTH: f32 = 640.0;
/// The passive (no preference) ratio of the available column: 64%.
pub const CONTENT_LANE_PASSIVE_RATIO: f32 = 0.64;
/// The passive ratio's clamp: at least 680, at most 920.
pub const CONTENT_LANE_PASSIVE_MIN_WIDTH: f32 = 680.0;
pub const CONTENT_LANE_PASSIVE_MAX_WIDTH: f32 = 920.0;
/// The safe zone between the content column's edge and the handle's inner edge.
pub const CONTENT_LANE_HANDLE_GAP: f32 = 24.0;
/// The band at the top of a lane the handles never start in, by default: a
/// pane's header row. A lane mounted inside a pane fills the whole pane —
/// its chrome band included — and a drag target there would steal the header's
/// own gestures (the pane lift/tab drag), so the host tells the lane how tall
/// the chrome above its content is.
pub const CONTENT_LANE_DEFAULT_TOP_INSET: f32 = 0.0;
/// The invisible drag strip's width, per side.
pub const CONTENT_LANE_HANDLE_WIDTH: f32 = 10.0;
/// The gutter below which the handle (strip and line) is not drawn at all.
pub const CONTENT_LANE_HANDLE_HIDE_GUTTER: f32 = 58.0;
/// The affordance line's width and height: a short, solid, rounded bar.
pub const CONTENT_LANE_HANDLE_LINE_WIDTH: f32 = 2.0;
pub const CONTENT_LANE_HANDLE_LINE_HEIGHT: f32 = 20.0;

/// The width of one of the lane's two invisible edges, and therefore which way
/// dragging that edge away from the centre widens the content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneEdge {
    /// The content column's left edge: dragging the pointer left widens it.
    Left,
    /// The content column's right edge: dragging the pointer right widens it.
    Right,
}

impl LaneEdge {
    /// The sign a pointer delta needs to widen the content when this edge is
    /// dragged: `-1` for the left edge, `1` for the right one.
    pub fn outward_sign(self) -> f32 {
        match self {
            Self::Left => -1.0,
            Self::Right => 1.0,
        }
    }
}

type DragStartHandler = Option<Rc<RefCell<dyn FnMut(f32, LaneEdge, f32, f32)>>>;
type DragMoveHandler = Option<Rc<RefCell<dyn FnMut(f32)>>>;
type DragEndHandler = Option<Rc<RefCell<dyn FnMut()>>>;

/// Whether the lane draws gutters (and handles) at `column` width. It needs a
/// full [`CONTENT_LANE_MIN_ACTIVE_COLUMN`] to be worth centring at all; a
/// narrower area is today's full-width layout.
pub fn lane_is_active(column: f32) -> bool {
    column.is_finite() && column >= CONTENT_LANE_MIN_ACTIVE_COLUMN
}

/// The width of the handle strip inside a gutter `gutter` wide: the full
/// [`CONTENT_LANE_HANDLE_WIDTH`] when the gutter can hold the safe zone, and
/// zero — `None` — when it cannot, so there is never a mispositioned hit area
/// on a narrow column.
pub fn lane_handle_width(gutter: f32) -> Option<f32> {
    (gutter >= CONTENT_LANE_HANDLE_HIDE_GUTTER).then_some(CONTENT_LANE_HANDLE_WIDTH)
}

/// The width of the centred content column for `column` available and an
/// optional user preference.
///
/// This is the whole width axis in one function: the element uses it for
/// layout, and the host uses it to commit a drag, so the two can never disagree
/// about what a width means.
pub fn resolve_content_width(column: f32, preference: Option<f32>) -> f32 {
    if !column.is_finite() {
        return column;
    }
    if !lane_is_active(column) {
        // Narrow: today's full-width layout, no centring, no preference.
        return column.max(0.0);
    }
    let max = (column - CONTENT_LANE_MARGIN_BUDGET).max(CONTENT_LANE_MIN_CONTENT_WIDTH);
    match preference {
        // A drag wins over the ratio; it is re-clamped on every layout so a
        // shrinking window cannot push the content under the margins.
        Some(preference) => preference
            .clamp(CONTENT_LANE_MIN_CONTENT_WIDTH, max)
            .min(column),
        // No preference: 64% of the column, clamped.
        None => (column * CONTENT_LANE_PASSIVE_RATIO)
            .clamp(
                CONTENT_LANE_PASSIVE_MIN_WIDTH,
                CONTENT_LANE_PASSIVE_MAX_WIDTH,
            )
            .min(max),
    }
}

/// The centred column `(left offset, content width)` for `column` available and
/// an optional user preference.
pub fn lane_column(column: f32, preference: Option<f32>) -> (f32, f32) {
    let width = resolve_content_width(column, preference);
    if lane_is_active(column) {
        (((column - width).max(0.0)) / 2.0, width)
    } else if column.is_finite() {
        (0.0, column.max(0.0))
    } else {
        (0.0, column)
    }
}

/// A centred aspect-ratio column around one child, with invisible draggable
/// edges.
///
/// The host owns the width preference and the drag state in app state and hands
/// them in every frame:
///
/// ```ignore
/// ContentLane::new(main)
///     .with_preference(state.content_lane_pref)
///     .with_dragging(state.content_lane_dragging)
///     .with_drag_edge(state.content_lane_drag_edge)
///     .with_on_drag_start(|pointer_x, edge, base_width, column| ...)
///     .with_on_drag_move(|pointer_x| ...)
///     .with_on_drag_end(|| ...)
///     .finish()
/// ```
///
/// The start callback reports everything the host needs to own the drag: the
/// pointer's window-space `x`, the edge that was grabbed, the content width it
/// started at, and the available column at that moment. The host computes
/// `width = base_width + 2 * edge.outward_sign() * (pointer_x - start_x)` on
/// every move, runs it through [`resolve_content_width`], and stores the result
/// as the preference; on release the preference is committed as-is.
pub struct ContentLane {
    child: Box<dyn Element>,
    /// The user's committed content width, if they have dragged one.
    preference: Option<f32>,
    /// Whether a drag is in progress (carried from app state so it survives
    /// the per-frame rebuild).
    dragging: bool,
    /// The edge being dragged, so its handle line can be drawn.
    drag_edge: Option<LaneEdge>,
    on_drag_start: DragStartHandler,
    on_drag_move: DragMoveHandler,
    on_drag_end: DragEndHandler,
    /// How tall the band at the top of the lane is that the handles must not
    /// cover: the pane's header row when the lane is mounted inside a pane.
    handle_top_inset: f32,
    /// The centred column's left offset inside the lane and its width, from the
    /// last layout.
    column: (f32, f32),
    /// The available width the column was resolved against, from the last
    /// layout.
    column_width: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ContentLane {
    /// A lane around `child` with no user preference: the ratio width model.
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            preference: None,
            dragging: false,
            drag_edge: None,
            on_drag_start: None,
            on_drag_move: None,
            on_drag_end: None,
            handle_top_inset: CONTENT_LANE_DEFAULT_TOP_INSET,
            column: (0.0, 0.0),
            column_width: 0.0,
            size: None,
            origin: None,
        }
    }

    /// Set the user's committed content width (`None` = the 64% ratio).
    pub fn with_preference(mut self, preference: Option<f32>) -> Self {
        self.preference = preference;
        self
    }

    /// Whether the drag is in progress, carried from app state so the per-frame
    /// rebuild does not forget it.
    pub fn with_dragging(mut self, dragging: bool) -> Self {
        self.dragging = dragging;
        self
    }

    /// The edge being dragged, carried from app state (it is what the drag's
    /// accent line is drawn on).
    pub fn with_drag_edge(mut self, edge: Option<LaneEdge>) -> Self {
        self.drag_edge = edge;
        self
    }

    /// Called on a primary press on a strip: the pointer's window-space `x`,
    /// the edge grabbed, the content width the lane was at, and the available
    /// column width.
    pub fn with_on_drag_start<F: FnMut(f32, LaneEdge, f32, f32) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_drag_start = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Called on every pointer move while dragging: the pointer's window-space
    /// `x`.
    pub fn with_on_drag_move<F: FnMut(f32) + 'static>(mut self, callback: F) -> Self {
        self.on_drag_move = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Called when the drag ends (the button comes back up).
    pub fn with_on_drag_end<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_drag_end = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Keep the draggable edges below the top `inset` of the lane: the band a
    /// pane's header row occupies when the lane is mounted inside a pane. A
    /// press in that band is never a lane drag, so the header keeps its own
    /// gestures (the pane lift toward the tab strip included).
    pub fn with_handle_top_inset(mut self, inset: f32) -> Self {
        self.handle_top_inset = inset.max(0.0);
        self
    }

    /// The content column's width from the last layout: what a drag starts
    /// from.
    fn content_width(&self) -> f32 {
        self.column.1
    }

    /// The gutter on one side of the content column, from the last layout.
    fn gutter(&self) -> f32 {
        self.column.0
    }

    /// Whether the column currently has gutters and handles.
    fn is_active(&self) -> bool {
        lane_is_active(self.column_width)
    }

    /// The content column's box, in window coordinates, from the last frame.
    fn column_rect(&self) -> Option<RectF> {
        let origin = self.origin?;
        let size = self.size?;
        Some(rectf(
            origin.x() + self.column.0,
            origin.y(),
            self.column.1,
            size.y,
        ))
    }

    /// The lane's own box, in window coordinates, from the last frame: the
    /// box the cursor is read against, so a lane the pointer is outside leaves
    /// another surface's cursor request alone.
    fn bounds(&self) -> Option<RectF> {
        let origin = self.origin?;
        let size = self.size?;
        Some(rectf(origin.x(), origin.y(), size.x, size.y))
    }

    /// The drag strip on `edge`, in window coordinates. Its inner edge is
    /// [`CONTENT_LANE_HANDLE_GAP`] outside the content column, so the strip (and
    /// the 24 px safe zone before it) never touches the pane/sidebar/window
    /// edge. It starts under [`Self::handle_top_inset`], so a press on the
    /// pane's header row is never a lane drag. `None` while the lane is
    /// inactive or the gutter is too narrow.
    fn strip_bounds(&self, edge: LaneEdge) -> Option<RectF> {
        if !self.is_active() {
            return None;
        }
        let strip = lane_handle_width(self.gutter())?;
        let column = self.column_rect()?;
        let top = column.min_y() + self.handle_top_inset;
        let height = column.height() - self.handle_top_inset;
        if height < 1.0 {
            return None;
        }
        Some(match edge {
            LaneEdge::Left => rectf(
                column.min_x() - CONTENT_LANE_HANDLE_GAP - strip,
                top,
                strip,
                height,
            ),
            LaneEdge::Right => rectf(column.max_x() + CONTENT_LANE_HANDLE_GAP, top, strip, height),
        })
    }

    /// The edge whose strip is under `position`, if any.
    fn edge_at(&self, position: Vector2F) -> Option<LaneEdge> {
        for edge in [LaneEdge::Left, LaneEdge::Right] {
            if let Some(bounds) = self.strip_bounds(edge) {
                if contains(bounds, position) {
                    return Some(edge);
                }
            }
        }
        None
    }

    /// The y the handle line is centred on: the pointer's own, clamped so the
    /// line stays inside the strip's own (top-inset) band.
    fn handle_line_rect(&self, ctx: &PaintContext, strip: RectF) -> Option<RectF> {
        let lane_top = strip.min_y();
        let lane_bottom = strip.max_y();
        if lane_bottom - lane_top < 1.0 {
            return None;
        }
        let height = CONTENT_LANE_HANDLE_LINE_HEIGHT.min(lane_bottom - lane_top);
        let centre_y = if ctx.cursor_inside {
            ctx.cursor_position.y
        } else {
            lane_top + (lane_bottom - lane_top) / 2.0
        };
        let y = (centre_y - height / 2.0).clamp(lane_top, (lane_bottom - height).max(lane_top));
        let x = strip.min_x() + (strip.width() - CONTENT_LANE_HANDLE_LINE_WIDTH) / 2.0;
        Some(rectf(x, y, CONTENT_LANE_HANDLE_LINE_WIDTH, height))
    }

    /// Ask the platform for the cursor this frame should show.
    ///
    /// `WindowControl::set_cursor` only tells the window when the value
    /// *changes*, so this is cheap even though it runs every frame. A pane can
    /// hold several lanes (one per pane in the pane area), so a lane only owns
    /// the cursor while the pointer is inside it: the pane under the pointer
    /// asks for the resize arrow over its strips and the arrow elsewhere, and
    /// the other lanes leave whatever another surface asked for alone. While a
    /// drag is in flight the pointer is captured and stays a resize arrow
    /// wherever it goes.
    fn request_cursor(&self, ctx: &PaintContext, app: &AppContext) {
        let over_strip = ctx.cursor_inside && self.edge_at(ctx.cursor_position).is_some();
        if self.dragging || over_strip {
            app.window_control.set_cursor(CursorKind::EwResize);
            return;
        }
        let inside = ctx.cursor_inside
            && self
                .bounds()
                .is_some_and(|bounds| contains(bounds, ctx.cursor_position));
        if inside {
            app.window_control.set_cursor(CursorKind::Default);
        }
    }

    /// Paint the handle affordance: a short solid rounded line, `Muted` under
    /// the hovering pointer or `Accent` on the edge being dragged. Nothing
    /// else in the gutter is ever painted.
    fn paint_handle(&self, ctx: &mut PaintContext, app: &AppContext) {
        if !self.is_active() {
            return;
        }
        let (edge, color) = if self.dragging {
            match self.drag_edge {
                Some(edge) => (edge, ColorToken::Accent),
                None => return,
            }
        } else if ctx.cursor_inside {
            match self.edge_at(ctx.cursor_position) {
                Some(edge) => (edge, ColorToken::Muted),
                None => return,
            }
        } else {
            return;
        };
        let Some(strip) = self.strip_bounds(edge) else {
            return;
        };
        let Some(line) = self.handle_line_rect(ctx, strip) else {
            return;
        };
        let color = app.theme.color(color);
        if let Some(renderer) = ctx.renderer.as_mut() {
            // A 2 px bar with a 1 px corner radius reads as a rounded end.
            renderer.fill_rounded_rect(line, color, CONTENT_LANE_HANDLE_LINE_WIDTH / 2.0);
        }
    }
}

impl Element for ContentLane {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let available = constraint.max.x;
        if !available.is_finite() {
            // An unbounded width (a lane measured inside a horizontal scroll
            // region, say): there is no column to centre, so pass the original
            // constraint through and never mark an active lane.
            let child_size = self.child.layout(constraint, ctx, app);
            self.column = (0.0, child_size.x);
            self.column_width = available;
            self.size = Some(child_size);
            return child_size;
        }

        let (offset, width) = lane_column(available, self.preference);
        self.column = (offset, width);
        self.column_width = available;
        let child_constraint = if self.is_active() {
            // The content column fills the lane's height; the gutters are the
            // only thing the lane takes away from the child.
            SizeConstraint::new(vec2f(0.0, constraint.min.y), vec2f(width, constraint.max.y))
        } else {
            constraint
        };
        let child_size = self.child.layout(child_constraint, ctx, app);

        let height = if constraint.max.y.is_finite() {
            constraint.max.y
        } else {
            child_size.y
        };
        let size = vec2f(available, height);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.request_cursor(ctx, app);
        // The gutters are deliberately empty at rest: the child paints the
        // content column, and the handle line is the only thing the lane ever
        // draws.
        self.child
            .paint(origin + vec2f(self.column.0, 0.0), ctx, app);
        self.paint_handle(ctx, app);
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
        // A drag owns every pointer move until it ends, wherever the pointer
        // has travelled: pointer capture is what keeps the handle usable when
        // the pointer leaves the 10 px strip.
        if self.dragging {
            match event {
                DispatchedEvent::MouseMove { position } => {
                    if let Some(cb) = &self.on_drag_move {
                        (cb.borrow_mut())(position.x);
                    }
                    return true;
                }
                DispatchedEvent::MouseUp { .. } => {
                    if let Some(cb) = &self.on_drag_end {
                        (cb.borrow_mut())();
                    }
                    self.dragging = false;
                    return true;
                }
                _ => {}
            }
        }
        // A primary press on a strip grabs the invisible edge. The host records
        // where the press landed, which edge it was and the width it started
        // from; this element keeps the drag flag set for the rest of the frame
        // so a move that arrives before the next rebuild is still handled.
        if let DispatchedEvent::MouseDown { position, button } = event {
            if *button == BUTTON_PRIMARY {
                if let Some(edge) = self.edge_at(*position) {
                    if let Some(cb) = &self.on_drag_start {
                        (cb.borrow_mut())(
                            position.x,
                            edge,
                            self.content_width(),
                            self.column_width,
                        );
                    }
                    self.dragging = true;
                    self.drag_edge = Some(edge);
                    return true;
                }
            }
        }
        self.child.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;
    use crate::render::{RenderCommand, Renderer};

    /// A child that records the constraint it was given and the origins it was
    /// painted at, and reports itself at a fixed size.
    struct ProbeChild {
        constraints: Rc<RefCell<Vec<SizeConstraint>>>,
        paint_origins: Rc<RefCell<Vec<Vector2F>>>,
        size: Vector2F,
    }

    impl ProbeChild {
        fn boxed() -> (
            Box<dyn Element>,
            Rc<RefCell<Vec<SizeConstraint>>>,
            Rc<RefCell<Vec<Vector2F>>>,
        ) {
            let constraints = Rc::new(RefCell::new(Vec::new()));
            let paint_origins = Rc::new(RefCell::new(Vec::new()));
            (
                Box::new(Self {
                    constraints: Rc::clone(&constraints),
                    paint_origins: Rc::clone(&paint_origins),
                    size: vec2f(0.0, 0.0),
                }),
                constraints,
                paint_origins,
            )
        }
    }

    impl Element for ProbeChild {
        fn layout(
            &mut self,
            constraint: SizeConstraint,
            _ctx: &mut LayoutContext,
            _app: &AppContext,
        ) -> Vector2F {
            self.constraints.borrow_mut().push(constraint);
            self.size
        }

        fn paint(&mut self, origin: Vector2F, _ctx: &mut PaintContext, _app: &AppContext) {
            self.paint_origins.borrow_mut().push(origin);
        }

        fn size(&self) -> Option<Vector2F> {
            Some(self.size)
        }

        fn origin(&self) -> Option<Point> {
            None
        }

        fn dispatch_event(
            &mut self,
            _event: &DispatchedEvent,
            _ctx: &mut EventContext,
            _app: &AppContext,
        ) -> bool {
            false
        }
    }

    fn probe_lane() -> (
        ContentLane,
        Rc<RefCell<Vec<SizeConstraint>>>,
        Rc<RefCell<Vec<Vector2F>>>,
    ) {
        let (child, constraints, origins) = ProbeChild::boxed();
        (ContentLane::new(child), constraints, origins)
    }

    fn layout_at(lane: &mut ContentLane, app: &AppContext, size: Vector2F) -> Vector2F {
        lane.layout(
            SizeConstraint::loose(size),
            &mut LayoutContext::default(),
            app,
        )
    }

    /// Lay out and paint at `origin`, the way a frame does, so the element
    /// records the bounds its edge strips hit-test against.
    fn frame_at(
        lane: &mut ContentLane,
        app: &AppContext,
        size: Vector2F,
        origin: Vector2F,
    ) -> Vec<RenderCommand> {
        let _ = layout_at(lane, app, size);
        let mut ctx = PaintContext::new(Renderer::new());
        lane.paint(origin, &mut ctx, app);
        ctx.renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default()
    }

    /// The commands a frame paints, with the pointer at `position`.
    fn frame_with_pointer_at(
        lane: &mut ContentLane,
        app: &AppContext,
        size: Vector2F,
        position: Vector2F,
    ) -> Vec<RenderCommand> {
        let _ = layout_at(lane, app, size);
        let mut ctx = PaintContext::new(Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = position;
        lane.paint(vec2f(0.0, 0.0), &mut ctx, app);
        ctx.renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default()
    }

    fn fills(commands: &[RenderCommand], color: crate::color::ColorU) -> Vec<RectF> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, color: c, .. } if *c == color => Some(*rect),
                _ => None,
            })
            .collect()
    }

    // ---- The width axis (aspect ratio + clamps) ---------------------------

    /// No preference: 64% of the column, clamped to [680, 920] and to the
    /// margin budget.
    #[test]
    fn the_passive_width_is_the_clamped_ratio_of_the_column() {
        // 64% inside the clamp band.
        assert_eq!(
            resolve_content_width(1600.0, None),
            920.0,
            "64% of 1600 clamped to 920"
        );
        assert_eq!(resolve_content_width(1200.0, None), 768.0, "64% of 1200");
        assert_eq!(resolve_content_width(1300.0, None), 832.0);
        // 64% under the floor: the floor wins.
        assert_eq!(resolve_content_width(1000.0, None), 680.0);
        assert_eq!(resolve_content_width(856.0, None), 680.0);
        // 64% over the ceiling: the ceiling wins (and is never past the margin
        // budget).
        assert_eq!(resolve_content_width(5000.0, None), 920.0);
    }

    /// The ratio is recomputed from the constraint, so a window resize keeps
    /// the aspect ratio live (the "aspect ratio" behaviour).
    #[test]
    fn the_ratio_follows_a_resize_while_no_preference_is_set() {
        assert_eq!(resolve_content_width(1200.0, None), 768.0);
        assert_eq!(resolve_content_width(1250.0, None), 800.0);
        assert_eq!(resolve_content_width(2000.0, None), 920.0);
        // Same 64% before the clamp bands: 1200 → 768, 1600 → 920 (clamped),
        // 1375 → 880.
        assert_eq!(resolve_content_width(1375.0, None), 880.0);
    }

    /// Below 856 available the lane is inactive: the content is the whole
    /// column, which is the small-window behaviour as it is today.
    #[test]
    fn a_narrow_column_is_inactive_and_full_width() {
        assert!(!lane_is_active(855.99));
        assert!(lane_is_active(856.0), "856 is the first active column");
        assert_eq!(resolve_content_width(800.0, None), 800.0);
        assert_eq!(resolve_content_width(700.0, None), 700.0);
        assert_eq!(lane_column(800.0, None), (0.0, 800.0));
        assert_eq!(
            resolve_content_width(800.0, Some(900.0)),
            800.0,
            "a preference cannot activate a narrow lane"
        );
    }

    /// A user preference wins over the ratio, clamped into the margin budget.
    #[test]
    fn a_preference_overrides_the_ratio_and_is_clamped() {
        assert_eq!(resolve_content_width(1600.0, Some(1000.0)), 1000.0);
        assert_eq!(
            resolve_content_width(1600.0, Some(100.0)),
            640.0,
            "the floor"
        );
        assert_eq!(
            resolve_content_width(1600.0, Some(5000.0)),
            1424.0,
            "1600 - the 176 margin budget is the ceiling"
        );
        assert_eq!(resolve_content_width(900.0, Some(5000.0)), 724.0);
        assert_eq!(resolve_content_width(870.0, Some(5000.0)), 694.0);
        assert_eq!(resolve_content_width(870.0, Some(100.0)), 640.0);
    }

    /// The column is centred and the gutters are equal.
    #[test]
    fn the_column_is_centred() {
        assert_eq!(lane_column(1600.0, None), (340.0, 920.0));
        let (offset, width) = lane_column(1001.0, None);
        assert_eq!(width, 680.0);
        assert!((offset - 160.5).abs() < 1e-3);
        assert!(
            (offset * 2.0 + width - 1001.0).abs() < 1e-3,
            "gutters are equal"
        );
    }

    /// An unbounded area passes the constraint through and never activates.
    #[test]
    fn an_unbounded_area_passes_the_constraint_through() {
        let app = AppContext::default();
        let (mut lane, constraints, _origins) = probe_lane();
        let constraint = SizeConstraint::loose(vec2f(f32::INFINITY, 800.0));
        let size = lane.layout(constraint, &mut LayoutContext::default(), &app);
        assert_eq!(
            constraints.borrow().first().copied().map(|c| c.max.x),
            Some(f32::INFINITY),
            "the constraint reaches the child untouched"
        );
        assert_eq!(size, vec2f(0.0, 0.0));
        assert!(!lane.is_active());
    }

    // ---- Layout ----------------------------------------------------------

    #[test]
    fn a_wide_area_gets_a_centred_ratio_column() {
        let app = AppContext::default();
        let (mut lane, constraints, origins) = probe_lane();
        let size = layout_at(&mut lane, &app, vec2f(1600.0, 900.0));

        assert_eq!(size, vec2f(1600.0, 900.0), "the lane fills the area");
        assert_eq!(
            constraints.borrow().first().map(|c| c.max),
            Some(vec2f(920.0, 900.0)),
            "the child gets the ratio column, not the whole area"
        );
        let _ = frame_at(&mut lane, &app, vec2f(1600.0, 900.0), vec2f(100.0, 40.0));
        assert_eq!(
            origins.borrow().last().copied(),
            Some(vec2f(440.0, 40.0)),
            "the column is centred: (1600 - 920) / 2 = 340"
        );
    }

    #[test]
    fn a_narrow_area_is_a_full_width_pass_through() {
        let app = AppContext::default();
        let (mut lane, constraints, origins) = probe_lane();
        let size = layout_at(&mut lane, &app, vec2f(800.0, 900.0));

        assert_eq!(size, vec2f(800.0, 900.0));
        assert_eq!(
            constraints.borrow().first().map(|c| c.max),
            Some(vec2f(800.0, 900.0)),
            "on a narrow screen the lane is today's full-width layout"
        );
        let _ = frame_at(&mut lane, &app, vec2f(800.0, 900.0), vec2f(0.0, 0.0));
        assert_eq!(origins.borrow().last().copied(), Some(vec2f(0.0, 0.0)));
    }

    #[test]
    fn a_preference_lays_the_child_out_at_that_width() {
        let app = AppContext::default();
        let (child, constraints, _origins) = ProbeChild::boxed();
        let mut lane = ContentLane::new(child).with_preference(Some(1000.0));
        let _ = layout_at(&mut lane, &app, vec2f(1600.0, 900.0));
        assert_eq!(
            constraints.borrow().first().map(|c| c.max),
            Some(vec2f(1000.0, 900.0))
        );
    }

    #[test]
    fn layout_tracks_a_resize_with_the_ratio() {
        let app = AppContext::default();
        let (mut lane, constraints, _origins) = probe_lane();
        let _ = layout_at(&mut lane, &app, vec2f(1250.0, 900.0));
        let _ = layout_at(&mut lane, &app, vec2f(2000.0, 900.0));
        let widths: Vec<f32> = constraints
            .borrow()
            .iter()
            .map(|constraint| constraint.max.x)
            .collect();
        assert_eq!(
            widths,
            vec![800.0, 920.0],
            "the ratio is re-evaluated each layout"
        );
    }

    // ---- Handles: geometry, hiding, hit area ------------------------------

    #[test]
    fn the_handle_needs_room_for_the_safe_zone() {
        assert_eq!(
            lane_handle_width(88.0),
            Some(10.0),
            "the margin budget's gutter"
        );
        assert_eq!(
            lane_handle_width(58.0),
            Some(10.0),
            "exactly at the threshold"
        );
        assert_eq!(lane_handle_width(57.9), None);
        assert_eq!(lane_handle_width(0.0), None);
    }

    /// The strips sit 24 px outside the content column, 10 px wide, in window
    /// coordinates.
    #[test]
    fn the_strips_sit_outside_the_column_with_a_safe_zone() {
        let app = AppContext::default();
        let (mut lane, _constraints, _origins) = probe_lane();
        let _ = frame_at(&mut lane, &app, vec2f(1600.0, 900.0), vec2f(0.0, 0.0));

        // Column: x in [340, 1260].
        assert_eq!(
            lane.strip_bounds(LaneEdge::Left),
            Some(rectf(340.0 - 24.0 - 10.0, 0.0, 10.0, 900.0))
        );
        assert_eq!(
            lane.strip_bounds(LaneEdge::Right),
            Some(rectf(1260.0 + 24.0, 0.0, 10.0, 900.0))
        );
        assert_eq!(
            lane.edge_at(vec2f(1289.0, 400.0)),
            Some(LaneEdge::Right),
            "the middle of the right strip"
        );
        assert_eq!(
            lane.edge_at(vec2f(311.0, 400.0)),
            Some(LaneEdge::Left),
            "the middle of the left strip"
        );
        // The 24 px safe zone and the content itself are not handles.
        assert_eq!(lane.edge_at(vec2f(1270.0, 400.0)), None);
        assert_eq!(lane.edge_at(vec2f(800.0, 400.0)), None);
    }

    #[test]
    fn an_inactive_lane_has_no_strips() {
        let app = AppContext::default();
        let (mut lane, _constraints, _origins) = probe_lane();
        let _ = frame_at(&mut lane, &app, vec2f(800.0, 900.0), vec2f(0.0, 0.0));
        assert_eq!(lane.strip_bounds(LaneEdge::Left), None);
        assert_eq!(lane.strip_bounds(LaneEdge::Right), None);
        assert_eq!(lane.edge_at(vec2f(2.0, 400.0)), None);
    }

    // ---- Drag: start, capture, end ---------------------------------------

    #[test]
    fn a_press_on_a_strip_reports_the_starting_state_and_starts_the_drag() {
        let app = AppContext::default();
        let starts: Rc<RefCell<Vec<(f32, LaneEdge, f32, f32)>>> = Rc::default();
        let starts_cb = Rc::clone(&starts);
        let (child, _c, _o) = ProbeChild::boxed();
        let mut lane = ContentLane::new(child).with_on_drag_start(move |x, edge, base, column| {
            starts_cb.borrow_mut().push((x, edge, base, column))
        });
        let _ = frame_at(&mut lane, &app, vec2f(1600.0, 900.0), vec2f(0.0, 0.0));

        let mut ctx = EventContext;
        let down = DispatchedEvent::MouseDown {
            position: vec2f(1289.0, 400.0),
            button: BUTTON_PRIMARY,
        };
        assert!(
            lane.dispatch_event(&down, &mut ctx, &app),
            "the strip grabs the press"
        );
        assert_eq!(
            *starts.borrow(),
            vec![(1289.0, LaneEdge::Right, 920.0, 1600.0)],
            "the host is told the pointer, edge, base width and column"
        );
    }

    #[test]
    fn a_press_on_the_content_or_the_safe_zone_is_the_childs() {
        let app = AppContext::default();
        let (mut lane, _c, _o) = probe_lane();
        let _ = frame_at(&mut lane, &app, vec2f(1600.0, 900.0), vec2f(0.0, 0.0));
        let mut ctx = EventContext;
        for x in [800.0, 1270.0, 1295.0, 20.0] {
            let down = DispatchedEvent::MouseDown {
                position: vec2f(x, 400.0),
                button: BUTTON_PRIMARY,
            };
            assert!(
                !lane.dispatch_event(&down, &mut ctx, &app),
                "{x} is not a handle"
            );
        }
    }

    #[test]
    fn only_the_primary_button_grabs_a_handle() {
        let app = AppContext::default();
        let starts = Rc::new(RefCell::new(0u32));
        let starts_cb = Rc::clone(&starts);
        let (child, _c, _o) = ProbeChild::boxed();
        let mut lane = ContentLane::new(child)
            .with_on_drag_start(move |_, _, _, _| *starts_cb.borrow_mut() += 1);
        let _ = frame_at(&mut lane, &app, vec2f(1600.0, 900.0), vec2f(0.0, 0.0));
        let mut ctx = EventContext;
        let down = DispatchedEvent::MouseDown {
            position: vec2f(1289.0, 400.0),
            button: 1,
        };
        assert!(!lane.dispatch_event(&down, &mut ctx, &app));
        assert_eq!(*starts.borrow(), 0);
    }

    #[test]
    fn a_drag_captures_moves_and_ends_on_release() {
        let app = AppContext::default();
        let moves: Rc<RefCell<Vec<f32>>> = Rc::default();
        let ended = Rc::new(RefCell::new(0u32));
        let moves_cb = Rc::clone(&moves);
        let ended_cb = Rc::clone(&ended);
        let (child, _c, _o) = ProbeChild::boxed();
        let mut lane = ContentLane::new(child)
            .with_on_drag_move(move |x| moves_cb.borrow_mut().push(x))
            .with_on_drag_end(move || *ended_cb.borrow_mut() += 1);
        let _ = frame_at(&mut lane, &app, vec2f(1600.0, 900.0), vec2f(0.0, 0.0));

        let mut ctx = EventContext;
        let down = DispatchedEvent::MouseDown {
            position: vec2f(1289.0, 400.0),
            button: BUTTON_PRIMARY,
        };
        assert!(lane.dispatch_event(&down, &mut ctx, &app));
        // The pointer leaves the 10 px strip; capture keeps the drag alive.
        for x in [1300.0, 1400.0, 100.0] {
            let moved = DispatchedEvent::MouseMove {
                position: vec2f(x, 400.0),
            };
            assert!(lane.dispatch_event(&moved, &mut ctx, &app));
        }
        assert_eq!(*moves.borrow(), vec![1300.0, 1400.0, 100.0]);
        let up = DispatchedEvent::MouseUp {
            position: vec2f(100.0, 400.0),
            button: BUTTON_PRIMARY,
        };
        assert!(lane.dispatch_event(&up, &mut ctx, &app));
        assert_eq!(*ended.borrow(), 1);
        // After the release a move is the child's again.
        let after = DispatchedEvent::MouseMove {
            position: vec2f(100.0, 400.0),
        };
        assert!(!lane.dispatch_event(&after, &mut ctx, &app));
    }

    // ---- Cursor ----------------------------------------------------------

    #[test]
    fn the_cursor_is_a_resize_arrow_over_a_strip_and_the_pointer_elsewhere() {
        let app = AppContext::default();
        let (mut lane, _c, _o) = probe_lane();
        let _ = layout_at(&mut lane, &app, vec2f(1600.0, 900.0));

        let mut over_strip = PaintContext::new(Renderer::new());
        over_strip.cursor_inside = true;
        over_strip.cursor_position = vec2f(1289.0, 400.0);
        lane.paint(vec2f(0.0, 0.0), &mut over_strip, &app);
        assert_eq!(app.window_control.cursor(), CursorKind::EwResize);

        let mut over_content = PaintContext::new(Renderer::new());
        over_content.cursor_inside = true;
        over_content.cursor_position = vec2f(800.0, 400.0);
        lane.paint(vec2f(0.0, 0.0), &mut over_content, &app);
        assert_eq!(app.window_control.cursor(), CursorKind::Default);

        // While dragging the pointer is captured: it keeps the resize arrow
        // even far from the strip.
        let (child, _c2, _o2) = ProbeChild::boxed();
        let mut dragged = ContentLane::new(child).with_dragging(true);
        let mut captured = PaintContext::new(Renderer::new());
        captured.cursor_inside = true;
        captured.cursor_position = vec2f(100.0, 400.0);
        dragged.paint(vec2f(0.0, 0.0), &mut captured, &app);
        assert_eq!(app.window_control.cursor(), CursorKind::EwResize);
    }

    #[test]
    fn an_inactive_lane_never_asks_for_a_resize_cursor() {
        let app = AppContext::default();
        let (mut lane, _c, _o) = probe_lane();
        let _ = layout_at(&mut lane, &app, vec2f(800.0, 900.0));
        let mut ctx = PaintContext::new(Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = vec2f(2.0, 400.0);
        lane.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        assert_eq!(app.window_control.cursor(), CursorKind::Default);
    }

    /// Several panes can hold a lane each: a lane owns the cursor only while
    /// the pointer is inside it, and a lane the pointer is outside must not
    /// clobber the resize arrow the hovered pane asked for.
    #[test]
    fn a_lane_the_pointer_is_outside_does_not_touch_the_cursor() {
        let app = AppContext::default();
        // The pane under the pointer asks for the resize arrow over its strip.
        let (mut hovered, _c, _o) = probe_lane();
        let _ = layout_at(&mut hovered, &app, vec2f(1600.0, 900.0));
        let mut over_strip = PaintContext::new(Renderer::new());
        over_strip.cursor_inside = true;
        over_strip.cursor_position = vec2f(1289.0, 400.0);
        hovered.paint(vec2f(0.0, 0.0), &mut over_strip, &app);
        assert_eq!(app.window_control.cursor(), CursorKind::EwResize);

        // The other pane's lane is painted later in the frame, with the same
        // pointer far outside it: it leaves the cursor alone.
        let (mut other, _c2, _o2) = probe_lane();
        let _ = layout_at(&mut other, &app, vec2f(1600.0, 900.0));
        let mut outside = PaintContext::new(Renderer::new());
        outside.cursor_inside = true;
        outside.cursor_position = vec2f(1289.0, 400.0);
        other.paint(vec2f(2000.0, 0.0), &mut outside, &app);
        assert_eq!(
            app.window_control.cursor(),
            CursorKind::EwResize,
            "a lane the pointer is outside leaves another pane's cursor request in place"
        );

        // Inside the second lane, away from its strips: it owns the cursor and
        // asks for the arrow.
        let mut inside = PaintContext::new(Renderer::new());
        inside.cursor_inside = true;
        inside.cursor_position = vec2f(2800.0, 400.0);
        other.paint(vec2f(2000.0, 0.0), &mut inside, &app);
        assert_eq!(app.window_control.cursor(), CursorKind::Default);
    }

    // ---- Handle band: the pane header is never a drag target -------------

    /// The lane fills the whole pane, header row included. Its handles start
    /// under the top inset, so the header band keeps its own gestures (the
    /// pane lift/tab drag) and the cursor there is the arrow.
    #[test]
    fn the_handles_start_below_the_top_inset() {
        let app = AppContext::default();
        let starts: Rc<RefCell<Vec<(f32, LaneEdge, f32, f32)>>> = Rc::default();
        let starts_cb = Rc::clone(&starts);
        let (child, _c, _o) = ProbeChild::boxed();
        let mut lane = ContentLane::new(child)
            .with_handle_top_inset(36.0)
            .with_on_drag_start(move |x, edge, base, column| {
                starts_cb.borrow_mut().push((x, edge, base, column))
            });
        let _ = frame_at(&mut lane, &app, vec2f(1600.0, 900.0), vec2f(0.0, 0.0));

        // Column x in [340, 1260]; the right strip is x in [1284, 1294] and
        // starts at y = 36, the header's own height.
        assert_eq!(
            lane.strip_bounds(LaneEdge::Right),
            Some(rectf(1284.0, 36.0, 10.0, 864.0))
        );
        assert_eq!(
            lane.edge_at(vec2f(1289.0, 10.0)),
            None,
            "the header band is not a handle"
        );

        let mut ctx = EventContext;
        let press_header = DispatchedEvent::MouseDown {
            position: vec2f(1289.0, 10.0),
            button: BUTTON_PRIMARY,
        };
        assert!(
            !lane.dispatch_event(&press_header, &mut ctx, &app),
            "a press on the header band falls through to the child"
        );
        assert!(starts.borrow().is_empty(), "no drag starts over the header");

        let press = DispatchedEvent::MouseDown {
            position: vec2f(1289.0, 400.0),
            button: BUTTON_PRIMARY,
        };
        assert!(lane.dispatch_event(&press, &mut ctx, &app));
        assert_eq!(
            *starts.borrow(),
            vec![(1289.0, LaneEdge::Right, 920.0, 1600.0)]
        );

        // The cursor follows the same band: arrow over the header, resize
        // arrow under it.
        let (child, _c3, _o3) = ProbeChild::boxed();
        let mut cursor_lane = ContentLane::new(child).with_handle_top_inset(36.0);
        let mut arrow = PaintContext::new(Renderer::new());
        let _ = layout_at(&mut cursor_lane, &app, vec2f(1600.0, 900.0));
        arrow.cursor_inside = true;
        arrow.cursor_position = vec2f(1289.0, 10.0);
        cursor_lane.paint(vec2f(0.0, 0.0), &mut arrow, &app);
        assert_eq!(app.window_control.cursor(), CursorKind::Default);
        arrow.cursor_position = vec2f(1289.0, 40.0);
        cursor_lane.paint(vec2f(0.0, 0.0), &mut arrow, &app);
        assert_eq!(app.window_control.cursor(), CursorKind::EwResize);
    }

    /// The handle line follows the strips' band: a pointer high over the pane
    /// still draws the line under the header, never inside it.
    #[test]
    fn the_handle_line_stays_below_the_top_inset() {
        let app = AppContext::default();
        let accent = app.theme.color(ColorToken::Accent);
        // A drag is in flight and the captured pointer is up in the header
        // band: the dragged edge's line still stays under the header.
        let (child, _c, _o) = ProbeChild::boxed();
        let mut lane = ContentLane::new(child)
            .with_handle_top_inset(36.0)
            .with_dragging(true)
            .with_drag_edge(Some(LaneEdge::Right));
        let _ = layout_at(&mut lane, &app, vec2f(1600.0, 900.0));
        let mut ctx = PaintContext::new(Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = vec2f(1289.0, 5.0);
        lane.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        let commands = ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default();
        let lines = fills(&commands, accent);
        assert_eq!(lines.len(), 1, "one handle line: {commands:?}");
        assert_eq!(lines[0].min_y(), 36.0, "clamped to the inset band's top");
    }

    // ---- Handle affordance line -----------------------------------------

    #[test]
    fn hovering_a_strip_draws_a_muted_line_centred_on_the_pointer() {
        let app = AppContext::default();
        let muted = app.theme.color(ColorToken::Muted);
        let (mut lane, _c, _o) = probe_lane();
        // Right strip x in [1284, 1294]; the 2 px line is centred in it.
        let commands =
            frame_with_pointer_at(&mut lane, &app, vec2f(1600.0, 900.0), vec2f(1289.0, 400.0));
        let lines = fills(&commands, muted);
        assert_eq!(lines.len(), 1, "one handle line: {commands:?}");
        assert_eq!(lines[0].width(), 2.0, "a 2 px line");
        assert_eq!(lines[0].height(), CONTENT_LANE_HANDLE_LINE_HEIGHT);
        assert_eq!(lines[0].min_x(), 1288.0, "centred in the 10 px strip");
        assert_eq!(lines[0].min_y(), 390.0, "centred on the pointer's y");
    }

    #[test]
    fn a_dragged_edge_draws_an_accent_line_and_the_other_edge_none() {
        let app = AppContext::default();
        let accent = app.theme.color(ColorToken::Accent);
        let muted = app.theme.color(ColorToken::Muted);
        let (child, _c, _o) = ProbeChild::boxed();
        let mut lane = ContentLane::new(child)
            .with_dragging(true)
            .with_drag_edge(Some(LaneEdge::Left));
        let commands =
            frame_with_pointer_at(&mut lane, &app, vec2f(1600.0, 900.0), vec2f(100.0, 400.0));
        let lines = fills(&commands, accent);
        assert_eq!(lines.len(), 1);
        // Left strip x in [306, 316]; line at 310.
        assert_eq!(lines[0].min_x(), 310.0);
        assert_eq!(lines[0].min_y(), 390.0);
        assert!(
            fills(&commands, muted).is_empty(),
            "the hover line is not drawn on top of a drag"
        );
    }

    #[test]
    fn the_handle_line_is_hidden_at_rest_and_on_a_narrow_column() {
        let app = AppContext::default();
        let muted = app.theme.color(ColorToken::Muted);
        let accent = app.theme.color(ColorToken::Accent);
        let (mut lane, _c, _o) = probe_lane();
        let commands = frame_at(&mut lane, &app, vec2f(1600.0, 900.0), vec2f(0.0, 0.0));
        assert!(
            fills(&commands, muted).is_empty(),
            "nothing is painted at rest"
        );

        let (mut narrow, _c2, _o2) = probe_lane();
        let commands =
            frame_with_pointer_at(&mut narrow, &app, vec2f(800.0, 900.0), vec2f(400.0, 400.0));
        assert!(fills(&commands, muted).is_empty());
        assert!(fills(&commands, accent).is_empty());
    }

    #[test]
    fn the_handle_line_stays_inside_the_lane_height() {
        let app = AppContext::default();
        let muted = app.theme.color(ColorToken::Muted);
        let (mut lane, _c, _o) = probe_lane();
        let commands =
            frame_with_pointer_at(&mut lane, &app, vec2f(1600.0, 900.0), vec2f(1289.0, 5.0));
        let lines = fills(&commands, muted);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].min_y(), 0.0, "clamped to the lane's top edge");
    }

    /// The width axis is one definition, shared by the element's layout and the
    /// host's drag commit in the app.
    #[test]
    fn the_constants_are_the_dsh_width_axis() {
        assert_eq!(CONTENT_LANE_MARGIN_BUDGET, 176.0);
        assert_eq!(CONTENT_LANE_MIN_ACTIVE_COLUMN, 856.0);
        assert_eq!(CONTENT_LANE_MIN_CONTENT_WIDTH, 640.0);
        assert_eq!(CONTENT_LANE_PASSIVE_RATIO, 0.64);
        assert_eq!(CONTENT_LANE_PASSIVE_MIN_WIDTH, 680.0);
        assert_eq!(CONTENT_LANE_PASSIVE_MAX_WIDTH, 920.0);
        assert_eq!(CONTENT_LANE_HANDLE_GAP, 24.0);
        assert_eq!(CONTENT_LANE_HANDLE_WIDTH, 10.0);
        assert_eq!(CONTENT_LANE_HANDLE_HIDE_GUTTER, 58.0);
        assert_eq!(CONTENT_LANE_HANDLE_LINE_HEIGHT, 20.0);
        assert_eq!(CONTENT_LANE_DEFAULT_TOP_INSET, 0.0);
    }
}
