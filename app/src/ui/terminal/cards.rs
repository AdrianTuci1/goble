//! The conversation cards in the pane's block list, and where they landed.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, ConversationCard, ConversationCardStatus, ConversationEntry, ConversationStatus,
    Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;

use crate::emulator::AgentViewCard;
use crate::ui::UiSnapshot;

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

/// What a conversation card draws that the block does not carry: the
/// conversation's subject, the harness whose mark leads the row, when it last
/// did anything, and whether the agent has answered in it.
#[derive(Clone, Debug)]
pub(super) struct CardInfo {
    pub(super) title: String,
    pub(super) harness: String,
    pub(super) status: ConversationCardStatus,
    pub(super) last_activity: String,
    pub(super) has_agent_reply: bool,
}

impl From<&ConversationEntry> for CardInfo {
    fn from(entry: &ConversationEntry) -> Self {
        Self {
            title: card_title(&entry.id, &entry.name),
            harness: entry.harness.clone(),
            // The sidebar's live state covers a turn in flight and one
            // suspended on the user in one value; the card's finer split
            // cannot tell them apart, so both read as a turn in flight.
            status: match entry.status {
                ConversationStatus::Waiting => ConversationCardStatus::Running,
                ConversationStatus::Default
                | ConversationStatus::Success
                | ConversationStatus::Error
                | ConversationStatus::Stopped => ConversationCardStatus::Idle,
            },
            last_activity: entry.timestamp.clone(),
            has_agent_reply: entry.has_agent_reply,
        }
    }
}

/// The title a card draws. A conversation the user has not named yet has no
/// subject, and the raw conversation id is not one: both read as a new
/// conversation.
fn card_title(conversation_id: &str, title: &str) -> String {
    if title.is_empty() || title == conversation_id {
        "New conversation".to_string()
    } else {
        title.to_string()
    }
}

/// The card info for every conversation the app's list holds, keyed by id. The
/// pane draws a card only for a conversation in this map whose agent has
/// answered; a block standing for a conversation the list does not know, or one
/// where nothing has been exchanged, draws nothing at all.
pub(super) fn card_infos(state: &UiSnapshot) -> HashMap<String, CardInfo> {
    state
        .conversations
        .iter()
        .map(|entry| (entry.id.clone(), CardInfo::from(entry)))
        .collect()
}

/// The row a conversation card draws: A8's [`ConversationCard`] element, built
/// from the block identity the pane's view carries and what the app's
/// conversation list knows about it. The list's subject is what the row is
/// titled; the block's stored label is only a fallback for a card built
/// without it.
pub(super) fn build_card(card: &AgentViewCard, info: Option<&CardInfo>) -> Box<dyn Element> {
    let title = info
        .map(|info| info.title.as_str())
        .unwrap_or(card.label.as_str());
    let mut element = ConversationCard::new(
        card.conversation_id.clone(),
        card_title(&card.conversation_id, title),
    );
    if let Some(info) = info {
        element = element
            .with_harness(info.harness.clone())
            .with_status(info.status)
            .with_last_activity(info.last_activity.clone());
    }
    element.finish()
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
