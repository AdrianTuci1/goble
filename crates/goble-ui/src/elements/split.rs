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
use crate::theme::ColorToken;

/// Half the thickness (along the cross axis) of the draggable divider band.
const DIVIDER_HALF: f32 = 4.0;

/// The smallest extent (along the main axis) a pane is allowed to have. The
/// ratio floor alone (`0.05`) can still leave a pane only a few pixels wide in
/// a small split, so both the ratio floor and this pixel floor are enforced at
/// drag and layout time.
const MIN_PANE_PX: f32 = 120.0;

pub struct SplitNode {
    axis: Axis,
    ratio: f32,
    first: Box<dyn Element>,
    second: Box<dyn Element>,
    dragging: bool,
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
            dragging: false,
            on_drag_start: None,
            on_drag_move: None,
            on_drag_end: None,
            size: None,
            origin: None,
        }
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
        self.size.map(|s| s.along(self.axis)).unwrap_or(0.0) * self.ratio
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
        let cross = self.size.map(|s| s.along(self.axis.invert())).unwrap_or(0.0);
        Some(match self.axis {
            Axis::Horizontal => rectf(
                origin.x() + boundary - DIVIDER_HALF,
                origin.y(),
                DIVIDER_HALF * 2.0,
                cross,
            ),
            Axis::Vertical => rectf(origin.x(), origin.y() + boundary - DIVIDER_HALF, cross, DIVIDER_HALF * 2.0),
        })
    }

    /// Clamp a ratio so that both children of a split of main-axis length
    /// `main` keep at least [`MIN_PANE_PX`] along the main axis. When the
    /// container is too small to give both panes their minimum, falls back to
    /// an equal split. The `0.05` ratio floor is kept as a lower bound so a
    /// very large split never hands the first pane less than the ratio floor.
    fn clamp_ratio(ratio: f32, main: f32) -> f32 {
        let min_ratio = (MIN_PANE_PX / main.max(1.0)).clamp(0.0, 0.5).max(0.05);
        ratio.clamp(min_ratio, 1.0 - min_ratio)
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
        };
        Self::clamp_ratio(pointer / main, main)
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
        // Clamp the ratio so both panes keep at least `MIN_PANE_PX` along the
        // main axis (the ratio floor alone can leave a pane only a few pixels
        // wide in a small split). Storing the clamped value keeps the divider
        // painted where the children actually begin.
        self.ratio = Self::clamp_ratio(self.ratio, main);
        let first_main = main * self.ratio;
        let second_main = main - first_main;

        let _ = self.first.layout(self.axis_constraint(first_main, cross), ctx, app);
        let _ = self.second.layout(self.axis_constraint(second_main, cross), ctx, app);

        let size = constraint.max;
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let boundary = self.boundary();
        self.first.paint(origin + self.axis_offset(0.0, 0.0), ctx, app);
        self.second.paint(origin + self.axis_offset(boundary, 0.0), ctx, app);

        let cross = self.size.map(|s| s.along(self.axis.invert())).unwrap_or(0.0);
        let color = if self.dragging {
            app.theme.color(ColorToken::Accent)
        } else {
            app.theme.color(ColorToken::Border)
        };
        if let Some(renderer) = ctx.renderer.as_mut() {
            let rule = match self.axis {
                Axis::Horizontal => rectf(origin.x + boundary - 0.5, origin.y, 1.0, cross),
                Axis::Vertical => rectf(origin.x, origin.y + boundary - 0.5, cross, 1.0),
            };
            renderer.fill_rounded_rect(rule, color, 0.0);
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
}
