use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::Vector2F;

pub struct Clipped {
    child: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Clipped {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child: Some(child),
            size: None,
            origin: None,
        }
    }
}

impl Extend<Box<dyn Element>> for Clipped {
    fn extend<T: IntoIterator<Item = Box<dyn Element>>>(&mut self, iter: T) {
        for child in iter {
            self.child = Some(child);
        }
    }
}

impl Element for Clipped {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = if let Some(child) = self.child.as_mut() {
            child.layout(constraint, ctx, app)
        } else {
            Vector2F::zero()
        };
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if let Some(child) = self.child.as_mut() {
            child.paint(origin, ctx, app);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}
