use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::image::{decoded_image, resolve_image_target, ChatImage, ImageFigure};
use super::quote_rail::{QuoteRail, QUOTE_RAIL_WIDTH};
use super::tool_call::{build_tool_call_rows, to_text_span};
use crate::color::ColorU;
use crate::elements::chat_content::{
    group_fragments_into_blocks, ChatAction, ChatBlock, ChatFragment, ChatRole, InlineStyle,
    ListItem, SubAgentRow, ToolCall, ToolDisplayMode,
};
use crate::elements::chat_typography;
use crate::elements::table_grid::{self, TableCell, TableGrid};
use crate::elements::terminal_block::TerminalBlockPlumbing;
use crate::elements::{
    Chip, Code, ConstrainedBox, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Empty,
    Expanded, Fill, Flex, Icon, InlineText, LayoutContext, MainAxisAlignment, MainAxisSize,
    Padding, PaintContext, Point, SizeConstraint, TerminalFilter, Text, TextSpan, TopbarButton,
};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, Vector2F};
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, SpacingToken};

/// The paragraph size of a transcript row; the marker is centred on the
/// first line's box rather than on the row's top edge. DSH's 14 px content
/// size, from the shared chat type sheet.
const MESSAGE_FONT_SIZE: f32 = chat_typography::BODY_SIZE;
/// An assistant paragraph's line height: DSH's 14 / 24.
const MESSAGE_LINE_HEIGHT: f32 = chat_typography::BODY_LH;
/// The user's own message: DSH's 14 / 22 bubble line height.
const USER_MESSAGE_LINE_HEIGHT: f32 = chat_typography::USER_LH;
/// How much of a still-streaming thought its row draws: its newest lines, and
/// at most one window of characters, so a thought written as a few long
/// paragraphs stays about seven wrapped rows. The tail is what the model is
/// writing now, and bounding it keeps the row's height — and the whole run the
/// text atlas rasterizes for it — from growing with the whole thought.
const LIVE_REASONING_LINES: usize = 6;
const LIVE_REASONING_CHARS: usize = 600;

/// Whether a block is prose (the markdown body, which DSH separates with its
/// 16 px block gap) or a process step (a reasoning row, a notice, a command
/// block, which keep the tighter step rhythm).
#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockFlow {
    Prose,
    Process,
}

impl BlockFlow {
    fn of(block: &ChatBlock) -> Self {
        match block {
            ChatBlock::Reasoning { .. } | ChatBlock::Notice { .. } | ChatBlock::Terminal(_) => {
                Self::Process
            }
            _ => Self::Prose,
        }
    }
}

/// Lays a user bubble out at DSH's capped width and paints it against the
/// transcript's trailing edge.
///
/// The row itself still spans the lane — the assistant's rows and the sticky
/// turn's row probe both expect a full-width row — but the bubble inside it is
/// only as wide as its text, capped at [`user_bubble_max_width`], and
/// right-aligned. A `Flex` row would also work, but the wrapper states the
/// constraint and the paint offset in one place: the bubble is laid out under
/// the capped width, then painted at `row_width - bubble_width`.
///
/// [`user_bubble_max_width`]: crate::elements::chat_typography::user_bubble_max_width
struct RightAlignedBubble {
    child: Box<dyn Element>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl RightAlignedBubble {
    fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            size: None,
            origin: None,
        }
    }
}

impl Element for RightAlignedBubble {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &crate::elements::AppContext,
    ) -> Vector2F {
        let lane = constraint.max.x;
        let max_width = chat_typography::user_bubble_max_width(lane);
        let child_constraint = SizeConstraint::new(
            vec2f(0.0, constraint.min.y),
            vec2f(max_width, constraint.max.y),
        );
        let child_size = self.child.layout(child_constraint, ctx, app);
        // The wrapper claims the lane so the sticky row probe, the transcript
        // column and the hover/selection surfaces all see the same row width;
        // only the painted bubble inside is narrow.
        let width = if lane.is_finite() {
            lane.max(child_size.x)
        } else {
            child_size.x
        };
        let size = vec2f(width, child_size.y);
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
        let row_width = self.size.map(|size| size.x).unwrap_or(0.0);
        let child_width = self.child.size().map(|size| size.x).unwrap_or(row_width);
        self.child.paint(
            origin + vec2f((row_width - child_width).max(0.0), 0.0),
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

/// The end of a thought that is still streaming, started on a line boundary
/// and led by an ellipsis row when anything was left out.
fn reasoning_tail(text: &str) -> String {
    let by_lines = text
        .rmatch_indices('\n')
        .nth(LIVE_REASONING_LINES - 1)
        .map_or(0, |(index, _)| index + 1);
    let rest = &text[by_lines..];
    let by_chars = rest
        .char_indices()
        .rev()
        .nth(LIVE_REASONING_CHARS - 1)
        .map_or(0, |(index, _)| index);
    let cut = by_lines + by_chars;
    if cut == 0 {
        return text.to_string();
    }
    let tail = &text[cut..];
    if by_chars == 0 {
        // The window starts a line: the ellipsis takes the row before it.
        return format!("…\n{tail}");
    }
    // The character window may land inside a line: begin at the next whole
    // one, with the ellipsis above it.
    match tail.split_once('\n') {
        Some((_, rest)) if !rest.is_empty() => format!("…\n{rest}"),
        _ => format!("…{tail}"),
    }
}

pub struct ChatMessageBubble {
    role: ChatRole,
    fragments: Vec<ChatFragment>,
    tool_calls: Vec<ToolCall>,
    on_action: Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    /// The transcript's text recording, shared with every text-bearing block so
    /// a drag over the transcript can select and copy what was drawn.
    text_selection: Option<Rc<RefCell<crate::elements::SelectableText>>>,
    /// App-owned per-block filter state, keyed by a terminal block's content
    /// key; shared so the tray's open flag + selection persist across frames.
    terminal_filters: Rc<RefCell<HashMap<String, TerminalFilter>>>,
    /// App-owned collapsed/expanded state per reasoning row, keyed by the
    /// reasoning fragment's `key`; shared so the row stays expanded across the
    /// per-frame rebuild instead of resetting to collapsed.
    reasoning_expanded: Rc<RefCell<HashMap<String, bool>>>,
    /// App-owned three-state fold per tool call, keyed by the call's
    /// [`tool_fold_key`](crate::elements::chat_content::tool_fold_key); shared so a call the user folded stays folded (and one
    /// they opened stays open) across the per-frame rebuild.
    tool_fold: Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    /// The tool row under the pointer, reported by the row's paint; the next
    /// frame draws that row's arrow and its white label.
    tool_hover: Rc<RefCell<Option<String>>>,
    /// The live sub-agent records the parent's tool-call rows read their status
    /// from, keyed by the id of the tool call that spawned the child. The app
    /// holds them from S4's `chat:subagent_*` events.
    sub_agents: HashMap<String, SubAgentRow>,
    /// The model the pane's turns run on, which a running child's row names
    /// beside what the child is doing. Empty draws no model there.
    sub_agent_model: String,
    /// The shared spinner phase a running sub-agent card's mark advances with.
    spinner_phase: f32,
    /// The whole-transcript filter, applied to every terminal block.
    global_terminal_filter: Option<TerminalFilter>,
    on_copy_terminal: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ChatMessageBubble {
    pub fn new(role: ChatRole, fragments: Vec<ChatFragment>) -> Self {
        Self {
            role,
            fragments,
            tool_calls: Vec::new(),
            on_action: None,
            text_selection: None,
            terminal_filters: Rc::new(RefCell::new(HashMap::new())),
            reasoning_expanded: Rc::new(RefCell::new(HashMap::new())),
            tool_fold: Rc::new(RefCell::new(HashMap::new())),
            tool_hover: Rc::new(RefCell::new(None)),
            sub_agents: HashMap::new(),
            sub_agent_model: String::new(),
            spinner_phase: 0.0,
            global_terminal_filter: None,
            on_copy_terminal: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_tool_calls(mut self, tool_calls: Vec<ToolCall>) -> Self {
        self.tool_calls = tool_calls;
        self
    }

    /// Share the app-owned cell that names the tool row under the pointer, so
    /// a chain's header can draw its arrow without a hover band behind it.
    pub fn with_tool_hover(mut self, tool_hover: Rc<RefCell<Option<String>>>) -> Self {
        self.tool_hover = tool_hover;
        self
    }

    /// Share the transcript's text recording with this bubble, so every text
    /// block it draws can be selected and copied.
    pub fn with_text_selection(
        mut self,
        selection: Rc<RefCell<crate::elements::SelectableText>>,
    ) -> Self {
        self.text_selection = Some(selection);
        self
    }

    /// Attach the transcript's recording to `element` when the bubble has one.
    fn recorded(&self, element: InlineText) -> InlineText {
        match &self.text_selection {
            Some(selection) => element.with_text_selection(Rc::clone(selection)),
            None => element,
        }
    }

    fn recorded_text(&self, element: Text) -> Text {
        match &self.text_selection {
            Some(selection) => element.with_text_selection(Rc::clone(selection)),
            None => element,
        }
    }

    fn recorded_code(&self, element: Code) -> Code {
        match &self.text_selection {
            Some(selection) => element.with_text_selection(Rc::clone(selection)),
            None => element,
        }
    }

    pub fn with_on_action<F: FnMut(ChatAction) + 'static>(mut self, callback: F) -> Self {
        self.on_action = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_terminal_filters(
        mut self,
        terminal_filters: Rc<RefCell<HashMap<String, TerminalFilter>>>,
    ) -> Self {
        self.terminal_filters = terminal_filters;
        self
    }

    /// Set the app-owned collapsed/expanded map for this bubble's reasoning
    /// rows, so a row the user expanded stays expanded across frames.
    pub fn with_reasoning_expanded(
        mut self,
        reasoning_expanded: Rc<RefCell<HashMap<String, bool>>>,
    ) -> Self {
        self.reasoning_expanded = reasoning_expanded;
        self
    }

    /// Set the app-owned three-state fold map for this bubble's tool calls, so
    /// a folded call stays folded across the per-frame rebuild.
    pub fn with_tool_fold(
        mut self,
        tool_fold: Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    ) -> Self {
        self.tool_fold = tool_fold;
        self
    }

    /// Set the live sub-agent records this bubble's rows read their status from,
    /// keyed by the id of the tool call that spawned each child.
    pub fn with_sub_agents(mut self, sub_agents: HashMap<String, SubAgentRow>) -> Self {
        self.sub_agents = sub_agents;
        self
    }

    /// Set the model a running child's row names beside what the child is
    /// doing (`Responding (deepseek)`).
    pub fn with_sub_agent_model(mut self, model: impl Into<String>) -> Self {
        self.sub_agent_model = model.into();
        self
    }

    /// Set the shared spinner phase a running sub-agent card's mark advances
    /// with, so it turns with the pane's dock and footer.
    pub fn with_spinner_phase(mut self, phase: f32) -> Self {
        self.spinner_phase = phase;
        self
    }

    pub fn with_global_terminal_filter(mut self, filter: Option<TerminalFilter>) -> Self {
        self.global_terminal_filter = filter;
        self
    }

    pub fn with_on_copy_terminal(
        mut self,
        on_copy: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    ) -> Self {
        self.on_copy_terminal = on_copy;
        self
    }

    pub fn role(&self) -> ChatRole {
        self.role
    }

    pub fn fragments(&self) -> &[ChatFragment] {
        &self.fragments
    }

    /// The plumbing every terminal block this bubble draws shares: the
    /// app-owned per-block filter state, the whole-transcript filter and the
    /// copy handler.
    fn terminal_plumbing(&self) -> TerminalBlockPlumbing {
        TerminalBlockPlumbing::new(
            self.terminal_filters.clone(),
            self.global_terminal_filter.clone(),
            self.on_copy_terminal.clone(),
        )
    }

    fn rebuild(&mut self, app: &crate::elements::AppContext, lane: f32) {
        // The transcript's prose rhythm: DSH's markdown block gap is 16 px; the
        // process rows keep the older, tighter rhythm (a reasoning row and a
        // notice are steps of the reply rather than prose blocks).
        let prose_gap = chat_typography::block_gap(app);
        let process_gap = app.theme.spacing_px(SpacingToken::Sm);
        let user = self.role == ChatRole::User;

        // A user bubble hugs its widest block up to DSH's cap. The assistant's
        // rows stay a full-width stretch column.
        let content_max = if user {
            (chat_typography::user_bubble_max_width(lane) - 2.0 * chat_typography::BAND_PADDING_X)
                .max(0.0)
        } else {
            f32::INFINITY
        };
        let mut column = Flex::column().with_cross_axis_alignment(if user {
            CrossAxisAlignment::Start
        } else {
            CrossAxisAlignment::Stretch
        });

        // The assistant's own words lead the calls they announce: the harness
        // stores a step's preamble in the same row as its calls, and grok-build
        // paints that message before the calls it made. The calls are part of
        // the reply and take no terminal plumbing: a call never draws a block,
        // a copy button or a filter, which belong to a command the user ran.
        //
        // The gaps are per pair rather than the column's uniform spacing: DSH's
        // prose-block gap is 16 px, while the process rows (reasoning, notices)
        // keep the tighter step rhythm. A heading carries its own DSH top
        // margin: 32 px over the blocks, i.e. the gap plus the rest of the
        // margin; h4's margin is the 16 px gap alone.
        let blocks = group_fragments_into_blocks(&self.fragments);
        let mut previous: Option<BlockFlow> = None;
        for block in &blocks {
            let flow = BlockFlow::of(block);
            let gap = match previous {
                None => 0.0,
                Some(BlockFlow::Prose) if flow == BlockFlow::Prose => prose_gap,
                Some(_) => process_gap,
            };
            if gap > 0.0 {
                column = column.with_child(Empty::new().with_size(vec2f(0.0, gap)).finish());
            }
            let mut element = self.render_block(block, app);
            // DSH's heading margins: h1–h3 keep 32 px above and 16 below, h4
            // 16 either side. The pair gap already supplies 16 below; the top
            // margin is split between the pair gap and a padding on the
            // heading, so the space above a heading is 32 from the prose above
            // it, or the whole margin when it opens the message.
            if let ChatBlock::Heading { level, .. } = block {
                if *level <= 3 {
                    let top = (chat_typography::H1_TOP_MARGIN - gap).max(0.0);
                    if top > 0.0 {
                        element =
                            Padding::new(element, EdgeInsets::new(0.0, top, 0.0, 0.0)).finish();
                    }
                }
            }
            if user && content_max.is_finite() {
                element = ConstrainedBox::new(element)
                    .with_max_width(content_max)
                    .finish();
            }
            column = column.with_child(element);
            previous = Some(flow);
        }

        if !self.tool_calls.is_empty() {
            // The reply's own prose hands the calls the same gap it gives the
            // next paragraph; a process row keeps the tight step rhythm.
            let gap = match previous {
                Some(BlockFlow::Prose) => prose_gap,
                Some(BlockFlow::Process) => process_gap,
                None => 0.0,
            };
            if gap > 0.0 {
                column = column.with_child(Empty::new().with_size(vec2f(0.0, gap)).finish());
            }
            let mut calls = build_tool_call_rows(
                &self.tool_calls,
                &self.tool_fold,
                &self.tool_hover,
                &self.sub_agents,
                &self.sub_agent_model,
                self.spinner_phase,
                &self.on_action,
                app,
            );
            if user && content_max.is_finite() {
                calls = ConstrainedBox::new(calls)
                    .with_max_width(content_max)
                    .finish();
            }
            column = column.with_child(calls);
        }

        // DSH's user message is a right-aligned bubble, not a pane-wide band:
        // its stack is capped at `min(content-width * 0.702, 82%)`, its surface
        // is the bubble token, and its 20 px radius carries the 10 × 16 inset.
        // The agent's own reply (and the tool rows above it) stays flat prose:
        // no fill, no border, no radius, at the transcript's full width.
        let content = column.finish();

        let band_padding = EdgeInsets::new(
            chat_typography::BAND_PADDING_X,
            chat_typography::BAND_PADDING_Y,
            chat_typography::BAND_PADDING_X,
            chat_typography::BAND_PADDING_Y,
        );
        let band = Container::new(content).with_padding(band_padding);
        self.root = Some(match self.role {
            ChatRole::User => {
                // Goble's `SurfaceRaised` is its neutral raised surface — the
                // equivalent of DSH's dark `--dsw-specific-bubble`
                // (neutral-bluish-850). Keeping it a theme token means the
                // light and preset palettes follow it too.
                let bubble = band
                    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                    .with_corner_radius(chat_typography::USER_BUBBLE_RADIUS)
                    .finish();
                RightAlignedBubble::new(bubble).finish()
            }
            // Assistant and tool rows keep the full-width, flat band: the
            // padding is the transcript's own inset, not a surface.
            ChatRole::Assistant | ChatRole::Tool => band.finish(),
        });
    }

    fn render_block(
        &self,
        block: &ChatBlock,
        app: &crate::elements::AppContext,
    ) -> Box<dyn Element> {
        match block {
            ChatBlock::Paragraph(spans) => {
                // Prose takes the Settings family through the one seam; an
                // inline code span keeps the mono family the span resolver
                // gave it, because DSH draws inline code with its code stack.
                // The user's own row is DSH's 14 / 22 bubble line; the agent's
                // prose is the 14 / 24 markdown base.
                let line_height = if self.role == ChatRole::User {
                    USER_MESSAGE_LINE_HEIGHT
                } else {
                    MESSAGE_LINE_HEIGHT
                };
                let family = chat_typography::chat_font(app);
                let text_spans = spans
                    .iter()
                    .map(|s| {
                        let span = to_text_span(s, app, &self.on_action);
                        if matches!(s.style, InlineStyle::Code) {
                            span
                        } else {
                            span.with_family(family)
                        }
                    })
                    .collect();
                self.recorded(InlineText::new(text_spans))
                    .with_font_size(MESSAGE_FONT_SIZE)
                    .with_line_height(line_height)
                    .with_fit_content(self.role == ChatRole::User)
                    .finish()
            }
            ChatBlock::Heading { level, text } => {
                // DSH's markdown heading ladder: 21/30, 19/28, 18/26, 14/24,
                // the first three bold and h4 semibold.
                let (font_size, line_height, weight) = match level {
                    1 => (
                        chat_typography::H1_SIZE,
                        chat_typography::H1_LH,
                        FontWeight::Bold,
                    ),
                    2 => (
                        chat_typography::H2_SIZE,
                        chat_typography::H2_LH,
                        FontWeight::Bold,
                    ),
                    3 => (
                        chat_typography::H3_SIZE,
                        chat_typography::H3_LH,
                        FontWeight::Bold,
                    ),
                    _ => (
                        chat_typography::H4_SIZE,
                        chat_typography::H4_LH,
                        FontWeight::SemiBold,
                    ),
                };
                self.recorded_text(
                    Text::new(text.clone())
                        .with_theme_color(ColorToken::Text, app)
                        .with_font_family(chat_typography::chat_font(app))
                        .with_font_size(font_size)
                        .with_line_height(line_height)
                        .with_weight(weight),
                )
                .finish()
            }
            ChatBlock::CodeBlock { lang, code } => {
                // Mono, pre-formatted, not word-wrapped; the fence's language
                // label rides along with the body, and the body is coloured by
                // that language when it resolves (otherwise it stays plain).
                let mut code_element = Code::new(code.clone())
                    // DSH's fenced body: 11 / 19 in the code stack, with an
                    // 11 px language banner.
                    .with_font_size(chat_typography::CODE_SIZE)
                    .with_language_font_size(chat_typography::CODE_LANGUAGE_SIZE)
                    .with_language_line_height(chat_typography::CODE_LANGUAGE_LH)
                    .with_line_height(chat_typography::CODE_LH)
                    .with_theme_color(ColorToken::Text, app)
                    // The transcript has one width and no horizontal scroll, so
                    // a fence wraps at the pane's edge instead of running off
                    // it where nobody can read the rest of the line.
                    .with_wrap(true);
                if let Some(lang) = lang {
                    code_element = code_element
                        .with_language(lang.clone())
                        .with_language_theme_color(ColorToken::Muted, app)
                        .with_highlight(lang);
                }
                // DSH's code card: radius 16, a 0.5px hairline and the quiet
                // code-block surface, inset by the block gap.
                Container::new(
                    Flex::column()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .with_child(self.recorded_code(code_element).finish())
                        .finish(),
                )
                .with_background(chat_typography::card_surface(app))
                .with_border(chat_typography::card_border(app))
                .with_corner_radius(chat_typography::card_radius(app))
                .with_padding(EdgeInsets::uniform(app.theme.spacing_px(SpacingToken::Lg)))
                .finish()
            }
            ChatBlock::Ansi { text } => {
                // Coloured output is prose. The escapes pick the glyph colours
                // and the transcript's own surface stays behind them, so the
                // block reads as a coloured paragraph rather than a code panel.
                // A terminal line is already complete, so each line is its own
                // flow; the column keeps them one under the other without the
                // blank line a hard break inside one flow would leave.
                let mut column =
                    Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
                for line in crate::elements::chat_content::ansi::parse(text) {
                    let spans: Vec<TextSpan> = line
                        .into_iter()
                        .map(|run| {
                            // A run with no foreground keeps the theme's text
                            // colour, exactly as a reset returns a terminal to
                            // its default.
                            let mut span = TextSpan::plain(run.text)
                                .with_family(chat_typography::chat_font(app));
                            if let Some(color) = run.color {
                                span = span.with_color(color);
                            }
                            span.with_weight(if run.bold {
                                FontWeight::Bold
                            } else {
                                FontWeight::Regular
                            })
                            .with_italic(run.italic)
                            .with_underline(run.underline)
                            .with_strikeout(run.strikeout)
                        })
                        .collect();
                    column = column.with_child(
                        self.recorded(InlineText::new(spans))
                            .with_font_size(MESSAGE_FONT_SIZE)
                            .with_line_height(MESSAGE_LINE_HEIGHT)
                            .finish(),
                    );
                }
                column.finish()
            }
            ChatBlock::List { items, start } => self.render_list(items, *start, app),
            // A quote is an indent plus a rail, not a rounded box.
            ChatBlock::BlockQuote(inner) => QuoteRail::new(
                self.render_blocks(inner, app),
                app.theme.spacing_px(SpacingToken::Md),
            )
            .finish(),
            ChatBlock::Table { header, rows } => self.render_table(header, rows, app),
            ChatBlock::Rule => Divider::horizontal().finish(),
            // A notice is DSH's compaction-marker row: one quiet
            // process-sized line, no band behind it.
            ChatBlock::Notice { text } => self
                .recorded_text(
                    Text::new(text.clone())
                        .with_color(chat_typography::chat_ink_quiet(app))
                        .with_font_family(chat_typography::chat_font(app))
                        .with_font_size(chat_typography::ROW_SIZE)
                        .with_line_height(chat_typography::ROW_LH),
                )
                .finish(),
            ChatBlock::Image { alt, url } => {
                let target = resolve_image_target(url);
                let action = target.action();
                if let Some(decoded) = decoded_image(&target) {
                    let on_action = self.on_action.clone();
                    let click = action.clone();
                    let image = ChatImage::new(decoded, self.role == ChatRole::User, app);
                    // The picture is the hit target: a local file opens in a
                    // file pane, a remote URL through the guarded opener. The
                    // caption below stays inert prose.
                    let image = Chip::flat(image.finish())
                        .with_on_click(move || {
                            if let Some(cb) = on_action.as_ref() {
                                (cb.borrow_mut())(click.clone());
                            }
                        })
                        .finish();
                    let caption = (!alt.trim().is_empty()).then(|| {
                        Text::new(alt.clone())
                            .with_color(chat_typography::chat_ink_quiet(app))
                            .with_font_family(chat_typography::chat_font(app))
                            .with_font_size(chat_typography::SMALL_SIZE)
                            .with_line_height(chat_typography::SMALL_LH)
                            .finish()
                    });
                    return ImageFigure::new(image, caption).finish();
                }
                // No local bytes to draw (a network URL, an unsupported file,
                // a failed decode): the alt text is the honest fallback. It
                // still opens the same target a decoded image would.
                let on_action = self.on_action.clone();
                Chip::new(
                    Text::new(alt.clone())
                        .with_theme_color(ColorToken::Accent, app)
                        .finish(),
                )
                .with_on_click(move || {
                    if let Some(cb) = on_action.as_ref() {
                        (cb.borrow_mut())(action.clone());
                    }
                })
                .finish()
            }
            ChatBlock::Action { label, payload } => {
                let on_action = self.on_action.clone();
                let payload = payload.clone();
                Chip::new(
                    Text::new(label.clone())
                        .with_theme_color(ColorToken::Accent, app)
                        .finish(),
                )
                .with_on_click(move || {
                    if let Some(cb) = on_action.as_ref() {
                        (cb.borrow_mut())(payload.clone());
                    }
                })
                .finish()
            }
            ChatBlock::Reasoning {
                key,
                mode,
                text,
                done,
            } => self.render_reasoning(key, mode, text, *done, app),
            ChatBlock::Terminal(data) => self.terminal_plumbing().element(data),
        }
    }

    /// A model reasoning (thinking) row, drawn the way grok-build's Thinking
    /// block is: a bold muted label — `Thought` once the step is over,
    /// `Thinking…` while it streams. Opening it shows the body behind the
    /// pager's dim quote rail; the body stays muted, since a thought is a
    /// quieter record than the commands it led to.
    fn render_reasoning(
        &self,
        key: &str,
        _mode: &str,
        text: &str,
        done: bool,
        app: &crate::elements::AppContext,
    ) -> Box<dyn Element> {
        // A step that is still streaming is open, the way grok-build keeps its
        // live thinking panel transparent; when it finishes it folds back to
        // the one-line `Thought` row unless the user opened it. An entry in the
        // app-owned map always wins, so a manual fold survives both phases.
        let opened = self.reasoning_expanded.borrow().get(key).copied();
        let expanded = opened.unwrap_or(!done);
        let label = if done { "Thought" } else { "Thinking…" };

        let toggle = self.reasoning_expanded.clone();
        let key = key.to_string();
        // DSH's disclosure row: 13 / 24 regular, tertiary ink that brightens to
        // secondary under the pointer; system family. The hover ink is a second
        // run the chip paints in place of the first while the pointer is over
        // it — the resting run is built before paint knows where the pointer is.
        let run = |ink: ColorU| {
            Text::new(label)
                .with_color(ink)
                .with_font_family(chat_typography::chat_font(app))
                .with_font_size(chat_typography::ROW_SIZE)
                .with_line_height(chat_typography::ROW_LH)
                .with_weight(FontWeight::Regular)
                .finish()
        };
        let header = Chip::flat(run(chat_typography::chat_ink_quiet(app)))
            .with_hover_variant(run(chat_typography::chat_ink(app)))
            .with_on_click(move || {
                let mut expanded = toggle.borrow_mut();
                let entry = expanded.entry(key.clone()).or_insert(false);
                *entry = !*entry;
            })
            .finish();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0)
            .with_child(header);
        if expanded {
            // A thought that is still streaming draws its tail only, bounded
            // to a few rows: grok-build draws a running thinking block
            // truncated to its last lines the same way, so the block keeps one
            // height while the model writes instead of growing to the whole
            // transcript. A body the user opened, and a finished one, draw in
            // full.
            let body = if done || opened == Some(true) {
                text.to_string()
            } else {
                reasoning_tail(text)
            };
            // The body is DSH's compact markdown: 13 / 20, system family, the
            // row's quiet ink, indented 22 px from the row's text origin (the
            // quote rail's own 2 px stroke plus the gap below).
            column = column.with_child(
                QuoteRail::new(
                    self.recorded_text(
                        Text::new(body)
                            .with_color(chat_typography::chat_ink_quiet(app))
                            .with_font_family(chat_typography::chat_font(app))
                            .with_font_size(chat_typography::ROW_SIZE)
                            .with_line_height(chat_typography::ROW_TIGHT_LH),
                    )
                    .finish(),
                    chat_typography::BODY_INDENT - QUOTE_RAIL_WIDTH,
                )
                .finish(),
            );
        }
        column.finish()
    }

    /// Render a nested block stack (a blockquote's or list item's content) in a
    /// column so it keeps its paragraph structure.
    fn render_blocks(
        &self,
        blocks: &[ChatBlock],
        app: &crate::elements::AppContext,
    ) -> Box<dyn Element> {
        // DSH's assistant block gap is 16 px; the nested stacks (a quote's or a
        // list item's blocks) keep it, so paragraphs stay distinct.
        let spacing = chat_typography::block_gap(app);
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);
        for block in blocks {
            column = column.with_child(self.render_block(block, app));
        }
        column.finish()
    }

    fn render_list(
        &self,
        items: &[ListItem],
        start: Option<u64>,
        app: &crate::elements::AppContext,
    ) -> Box<dyn Element> {
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing / 2.0);
        for (i, item) in items.iter().enumerate() {
            let marker = match start {
                Some(first) => format!("{}. ", first + i as u64),
                None => match item.checked {
                    Some(true) => "☑ ".to_string(),
                    Some(false) => "☐ ".to_string(),
                    None => "• ".to_string(),
                },
            };
            let content = self.render_blocks(&group_fragments_into_blocks(&item.content), app);
            // The row claims the width it was given and the item's content
            // takes what the marker leaves, so its paragraphs wrap at the
            // pane's edge. A content-sized row measures the text unbounded and
            // draws a long item past the panel.
            let row = Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(2.0)
                .with_child(
                    self.recorded_text(
                        Text::new(marker)
                            .with_color(chat_typography::chat_ink_quiet(app))
                            .with_font_family(chat_typography::chat_font(app))
                            .with_font_size(chat_typography::BODY_SIZE)
                            .with_line_height(chat_typography::BODY_LH),
                    )
                    .finish(),
                )
                .with_child(Expanded::new(content).with_flex(1.0).finish())
                .finish();
            column = column.with_child(row);
        }
        column.finish()
    }

    fn render_table(
        &self,
        header: &[String],
        rows: &[Vec<String>],
        app: &crate::elements::AppContext,
    ) -> Box<dyn Element> {
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let weights = table_grid::column_weights(header, rows);
        let grid = TableGrid::new(spacing);
        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        column = column.with_child(grid.row(
            self.render_table_row(header, true, &weights, &grid, app),
            !rows.is_empty(),
        ));
        for (index, row) in rows.iter().enumerate() {
            column = column.with_child(grid.row(
                self.render_table_row(row, false, &weights, &grid, app),
                index + 1 < rows.len(),
            ));
        }
        let table = grid.outline(column.finish(), app);

        // The table's footer: one copy control at the trailing edge, under the
        // grid, holding the rows as CSV. A paste into a spreadsheet wants the
        // data, not the drawn rules.
        let csv = table_csv(header, rows);
        let on_action = self.on_action.clone();
        let copy = TopbarButton::new(
            Icon::new("copy")
                .with_size(14.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_size(22.0)
        .with_on_click(move || {
            if let Some(cb) = on_action.as_ref() {
                (cb.borrow_mut())(ChatAction::CopyText(csv.clone()));
            }
        })
        .finish();
        let footer = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::End)
            .with_child(copy)
            .finish();

        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0)
            .with_child(table)
            .with_child(footer)
            .finish()
    }

    fn render_table_row(
        &self,
        cells: &[String],
        header: bool,
        weights: &[f32],
        grid: &TableGrid,
        app: &crate::elements::AppContext,
    ) -> Vec<TableCell> {
        // The row claims the transcript's width and splits it by the table's
        // column weights. A cell is an inline flow so even an over-wide token
        // breaks inside its column instead of running past the pane's edge.
        weights
            .iter()
            .enumerate()
            .map(|(index, weight)| {
                let text = cells.get(index).cloned().unwrap_or_default();
                let span = crate::elements::TextSpan::plain(text)
                    .with_color(app.theme.color(ColorToken::Text))
                    .with_family(chat_typography::chat_font(app))
                    .with_weight(if header {
                        // DSH's table head is the 500 weight; `Medium` is the
                        // bundled Roboto's closest static weight.
                        FontWeight::Medium
                    } else {
                        FontWeight::Regular
                    });
                let cell = self
                    .recorded(
                        InlineText::new(vec![span])
                            .with_font_size(chat_typography::TABLE_SIZE)
                            .with_line_height(chat_typography::TABLE_LH),
                    )
                    .finish();
                grid.cell(cell, *weight)
            })
            .collect()
    }
}

/// The rows of a table as CSV: the header first, then one line per body row,
/// every row padded to the widest one. A field is quoted only when it carries
/// a comma, a quote or a line break, and its quotes are doubled.
fn table_csv(header: &[String], rows: &[Vec<String>]) -> String {
    let width = std::iter::once(header.len())
        .chain(rows.iter().map(Vec::len))
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    let mut first = true;
    for row in std::iter::once(header).chain(rows.iter().map(|row| row.as_slice())) {
        if !first {
            out.push('\n');
        }
        first = false;
        for column in 0..width {
            if column > 0 {
                out.push(',');
            }
            let cell = row.get(column).map(String::as_str).unwrap_or("");
            if cell.contains(|c: char| matches!(c, ',' | '"' | '\n' | '\r')) {
                out.push('"');
                for glyph in cell.chars() {
                    if glyph == '"' {
                        out.push('"');
                    }
                    out.push(glyph);
                }
                out.push('"');
            } else {
                out.push_str(cell);
            }
        }
    }
    out
}

impl Element for ChatMessageBubble {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &crate::elements::AppContext,
    ) -> Vector2F {
        self.rebuild(app, constraint.max.x);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
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
        self.root.as_mut().unwrap().paint(origin, ctx, app);
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
        self.root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}
