use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, Empty, Fill, Flex,
    LayoutContext, MainAxisAlignment, PaintContext, Point, SizeConstraint, Text,
};
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

pub struct Header {
    root: Box<dyn Element>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Header {
    pub fn new(
        title: impl Into<String>,
        leading: Option<Box<dyn Element>>,
        trailing: Option<Box<dyn Element>>,
        app: &AppContext,
    ) -> Self {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let title = Text::new(title).with_theme_color(ColorToken::Text, app).finish();
        let leading = leading.unwrap_or_else(|| Empty::new().finish());
        let trailing = trailing.unwrap_or_else(|| Empty::new().finish());

        let row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing)
            .with_child(leading)
            .with_child(title)
            .with_child(trailing)
            .finish();

        let root = Container::new(row)
            .with_padding(EdgeInsets::new(spacing, 0.0, spacing, 0.0))
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_border(app.theme.color(ColorToken::Border).into())
            .finish();

        Self {
            root,
            size: None,
            origin: None,
        }
    }
}

impl Element for Header {
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
    use crate::geometry::vec2f;

    #[test]
    fn header_layouts_non_zero() {
        let app = AppContext::default();
        let mut header = Header::new("Chat", None, None, &app);
        let size = header.layout(
            SizeConstraint::loose(vec2f(400.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }
}
