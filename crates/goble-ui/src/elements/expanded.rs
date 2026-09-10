use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;

/// Wraps a child and asks the parent [`Flex`](crate::elements::Flex) to give it
/// all remaining main-axis space (like Flutter's `Expanded`).
///
/// The flex amount is controlled by [`Expanded::with_flex`]; the default is
/// `1.0`. Only direct children of a `Flex` participate in the expansion.
pub struct Expanded {
    child: Box<dyn Element>,
    flex: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Expanded {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            flex: 1.0,
            size: None,
            origin: None,
        }
    }

    pub fn with_flex(mut self, flex: f32) -> Self {
        self.flex = flex;
        self
    }
}

impl Extend<Box<dyn Element>> for Expanded {
    fn extend<T: IntoIterator<Item = Box<dyn Element>>>(&mut self, iter: T) {
        for child in iter {
            self.child = child;
        }
    }
}

impl Element for Expanded {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.child.layout(constraint, ctx, app);
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

    fn flex_grow(&self) -> Option<f32> {
        Some(self.flex)
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.child.dispatch_event(event, ctx, app)
    }
}
