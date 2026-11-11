use std::cell::RefCell;
use std::rc::Rc;

use goble_terminal::Block;

use crate::color::ColorU;
use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    harness_bar_mark, AppContext, Border, Container, CrossAxisAlignment, EdgeInsets, Element,
    EventContext, Flex, Icon, LayoutContext, MainAxisAlignment, MainAxisSize, PaintContext, Point,
    SizeConstraint, Spacer, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

/// Width of the accent rail the card painted before it became a raised band.
/// Kept exported for callers that still refer to the card's old mark.
pub const CARD_RAIL_WIDTH: f32 = 2.0;

const FONT_SIZE: f32 = 13.0;
const PADDING_X: f32 = 12.0;
const PADDING_Y: f32 = 12.0;

/// The harness mark the row leads with, and the trailing glyphs that say the
/// row opens.
const HARNESS_SIZE: f32 = 16.0;
const TRAILING_ICON_SIZE: f32 = 14.0;

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
/// What the block does not carry — the harness the conversation runs on, and
/// what it is doing — the app that holds the conversation sets.
///
/// It is drawn as the pane's own row rather than as terminal output: a
/// full-width raised band with one hairline, the harness's mark at its leading
/// edge, the conversation's name, and the expand and open glyphs at its
/// trailing edge.
pub struct ConversationCard {
    conversation_id: String,
    title: String,
    /// Which harness runs the conversation. Unknown keys draw the native mark.
    harness: String,
    status: ConversationCardStatus,
    last_activity: String,
    on_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    state: InteractiveState,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ConversationCard {
    /// The card for the conversation `conversation_id`, titled `label`. An
    /// unnamed conversation falls back to its id, so a card always has a title.
    ///
    /// This is the pane's path: its view carries the identity off the block as
    /// two fields, not the block itself. [`Self::from_block`] is the same thing
    /// from the block.
    pub fn new(conversation_id: impl Into<String>, label: impl Into<String>) -> Self {
        let conversation_id = conversation_id.into();
        let label = label.into();
        let title = if label.is_empty() {
            conversation_id.clone()
        } else {
            label
        };
        Self {
            conversation_id,
            title,
            harness: "goble".to_string(),
            status: ConversationCardStatus::default(),
            last_activity: String::new(),
            on_click: None,
            state: InteractiveState::default(),
            root: None,
            size: None,
            origin: None,
        }
    }

    /// The card for the agent-view block `block`. `None` for a shell block:
    /// only a conversation block has a card.
    pub fn from_block(block: &Block) -> Option<Self> {
        Some(Self::new(
            block.conversation_id()?,
            block.label().unwrap_or_default(),
        ))
    }

    /// Which harness the row leads with. Unknown keys draw the native mark.
    pub fn with_harness(mut self, harness: impl Into<String>) -> Self {
        self.harness = harness.into();
        self
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

    pub fn harness(&self) -> &str {
        &self.harness
    }

    pub fn status(&self) -> ConversationCardStatus {
        self.status
    }

    pub fn last_activity(&self) -> &str {
        &self.last_activity
    }

    fn ensure_root(&mut self, app: &AppContext) {
        if self.root.is_some() {
            return;
        }
        let sm = app.theme.spacing_px(SpacingToken::Sm);

        // The harness's mark, the conversation's name, then the two trailing
        // glyphs: expand, and move into the conversation.
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(harness_bar_mark(&self.harness, HARNESS_SIZE, app))
            .with_child(
                Text::new(self.title.clone())
                    .with_font_size(FONT_SIZE)
                    .with_theme_color(ColorToken::Text, app)
                    .with_max_lines(1)
                    .finish(),
            )
            .with_child(Spacer::new().finish())
            .with_child(
                Icon::new("maximize")
                    .with_size(TRAILING_ICON_SIZE)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(
                Icon::new("chevron-right")
                    .with_size(TRAILING_ICON_SIZE)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .finish();

        // The band carries the hairline and the inset; its surface is painted
        // by `paint`, where the hover tint can replace it.
        self.root = Some(
            Container::new(row)
                .with_padding(EdgeInsets::new(PADDING_X, PADDING_Y, PADDING_X, PADDING_Y))
                .with_border(
                    Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)),
                )
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
        // The band: raised at rest, the hover tint under the pointer. The fill
        // is painted here rather than by the container so the tint is not
        // covered by the row's own surface.
        let surface = if self.state.hover {
            ColorToken::Hover
        } else {
            ColorToken::SurfaceRaised
        };
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rect(
                rectf(origin.x, origin.y, size.x, size.y),
                app.theme.color(surface),
            );
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
    use crate::elements::harness_accent;
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
    fn the_card_is_a_raised_band_with_one_hairline() {
        let (list, id) = conversation_block("conv-1", "Fix the build");
        let card = ConversationCard::from_block(list.get(id).unwrap()).unwrap();
        let mut element: Box<dyn Element> = card.finish();

        let app = AppContext::default();
        let commands = render_element(&mut element, vec2f(420.0, 40.0), &app);
        let counts = command_counts(&commands);

        assert_eq!(counts.stroke_rect, 1, "the band draws one hairline");
        let band = commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == app.theme.color(ColorToken::SurfaceRaised) =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the row sits on a raised surface");
        let (border, width) = commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::StrokeRect {
                    rect, color, width, ..
                } if *color == app.theme.color(ColorToken::Border) => Some((*rect, *width)),
                _ => None,
            })
            .expect("the band keeps a hairline");
        assert_eq!(width, 1.0, "the rule is one hairline");
        assert_eq!(border, band, "the hairline is the band's own edge");
    }

    #[test]
    fn the_card_leads_with_the_native_mark_and_opens_with_two_glyphs() {
        let (list, id) = conversation_block("conv-1", "Fix the build");
        let card = ConversationCard::from_block(list.get(id).unwrap()).unwrap();
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
        assert!(texts.contains(&"Fix the build"), "the title: {texts:?}");
        assert!(
            !texts.iter().any(|text| *text == "▸" || *text == "idle"),
            "the row drops the pager marker and the status text: {texts:?}"
        );

        // The native harness leads with the octopus alone: the atlas glyph for
        // it is a rounded square, the app's frame rather than the harness.
        assert!(
            commands.iter().any(|c| matches!(
                c,
                RenderCommand::DrawImage { source, .. } if source == "harness-goble-octopus"
            )),
            "the native mark is the frameless octopus"
        );

        let icons: Vec<&str> = commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawIcon { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            icons.contains(&"maximize-01"),
            "the expand glyph: {icons:?}"
        );
        assert!(
            icons.contains(&"chevron-right"),
            "the open glyph: {icons:?}"
        );
    }

    #[test]
    fn another_harness_leads_with_its_catalog_glyph_in_its_accent() {
        let (list, id) = conversation_block("conv-1", "Fix the build");
        let card = ConversationCard::from_block(list.get(id).unwrap())
            .unwrap()
            .with_harness("claude");
        let mut element: Box<dyn Element> = card.finish();

        let app = AppContext::default();
        let commands = render_element(&mut element, vec2f(420.0, 40.0), &app);

        let mark = commands.iter().find_map(|c| match c {
            RenderCommand::DrawIcon {
                name, color, size, ..
            } if name == "harness-claude" => Some((*color, *size)),
            _ => None,
        });
        assert_eq!(
            mark,
            Some((harness_accent("claude", &app), HARNESS_SIZE)),
            "the catalog glyph, in the harness family's accent"
        );
        assert!(
            !commands.iter().any(|c| matches!(
                c,
                RenderCommand::DrawImage { source, .. } if source.starts_with("harness-goble")
            )),
            "a non-native harness draws no Goble artwork"
        );
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
