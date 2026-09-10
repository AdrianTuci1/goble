use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, Fill, Flex, LayoutContext,
    MainAxisAlignment, PaintContext, Point, SizeConstraint, Text,
};
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

pub struct Modal {
    root: Box<dyn Element>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    title: String,
}

impl Modal {
    pub fn new(
        title: impl Into<String>,
        body: Box<dyn Element>,
        footer: Option<Box<dyn Element>>,
        app: &AppContext,
    ) -> Self {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let title_text = title.into();
        let title = Text::new(&title_text)
            .with_theme_color(ColorToken::Text, app)
            .finish();

        let mut col = Flex::column()
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing)
            .with_child(title)
            .with_child(body);

        if let Some(footer) = footer {
            col = col.with_child(footer);
        }

        let root = Container::new(col.finish())
            .with_padding(EdgeInsets::new(spacing, spacing, spacing, spacing))
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_border(app.theme.color(ColorToken::Border).into())
            .finish();

        Self {
            root,
            size: None,
            origin: None,
            title: title_text,
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }
}

impl Element for Modal {
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
