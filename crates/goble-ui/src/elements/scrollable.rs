use crate::elements::{
    AppContext, Axis, Element, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, Vector2F};

/// A scrollable region.
///
/// The child is laid out with unbounded space along the scroll axis and the
/// scrollable viewport itself fills the available space on that axis. Clipping
/// and scroll offsets are not implemented yet.
pub struct Scrollable {
    child: Box<dyn Element>,
    axis: Axis,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Scrollable {
    pub fn new(child: Box<dyn Element>, axis: Axis) -> Self {
        Self {
            child,
            axis,
            size: None,
            origin: None,
        }
    }

    pub fn axis(&self) -> Axis {
        self.axis
    }
}

impl Element for Scrollable {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let (_, size) = match self.axis {
            Axis::Vertical => {
                let child_size = self.child.layout(
                    SizeConstraint::new(vec2f(0.0, 0.0), vec2f(constraint.max.x, f32::INFINITY)),
                    ctx,
                    app,
                );
                let viewport = if constraint.max.y.is_finite() {
                    vec2f(
                        child_size.x.min(constraint.max.x).max(constraint.min.x),
                        constraint.max.y,
                    )
                } else {
                    child_size
                };
                (child_size, viewport)
            }
            Axis::Horizontal => {
                let child_size = self.child.layout(
                    SizeConstraint::new(vec2f(0.0, 0.0), vec2f(f32::INFINITY, constraint.max.y)),
                    ctx,
                    app,
                );
                let viewport = if constraint.max.x.is_finite() {
                    vec2f(
                        constraint.max.x,
                        child_size.y.min(constraint.max.y).max(constraint.min.y),
                    )
                } else {
                    child_size
                };
                (child_size, viewport)
            }
        };
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.child.paint(origin, ctx, app);
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
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
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
}
