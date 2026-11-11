use crate::elements::{Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, Vector2F};
use crate::theme::ColorToken;

/// Width of a quote's rail, matching the pager's other rails.
pub(crate) const QUOTE_RAIL_WIDTH: f32 = 2.0;

/// A blockquote drawn as a row: its content indented past a muted vertical
/// rail. A quote is an indent plus a rail, not a rounded box, so the rail is
/// sized to exactly the content's height and the box disappears.
pub(crate) struct QuoteRail {
    child: Box<dyn Element>,
    gap: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl QuoteRail {
    pub(crate) fn new(child: Box<dyn Element>, gap: f32) -> Self {
        Self {
            child,
            gap,
            size: None,
            origin: None,
        }
    }
}

impl Element for QuoteRail {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &crate::elements::AppContext,
    ) -> Vector2F {
        let inset = QUOTE_RAIL_WIDTH + self.gap;
        let inner_max = Vector2F::new((constraint.max.x - inset).max(0.0), constraint.max.y);
        let child_size =
            self.child
                .layout(SizeConstraint::new(Vector2F::zero(), inner_max), ctx, app);
        let size = Vector2F::new(child_size.x + inset, child_size.y);
        self.size = Some(size);
        size
    }

    fn paint(
        &mut self,
        origin: Vector2F,
        ctx: &mut PaintContext,
        app: &crate::elements::AppContext,
    ) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rect(
                rectf(origin.x, origin.y, QUOTE_RAIL_WIDTH, size.y),
                app.theme.color(ColorToken::Muted),
            );
        }
        self.child.paint(
            Vector2F::new(origin.x + QUOTE_RAIL_WIDTH + self.gap, origin.y),
            ctx,
            app,
        );
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
        app: &crate::elements::AppContext,
    ) -> bool {
        self.child.dispatch_event(event, ctx, app)
    }
}
