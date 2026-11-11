use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, Vector2F};
use crate::theme::ColorToken;

/// The width of one bracket stroke and the room between it and the words.
/// The stroke is painted in the transcript row's own padding, outside the text
/// column, so a bracket never moves a line's first character.
const BRACKET_WIDTH: f32 = 2.0;
const BRACKET_GAP: f32 = 6.0;

/// A selected or opened tool block, bracketed down both sides the way the
/// pager marks a block: two square strokes in the free margin beside the text.
///
/// The wrapper takes no layout space of its own — the child is laid out at the
/// same width it would have alone — and paints the strokes outside its bounds,
/// inside the transcript row's padding.
pub(super) struct ToolBracket {
    child: Box<dyn Element>,
    shown: bool,
    color: ColorToken,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ToolBracket {
    pub(super) fn new(child: Box<dyn Element>, shown: bool) -> Self {
        Self {
            child,
            shown,
            color: ColorToken::Muted,
            size: None,
            origin: None,
        }
    }
}

impl Element for ToolBracket {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.child.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if self.shown {
            if let Some(size) = self.size {
                if let Some(renderer) = ctx.renderer.as_mut() {
                    let color = app.theme.color(self.color);
                    renderer.fill_rect(
                        rectf(origin.x - BRACKET_GAP, origin.y, BRACKET_WIDTH, size.y),
                        color,
                    );
                    renderer.fill_rect(
                        rectf(
                            origin.x + size.x + BRACKET_GAP - BRACKET_WIDTH,
                            origin.y,
                            BRACKET_WIDTH,
                            size.y,
                        ),
                        color,
                    );
                }
            }
        }
        self.child.paint(origin, ctx, app);
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
        self.child.dispatch_event(event, ctx, app)
    }
}
