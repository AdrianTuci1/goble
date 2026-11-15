use std::cell::RefCell;
use std::rc::Rc;

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
///
/// Whether a closed block is bracketed is read from the rows' own hover after
/// they paint, so the strokes leave with the pointer on the frame it leaves:
/// the app's hover cell is one frame behind when it is read while building the
/// tree, which would otherwise leave a stroke behind a row the pointer left.
pub(super) struct ToolBracket {
    child: Box<dyn Element>,
    /// Whether the block itself is open: a chain the user expanded keeps its
    /// bracket while it is, independent of the pointer.
    open: bool,
    hover: Rc<RefCell<Option<String>>>,
    /// The rows whose hover brackets the block: its own header, and for a
    /// chain every member, so the pair frames the run while any of it is under
    /// the pointer.
    hover_keys: Vec<String>,
    /// The block's answer to a double click anywhere inside it: the whole
    /// section the rail frames closes, not only the row the pointer landed on.
    on_double_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ToolBracket {
    pub(super) fn new(
        child: Box<dyn Element>,
        open: bool,
        hover: Rc<RefCell<Option<String>>>,
        hover_keys: Vec<String>,
    ) -> Self {
        Self {
            child,
            open,
            hover,
            hover_keys,
            on_double_click: None,
            size: None,
            origin: None,
        }
    }

    /// Answer a double click over the block's own rows — its header, its body,
    /// or the free margin the strokes are painted in — with this callback. The
    /// rows answer a double click of their own first, so a block nested in this
    /// one keeps its own gesture.
    pub(super) fn with_on_double_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_double_click = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Whether the block is under the pointer this frame. Read after the child
    /// painted, which is what fills the hover cell for the frame being drawn.
    fn hovered(&self) -> bool {
        let cell = self.hover.borrow();
        cell.as_deref()
            .is_some_and(|key| self.hover_keys.iter().any(|wanted| wanted == key))
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
        self.child.paint(origin, ctx, app);
        if !self.open && !self.hovered() {
            return;
        }
        let Some(size) = self.size else { return };
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let color = app.theme.color(ColorToken::Muted);
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
        if self.child.dispatch_event(event, ctx, app) {
            return true;
        }
        // A double click the rows did not take is the block's: the section the
        // strokes frame is the section this gesture closes, wherever in it the
        // pointer landed — including the margin the strokes are painted in.
        let Some(callback) = self.on_double_click.as_ref() else {
            return false;
        };
        let DispatchedEvent::MouseDoubleClick { position, button } = event else {
            return false;
        };
        if *button != crate::event::BUTTON_PRIMARY {
            return false;
        }
        let (Some(size), Some(origin)) = (self.size.as_ref(), self.origin.as_ref()) else {
            return false;
        };
        let bounds = rectf(
            origin.x() - BRACKET_GAP,
            origin.y(),
            size.x + 2.0 * BRACKET_GAP,
            size.y,
        );
        if !crate::elements::interactive::contains(bounds, *position) {
            return false;
        }
        (callback.borrow_mut())();
        true
    }
}
