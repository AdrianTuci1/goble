use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, Vector2F};

/// The rows of one tool block — a call and the body it opens, or a run and its
/// folded members — under one gesture: a double click anywhere in the block
/// closes it.
///
/// The wrapper takes no layout space of its own — the child is laid out at the
/// same width it would have alone — and paints nothing: a tool block reads as
/// bare rows in the transcript, with no rail, bracket or rule beside it. What
/// the block's open or hovered state changes is the rows' own ink and mark, and
/// an open call's body card, not a frame around them.
pub(super) struct ToolBlockGesture {
    child: Box<dyn Element>,
    /// The block's answer to a double click anywhere inside it: the whole
    /// section the block draws closes, not only the row the pointer landed on.
    on_double_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ToolBlockGesture {
    pub(super) fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            on_double_click: None,
            size: None,
            origin: None,
        }
    }

    /// Answer a double click over the block's own rows — its header, its body,
    /// or the gap between them — with this callback. The rows answer a double
    /// click of their own first, so a block nested in this one keeps its own
    /// gesture.
    pub(super) fn with_on_double_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_double_click = Some(Rc::new(RefCell::new(callback)));
        self
    }
}

impl Element for ToolBlockGesture {
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
        // A double click the rows did not take is the block's: the section this
        // gesture covers is the section it closes, wherever in it the pointer
        // landed.
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
        let bounds = rectf(origin.x(), origin.y(), size.x, size.y);
        if !crate::elements::interactive::contains(bounds, *position) {
            return false;
        }
        (callback.borrow_mut())();
        true
    }
}
