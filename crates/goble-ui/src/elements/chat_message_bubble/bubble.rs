use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::elements::chat_content::{group_fragments_into_blocks, ChatAction, ChatBlock, ChatFragment, ChatRole, ListItem, SubAgentRow, ToolCall, ToolDisplayMode};
use crate::elements::{Chip, Code, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Fill, Flex, InlineText, LayoutContext, PaintContext, Point, SizeConstraint, TerminalFilter, Text};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};
use super::quote_rail::QuoteRail;
use crate::elements::terminal_block::TerminalBlockPlumbing;
use super::tool_call::{build_tool_call_rows, to_text_span};

pub struct ChatMessageBubble {
    role: ChatRole,
    fragments: Vec<ChatFragment>,
    tool_calls: Vec<ToolCall>,
    on_action: Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
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
    /// The live sub-agent records the parent's tool-call rows read their status
    /// from, keyed by the id of the tool call that spawned the child. The app
    /// holds them from S4's `chat:subagent_*` events.
    sub_agents: HashMap<String, SubAgentRow>,
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
            terminal_filters: Rc::new(RefCell::new(HashMap::new())),
            reasoning_expanded: Rc::new(RefCell::new(HashMap::new())),
            tool_fold: Rc::new(RefCell::new(HashMap::new())),
            sub_agents: HashMap::new(),
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

    pub fn with_global_terminal_filter(mut self, filter: Option<TerminalFilter>) -> Self {
        self.global_terminal_filter = filter;
        self
    }

    pub fn with_on_copy_terminal(mut self, on_copy: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>) -> Self {
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

    fn rebuild(&mut self, app: &crate::elements::AppContext) {
        let padding = app.theme.spacing_px(SpacingToken::Md);
        let spacing = app.theme.spacing_px(SpacingToken::Sm);

        // A transcript row is a full-width band, not a pill: the agent's own
        // reply (and the tool rows above it) draws no box at all — no fill, no
        // border, no corner radius — and the user's own message is the one
        // surface, a square grey band that spans the pane. `Md` (12 px at the
        // default density) is the transcript's padding at both edges, so no
        // line of text touches the pane's border.
        let bg = match self.role {
            ChatRole::User => Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)),
            ChatRole::Assistant | ChatRole::Tool => Fill::None,
        };

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        // Tool invocations attached to this (assistant) message render as
        // inline rows above the prose so the read is "the agent called these
        // tools, then produced this reply".
        if !self.tool_calls.is_empty() {
            column = column.with_child(build_tool_call_rows(
                &self.tool_calls,
                &self.tool_fold,
                &self.sub_agents,
                &self.on_action,
                &self.terminal_plumbing(),
                app,
            ));
        }

        for block in group_fragments_into_blocks(&self.fragments) {
            column = column.with_child(self.render_block(&block, app));
        }

        // No width cap and no radius: the row is laid out at whatever the
        // transcript gives it, which is the pane's full width.
        self.root = Some(
            Container::new(column.finish())
                .with_background(bg)
                .with_padding(EdgeInsets::uniform(padding))
                .finish(),
        );
    }

    fn render_block(&self, block: &ChatBlock, app: &crate::elements::AppContext) -> Box<dyn Element> {
        let padding = app.theme.spacing_px(SpacingToken::Md);
        match block {
            ChatBlock::Paragraph(spans) => {
                let text_spans = spans
                    .iter()
                    .map(|s| to_text_span(s, app, &self.on_action))
                    .collect();
                InlineText::new(text_spans).with_font_size(12.0).finish()
            }
            ChatBlock::Heading { level, text } => {
                let font_size = match level {
                    1 => 20.0,
                    2 => 18.0,
                    3 => 16.0,
                    _ => 14.0,
                };
                Text::new(text.clone())
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(font_size)
                    .finish()
            }
            ChatBlock::CodeBlock { lang, code } => {
                // Mono, pre-formatted, not word-wrapped; the fence's language
                // label rides along with the body, and the body is coloured by
                // that language when it resolves (otherwise it stays plain).
                let mut code_element =
                    Code::new(code.clone()).with_theme_color(ColorToken::Text, app);
                if let Some(lang) = lang {
                    code_element = code_element
                        .with_language(lang.clone())
                        .with_language_theme_color(ColorToken::Muted, app)
                        .with_highlight(lang);
                }
                // A fenced block is a full-width panel band — the same band
                // treatment the read excerpt and the terminal block use — not
                // a rounded card.
                Container::new(
                    Flex::column()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .with_child(code_element.finish())
                        .finish(),
                )
                .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                .with_padding(EdgeInsets::uniform(padding))
                .finish()
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
            ChatBlock::Image { alt, url } => {
                let on_action = self.on_action.clone();
                let url = url.clone();
                Chip::new(
                    Text::new(alt.clone())
                        .with_theme_color(ColorToken::Accent, app)
                        .finish(),
                )
                .with_on_click(move || {
                    if let Some(cb) = on_action.as_ref() {
                        (cb.borrow_mut())(ChatAction::OpenUrl(url.clone()));
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

    /// A model reasoning (thinking) row. It is recessed — muted text, no card —
    /// and collapsed to a header until the app-owned expand flag for `key` is
    /// set; the header is clickable to toggle that flag.
    fn render_reasoning(
        &self,
        key: &str,
        mode: &str,
        text: &str,
        done: bool,
        app: &crate::elements::AppContext,
    ) -> Box<dyn Element> {
        let expanded = *self.reasoning_expanded.borrow().get(key).unwrap_or(&false);
        let marker = if expanded { "▾ " } else { "▸ " };
        let mut label = if mode.is_empty() {
            "Thinking".to_string()
        } else {
            format!("Thinking · {mode}")
        };
        if !done {
            label.push('…');
        }

        let toggle = self.reasoning_expanded.clone();
        let key = key.to_string();
        let header = Chip::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(2.0)
                .with_child(
                    Text::new(marker)
                        .with_theme_color(ColorToken::Muted, app)
                        .with_font_size(11.0)
                        .finish(),
                )
                .with_child(
                    Text::new(label)
                        .with_theme_color(ColorToken::Muted, app)
                        .with_font_size(11.0)
                        .finish(),
                )
                .finish(),
        )
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
            column = column.with_child(
                Text::new(text.to_string())
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(11.0)
                    .finish(),
            );
        }
        column.finish()
    }

    /// Render a nested block stack (a blockquote's or list item's content) in a
    /// column so it keeps its paragraph structure.
    fn render_blocks(&self, blocks: &[ChatBlock], app: &crate::elements::AppContext) -> Box<dyn Element> {
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
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
            let row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(2.0)
                .with_child(
                    Text::new(marker)
                        .with_theme_color(ColorToken::Muted, app)
                        .with_font_size(12.0)
                        .finish(),
                )
                .with_child(content)
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
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing / 2.0)
            .with_child(self.render_table_row(header, app));
        column = column.with_child(Divider::horizontal().finish());
        for row in rows {
            column = column.with_child(self.render_table_row(row, app));
        }
        column.finish()
    }

    fn render_table_row(&self, cells: &[String], app: &crate::elements::AppContext) -> Box<dyn Element> {
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let mut row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(spacing);
        for cell in cells {
            row = row.with_child(
                Text::new(cell.clone())
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(12.0)
                    .finish(),
            );
        }
        row.finish()
    }
}

impl Element for ChatMessageBubble {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &crate::elements::AppContext,
    ) -> Vector2F {
        self.rebuild(app);
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
