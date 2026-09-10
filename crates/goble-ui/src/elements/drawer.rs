use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, Fill, Flex, LayoutContext,
    MainAxisAlignment, PaintContext, Point, SizeConstraint,
};
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawerAnchor {
    Left,
    Right,
}

pub struct Drawer {
    root: Box<dyn Element>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    anchor: DrawerAnchor,
    width: f32,
}

impl Drawer {
    pub fn new(
        children: Vec<Box<dyn Element>>,
        anchor: DrawerAnchor,
        width: f32,
        app: &AppContext,
    ) -> Self {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let mut col = Flex::column()
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);
        for child in children {
            col = col.with_child(child);
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
            anchor,
            width,
        }
    }

    pub fn anchor(&self) -> DrawerAnchor {
        self.anchor
    }
}

impl Element for Drawer {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let mut tight = constraint;
        tight.max.x = tight.max.x.min(self.width);
        let size = self.root.layout(tight, ctx, app);
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
