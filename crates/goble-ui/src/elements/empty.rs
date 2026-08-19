use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::Vector2F;

#[derive(Default)]
pub struct Empty {
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Empty {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_size(mut self, size: Vector2F) -> Self {
        self.size = Some(size);
        self
    }
}

impl Element for Empty {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        self.size.unwrap_or(Vector2F::zero())
    }

    fn paint(&mut self, origin: Vector2F, _ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}
