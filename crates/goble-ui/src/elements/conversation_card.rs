use std::cell::RefCell;
use std::rc::Rc;

use goble_terminal::Block;

use crate::color::ColorU;
use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, EventContext, Flex,
    LayoutContext, MainAxisAlignment, MainAxisSize, PaintContext, Point, SizeConstraint, Spacer,
    Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, Vector2F};
use crate::theme::{ColorToken, FontFamily, SpacingToken};

/// Width of the accent rail down the card's left edge.
pub const CARD_RAIL_WIDTH: f32 = 2.0;

const FONT_SIZE: f32 = 12.0;
const PADDING_Y: f32 = 6.0;
const RAIL_GAP: f32 = 8.0;

/// What the conversation the card stands for is doing right now.
///
/// The status is the card's live half: a conversation at rest reads neutral,
/// one with a turn in flight reads accent, and one suspended on the user (an
/// `ask_user` question or a command proposal) reads warning. "Finished" and
/// "failed" are not states here — that is what last activity is for, and it is
/// what survives a restart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ConversationCardStatus {
    #[default]
    Idle,
    Running,
    Waiting,
}

impl ConversationCardStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Waiting => "waiting",
        }
    }

    pub const fn color_token(self) -> ColorToken {
        match self {
            Self::Idle => ColorToken::Muted,
            Self::Running => ColorToken::Accent,
            Self::Waiting => ColorToken::Warning,
        }
    }

    pub fn color(self, app: &AppContext) -> ColorU {
        app.theme.color(self.color_token())
    }
}

/// The card a conversation leaves behind in the terminal: one row that says
/// "there is a conversation here, click to enter".
///
/// It reads its identity straight off the agent-view block A7 pushes into the
/// pane's list ([`Block::conversation_id`] / [`Block::label`]), so the card and
/// the agent view can never disagree about which conversation they stand for.
/// The two things the block does not carry — what the conversation is doing and
/// when it last did it — are set by the app that holds the conversation.
///
/// It is drawn in the pager idiom: a plain inline row with an accent rail, no
/// border and no rounded container, so it reads as part of the scrollback
/// rather than a widget sitting on top of it.
pub struct ConversationCard {
    conversation_id: String,
    title: String,
    status: ConversationCardStatus,
    last_activity: String,
    on_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    state: InteractiveState,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ConversationCard {
    /// The card for the agent-view block `block`. `None` for a shell block:
    /// only a conversation block has a card. An unnamed conversation falls back
    /// to its id, so a card always has a title.
    pub fn from_block(block: &Block) -> Option<Self> {
        let conversation_id = block.conversation_id()?.to_string();
        let label = block.label().unwrap_or_default();
        let title = if label.is_empty() {
            conversation_id.clone()
        } else {
            label.to_string()
        };
        Some(Self {
            conversation_id,
            title,
            status: ConversationCardStatus::default(),
            last_activity: String::new(),
            on_click: None,
            state: InteractiveState::default(),
            root: None,
            size: None,
            origin: None,
        })
    }

    pub fn with_status(mut self, status: ConversationCardStatus) -> Self {
        self.status = status;
        self
    }

    /// When the conversation last did anything, already rendered for display
    /// (the app owns the clock and the phrasing — "2m ago", "yesterday").
    pub fn with_last_activity(mut self, last_activity: impl Into<String>) -> Self {
        self.last_activity = last_activity.into();
        self
    }

    /// Clicking the card enters the conversation's agent view.
    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn conversation_id(&self) -> &str {
        &self.conversation_id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn status(&self) -> ConversationCardStatus {
        self.status
    }

    fn ensure_root(&mut self, app: &AppContext) {
        if self.root.is_some() {
            return;
        }
        let sm = app.theme.spacing_px(SpacingToken::Sm);

        // The pager's "there is more here" marker, the conversation's name, then
        // what it is doing and when it last did it, pushed to the right edge.
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(
                Text::new("▸")
                    .with_font_size(FONT_SIZE)
                    .with_font_family(FontFamily::Mono)
                    .with_theme_color(ColorToken::Accent, app)
                    .finish(),
            )
            .with_child(
                Text::new(self.title.clone())
                    .with_font_size(FONT_SIZE)
                    .with_theme_color(ColorToken::Text, app)
                    .with_max_lines(1)
                    .finish(),
            )
            .with_child(Spacer::new().finish())
            .with_child(
                Text::new(self.status.label())
                    .with_font_size(FONT_SIZE)
                    .with_theme_color(self.status.color_token(), app)
                    .with_max_lines(1)
                    .finish(),
            )
            .with_child(
                Text::new(self.last_activity.clone())
                    .with_font_size(FONT_SIZE)
                    .with_theme_color(ColorToken::Muted, app)
                    .with_max_lines(1)
                    .finish(),
            )
            .finish();

        // The rail is painted in `paint`; the content is inset past it so the
        // two never overlap. No border, no corner radius: a row, not a widget.
        self.root = Some(
            Container::new(row)
                .with_padding(EdgeInsets::new(
                    PADDING_Y,
                    CARD_RAIL_WIDTH + RAIL_GAP,
                    PADDING_Y,
                    sm,
                ))
                .finish(),
        );
    }
}

impl Element for ConversationCard {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.ensure_root(app);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        if let Some(renderer) = ctx.renderer.as_mut() {
            if self.state.hover {
                renderer.fill_rect(
                    rectf(origin.x, origin.y, size.x, size.y),
                    app.theme.color(ColorToken::Hover),
                );
            }
            // The rail carries the status colour and reads as the row's mark.
            if size.y > 0.0 {
                renderer.fill_rect(
                    rectf(origin.x, origin.y, CARD_RAIL_WIDTH, size.y),
                    self.status.color(app),
                );
            }
        }
        if let Some(root) = self.root.as_mut() {
            root.paint(origin, ctx, app);
        }
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
        _app: &AppContext,
    ) -> bool {
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };

        let cb = self.on_click.clone();
        let mut on_click = move || {
            if let Some(cb) = cb.as_ref() {
                (cb.borrow_mut())();
            }
        };

        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut on_click)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;
    use crate::test_util::{command_counts, render_element};
    use goble_terminal::{BlockId, BlockList, ScreenSize};

    /// A block list holding one conversation's card, and the card's block id.
    fn conversation_block(conversation_id: &str, label: &str) -> (BlockList, BlockId) {
        let mut list = BlockList::new(ScreenSize::new(80, 24));
        let id = list.push_agent_view_block(conversation_id, label);
        (list, id)
    }

    #[test]
    fn the_card_reads_the_block_it_stands_for() {
        let (list, id) = conversation_block("conv-1", "Fix the build");
        let block = list.get(id).expect("the card block");

        let card = ConversationCard::from_block(block).expect("a conversation block has a card");

        assert_eq!(card.conversation_id(), "conv-1");
        assert_eq!(card.title(), "Fix the build");
    }

    #[test]
    fn an_unnamed_conversation_falls_back_to_its_id() {
        let (list, id) = conversation_block("conv-1", "");
        let block = list.get(id).expect("the card block");

        let card = ConversationCard::from_block(block).expect("a conversation block has a card");

        assert_eq!(card.title(), "conv-1");
    }

    #[test]
    fn a_shell_block_has_no_card() {
        let list = BlockList::new(ScreenSize::new(80, 24));

        assert!(ConversationCard::from_block(list.active_block()).is_none());
    }

    #[test]
    fn the_card_draws_a_rail_not_a_bordered_widget() {
        let (list, id) = conversation_block("conv-1", "Fix the build");
        let card = ConversationCard::from_block(list.get(id).unwrap())
            .unwrap()
            .with_status(ConversationCardStatus::Running)
            .with_last_activity("2m ago");
        let mut element: Box<dyn Element> = card.finish();

        let app = AppContext::default();
        let commands = render_element(&mut element, vec2f(420.0, 40.0), &app);
        let counts = command_counts(&commands);

        assert_eq!(counts.stroke_rect, 0, "a pager row draws no border");
        let rail = commands.iter().find_map(|c| match c {
            RenderCommand::FillRect {
                rect,
                corner_radius,
                ..
            } if rect.width() == CARD_RAIL_WIDTH => Some(*corner_radius),
            _ => None,
        });
        assert_eq!(rail, Some(0.0), "the card paints a square status rail");
    }

    #[test]
    fn the_card_shows_title_status_and_last_activity() {
        let (list, id) = conversation_block("conv-1", "Fix the build");
        let card = ConversationCard::from_block(list.get(id).unwrap())
            .unwrap()
            .with_status(ConversationCardStatus::Waiting)
            .with_last_activity("5m ago");
        let mut element: Box<dyn Element> = card.finish();

        let app = AppContext::default();
        let commands = render_element(&mut element, vec2f(420.0, 40.0), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert!(texts.contains(&"▸"), "the pager marker: {texts:?}");
        assert!(texts.contains(&"Fix the build"), "the title: {texts:?}");
        assert!(texts.contains(&"waiting"), "the status: {texts:?}");
        assert!(texts.contains(&"5m ago"), "the last activity: {texts:?}");
    }

    #[test]
    fn clicking_the_card_enters_its_conversation() {
        let clicked = Rc::new(RefCell::new(false));
        let clicked_clone = clicked.clone();
        let (list, id) = conversation_block("conv-1", "Fix the build");
        let mut card = ConversationCard::from_block(list.get(id).unwrap())
            .unwrap()
            .with_on_click(move || *clicked_clone.borrow_mut() = true);

        let app = AppContext::default();
        card.layout(
            SizeConstraint::loose(vec2f(420.0, 40.0)),
            &mut LayoutContext,
            &app,
        );
        card.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext;
        assert!(card.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(10.0, 10.0),
                button: 0,
            },
            &mut event_ctx,
            &app,
        ));
        assert!(card.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: vec2f(10.0, 10.0),
                button: 0,
            },
            &mut event_ctx,
            &app,
        ));
        assert!(*clicked.borrow(), "the card click opens the agent view");
    }

    #[test]
    fn hovering_the_card_tints_the_row() {
        let (list, id) = conversation_block("conv-1", "Fix the build");
        let mut card = ConversationCard::from_block(list.get(id).unwrap()).unwrap();
        let app = AppContext::default();
        card.layout(
            SizeConstraint::loose(vec2f(420.0, 40.0)),
            &mut LayoutContext,
            &app,
        );
        card.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
        let idle = card.state.hover;

        let mut event_ctx = EventContext;
        card.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: vec2f(10.0, 10.0),
            },
            &mut event_ctx,
            &app,
        );

        assert!(!idle);
        assert!(card.state.hover, "the pointer in the row hovers the card");
    }
}
