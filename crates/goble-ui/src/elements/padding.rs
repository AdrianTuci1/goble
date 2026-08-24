use crate::elements::{
    AppContext, Container, Element, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::geometry::Vector2F;
use crate::style::EdgeInsets;

/// Adds padding around a child without any background or border.
pub struct Padding {
    root: Box<dyn Element>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Padding {
    pub fn new(child: Box<dyn Element>, insets: EdgeInsets) -> Self {
        Self {
            root: Container::new(child).with_padding(insets).finish(),
            size: None,
            origin: None,
        }
    }

    pub fn uniform(child: Box<dyn Element>, value: f32) -> Self {
        Self::new(child, EdgeInsets::uniform(value))
    }
}

impl Element for Padding {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.root.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.paint(origin, ctx, app);
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
    use crate::elements::Empty;
    use crate::geometry::vec2f;

    #[test]
    fn padding_adds_insets() {
        let app = AppContext::default();
        let mut padding =
            Padding::uniform(Empty::new().with_size(vec2f(50.0, 50.0)).finish(), 10.0);
        let size = padding.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size, vec2f(70.0, 70.0));
    }
}
