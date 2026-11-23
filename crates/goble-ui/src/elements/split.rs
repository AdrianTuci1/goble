//! Generic resizable split/paned container.
//!
//! Splits two children along an [`Axis`] at a draggable ratio, generalizing
//! the app's fixed sidebar+main split so the same mechanism can be reused for
//! pane trees (a "space" with vertical/horizontal panes, warp-new style).
//!
//! The ratio (the fraction of the main axis given to the *first* child) and the
//! dragging flag live outside this element (in app state), so they survive the
//! per-frame tree rebuild. The drag callbacks receive the new ratio computed
//! from the absolute pointer position.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::contains;
use crate::elements::{
    AppContext, Axis, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
    Vector2FExt,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, RectF, Vector2F};
use crate::platform::window::CursorKind;

/// Half the thickness (along the cross axis) of the invisible grab band on the
/// boundary between the two children. The split paints no divider: the band is
/// the drag target, and the resize cursor over it is its whole affordance.
const DIVIDER_HALF: f32 = 4.0;

/// The smallest extent (along the main axis) a pane is allowed to have unless
/// the host asks for its own floor (see [`SplitNode::with_min_pane_px`]). The
/// ratio floor alone (`0.05`) can still leave a pane only a few pixels wide in
/// a small split, so both the ratio floor and this pixel floor are enforced at
/// drag and layout time.
pub const MIN_PANE_PX: f32 = 120.0;

pub struct SplitNode {
    axis: Axis,
    ratio: f32,
    first: Box<dyn Element>,
    second: Box<dyn Element>,
    /// The floors the two children may be dragged down to along the main axis.
    /// The app passes each subtree's own minimum (a pane's `PANE_MIN_WIDTH`
    /// plus its frame gutters, or the summed minimum for a nested split), so a
    /// split can never hand a pane less than the owner's minimum; the default
    /// keeps the generic element's own floor.
    min_first_px: f32,
    min_second_px: f32,
    dragging: bool,
    /// Extra main-axis space between the children, drawn as a gutter band.
    gap: f32,
    on_close: Option<Rc<RefCell<dyn FnMut()>>>,
    on_drag_start: Option<Rc<RefCell<dyn FnMut()>>>,
    on_drag_move: Option<Rc<RefCell<dyn FnMut(f32)>>>,
    on_drag_end: Option<Rc<RefCell<dyn FnMut()>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SplitNode {
    pub fn new(axis: Axis, ratio: f32, first: Box<dyn Element>, second: Box<dyn Element>) -> Self {
        Self {
            axis,
            ratio: ratio.clamp(0.05, 0.95),
            first,
            second,
            min_first_px: MIN_PANE_PX,
            min_second_px: MIN_PANE_PX,
            dragging: false,
            gap: 0.0,
            on_close: None,
            on_drag_start: None,
            on_drag_move: None,
            on_drag_end: None,
            size: None,
            origin: None,
        }
    }

    /// The floor both children may shrink to along the main axis, in pixels.
    pub fn with_min_pane_px(mut self, min_pane_px: f32) -> Self {
        let min_pane_px = min_pane_px.max(1.0);
        self.min_first_px = min_pane_px;
        self.min_second_px = min_pane_px;
        self
    }

    /// The floors the two children may shrink to along the main axis, in
    /// pixels, one per side. The app passes each subtree's own minimum, so a
    /// nested split keeps the width its own panes need while the sibling keeps
    /// the rest.
    pub fn with_min_panes(mut self, min_first_px: f32, min_second_px: f32) -> Self {
        self.min_first_px = min_first_px.max(1.0);
        self.min_second_px = min_second_px.max(1.0);
        self
    }

    /// A vertical split's gutter band, in pixels. It is taken out of the two
    /// children's share along the main axis and painted between them.
    pub fn with_gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }

    /// The close control at the trailing end of a vertical split's band.
    pub fn with_on_close<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_close = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Whether a drag is in progress (carried from app state so it survives the
    /// per-frame rebuild).
    pub fn with_dragging(mut self, dragging: bool) -> Self {
        self.dragging = dragging;
        self
    }

    pub fn with_on_drag_start<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_drag_start = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_drag_move<F: FnMut(f32) + 'static>(mut self, callback: F) -> Self {
        self.on_drag_move = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_drag_end<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_drag_end = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Main-axis position (within this element) where the divider sits.
    fn boundary(&self) -> f32 {
        let main = self.size.map(|s| s.along(self.axis)).unwrap_or(0.0);
        self.gap / 2.0 + (main - self.gap).max(0.0) * self.ratio
    }

    /// Build a [`SizeConstraint`] giving `main` along the axis and `cross` across.
    fn axis_constraint(&self, main: f32, cross: f32) -> SizeConstraint {
        SizeConstraint::new(vec2f(0.0, 0.0), self.axis.to_point(main, cross))
    }

    /// Convert a (main, cross) offset to a [`Vector2F`] for this axis.
    fn axis_offset(&self, main: f32, cross: f32) -> Vector2F {
        self.axis.to_point(main, cross)
    }

    /// World-space bounds of the divider grab band.
    fn handle_bounds(&self) -> Option<RectF> {
        let origin = self.origin?;
        let boundary = self.boundary();
        let cross = self
            .size
            .map(|s| s.along(self.axis.invert()))
            .unwrap_or(0.0);
        Some(match self.axis {
            Axis::Horizontal => rectf(
                origin.x() + boundary - DIVIDER_HALF,
                origin.y(),
                DIVIDER_HALF * 2.0,
                cross,
            ),
            Axis::Vertical => rectf(
                origin.x(),
                origin.y() + boundary
                    - (if self.gap > 0.0 {
                        self.gap / 2.0
                    } else {
                        DIVIDER_HALF
                    }),
                cross,
                if self.gap > 0.0 {
                    self.gap
                } else {
                    DIVIDER_HALF * 2.0
                },
            ),
        })
    }

    /// Clamp a ratio so that both children of a split of main-axis length
    /// `main` keep at least their own floor (`min_first_px` / `min_second_px`).
    /// When the container is too small to give both panes their minimum, falls
    /// back to an equal split (the responsive layout is what keeps the window
    /// wide enough in the app; the element never produces a degenerate child).
    /// The `0.05` ratio floor is kept as a lower bound so a very large split
    /// never hands the first pane less than the ratio floor.
    fn clamp_ratio_with(ratio: f32, main: f32, min_first_px: f32, min_second_px: f32) -> f32 {
        let main = main.max(1.0);
        let min_ratio = (min_first_px / main).clamp(0.0, 0.5).max(0.05);
        let max_ratio = 1.0 - (min_second_px / main).clamp(0.0, 0.5).max(0.05);
        // The halves are clamped, so `min_ratio <= 0.5 <= max_ratio` always: a
        // container too small for both floors resolves to the equal split
        // instead of squeezing a child to nothing.
        ratio.clamp(min_ratio, max_ratio)
    }

    /// [`Self::clamp_ratio_with`] at the element's default floor: the generic
    /// floor the tests and other split users read.
    fn clamp_ratio(ratio: f32, main: f32) -> f32 {
        Self::clamp_ratio_with(ratio, main, MIN_PANE_PX, MIN_PANE_PX)
    }

    /// New ratio (0..1) for an absolute pointer position along the main axis.
    fn ratio_from_position(&self, position: Vector2F) -> f32 {
        let Some(origin) = self.origin else {
            return self.ratio;
        };
        let main = self.size.map(|s| s.along(self.axis)).unwrap_or(0.0);
        if main <= 0.0 {
            return self.ratio;
        }
        let pointer = match self.axis {
            Axis::Horizontal => position.x - origin.x(),
            Axis::Vertical => position.y - origin.y(),
        } - self.gap / 2.0;
        let available = (main - self.gap).max(1.0);
        Self::clamp_ratio_with(
            pointer / available,
            available,
            self.min_first_px,
            self.min_second_px,
        )
    }

    /// Ask the platform for the cursor this frame should show.
    ///
    /// The split has no painted divider; the only affordance at the boundary is
    /// the resize cursor over the invisible grab band (and while a drag is in
    /// flight, where the pointer is captured and keeps the cursor wherever it
    /// goes). Like a content lane, a split only owns the cursor while the
    /// pointer is over its own band, so a split the pointer is outside leaves
    /// another surface's request in place.
    fn request_cursor(&self, ctx: &PaintContext, app: &AppContext) {
        let over_handle = ctx.cursor_inside
            && self
                .handle_bounds()
                .is_some_and(|bounds| contains(bounds, ctx.cursor_position));
        if !self.dragging && !over_handle {
            return;
        }
        let kind = match self.axis {
            // The children are side by side: the boundary is a vertical edge.
            Axis::Horizontal => CursorKind::EwResize,
            // The children are stacked: the boundary is a horizontal edge.
            Axis::Vertical => CursorKind::NsResize,
        };
        app.window_control.set_cursor(kind);
    }
}

impl Element for SplitNode {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let main = constraint.max.along(self.axis).max(0.0);
        let cross = constraint.max.along(self.axis.invert()).max(0.0);
        // Clamp the ratio so both panes keep their minimum along the main axis
        // (the ratio floor alone can leave a pane only a few pixels wide in a
        // small split). Storing the clamped value keeps the divider painted
        // where the children actually begin.
        let available = (main - self.gap).max(0.0);
        self.ratio =
            Self::clamp_ratio_with(self.ratio, available, self.min_first_px, self.min_second_px);
        let first_main = available * self.ratio;
        let second_main = available - first_main;

        let _ = self
            .first
            .layout(self.axis_constraint(first_main, cross), ctx, app);
        let _ = self
            .second
            .layout(self.axis_constraint(second_main, cross), ctx, app);

        let size = constraint.max;
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let boundary = self.boundary();
        self.first
            .paint(origin + self.axis_offset(0.0, 0.0), ctx, app);
        self.second.paint(
            origin + self.axis_offset(boundary + self.gap / 2.0, 0.0),
            ctx,
            app,
        );

        if self.gap > 0.0 && self.axis == Axis::Vertical {
            let cross = self.size.map(|s| s.x).unwrap_or(0.0);
            let band = rectf(
                origin.x,
                origin.y + boundary - self.gap / 2.0,
                cross,
                self.gap,
            );
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.fill_rect(band, app.theme.color(crate::theme::ColorToken::Bg));
                renderer.fill_rect(
                    rectf(band.min_x(), band.min_y(), band.width(), 1.0),
                    app.theme.color(crate::theme::ColorToken::Border),
                );
                renderer.draw_icon(
                    vec2f(
                        band.max_x() - 24.0,
                        band.min_y() + (band.height() - 16.0) / 2.0,
                    ),
                    "x-close",
                    16.0,
                    app.theme.color(crate::theme::ColorToken::Muted),
                );
            }
        }
        // A split paints no divider, neither at rest nor while dragging: the
        // panes keep their own separation (the app wears one outline around
        // each tab and leaves the window's `Bg` between them), and a rule here
        // would be a third line inside that gutter. The boundary reads through
        // the resize cursor over its grab band, the same cursor-only affordance
        // the agent content-lane handles use.
        self.request_cursor(ctx, app);
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
        if self.dragging {
            match event {
                DispatchedEvent::MouseMove { position } => {
                    if let Some(cb) = &self.on_drag_move {
                        (cb.borrow_mut())(self.ratio_from_position(*position));
                    }
                    return true;
                }
                DispatchedEvent::MouseUp { .. } => {
                    if let Some(cb) = &self.on_drag_end {
                        (cb.borrow_mut())();
                    }
                    return true;
                }
                _ => {}
            }
        }
        if let DispatchedEvent::MouseDown { position, .. } = event {
            if self.gap > 0.0 && self.axis == Axis::Vertical {
                if let (Some(origin), Some(size)) = (self.origin, self.size) {
                    let boundary = self.boundary();
                    let close = rectf(
                        origin.x() + size.x - 32.0,
                        origin.y() + boundary - self.gap / 2.0,
                        32.0,
                        self.gap,
                    );
                    if contains(close, *position) {
                        if let Some(cb) = &self.on_close {
                            (cb.borrow_mut())();
                        }
                        return true;
                    }
                }
            }
            if let Some(bounds) = self.handle_bounds() {
                if contains(bounds, *position) {
                    if let Some(cb) = &self.on_drag_start {
                        (cb.borrow_mut())();
                    }
                    return true;
                }
            }
        }
        if self.first.dispatch_event(event, ctx, app) {
            return true;
        }
        self.second.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{RenderCommand, Renderer};

    #[test]
    fn clamp_ratio_respects_min_pane_pixels() {
        // A 600px split: each pane must keep at least 120px, so the ratio is
        // bounded to [0.2, 0.8].
        assert!((SplitNode::clamp_ratio(0.01, 600.0) - 0.2).abs() < 1e-6);
        assert!((SplitNode::clamp_ratio(0.99, 600.0) - 0.8).abs() < 1e-6);
        assert!((SplitNode::clamp_ratio(0.5, 600.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn clamp_ratio_keeps_ratio_floor_on_large_splits() {
        // A 4000px split: MIN/main (0.03) drops below the 0.05 ratio floor, so
        // the ratio floor wins and the ratio stays within [0.05, 0.95].
        assert!((SplitNode::clamp_ratio(0.001, 4000.0) - 0.05).abs() < 1e-6);
        assert!((SplitNode::clamp_ratio(0.999, 4000.0) - 0.95).abs() < 1e-6);
    }

    #[test]
    fn clamp_ratio_falls_back_to_equal_split_when_too_small() {
        // A 100px split cannot give both panes 120px; it falls back to an
        // equal split so neither pane is empty.
        assert!((SplitNode::clamp_ratio(0.1, 100.0) - 0.5).abs() < 1e-6);
        assert!((SplitNode::clamp_ratio(0.9, 100.0) - 0.5).abs() < 1e-6);
    }

    /// A child that paints nothing, so every command in a frame belongs to the
    /// split itself.
    struct SilentChild;

    impl Element for SilentChild {
        fn layout(
            &mut self,
            _constraint: SizeConstraint,
            _ctx: &mut LayoutContext,
            _app: &AppContext,
        ) -> Vector2F {
            vec2f(0.0, 0.0)
        }

        fn paint(&mut self, _origin: Vector2F, _ctx: &mut PaintContext, _app: &AppContext) {}

        fn size(&self) -> Option<Vector2F> {
            None
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

    fn split(axis: Axis, dragging: bool) -> SplitNode {
        SplitNode::new(axis, 0.5, Box::new(SilentChild), Box::new(SilentChild))
            .with_dragging(dragging)
    }

    /// Lay the split out in a 600x400 box and paint it, returning the frame's
    /// commands. `pointer` is the cursor position the frame is painted with.
    fn frame_at(axis: Axis, dragging: bool, pointer: Vector2F) -> Vec<RenderCommand> {
        let app = AppContext::default();
        let mut node = split(axis, dragging);
        let _ = node.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = pointer;
        node.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    /// No rule is ever drawn at the split boundary — idle or dragging. The
    /// gutter between the panes is the window's `Bg`, and the resize cursor is
    /// the boundary's whole affordance.
    #[test]
    fn a_split_never_paints_a_divider() {
        for axis in [Axis::Horizontal, Axis::Vertical] {
            for dragging in [false, true] {
                let commands = frame_at(axis, dragging, vec2f(299.0, 199.0));
                assert!(
                    commands.is_empty(),
                    "{axis:?} split (dragging {dragging}) paints no rule: {commands:?}"
                );
            }
        }
    }

    /// The resize cursor over the invisible grab band is the boundary's only
    /// affordance, and it stays for the whole of a captured drag. Away from the
    /// band a split leaves the cursor alone.
    /// A child that records the main-axis extent it was laid out at, and
    /// paints nothing.
    struct RecordingChild {
        main_axis: Axis,
        extents: Rc<RefCell<Vec<f32>>>,
    }

    impl Element for RecordingChild {
        fn layout(
            &mut self,
            constraint: SizeConstraint,
            _ctx: &mut LayoutContext,
            _app: &AppContext,
        ) -> Vector2F {
            self.extents
                .borrow_mut()
                .push(constraint.max.along(self.main_axis));
            constraint.max
        }

        fn paint(&mut self, _origin: Vector2F, _ctx: &mut PaintContext, _app: &AppContext) {}

        fn size(&self) -> Option<Vector2F> {
            None
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

    fn recording_split(
        axis: Axis,
        ratio: f32,
        min_first: f32,
        min_second: f32,
    ) -> (SplitNode, Rc<RefCell<Vec<f32>>>, Rc<RefCell<Vec<f32>>>) {
        let first = Rc::new(RefCell::new(Vec::new()));
        let second = Rc::new(RefCell::new(Vec::new()));
        let node = SplitNode::new(
            axis,
            ratio,
            Box::new(RecordingChild {
                main_axis: axis,
                extents: Rc::clone(&first),
            }),
            Box::new(RecordingChild {
                main_axis: axis,
                extents: Rc::clone(&second),
            }),
        )
        .with_min_panes(min_first, min_second);
        (node, first, second)
    }

    /// Each side of a split stops at its OWN floor: a nested split that needs
    /// the width of two panes keeps it while the sibling takes what is left.
    #[test]
    fn a_split_stops_a_drag_at_each_sides_own_minimum() {
        // A 1000 px split with a 368 px and a 240 px floor: the ratio is
        // bounded to [0.368, 0.76].
        assert!((SplitNode::clamp_ratio_with(0.1, 1000.0, 368.0, 240.0) - 0.368).abs() < 1e-6);
        assert!((SplitNode::clamp_ratio_with(0.9, 1000.0, 368.0, 240.0) - 0.76).abs() < 1e-6);
        assert!((SplitNode::clamp_ratio_with(0.5, 1000.0, 368.0, 240.0) - 0.5).abs() < 1e-6);

        // Laid out below the first floor, the split hands the first child
        // exactly its minimum and the second the rest: no degenerate panes.
        let app = AppContext::default();
        let (mut node, first, second) = recording_split(Axis::Horizontal, 0.1, 368.0, 240.0);
        let _ = node.layout(
            SizeConstraint::loose(vec2f(1000.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(first.borrow().last().copied(), Some(368.0));
        assert_eq!(second.borrow().last().copied(), Some(632.0));

        // Not even the two floors fit: the equal split keeps both children
        // alive instead of one collapsing to zero.
        let (mut node, first, second) = recording_split(Axis::Horizontal, 0.1, 368.0, 368.0);
        let _ = node.layout(
            SizeConstraint::loose(vec2f(500.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(first.borrow().last().copied(), Some(250.0));
        assert_eq!(second.borrow().last().copied(), Some(250.0));
    }

    #[test]
    fn the_boundary_shows_a_resize_cursor_instead_of_a_line() {
        let app = AppContext::default();
        // Side-by-side panes: the boundary is a vertical edge at x = 300.
        let mut horizontal = split(Axis::Horizontal, false);
        let _ = horizontal.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = vec2f(300.0, 200.0);
        horizontal.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        assert_eq!(
            app.window_control.cursor(),
            CursorKind::EwResize,
            "a vertical boundary asks for the east-west resize arrow"
        );

        // Stacked panes: the boundary is a horizontal edge at y = 200.
        let mut vertical = split(Axis::Vertical, false);
        let _ = vertical.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = vec2f(300.0, 200.0);
        vertical.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        assert_eq!(
            app.window_control.cursor(),
            CursorKind::NsResize,
            "a horizontal boundary asks for the north-south resize arrow"
        );

        // Away from the band the split makes no request: the pointer stays on
        // the platform default another surface left it on.
        app.window_control.set_cursor(CursorKind::Default);
        let mut ctx = PaintContext::new(Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = vec2f(100.0, 100.0);
        horizontal.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        assert_eq!(app.window_control.cursor(), CursorKind::Default);

        // While dragging the pointer is captured: the arrow stays wherever the
        // pointer has travelled.
        let mut dragged = split(Axis::Horizontal, true);
        let _ = dragged.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = vec2f(20.0, 20.0);
        dragged.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        assert_eq!(app.window_control.cursor(), CursorKind::EwResize);
    }
}
