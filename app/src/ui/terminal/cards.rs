//! The conversation cards in the pane's block list, and where they landed.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, ConversationCard, Element, EventContext, LayoutContext, PaintContext, Point,
    SizeConstraint,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;

use crate::emulator::AgentViewCard;

/// Where one conversation card landed in the terminal view, in window
/// coordinates, so a click on the card is a click on the conversation it names.
#[derive(Debug, Clone)]
pub(super) struct CardRect {
    pub(super) conversation_id: String,
    origin: Vector2F,
    size: Vector2F,
}

impl CardRect {
    pub(super) fn contains(&self, position: Vector2F) -> bool {
        position.x >= self.origin.x
            && position.x <= self.origin.x + self.size.x
            && position.y >= self.origin.y
            && position.y <= self.origin.y + self.size.y
    }
}

/// The row a conversation card draws: A8's [`ConversationCard`] element, built
/// from the block identity the pane's view carries. The card draws its own
/// status rail and no border, so it reads as part of the scrollback.
pub(super) fn build_card(card: &AgentViewCard) -> Box<dyn Element> {
    ConversationCard::new(card.conversation_id.clone(), card.label.clone()).finish()
}

/// Draws a conversation card and records where it landed, so the pane can turn
/// a pointer event into a click on the card rather than on the grid behind it.
pub(super) struct CardProbe {
    pub(super) inner: Box<dyn Element>,
    pub(super) cards: Rc<RefCell<Vec<CardRect>>>,
    pub(super) conversation_id: String,
    pub(super) size: Option<Vector2F>,
    pub(super) origin: Option<Point>,
}

impl Element for CardProbe {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.inner.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        if let Some(size) = self.size {
            self.cards.borrow_mut().push(CardRect {
                conversation_id: self.conversation_id.clone(),
                origin,
                size,
            });
        }
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.inner.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    /// The probe records where the card landed; the pane itself is what turns a
    /// pointer event on that spot into a click on the conversation, so the card
    /// never consumes one here.
    fn dispatch_event(
        &mut self,
        _event: &DispatchedEvent,
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        false
    }
}
