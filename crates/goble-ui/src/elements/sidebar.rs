use crate::elements::{
    AppContext, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets, Element, Flex,
    LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;

const DEFAULT_SIDEBAR_WIDTH: f32 = 240.0;

pub struct Sidebar {
    root: Box<dyn Element>,
    width: f32,
    collapsed: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Sidebar {
    pub fn new(children: impl IntoIterator<Item = Box<dyn Element>>) -> Self {
        let column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_children(children)
            .finish();
        let root = Container::new(column)
            .with_padding(EdgeInsets::uniform(8.0))
            .finish();
        Self {
            root,
            width: DEFAULT_SIDEBAR_WIDTH,
            collapsed: false,
            size: None,
            origin: None,
        }
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width;
        self.root = ConstrainedBox::new(self.root).with_width(width).finish();
        self
    }

    pub fn with_collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }
}

impl Element for Sidebar {
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

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        self.root.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{Avatar, SidebarItem, Text};
    use crate::geometry::vec2f;

    #[test]
    fn sidebar_layouts_with_items() {
        let app = AppContext::default();
        let item = SidebarItem::new(
            Avatar::new("U").finish(),
            Text::new("Chat").finish(),
            None,
            false,
            &app,
        )
        .finish();
        let mut sidebar = Sidebar::new([item]).with_width(200.0);
        let size = sidebar.layout(
            SizeConstraint::loose(vec2f(400.0, 800.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size.x, 200.0);
        assert!(size.y > 0.0);
    }
}
