use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::chat_content::{
    group_fragments_into_blocks, ChatAction, ChatBlock, ChatFragment, ChatRole, InlineSpan as
    ModelSpan, InlineStyle, ToolCall,
};
use crate::elements::{
    resolve_inline_span, Border, Chip, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets,
    Element, Fill, Flex, Icon, InlineText, LayoutContext, MainAxisAlignment, PaintContext, Point,
    SizeConstraint, TerminalBlock, Text, TextSpan,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

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

/// A raised `surface_2` card per tool invocation, matching warp-new's tool /
/// command block: full-width, 1px `surface_2` border, radius 8, leading status
/// icon and mono body. Stacked one per call.
fn build_tool_call_cards(
    tool_calls: &[ToolCall],
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(8.0);

    for call in tool_calls {
        let label = if call.arguments.is_empty() || call.arguments == "{}" {
            call.name.clone()
        } else {
            format!("{} {}", call.name, call.arguments)
        };
        column = column.with_child(
            Container::new(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.0)
                    .with_child(
                        Icon::new("cpu")
                            .with_size(14.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_child(
                        Flex::column()
                            .with_cross_axis_alignment(CrossAxisAlignment::Start)
                            .with_spacing(2.0)
                            .with_child(
                                Text::new("tool")
                                    .with_theme_color(ColorToken::Muted, app)
                                    .with_font_size(10.0)
                                    .finish(),
                            )
                            .with_child(
                                Text::new(label)
                                    .with_theme_color(ColorToken::Text, app)
                                    .with_font_size(12.0)
                                    .finish(),
                            )
                            .finish(),
                    )
                    .finish(),
            )
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_border(
                Border::all(1.0).with_border_fill(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised))),
            )
            .with_padding(EdgeInsets::new(16.0, 12.0, 16.0, 12.0))
            .with_corner_radius(8.0)
            .finish(),
        );
    }
    column.finish()
}

pub struct ChatMessageBubble {
    role: ChatRole,
    fragments: Vec<ChatFragment>,
    tool_calls: Vec<ToolCall>,
    on_action: Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
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

    pub fn role(&self) -> ChatRole {
        self.role
    }

    pub fn fragments(&self) -> &[ChatFragment] {
        &self.fragments
    }

    fn rebuild(&mut self, app: &crate::elements::AppContext, max_width: f32) {
        let padding = app.theme.spacing_px(SpacingToken::Md);
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let radius = app.theme.radius_px();

        // Warp-new renders a message block on `surface_1`, and a tool/command
        // block is a raised `surface_2` card inside it (1px `surface_2` border,
        // radius 8) — not a chip row and not floating on the transcript bg. So
        // the assistant/tool rows use the same `Surface` as the block, and the
        // terminal-style command area is what carries the `SurfaceRaised`
        // (`surface_2`) styling and border.
        let bg = match self.role {
            ChatRole::User => app.theme.color(ColorToken::SurfaceRaised),
            ChatRole::Assistant => app.theme.color(ColorToken::Surface),
            ChatRole::Tool => app.theme.color(ColorToken::Surface),
        };

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        // Tool invocations attached to this (assistant) message render as a
        // raised card above the prose so the read is "the agent called these
        // tools, then produced this reply".
        if !self.tool_calls.is_empty() {
            column = column.with_child(build_tool_call_cards(&self.tool_calls, app));
        }

        for block in group_fragments_into_blocks(&self.fragments) {
            match block {
                ChatBlock::Paragraph(spans) => {
                    let text_spans = spans.iter().map(|s| to_text_span(s, app)).collect();
                    column = column.with_child(
                        InlineText::new(text_spans)
                            .with_font_size(12.0)
                            .finish(),
                    );
                }
                ChatBlock::Heading { level, text } => {
                    let font_size = match level {
                        1 => 20.0,
                        2 => 18.0,
                        3 => 16.0,
                        _ => 14.0,
                    };
                    column = column.with_child(
                        Text::new(text.clone())
                            .with_theme_color(ColorToken::Text, app)
                            .with_font_size(font_size)
                            .finish(),
                    );
                }
                ChatBlock::CodeBlock { lang, code } => {
                    let mut code_column = Flex::column()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .with_spacing(2.0);
                    if let Some(lang) = lang {
                        code_column = code_column.with_child(
                            Text::new(lang.clone())
                                .with_theme_color(ColorToken::Muted, app)
                                .with_font_size(10.0)
                                .finish(),
                        );
                    }
                    code_column = code_column.with_child(
                        Text::new(code.clone())
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    );
                    let code_container = Container::new(code_column.finish())
                        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                        .with_padding(EdgeInsets::uniform(padding))
                        .with_corner_radius(radius)
                        .finish();
                    column = column.with_child(code_container);
                }
                ChatBlock::List { items, ordered } => {
                    let mut list_column = Flex::column()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .with_spacing(spacing / 2.0);
                    for (i, item) in items.iter().enumerate() {
                        let prefix = if ordered {
                            format!("{}. ", i + 1)
                        } else {
                            "• ".to_string()
                        };
                        let row = Flex::row().with_child(
                            Text::new(format!("{}{}", prefix, item))
                                .with_theme_color(ColorToken::Text, app)
                                .finish(),
                        );
                        list_column = list_column.with_child(row.finish());
                    }
                    column = column.with_child(list_column.finish());
                }
                ChatBlock::BlockQuote(text) => {
                    let quote = Container::new(
                        Text::new(text.clone())
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                    .with_padding(EdgeInsets::new(
                        padding / 2.0,
                        padding,
                        padding / 2.0,
                        padding,
                    ))
                    .with_corner_radius(radius)
                    .finish();
                    column = column.with_child(quote);
                }
                ChatBlock::Action { label, payload } => {
                    let on_action = self.on_action.clone();
                    let payload = payload.clone();
                    let chip = Chip::new(
                        Text::new(label.clone())
                            .with_theme_color(ColorToken::Accent, app)
                            .finish(),
                    )
                    .with_on_click(move || {
                        if let Some(cb) = on_action.as_ref() {
                            (cb.borrow_mut())(payload.clone());
                        }
                    })
                    .finish();
                    column = column.with_child(chip);
                }
                ChatBlock::Terminal(data) => {
                    let block = TerminalBlock::new()
                        .with_title(data.title.clone())
                        .with_status(data.status.unwrap_or_default())
                        .with_lines(data.lines.clone())
                        .finish();
                    column = column.with_child(block);
                }
            }
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
    use crate::elements::{AppContext, LayoutContext, PaintContext, TerminalData, TerminalLine};
    use crate::geometry::vec2f;
    use crate::render::{RenderCommand, Renderer};

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

    #[test]
    fn tool_bubble_renders_card_and_result_block() {
        let app = AppContext::default();
        let mut bubble = ChatMessageBubble::new(
            ChatRole::Tool,
            vec![ChatFragment::terminal(TerminalData::new(
                "call_1",
                vec![TerminalLine::output("file.txt")],
            ))],
        )
        .with_tool_calls(vec![ToolCall {
            name: "ls".to_string(),
            arguments: "{}".to_string(),
        }]);
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
        // The invocation card draws a "cpu" icon (atlas name "prompt"); the
        // tool-result block draws a "terminal" header icon.
        let tool_icons = commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::DrawIcon { name, .. } if name == "prompt"))
            .count();
        let terminal_icons = commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::DrawIcon { name, .. } if name == "terminal"))
            .count();
        assert!(tool_icons >= 1, "expected a tool invocation card icon");
        assert!(terminal_icons >= 1, "expected a tool-result block icon");
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
}
