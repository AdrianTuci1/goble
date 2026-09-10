use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::elements::chat_content::{
    group_fragments_into_blocks, ChatAction, ChatBlock, ChatFragment, ChatRole,
    InlineSpan as ModelSpan, InlineStyle, ListItem, ToolCall,
};
use goble_core::harness::{tool_presentation_for, ToolCallStatus, ToolPresentation};

use crate::elements::{
    resolve_inline_span, terminal_block, Chip, Code, ConstrainedBox, Container, CrossAxisAlignment,
    Diff, DiffLine, DiffLineKind, Divider, EdgeInsets, Element, Fill, Flex, Hunk, InlineText,
    LayoutContext, MainAxisAlignment, PaintContext, Point, SizeConstraint, TerminalCopyHandler,
    TerminalData, TerminalFilter, TerminalStatus, Text, TextSpan,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, FontFamily, SpacingToken};

const BUBBLE_MAX_WIDTH_RATIO: f32 = 0.8;

/// Resolve a model [`ModelSpan`] (text + style) into a renderable [`TextSpan`]
/// using the current theme colors.
fn to_text_span(span: &ModelSpan, app: &crate::elements::AppContext) -> TextSpan {
    let text = span.text.clone();
    let (bold, italic) = match &span.style {
        InlineStyle::Plain => (false, false),
        InlineStyle::Bold => (true, false),
        InlineStyle::Italic => (false, true),
        InlineStyle::BoldItalic => (true, true),
        InlineStyle::Code | InlineStyle::Link(_) => (false, false),
    };
    let is_code = matches!(span.style, InlineStyle::Code);
    let is_link = matches!(span.style, InlineStyle::Link(_));
    resolve_inline_span(text, bold, italic, is_code, is_link, app)
}

/// The status affordance drawn on a tool call's first row: a glyph and the
/// colour that carries the state. Read from the call's persisted
/// [`ToolCallStatus`], never inferred from the result text.
fn tool_status_affordance(status: ToolCallStatus) -> (&'static str, ColorToken) {
    match status {
        ToolCallStatus::Pending => ("○", ColorToken::Muted),
        ToolCallStatus::Running => ("◐", ColorToken::Accent),
        ToolCallStatus::Finished => ("●", ColorToken::Success),
        ToolCallStatus::Error => ("◆", ColorToken::Error),
    }
}

/// What a terminal block needs from the app wherever it is drawn: the app-owned
/// per-block filter map, the whole-transcript filter and the copy handler.
/// Bundled so a command's segment and a terminal fragment draw through one
/// renderer with one set of plumbing.
#[derive(Clone)]
struct TerminalBlockPlumbing {
    filters: Rc<RefCell<HashMap<String, TerminalFilter>>>,
    global_filter: Option<TerminalFilter>,
    on_copy: Option<TerminalCopyHandler>,
}

impl TerminalBlockPlumbing {
    /// Draw `data` as the terminal block, resolving its app-owned filter state
    /// by the block's content key.
    fn element(&self, data: &TerminalData) -> Box<dyn Element> {
        let filter = self
            .filters
            .borrow_mut()
            .entry(data.filter_key())
            .or_default()
            .clone();
        terminal_block(
            data,
            filter,
            self.global_filter.clone(),
            self.on_copy.clone(),
        )
    }
}

/// Tool invocations drawn as inline rows in the pager idiom: a status glyph and
/// the tool on the first row, then the body its own definition calls for.
/// Nothing is wrapped in a border, a box or a card; a call with nothing to show
/// is a single row, and a call with a body expands in place. The exception is a
/// command, whose body is the terminal block.
fn build_tool_call_rows(
    tool_calls: &[ToolCall],
    plumbing: &TerminalBlockPlumbing,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);

    for call in tool_calls {
        let (glyph, color) = tool_status_affordance(call.status);
        let header = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.0)
            .with_child(
                Text::new(glyph)
                    .with_theme_color(color, app)
                    .with_font_family(FontFamily::Mono)
                    .with_font_size(12.0)
                    .finish(),
            )
            .with_child(
                Text::new(call.name.clone())
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_family(FontFamily::Mono)
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish();

        let mut call_column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(2.0)
            .with_child(header);

        for row in build_tool_call_body(call, plumbing, app) {
            call_column = call_column.with_child(row);
        }
        column = column.with_child(call_column.finish());
    }
    column.finish()
}

/// One body row of a tool call: mono, at the tool row's size.
fn tool_body_row(
    text: impl Into<String>,
    color: ColorToken,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    Text::new(text.into())
        .with_theme_color(color, app)
        .with_font_family(FontFamily::Mono)
        .with_font_size(12.0)
        .finish()
}

/// A string argument, when the call carries one.
fn argument_str<'a>(arguments: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    arguments.get(key).and_then(|value| value.as_str())
}

/// The tool's result as the single row it is drawn in: muted, and absent when
/// there is nothing to show.
fn result_row(call: &ToolCall, app: &crate::elements::AppContext) -> Option<Box<dyn Element>> {
    let result = call.result.as_deref()?;
    if result.is_empty() {
        return None;
    }
    Some(tool_body_row(result.to_string(), ColorToken::Muted, app))
}

/// The body a tool call shows, shaped by the presentation its definition
/// declares in the harness registry. The shape is read from the definition the
/// harness dispatches on, so the renderer never matches on the call's name.
fn build_tool_call_body(
    call: &ToolCall,
    plumbing: &TerminalBlockPlumbing,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let arguments: serde_json::Value =
        serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
    match tool_presentation_for(&call.name) {
        ToolPresentation::Command => command_body(&arguments, call, plumbing, app),
        ToolPresentation::Path => path_body(&arguments, call, app),
        ToolPresentation::Diff => diff_body(&arguments, call, app),
        ToolPresentation::Search => search_body(&arguments, call, app),
        ToolPresentation::SubAgent => sub_agent_body(&arguments, call, app),
        ToolPresentation::Generic => generic_body(call, app),
    }
}

/// A command: the terminal block its segment is, built from the command and its
/// output. It is the same block terminal mode draws, not a re-drawn agent-output
/// row, so one command is one block wherever it appears.
fn command_body(
    arguments: &serde_json::Value,
    call: &ToolCall,
    plumbing: &TerminalBlockPlumbing,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let Some(command) = argument_str(arguments, "command") else {
        return result_row(call, app).into_iter().collect();
    };
    let data = TerminalData::for_command(
        command,
        call.result.as_deref().unwrap_or(""),
        command_status(call.status),
    );
    vec![plumbing.element(&data)]
}

/// The block's state, read from the call's persisted status. A call that has
/// not reached a terminal state is the block still running.
fn command_status(status: ToolCallStatus) -> TerminalStatus {
    match status {
        ToolCallStatus::Pending | ToolCallStatus::Running => TerminalStatus::Running,
        ToolCallStatus::Finished => TerminalStatus::Success,
        ToolCallStatus::Error => TerminalStatus::Error,
    }
}

/// A path: the path itself, then whatever the tool returned for it.
fn path_body(
    arguments: &serde_json::Value,
    call: &ToolCall,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let mut rows = Vec::new();
    if let Some(path) = argument_str(arguments, "path") {
        rows.push(tool_body_row(path.to_string(), ColorToken::Text, app));
    }
    rows.extend(result_row(call, app));
    rows
}

/// An edit: the edited path, then the replaced text as diff rows.
fn diff_body(
    arguments: &serde_json::Value,
    call: &ToolCall,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let mut rows = Vec::new();
    if let Some(path) = argument_str(arguments, "path") {
        rows.push(tool_body_row(path.to_string(), ColorToken::Text, app));
    }
    if let (Some(old_text), Some(new_text)) = (
        argument_str(arguments, "old_text"),
        argument_str(arguments, "new_text"),
    ) {
        let hunks = edit_hunks(old_text, new_text);
        if !hunks.is_empty() {
            rows.push(Box::new(Diff::new(hunks)) as Box<dyn Element>);
        }
    }
    // The result is what an edit reports when it fails ("old_text not found"),
    // so it is shown rather than hidden behind the diff.
    rows.extend(result_row(call, app));
    rows
}

/// The edit as one unified-diff hunk: the replaced text removed, the
/// replacement added. `edit_file` substitutes the first occurrence, so there is
/// no surrounding context to carry and the hunk is numbered from its own start.
fn edit_hunks(old_text: &str, new_text: &str) -> Vec<Hunk> {
    let removed: Vec<&str> = old_text.lines().collect();
    let added: Vec<&str> = new_text.lines().collect();
    let mut lines = Vec::new();
    for (index, text) in removed.iter().enumerate() {
        lines.push(DiffLine {
            kind: DiffLineKind::Removed,
            old_line: Some(index as u32 + 1),
            new_line: None,
            text: text.to_string(),
        });
    }
    for (index, text) in added.iter().enumerate() {
        lines.push(DiffLine {
            kind: DiffLineKind::Added,
            old_line: None,
            new_line: Some(index as u32 + 1),
            text: text.to_string(),
        });
    }
    if lines.is_empty() {
        return Vec::new();
    }
    vec![Hunk {
        old_start: 1,
        old_count: removed.len() as u32,
        new_start: 1,
        new_count: added.len() as u32,
        section: String::new(),
        lines,
    }]
}

/// A search: the query, then the sources its result names.
fn search_body(
    arguments: &serde_json::Value,
    call: &ToolCall,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let mut rows = Vec::new();
    if let Some(query) = argument_str(arguments, "query") {
        rows.push(tool_body_row(query.to_string(), ColorToken::Text, app));
    }
    if let Some(result) = call.result.as_deref() {
        let sources = search_sources(result);
        if sources.is_empty() {
            // No parsable source (an error, say): show the result itself rather
            // than hiding what the tool said.
            if !result.is_empty() {
                rows.push(tool_body_row(result.to_string(), ColorToken::Muted, app));
            }
        } else {
            for source in sources {
                rows.push(tool_body_row(source, ColorToken::Accent, app));
            }
        }
    }
    rows
}

/// The URLs a search result names, one per source, in result order.
fn search_sources(result: &str) -> Vec<String> {
    result
        .lines()
        .filter_map(|line| line.strip_prefix("URL: "))
        .filter(|url| !url.is_empty())
        .map(str::to_string)
        .collect()
}

/// A sub-agent: its id, the input it was given, then its output.
fn sub_agent_body(
    arguments: &serde_json::Value,
    call: &ToolCall,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let mut rows = Vec::new();
    if let Some(agent) =
        argument_str(arguments, "agent_id").or_else(|| argument_str(arguments, "id"))
    {
        rows.push(tool_body_row(agent.to_string(), ColorToken::Text, app));
    }
    if let Some(input) =
        argument_str(arguments, "input").or_else(|| argument_str(arguments, "prompt"))
    {
        rows.push(tool_body_row(input.to_string(), ColorToken::Muted, app));
    }
    rows.extend(result_row(call, app));
    rows
}

/// A tool with no declared shape: its arguments, then its result.
fn generic_body(call: &ToolCall, app: &crate::elements::AppContext) -> Vec<Box<dyn Element>> {
    let mut rows = Vec::new();
    if !call.arguments.is_empty() && call.arguments != "{}" {
        rows.push(tool_body_row(
            call.arguments.clone(),
            ColorToken::Muted,
            app,
        ));
    }
    rows.extend(result_row(call, app));
    rows
}

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
        TerminalBlockPlumbing {
            filters: self.terminal_filters.clone(),
            global_filter: self.global_terminal_filter.clone(),
            on_copy: self.on_copy_terminal.clone(),
        }
    }

    fn rebuild(&mut self, app: &crate::elements::AppContext, max_width: f32) {
        let padding = app.theme.spacing_px(SpacingToken::Md);
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let radius = app.theme.radius_px();

        // Warp-new renders a message block on `surface_1`; the tool/command
        // area is what carries the `SurfaceRaised` (`surface_2`) styling. A
        // tool call is not a raised card, though: it is inline rows, so its
        // styling is carried by the status glyph's colour, not a box.
        let bg = match self.role {
            ChatRole::User => app.theme.color(ColorToken::SurfaceRaised),
            ChatRole::Assistant => app.theme.color(ColorToken::Surface),
            ChatRole::Tool => app.theme.color(ColorToken::Surface),
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
                &self.terminal_plumbing(),
                app,
            ));
        }

        for block in group_fragments_into_blocks(&self.fragments) {
            column = column.with_child(self.render_block(&block, app));
        }

        let bubble = Container::new(column.finish())
            .with_background(Fill::Solid(bg))
            .with_padding(EdgeInsets::uniform(padding))
            .with_corner_radius(radius)
            .finish();

        let alignment = match self.role {
            ChatRole::User => MainAxisAlignment::End,
            ChatRole::Assistant | ChatRole::Tool => MainAxisAlignment::Start,
        };

        let bubble_max_width = (max_width * BUBBLE_MAX_WIDTH_RATIO).max(80.0);
        self.root = Some(
            Flex::row()
                .with_main_axis_alignment(alignment)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(
                    ConstrainedBox::new(bubble)
                        .with_max_width(bubble_max_width)
                        .finish(),
                )
                .finish(),
        );
    }

    fn render_block(&self, block: &ChatBlock, app: &crate::elements::AppContext) -> Box<dyn Element> {
        let padding = app.theme.spacing_px(SpacingToken::Md);
        let radius = app.theme.radius_px();
        match block {
            ChatBlock::Paragraph(spans) => {
                let text_spans = spans.iter().map(|s| to_text_span(s, app)).collect();
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
                // label rides along with the body.
                let mut code_element =
                    Code::new(code.clone()).with_theme_color(ColorToken::Text, app);
                if let Some(lang) = lang {
                    code_element = code_element
                        .with_language(lang.clone())
                        .with_language_theme_color(ColorToken::Muted, app);
                }
                Container::new(code_element.finish())
                    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                    .with_padding(EdgeInsets::uniform(padding))
                    .with_corner_radius(radius)
                    .finish()
            }
            ChatBlock::List { items, start } => self.render_list(items, *start, app),
            ChatBlock::BlockQuote(inner) => Container::new(self.render_blocks(inner, app))
                .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                .with_padding(EdgeInsets::new(
                    padding,
                    padding / 2.0,
                    padding,
                    padding / 2.0,
                ))
                .with_corner_radius(radius)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorU;
    use crate::elements::chat_content::ChatMessage;
    use crate::elements::{
        terminal_block, AppContext, LayoutContext, PaintContext, TerminalData, TerminalLine,
        TerminalStatus,
    };
    use crate::geometry::vec2f;
    use crate::platform::text_atlas::FontWeight;
    use crate::render::{RenderCommand, Renderer};
    use crate::test_util::render_element;
    use goble_core::harness::ToolCallStatus;

    #[test]
    fn bubble_layouts_non_zero() {
        let app = AppContext::default();
        let mut bubble =
            ChatMessageBubble::new(ChatRole::Assistant, vec![ChatFragment::text("Hello")]);
        let size = bubble.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    /// Paint an assistant bubble holding only `calls`; return its render
    /// commands and its measured height.
    fn paint_tool_calls(calls: Vec<ToolCall>) -> (Vec<RenderCommand>, f32) {
        let app = AppContext::default();
        let mut bubble =
            ChatMessageBubble::new(ChatRole::Assistant, Vec::new()).with_tool_calls(calls);
        let size = bubble.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut paint_ctx = PaintContext::new(Renderer::new());
        bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default();
        (commands, size.y)
    }

    fn drawn_texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// How many distinct rows (by text baseline) the commands draw.
    fn drawn_rows(commands: &[RenderCommand]) -> usize {
        let mut rows: Vec<i32> = commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText { origin, .. } => Some((origin.y * 10.0).round() as i32),
                _ => None,
            })
            .collect();
        rows.sort_unstable();
        rows.dedup();
        rows.len()
    }

    /// The drawn text runs as `(text, colour, family, size)`, so two elements
    /// can be compared without depending on where each was laid out.
    fn text_runs(commands: &[RenderCommand]) -> Vec<(String, ColorU, FontFamily, f32)> {
        commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText {
                    text,
                    color,
                    font_family,
                    font_size,
                    ..
                } => Some((text.clone(), *color, *font_family, *font_size)),
                _ => None,
            })
            .collect()
    }

    fn call(name: &str, arguments: &str, status: ToolCallStatus, result: Option<&str>) -> ToolCall {
        ToolCall {
            id: format!("call_{name}"),
            name: name.to_string(),
            arguments: arguments.to_string(),
            status,
            result: result.map(str::to_string),
        }
    }

    /// A tool call is inline rows: no border stroke and no raised `surface_2`
    /// card background.
    #[test]
    fn tool_call_draws_no_border_or_card() {
        let (commands, _) =
            paint_tool_calls(vec![call("ls", "{}", ToolCallStatus::Finished, None)]);

        let borders = commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
            .count();
        assert_eq!(borders, 0, "a tool call must not draw a border container");

        let app = AppContext::default();
        let card_bg = app.theme.color(ColorToken::SurfaceRaised);
        let card_fills = commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == card_bg))
            .count();
        assert_eq!(
            card_fills, 0,
            "a tool call must not paint a raised card background"
        );
    }

    /// A call with nothing to show collapses to a single row — the status glyph
    /// and the tool name, on one baseline, with no argument or result row.
    #[test]
    fn collapsed_tool_call_renders_in_one_row() {
        let (commands, _) =
            paint_tool_calls(vec![call("ls", "{}", ToolCallStatus::Finished, None)]);

        assert_eq!(
            drawn_rows(&commands),
            1,
            "a collapsed tool call must occupy one row"
        );
        let texts = drawn_texts(&commands);
        assert!(
            texts.iter().any(|t| t == "ls"),
            "the tool name is drawn, got {texts:?}"
        );
        assert_eq!(
            texts.len(),
            2,
            "only the status glyph and the tool name are drawn, got {texts:?}"
        );
    }

    /// A tool whose definition declares no shape (here `list_entities`) expands
    /// in place: its arguments and its result are drawn as their own rows
    /// beneath the header.
    #[test]
    fn expanded_generic_tool_call_draws_arguments_and_result_in_place() {
        let (commands, _) = paint_tool_calls(vec![call(
            "list_entities",
            r#"{"kind":"agent"}"#,
            ToolCallStatus::Running,
            Some("2 entities"),
        )]);

        let texts = drawn_texts(&commands);
        assert!(
            texts.iter().any(|t| t == "list_entities"),
            "the tool name is drawn, got {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == r#"{"kind":"agent"}"#),
            "the arguments row is drawn, got {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "2 entities"),
            "the result row is drawn, got {texts:?}"
        );
        assert_eq!(
            drawn_rows(&commands),
            3,
            "header + arguments + result are three inline rows, got {texts:?}"
        );
    }

    /// Each call is presented by the shape its own definition declares; the
    /// renderer asks the harness registry instead of matching the call's name.
    #[test]
    fn tool_shapes_are_read_from_the_harness_definitions() {
        assert_eq!(
            tool_presentation_for("run_command"),
            ToolPresentation::Command
        );
        assert_eq!(tool_presentation_for("read_file"), ToolPresentation::Path);
        assert_eq!(tool_presentation_for("edit_file"), ToolPresentation::Diff);
        assert_eq!(
            tool_presentation_for("web_search"),
            ToolPresentation::Search
        );
        assert_eq!(
            tool_presentation_for("run_agent"),
            ToolPresentation::SubAgent
        );
        assert_eq!(
            tool_presentation_for("credentials"),
            ToolPresentation::Generic
        );
    }

    /// A command is drawn as the terminal block: its command line, its output
    /// and the block's own title, never the raw argument JSON.
    #[test]
    fn command_call_shows_the_command_and_its_output() {
        let (commands, _) = paint_tool_calls(vec![call(
            "run_command",
            r#"{"command":"cargo test -p goble-ui"}"#,
            ToolCallStatus::Finished,
            Some("test result: ok. 225 passed"),
        )]);

        let texts = drawn_texts(&commands);
        assert!(
            texts.iter().any(|t| t == "cargo test -p goble-ui"),
            "the command is the block's title and its command line, got {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "test result: ok. 225 passed"),
            "the output is shown, got {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains(r#"{"command""#)),
            "the raw argument JSON is not shown, got {texts:?}"
        );
    }

    /// The segment a command the agent ran is drawn as is the terminal block,
    /// and it renders identically in the two places a block appears: the
    /// transcript and terminal mode.
    #[test]
    fn a_command_segment_is_the_same_block_terminal_mode_draws() {
        let app = AppContext::default();
        let data = TerminalData::for_command(
            "cargo test -p goble-ui",
            "test result: ok. 231 passed",
            TerminalStatus::Success,
        );

        // Terminal mode: the block on its own, exactly as the pane draws it.
        let mut block = terminal_block(&data, TerminalFilter::default(), None, None);
        let block_runs = text_runs(&render_element(&mut block, vec2f(600.0, 200.0), &app));

        // The transcript: the same command as the agent's tool call.
        let (commands, _) = paint_tool_calls(vec![call(
            "run_command",
            r#"{"command":"cargo test -p goble-ui"}"#,
            ToolCallStatus::Finished,
            Some("test result: ok. 231 passed"),
        )]);
        let transcript_runs = text_runs(&commands);

        assert!(
            !block_runs.is_empty(),
            "the shared block draws its title, command and output"
        );
        // The transcript draws the tool-call header (status glyph + tool name)
        // and then the block itself; the block runs must match exactly.
        assert_eq!(
            &transcript_runs[2..],
            block_runs.as_slice(),
            "the transcript's command segment must be the block terminal mode draws"
        );
    }

    /// A read shows a path rather than the arguments blob.
    #[test]
    fn read_call_shows_the_path() {
        let (commands, _) = paint_tool_calls(vec![call(
            "read_file",
            r#"{"path":"src/lib.rs"}"#,
            ToolCallStatus::Finished,
            Some("fn main() {}"),
        )]);

        let texts = drawn_texts(&commands);
        assert!(
            texts.iter().any(|t| t == "src/lib.rs"),
            "the path is shown, got {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains(r#"{"path""#)),
            "the raw argument JSON is not shown, got {texts:?}"
        );
    }

    /// An edit shows diff rows: the removed text, the added text and the Q9
    /// element's `-`/`+` markers.
    #[test]
    fn edit_call_shows_diff_rows() {
        let (commands, _) = paint_tool_calls(vec![call(
            "edit_file",
            r#"{"path":"src/lib.rs","old_text":"let x = 1;","new_text":"let x = 2;"}"#,
            ToolCallStatus::Finished,
            Some(r#"edited "src/lib.rs""#),
        )]);

        let texts = drawn_texts(&commands);
        assert!(
            texts.iter().any(|t| t == "src/lib.rs"),
            "the edited path is shown, got {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "let x = 1;"),
            "the removed text is a diff row, got {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "let x = 2;"),
            "the added text is a diff row, got {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "-"),
            "the removed row's marker is drawn, got {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "+"),
            "the added row's marker is drawn, got {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("old_text")),
            "the raw argument JSON is not shown, got {texts:?}"
        );
    }

    /// A web search shows the query and the sources its result names.
    #[test]
    fn web_search_call_shows_the_query_and_its_sources() {
        let result = "2 results\nTITLE: Async Rust\nURL: https://example.com/async\nSNIPPET: a\n\nTITLE: Tokio\nURL: https://example.com/tokio\nSNIPPET: b\n";
        let (commands, _) = paint_tool_calls(vec![call(
            "web_search",
            r#"{"query":"rust async"}"#,
            ToolCallStatus::Finished,
            Some(result),
        )]);

        let texts = drawn_texts(&commands);
        assert!(
            texts.iter().any(|t| t == "rust async"),
            "the query is shown, got {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "https://example.com/async"),
            "the first source is shown, got {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "https://example.com/tokio"),
            "the second source is shown, got {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("TITLE:")),
            "the raw result is not shown, got {texts:?}"
        );
    }

    /// A sub-agent shows its own rows: its id, the input, then its output.
    #[test]
    fn sub_agent_call_shows_its_own_rows() {
        let (commands, _) = paint_tool_calls(vec![call(
            "run_agent",
            r#"{"agent_id":"researcher","input":"summarize the changelog"}"#,
            ToolCallStatus::Running,
            None,
        )]);

        let texts = drawn_texts(&commands);
        assert!(
            texts.iter().any(|t| t == "researcher"),
            "the sub-agent id is shown, got {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "summarize the changelog"),
            "the sub-agent's input is shown, got {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("agent_id")),
            "the raw argument JSON is not shown, got {texts:?}"
        );
    }

    /// The status affordance is read from the persisted status: the glyph and
    /// colour change with it, and no result text is inspected to infer it.
    #[test]
    fn status_affordance_reflects_the_persisted_status() {
        let app = AppContext::default();
        let cases = [
            (ToolCallStatus::Pending, "○", ColorToken::Muted),
            (ToolCallStatus::Running, "◐", ColorToken::Accent),
            (ToolCallStatus::Finished, "●", ColorToken::Success),
            (ToolCallStatus::Error, "◆", ColorToken::Error),
        ];
        for (status, glyph, token) in cases {
            let (commands, _) = paint_tool_calls(vec![call("ls", "{}", status, None)]);
            let drawn = commands.iter().find_map(|c| match c {
                RenderCommand::DrawText { text, color, .. } if text == glyph => Some(*color),
                _ => None,
            });
            assert_eq!(
                drawn,
                Some(app.theme.color(token)),
                "status {status:?} draws {glyph:?} in {token:?}"
            );
        }
    }

    /// A `role="tool"` result row still renders as its terminal block; the tool
    /// call no longer adds a card beside it.
    #[test]
    fn tool_result_block_still_renders_for_a_tool_row() {
        let app = AppContext::default();
        let mut bubble = ChatMessageBubble::new(
            ChatRole::Tool,
            vec![ChatFragment::terminal(TerminalData::new(
                "call_1",
                vec![TerminalLine::output("file.txt")],
            ))],
        );
        let size = bubble.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);

        let mut paint_ctx = PaintContext::new(Renderer::new());
        bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default();
        let copy_icons = commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::DrawIcon { name, .. } if name == "copy"))
            .count();
        assert!(copy_icons >= 1, "expected a terminal block copy control");
        let text = drawn_texts(&commands);
        assert!(
            text.iter().any(|t| t == "call_1"),
            "expected the block title to be drawn, got {text:?}"
        );
        assert!(
            text.iter().any(|t| t == "file.txt"),
            "expected the block output to be drawn, got {text:?}"
        );
    }

    #[test]
    fn action_click_fires_callback() {
        let app = AppContext::default();
        let action = ChatAction::Custom("test".to_string());
        let triggered = Rc::new(RefCell::new(None));
        let triggered_clone = triggered.clone();
        let mut bubble = ChatMessageBubble::new(
            ChatRole::Assistant,
            vec![ChatFragment::action("Run", action.clone())],
        )
        .with_on_action(move |a| *triggered_clone.borrow_mut() = Some(a));

        bubble.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        bubble.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = crate::elements::EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(20.0, 20.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(20.0, 20.0),
            button: 0,
        };

        assert!(bubble.dispatch_event(&down, &mut event_ctx, &app));
        assert!(bubble.dispatch_event(&up, &mut event_ctx, &app));
        assert_eq!(triggered.borrow().as_ref(), Some(&action));
    }

    /// Paint a Markdown message and return the style of every text run it draws.
    fn painted_runs(markdown: &str) -> Vec<(String, FontWeight, bool)> {
        let app = AppContext::default();
        let message = ChatMessage::from_markdown(ChatRole::Assistant, markdown);
        let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments);
        bubble.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut paint_ctx = PaintContext::new(Renderer::new());
        bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText {
                    text,
                    font_weight,
                    font_italic,
                    ..
                } => Some((text, font_weight, font_italic)),
                _ => None,
            })
            .collect()
    }

    fn run_style(markdown: &str) -> (FontWeight, bool) {
        painted_runs(markdown)
            .into_iter()
            .find_map(|(text, weight, italic)| (text == "x").then_some((weight, italic)))
            .unwrap_or_else(|| panic!("no run drawing \"x\" for {markdown:?}"))
    }

    #[test]
    fn italic_is_not_bold_and_bold_italic_is_both() {
        assert_eq!(
            run_style("_x_"),
            (FontWeight::Regular, true),
            "_x_ must render italic and not bold"
        );
        assert_eq!(
            run_style("***x***"),
            (FontWeight::Bold, true),
            "***x*** must render bold and italic"
        );
        assert_eq!(
            run_style("**x**"),
            (FontWeight::Bold, false),
            "**x** must stay bold-only"
        );
    }

    /// Paint a bubble holding one reasoning row and return its commands.
    fn reasoning_commands(
        expanded: &Rc<RefCell<HashMap<String, bool>>>,
        key: &str,
    ) -> Vec<RenderCommand> {
        let app = AppContext::default();
        let message = ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::reasoning(
                key,
                "contemplating",
                "weighing the options carefully",
                true,
            )],
        );
        let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments)
            .with_reasoning_expanded(expanded.clone());
        bubble.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut paint_ctx = PaintContext::new(Renderer::new());
        bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default()
    }

    #[test]
    fn reasoning_row_is_recessed_and_collapsed_until_expanded() {
        use crate::color::ColorU;

        let app = AppContext::default();
        let expanded = Rc::new(RefCell::new(HashMap::new()));
        let key = "c1:0";

        let collapsed = reasoning_commands(&expanded, key);
        let texts: Vec<(String, ColorU)> = collapsed
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, color, .. } => Some((text.clone(), *color)),
                _ => None,
            })
            .collect();

        let header = texts
            .iter()
            .find(|(text, _)| text.contains("Thinking"))
            .unwrap_or_else(|| panic!("the reasoning header must be drawn, got {texts:?}"));
        assert_eq!(
            header.1,
            app.theme.color(ColorToken::Muted),
            "the reasoning row must be recessed (muted), got {header:?}"
        );
        assert!(
            texts.iter().any(|(text, _)| text == "▸ "),
            "a collapsed row shows the closed marker, got {texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|(text, _)| text.contains("weighing the options")),
            "the body must be hidden while collapsed, got {texts:?}"
        );

        // The app-owned flag is what opens the row.
        expanded.borrow_mut().insert(key.to_string(), true);
        let open: Vec<String> = reasoning_commands(&expanded, key)
            .into_iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            open.iter()
                .any(|text| text.contains("weighing the options")),
            "an expanded row draws its body, got {open:?}"
        );
        assert!(
            open.iter().any(|text| text == "▾ "),
            "an expanded row shows the open marker, got {open:?}"
        );
    }

    #[test]
    fn reasoning_header_click_toggles_the_row() {
        let app = AppContext::default();
        let expanded = Rc::new(RefCell::new(HashMap::new()));
        let key = "c1:0";
        let mut bubble = ChatMessageBubble::new(
            ChatRole::Assistant,
            vec![ChatFragment::reasoning(key, "contemplating", "body", true)],
        )
        .with_reasoning_expanded(expanded.clone());
        bubble.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        bubble.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = crate::elements::EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(20.0, 20.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(20.0, 20.0),
            button: 0,
        };
        assert!(bubble.dispatch_event(&down, &mut event_ctx, &app));
        assert!(bubble.dispatch_event(&up, &mut event_ctx, &app));
        assert_eq!(
            expanded.borrow().get(key),
            Some(&true),
            "clicking the header must expand the row"
        );
    }

    #[test]
    fn fenced_code_block_paints_in_the_mono_family() {
        use crate::theme::FontFamily;

        let app = AppContext::default();
        let message = ChatMessage::from_markdown(ChatRole::Assistant, "```rust\nlet x = 1;\n```");
        let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments);
        bubble.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut paint_ctx = PaintContext::new(Renderer::new());
        bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let runs: Vec<(String, FontFamily)> = paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText {
                    text, font_family, ..
                } => Some((text, font_family)),
                _ => None,
            })
            .collect();

        let body = runs
            .iter()
            .find(|(text, _)| text.contains("let x = 1;"))
            .unwrap_or_else(|| panic!("the fenced body must be drawn, got {runs:?}"));
        assert_eq!(
            body.1,
            FontFamily::Mono,
            "the code body must be mono, got {runs:?}"
        );
        assert!(
            runs.iter()
                .any(|(text, family)| text == "rust" && *family == FontFamily::Mono),
            "the fence's language label must be drawn in mono, got {runs:?}"
        );
    }
}
