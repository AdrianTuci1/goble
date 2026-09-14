use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::chat_content::{
    ChatAction, ChatFragment, ChatFragmentKind, ChatMessage, ChatRole, ListItem,
};
use crate::elements::chat_message_bubble::QuoteRail;
use crate::elements::{
    terminal_block, AppContext, Avatar, Chip, Container, CrossAxisAlignment, Divider, EdgeInsets,
    Element, Empty, Fill, Flex, LayoutContext, PaintContext, Point, SizeConstraint, Spacer,
    TerminalData, TerminalFilter, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

/// Renders a single message inside a group-chat thread.
///
/// When `show_header` is true the message shows an avatar, author name, and
/// timestamp. When false only the content is rendered, indented to align with
/// messages that do have a header.
pub struct GroupChatMessage {
    message: ChatMessage,
    show_header: bool,
    avatar_size: f32,
    on_action: Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl GroupChatMessage {
    pub fn new(message: ChatMessage) -> Self {
        Self {
            message,
            show_header: true,
            avatar_size: 36.0,
            on_action: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_show_header(mut self, show: bool) -> Self {
        self.show_header = show;
        self
    }

    pub fn with_avatar_size(mut self, size: f32) -> Self {
        self.avatar_size = size;
        self
    }

    pub fn with_on_action<F: FnMut(ChatAction) + 'static>(mut self, callback: F) -> Self {
        self.on_action = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn author_label(&self) -> String {
        self.message
            .author_name
            .clone()
            .unwrap_or_else(|| role_label(self.message.role).to_string())
    }

    fn initials(&self) -> String {
        self.author_label()
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .collect::<String>()
            .to_uppercase()
    }

    fn avatar_color_token(&self) -> ColorToken {
        match self.message.role {
            ChatRole::User => ColorToken::Accent,
            ChatRole::Assistant => ColorToken::Success,
            ChatRole::Tool => ColorToken::Warning,
        }
    }

    fn rebuild(&mut self, app: &AppContext) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let radius = app.theme.radius_px();

        let content_column = self.build_content_column(app, spacing, radius);

        let row = if self.show_header {
            let avatar = Avatar::new(self.initials())
                .with_size(self.avatar_size)
                .with_theme_background(self.avatar_color_token(), app)
                .with_theme_foreground(ColorToken::Text, app)
                .finish();

            let mut header = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(spacing / 2.0)
                .with_child(
                    Text::new(self.author_label())
                        .with_theme_color(ColorToken::Text, app)
                        .with_font_size(12.0)
                        .finish(),
                );
            if let Some(ts) = self.message.timestamp.as_ref() {
                header = header.with_child(
                    Text::new(ts.clone())
                        .with_theme_color(ColorToken::Muted, app)
                        .with_font_size(10.0)
                        .finish(),
                );
            }

            let right_column = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(sm / 2.0)
                .with_child(header.finish())
                .with_child(content_column)
                .finish();

            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(spacing)
                .with_child(avatar)
                .with_child(right_column)
        } else {
            let spacer = Empty::new()
                .with_size(crate::geometry::vec2f(self.avatar_size, 1.0))
                .finish();

            let right_column = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(sm / 2.0)
                .with_child(content_column)
                .finish();

            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(spacing)
                .with_child(spacer)
                .with_child(right_column)
        };

        self.root = Some(row.finish());
    }

    fn build_content_column(
        &self,
        app: &AppContext,
        spacing: f32,
        radius: f32,
    ) -> Box<dyn Element> {
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing / 2.0);

        let mut inline_buffer: Vec<Box<dyn Element>> = Vec::new();

        for fragment in &self.message.fragments {
            match &fragment.kind {
                ChatFragmentKind::LineBreak => {
                    column = flush_inline(&mut inline_buffer, column);
                    column = column.with_child(Spacer::new().finish());
                }
                ChatFragmentKind::CodeBlock { lang, code } => {
                    column = flush_inline(&mut inline_buffer, column);
                    column = column.with_child(self.render_code_block(
                        app,
                        lang.clone(),
                        code.clone(),
                        spacing,
                    ));
                }
                ChatFragmentKind::Heading { level, text } => {
                    column = flush_inline(&mut inline_buffer, column);
                    column = column.with_child(self.render_heading(app, *level, text.clone()));
                }
                ChatFragmentKind::List { items, start } => {
                    column = flush_inline(&mut inline_buffer, column);
                    column = column.with_child(self.render_list(app, items, *start));
                }
                ChatFragmentKind::BlockQuote(content) => {
                    column = flush_inline(&mut inline_buffer, column);
                    column = column.with_child(self.render_block_quote(app, content, spacing));
                }
                ChatFragmentKind::Table { header, rows } => {
                    column = flush_inline(&mut inline_buffer, column);
                    column = column.with_child(self.render_table(app, header, rows, spacing));
                }
                ChatFragmentKind::Rule => {
                    column = flush_inline(&mut inline_buffer, column);
                    column = column.with_child(Divider::horizontal().finish());
                }
                ChatFragmentKind::Terminal(data) => {
                    column = flush_inline(&mut inline_buffer, column);
                    column = column.with_child(self.render_terminal(data));
                }
                _ => {
                    inline_buffer.push(self.render_inline_fragment(app, fragment, radius));
                }
            }
        }
        column = flush_inline(&mut inline_buffer, column);

        column.finish()
    }

    fn render_inline_fragment(
        &self,
        app: &AppContext,
        fragment: &ChatFragment,
        radius: f32,
    ) -> Box<dyn Element> {
        let padding = app.theme.spacing_px(SpacingToken::Md);
        match &fragment.kind {
            ChatFragmentKind::Text(text) => Text::new(text.clone())
                .with_theme_color(ColorToken::Text, app)
                .finish(),
            ChatFragmentKind::Bold(text) => Text::new(text.clone())
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
            ChatFragmentKind::Italic(text) => Text::new(text.clone())
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
            ChatFragmentKind::BoldItalic(text) => Text::new(text.clone())
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
            ChatFragmentKind::Code(code) => Container::new(
                Text::new(code.clone())
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_padding(EdgeInsets::uniform(padding / 4.0))
            .with_corner_radius(radius / 2.0)
            .finish(),
            ChatFragmentKind::Link { label, url } => {
                let on_action = self.on_action.clone();
                let url = url.clone();
                Chip::new(
                    Text::new(label.clone())
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
            ChatFragmentKind::Image { alt, url } => {
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
            ChatFragmentKind::Action { label, payload } => {
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
            _ => Empty::new().finish(),
        }
    }

    /// A terminal segment — a command the agent ran and its output — drawn
    /// through the one terminal-block renderer, the same element the
    /// transcript's command tool call and terminal mode draw. One block, one
    /// renderer.
    fn render_terminal(&self, data: &TerminalData) -> Box<dyn Element> {
        terminal_block(data, TerminalFilter::default(), None, None)
    }

    fn render_code_block(
        &self,
        app: &AppContext,
        lang: Option<String>,
        code: String,
        padding: f32,
    ) -> Box<dyn Element> {
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0);
        if let Some(lang) = lang {
            column = column.with_child(
                Text::new(lang)
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(10.0)
                    .finish(),
            );
        }
        column = column.with_child(
            Text::new(code)
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        );
        // A fenced block is a band, not a rounded box. The column stretches, so
        // the band spans the message width.
        Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_padding(EdgeInsets::uniform(padding / 2.0))
            .finish()
    }

    fn render_heading(&self, app: &AppContext, level: u8, text: String) -> Box<dyn Element> {
        let font_size = match level {
            1 => 18.0,
            2 => 16.0,
            3 => 15.0,
            _ => 14.0,
        };
        Text::new(text)
            .with_theme_color(ColorToken::Text, app)
            .with_font_size(font_size)
            .finish()
    }

    fn render_list(
        &self,
        app: &AppContext,
        items: &[ListItem],
        start: Option<u64>,
    ) -> Box<dyn Element> {
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0);
        for (i, item) in items.iter().enumerate() {
            let prefix = match start {
                Some(first) => format!("{}. ", first + i as u64),
                None => match item.checked {
                    Some(true) => "☑ ".to_string(),
                    Some(false) => "☐ ".to_string(),
                    None => "• ".to_string(),
                },
            };
            column = column.with_child(
                Text::new(format!("{}{}", prefix, fragment_plain_text(&item.content)))
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            );
        }
        column.finish()
    }

    fn render_block_quote(
        &self,
        app: &AppContext,
        content: &[ChatFragment],
        padding: f32,
    ) -> Box<dyn Element> {
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(padding / 4.0);
        let mut paragraph: Vec<ChatFragment> = Vec::new();
        let mut paragraphs: Vec<Vec<ChatFragment>> = Vec::new();
        for fragment in content {
            if matches!(fragment.kind, ChatFragmentKind::LineBreak) {
                if !paragraph.is_empty() {
                    paragraphs.push(std::mem::take(&mut paragraph));
                }
            } else {
                paragraph.push(fragment.clone());
            }
        }
        if !paragraph.is_empty() {
            paragraphs.push(paragraph);
        }
        for paragraph in &paragraphs {
            column = column.with_child(
                Text::new(fragment_plain_text(paragraph))
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        }
        // A quote is an indent plus a rail, not a rounded box.
        QuoteRail::new(column.finish(), padding / 2.0).finish()
    }

    fn render_table(
        &self,
        app: &AppContext,
        header: &[String],
        rows: &[Vec<String>],
        spacing: f32,
    ) -> Box<dyn Element> {
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing / 4.0);
        column = column.with_child(self.render_table_row(app, header, spacing));
        column = column.with_child(Divider::horizontal().finish());
        for row in rows {
            column = column.with_child(self.render_table_row(app, row, spacing));
        }
        column.finish()
    }

    fn render_table_row(
        &self,
        app: &AppContext,
        cells: &[String],
        spacing: f32,
    ) -> Box<dyn Element> {
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

/// Concatenate the human-readable text of a nested fragment run, recursing into
/// blockquotes and lists so their content is not lost.
fn fragment_plain_text(fragments: &[ChatFragment]) -> String {
    let mut out = String::new();
    for fragment in fragments {
        match &fragment.kind {
            ChatFragmentKind::Text(t)
            | ChatFragmentKind::Bold(t)
            | ChatFragmentKind::Italic(t)
            | ChatFragmentKind::BoldItalic(t)
            | ChatFragmentKind::Code(t) => out.push_str(t),
            ChatFragmentKind::Link { label, .. } => out.push_str(label),
            ChatFragmentKind::Image { alt, .. } => out.push_str(alt),
            ChatFragmentKind::BlockQuote(inner) => out.push_str(&fragment_plain_text(inner)),
            ChatFragmentKind::List { items, .. } => {
                for item in items {
                    out.push_str(&fragment_plain_text(&item.content));
                    out.push(' ');
                }
            }
            ChatFragmentKind::LineBreak => out.push(' '),
            _ => {}
        }
    }
    out.trim().to_string()
}

fn role_label(role: ChatRole) -> &'static str {
    match role {
        ChatRole::User => "You",
        ChatRole::Assistant => "Assistant",
        ChatRole::Tool => "Tool",
    }
}

fn flush_inline(buffer: &mut Vec<Box<dyn Element>>, mut column: Flex) -> Flex {
    if !buffer.is_empty() {
        let mut row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(2.0);
        for child in buffer.drain(..) {
            row = row.with_child(child);
        }
        column = column.with_child(row.finish());
    }
    column
}

impl Element for GroupChatMessage {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
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
        app: &AppContext,
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
    use crate::elements::{AppContext, LayoutContext, TerminalLine, TerminalStatus};
    use crate::geometry::vec2f;
    use crate::render::{RenderCommand, Renderer};

    /// The drawn text runs as `(text, size)`, so two elements can be compared
    /// without depending on where each was laid out.
    fn text_runs(commands: &[RenderCommand]) -> Vec<(String, f32)> {
        commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText {
                    text, font_size, ..
                } => Some((text.clone(), *font_size)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn group_chat_message_layouts_with_header() {
        let app = AppContext::default();
        let message = ChatMessage::new(ChatRole::User, vec![ChatFragment::text("Hello")])
            .with_author_name("Ada")
            .with_timestamp("10:42");
        let mut msg = GroupChatMessage::new(message);
        let size = msg.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn group_chat_message_compact_layouts() {
        let app = AppContext::default();
        let message = ChatMessage::new(ChatRole::User, vec![ChatFragment::text("Second message")])
            .with_author_name("Ada");
        let mut msg = GroupChatMessage::new(message).with_show_header(false);
        let size = msg.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    /// R4: a tool-result terminal segment draws through the shared
    /// `terminal_block` renderer instead of disappearing as `Empty`. The same
    /// data through the shared renderer paints the same block.
    #[test]
    fn terminal_fragment_draws_the_shared_block() {
        let app = AppContext::default();
        let data = TerminalData::new(
            "cargo test",
            vec![
                TerminalLine::command("cargo test"),
                TerminalLine::output("test result: ok. 42 passed"),
            ],
        )
        .with_status(TerminalStatus::Success);

        let message = ChatMessage::new(ChatRole::Tool, vec![ChatFragment::terminal(data.clone())]);
        let mut msg = GroupChatMessage::new(message).with_show_header(false);
        msg.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut paint_ctx = PaintContext::new(Renderer::new());
        msg.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx.renderer.take().unwrap().commands().to_vec();
        let runs = text_runs(&commands);

        let mut shared = terminal_block(&data, TerminalFilter::default(), None, None);
        let shared_commands =
            crate::test_util::render_element(&mut shared, vec2f(400.0, 400.0), &app);

        assert_eq!(
            runs,
            text_runs(&shared_commands),
            "the terminal fragment must draw the shared terminal block"
        );
        assert!(
            runs.iter().any(|(text, _)| text == "cargo test"),
            "the block's command line should be drawn, got {runs:?}"
        );
        assert!(
            runs.iter()
                .any(|(text, _)| text == "test result: ok. 42 passed"),
            "the block's output should be drawn, got {runs:?}"
        );
    }

    /// H5: a fenced block is a band, not a rounded box; the label survives.
    #[test]
    fn code_block_draws_a_band_not_a_rounded_box() {
        let app = AppContext::default();
        let message = ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::code_block(
                Some("rust".to_string()),
                "let x = 1;",
            )],
        );
        let mut msg = GroupChatMessage::new(message).with_show_header(false);
        msg.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut paint_ctx = PaintContext::new(Renderer::new());
        msg.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx.renderer.take().unwrap().commands().to_vec();

        let band = app.theme.color(ColorToken::SurfaceRaised);
        assert!(
            !commands.iter().any(|c| matches!(
                c,
                RenderCommand::FillRect { color, corner_radius, .. }
                    if *color == band && *corner_radius > 0.0
            )),
            "the fence must not be a rounded box, got {commands:?}"
        );
        assert!(
            commands.iter().any(|c| matches!(
                c,
                RenderCommand::FillRect { color, corner_radius, .. }
                    if *color == band && *corner_radius == 0.0
            )),
            "the fence must sit on a flat band, got {commands:?}"
        );
        let runs = text_runs(&commands);
        assert!(
            runs.iter().any(|(text, _)| text == "rust"),
            "the fence's language label must survive, got {runs:?}"
        );
    }

    /// H5: a blockquote is an indent plus a rail, not a rounded box.
    #[test]
    fn block_quote_draws_an_indent_and_a_rail() {
        let app = AppContext::default();
        let message = ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::block_quote(vec![ChatFragment::text(
                "quoted line",
            )])],
        );
        let mut msg = GroupChatMessage::new(message).with_show_header(false);
        msg.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut paint_ctx = PaintContext::new(Renderer::new());
        msg.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx.renderer.take().unwrap().commands().to_vec();

        let box_bg = app.theme.color(ColorToken::SurfaceRaised);
        assert!(
            !commands.iter().any(|c| matches!(
                c,
                RenderCommand::FillRect { color, .. } if *color == box_bg
            )),
            "a quote must not paint a raised box, got {commands:?}"
        );
        let rail = commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if rect.width() == crate::elements::chat_message_bubble::QUOTE_RAIL_WIDTH => {
                    Some((*color, *corner_radius))
                }
                _ => None,
            })
            .expect("the quote must paint a rail");
        assert_eq!(rail.1, 0.0, "the rail is square");
        assert_eq!(
            rail.0,
            app.theme.color(ColorToken::Muted),
            "the rail is muted"
        );
        let runs = text_runs(&commands);
        assert!(
            runs.iter().any(|(text, _)| text == "quoted line"),
            "the quoted text must be drawn, got {runs:?}"
        );
    }
}
