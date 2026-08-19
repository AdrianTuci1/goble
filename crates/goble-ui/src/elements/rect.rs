use crate::elements::{
    AppContext, Element, Fill, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::geometry::Vector2F;

pub struct Rect {
    size: Option<Vector2F>,
    origin: Option<Point>,
    background: Fill,
    corner_radius: f32,
}

impl Default for Rect {
    fn default() -> Self {
        Self {
            size: None,
            origin: None,
            background: Fill::None,
            corner_radius: 0.0,
        }
    }
}

impl Rect {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_background(mut self, fill: impl Into<Fill>) -> Self {
        self.background = fill.into();
        self
    }

    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }

    pub fn with_size(mut self, size: Vector2F) -> Self {
        self.size = Some(size);
        self
    }
}

impl Element for Rect {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = self
            .size
            .unwrap_or_else(|| Vector2F::new(constraint.width(), constraint.height()));
        self.size = Some(size);
        size
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
