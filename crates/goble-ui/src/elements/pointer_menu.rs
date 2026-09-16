use crate::elements::{AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, Vector2F};

/// How close to the window's edge a menu hung from the pointer is still allowed
/// to open: the margin every floating panel of the app keeps.
const EDGE_MARGIN: f32 = 6.0;

/// A menu hung from a point on the window — the pointer a context menu was
/// opened at — rather than from a control.
///
/// A [`PopupMenu`](crate::elements::PopupMenu) anchors its panel to its
/// trigger's own rectangle, which is what a drop-down wants and the wrong thing
/// for a right-click: warp-new opens a tab's menu at the pointer itself
/// (`app/src/workspace/view.rs::toggle_tab_right_click_menu`, with
/// `TabContextMenuAnchor::Pointer`). This element is the seam: it holds a menu
/// whose trigger is nothing, lays the menu out with the room the window has,
/// takes no room in the tree itself, and paints it at the anchor.
///
/// The anchor is clamped so the whole panel stays inside the window — the
/// pointer may be at the very corner — and the clamp is applied at paint, which
/// is where the panel's own size is known and where the menu records the origin
/// it hit-tests against afterwards. Placement and hit-testing therefore agree by
/// construction.
pub struct PointerMenu {
    menu: Box<dyn Element>,
    anchor: Vector2F,
    /// The menu's measured size, and the room the frame gave this element: both
    /// are what the clamp is worked out from.
    menu_size: Option<Vector2F>,
    window: Option<Vector2F>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl PointerMenu {
    pub fn new(menu: Box<dyn Element>, anchor: Vector2F) -> Self {
        Self {
            menu,
            anchor,
            menu_size: None,
            window: None,
            size: None,
            origin: None,
        }
    }

    /// Where the panel is actually drawn: the anchor, pulled inward so the
    /// panel's near edge is never nearer to a window edge than [`EDGE_MARGIN`].
    /// A panel wider than the window itself keeps the margin off its near edge
    /// rather than going off the far one.
    fn placed(&self) -> Vector2F {
        let Some(menu) = self.menu_size else {
            return self.anchor;
        };
        let Some(window) = self.window else {
            return self.anchor;
        };
        vec2f(
            self.anchor
                .x
                .min((window.x - menu.x - EDGE_MARGIN).max(EDGE_MARGIN)),
            self.anchor
                .y
                .min((window.y - menu.y - EDGE_MARGIN).max(EDGE_MARGIN)),
        )
    }
}

impl Element for PointerMenu {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // The window is what the clamp is measured against, so an unbounded
        // frame (a tree laid out loose, as the offline tests lay it out) leaves
        // the anchor exactly where the pointer was.
        self.window = constraint.max.x.is_finite().then_some(constraint.max);
        let size = self.menu.layout(constraint, ctx, app);
        self.menu_size = Some(size);
        self.size = Some(Vector2F::zero());
        Vector2F::zero()
    }

    fn paint(&mut self, _origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        let at = self.placed();
        self.origin = Some(Point::from_vec2f(at, Default::default()));
        self.menu.paint(at, ctx, app);
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
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.menu.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{AppContext, Container, Element, Empty, Fill};
    use crate::render::RenderCommand;
    use crate::test_util::render_element;
    use crate::theme::ColorToken;

    /// The size of the box standing in for a menu, and the window it opens in.
    fn menu() -> Vector2F {
        vec2f(160.0, 120.0)
    }

    fn window() -> Vector2F {
        vec2f(1024.0, 768.0)
    }

    /// A menu of a known size: one filled box, so the frame's only fill is the
    /// panel and its rectangle is where the panel landed.
    fn boxed_menu(app: &AppContext) -> Box<dyn Element> {
        Container::new(Empty::new().with_size(menu()).finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .finish()
    }

    /// The rectangle of the one fill the frame drew.
    fn panel(commands: &[RenderCommand]) -> crate::geometry::RectF {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, .. } => Some(*rect),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the menu's panel is drawn: {commands:?}"))
    }

    /// The panel opens exactly at the point the pointer was at — the whole
    /// point of hanging a menu from a point rather than from a control — and
    /// takes no room of its own in the tree it is a child of.
    #[test]
    fn the_panel_opens_at_the_pointer_and_takes_no_room() {
        let app = AppContext::default();
        let at = vec2f(100.0, 50.0);
        let mut element: Box<dyn Element> =
            Box::new(PointerMenu::new(boxed_menu(&app), at));
        let commands = render_element(&mut element, window(), &app);

        let rect = panel(&commands);
        assert_eq!(
            (rect.min_x(), rect.min_y()),
            (at.x, at.y),
            "the panel's near corner is the pointer's own point"
        );
        assert_eq!(
            element.size(),
            Some(Vector2F::zero()),
            "a menu hung from the pointer takes no room in its parent"
        );
    }

    /// A pointer at the window's corner still opens a whole panel: the anchor
    /// is pulled inward until the panel fits, one [`EDGE_MARGIN`] off each edge.
    #[test]
    fn a_panel_at_the_windows_corner_is_pulled_back_inside_it() {
        let app = AppContext::default();
        let (window_w, window_h) = (window().x, window().y);
        let at = vec2f(window_w - 4.0, window_h - 4.0);
        let mut element: Box<dyn Element> =
            Box::new(PointerMenu::new(boxed_menu(&app), at));
        let commands = render_element(&mut element, window(), &app);

        let rect = panel(&commands);
        assert_eq!(rect.max_x(), window_w - EDGE_MARGIN, "the panel stays in the window");
        assert_eq!(rect.max_y(), window_h - EDGE_MARGIN, "on both axes");
        assert_eq!(rect.width(), menu().x, "and it is drawn whole, not clipped");
        assert_eq!(rect.height(), menu().y, "on either axis");
    }

    /// With no window to fit into (a tree laid out loose, as an offline test
    /// lays one out) there is no edge to pull back from: the anchor stands.
    #[test]
    fn an_unbounded_frame_leaves_the_anchor_where_the_pointer_was() {
        let app = AppContext::default();
        let at = vec2f(4000.0, 3000.0);
        let mut element: Box<dyn Element> =
            Box::new(PointerMenu::new(boxed_menu(&app), at));
        use crate::elements::{LayoutContext, PaintContext, SizeConstraint};
        use crate::render::Renderer;
        let _ = element.layout(
            SizeConstraint::new(Vector2F::zero(), vec2f(f32::INFINITY, f32::INFINITY)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        element.paint(Vector2F::zero(), &mut ctx, &app);
        let commands = ctx
            .renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default();
        let rect = panel(&commands);
        assert_eq!((rect.min_x(), rect.min_y()), (at.x, at.y));
    }
}
