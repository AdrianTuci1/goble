use crate::elements::{
    AppContext, Container, CrossAxisAlignment, Element, Fill, Flex, LayoutContext, PaintContext,
    Point, SizeConstraint,
};
use crate::geometry::Vector2F;
use crate::theme::ColorToken;

pub struct Page {
    root: Box<dyn Element>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Page {
    pub fn new(
        header: Option<Box<dyn Element>>,
        body: Box<dyn Element>,
        app: &AppContext,
    ) -> Self {
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(body);
        if let Some(header) = header {
            column = column.with_child(header);
        }
        let root = Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .finish();
        Self {
            root,
            size: None,
            origin: None,
        }
    }

    pub fn with_max_width(self, _width: f32) -> Self {
        // TODO: wrap content in a centered ConstrainedBox once alignment supports it.
        self
    }
}

impl Element for Page {
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
