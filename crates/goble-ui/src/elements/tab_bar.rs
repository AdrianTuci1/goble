use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, Fill, Flex, LayoutContext,
    MainAxisAlignment, PaintContext, Point, SizeConstraint, Text,
};
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

pub struct Tab {
    label: String,
    selected: bool,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Tab {
    pub fn new(label: impl Into<String>, selected: bool) -> Self {
        Self {
            label: label.into(),
            selected,
            root: None,
            size: None,
            origin: None,
        }
    }

    fn rebuild(&mut self, app: &AppContext) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let color = if self.selected {
            ColorToken::Accent
        } else {
            ColorToken::Muted
        };
        let bg = if self.selected {
            ColorToken::Surface
        } else {
            ColorToken::Bg
        };
        let label = Text::new(&self.label).with_theme_color(color, app).finish();
        let root = Container::new(label)
            .with_padding(EdgeInsets::new(spacing, spacing, spacing, spacing))
            .with_background(Fill::Solid(app.theme.color(bg)))
            .finish();
        self.root = Some(root);
    }
}

impl Element for Tab {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        if self.root.is_none() {
            self.rebuild(app);
        }
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.as_mut().unwrap().paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

pub struct TabBar {
    root: Box<dyn Element>,
    tabs: Vec<Tab>,
    selected_index: usize,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TabBar {
    pub fn new(tabs: Vec<Tab>, selected_index: usize, app: &AppContext) -> Self {
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let mut row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing);
        for tab in &tabs {
            row = row.with_child(Box::new(Tab::new(tab.label.clone(), tab.selected)));
        }

        let root = Container::new(row.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .with_border(app.theme.color(ColorToken::Border).into())
            .finish();

        Self {
            root,
            tabs,
            selected_index,
            size: None,
            origin: None,
        }
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }
}

impl Element for TabBar {
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
