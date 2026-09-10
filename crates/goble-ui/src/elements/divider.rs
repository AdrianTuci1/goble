use crate::elements::{
    AppContext, Axis, Element, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::theme::ColorToken;

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

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if let Some(size) = self.size {
            let color = app.theme.color(ColorToken::Border);
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.fill_rect(rectf(origin.x, origin.y, size.x, size.y), color);
            }
        }
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

    #[test]
    fn divider_paints_a_visible_line() {
        let app = AppContext::default();
        let mut element: Box<dyn Element> = Box::new(Divider::horizontal());
        let commands = crate::test_util::render_element(&mut element, vec2f(200.0, 100.0), &app);
        let counts = crate::test_util::command_counts(&commands);
        assert!(counts.fill_rect > 0, "divider should paint a line");
    }
}
