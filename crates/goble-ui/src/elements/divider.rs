use crate::elements::{AppContext, Axis, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{vec2f, Vector2F};

const DEFAULT_THICKNESS: f32 = 1.0;

pub struct Divider {
    axis: Axis,
    thickness: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Divider {
    pub fn new(axis: Axis) -> Self {
        Self {
            axis,
            thickness: DEFAULT_THICKNESS,
            size: None,
            origin: None,
        }
    }

    pub fn horizontal() -> Self {
        Self::new(Axis::Horizontal)
    }

    pub fn vertical() -> Self {
        Self::new(Axis::Vertical)
    }

    pub fn with_thickness(mut self, thickness: f32) -> Self {
        self.thickness = thickness.max(0.0);
        self
    }
}

impl Element for Divider {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = match self.axis {
            Axis::Horizontal => vec2f(constraint.max.x, self.thickness),
            Axis::Vertical => vec2f(self.thickness, constraint.max.y),
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;

    #[test]
    fn horizontal_divider_fills_width() {
        let app = AppContext::default();
        let mut divider = Divider::horizontal();
        let size = divider.layout(
            SizeConstraint::loose(vec2f(200.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size.x, 200.0);
        assert_eq!(size.y, DEFAULT_THICKNESS);
    }

    #[test]
    fn vertical_divider_fills_height() {
        let app = AppContext::default();
        let mut divider = Divider::vertical();
        let size = divider.layout(
            SizeConstraint::loose(vec2f(100.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size.x, DEFAULT_THICKNESS);
        assert_eq!(size.y, 200.0);
    }
}
