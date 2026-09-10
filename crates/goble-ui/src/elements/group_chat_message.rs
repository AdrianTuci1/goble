use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::chat_content::{
    ChatAction, ChatFragment, ChatFragmentKind, ChatMessage, ChatRole,
};
use crate::elements::{
    AppContext, Avatar, Chip, Container, CrossAxisAlignment, EdgeInsets, Element, Empty, Fill,
    Flex, LayoutContext, PaintContext, Point, SizeConstraint, Spacer, Text,
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
                        radius,
                    ));
                }
                ChatFragmentKind::Heading { level, text } => {
                    column = flush_inline(&mut inline_buffer, column);
                    column = column.with_child(self.render_heading(app, *level, text.clone()));
                }
                ChatFragmentKind::List { items, ordered } => {
                    column = flush_inline(&mut inline_buffer, column);
                    column = column.with_child(self.render_list(app, items.clone(), *ordered));
                }
                ChatFragmentKind::BlockQuote(text) => {
                    column = flush_inline(&mut inline_buffer, column);
                    column = column.with_child(self.render_block_quote(
                        app,
                        text.clone(),
                        spacing,
                        radius,
                    ));
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

    fn render_code_block(
        &self,
        app: &AppContext,
        lang: Option<String>,
        code: String,
        padding: f32,
        radius: f32,
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
        Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_padding(EdgeInsets::uniform(padding / 2.0))
            .with_corner_radius(radius)
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

    fn render_list(&self, app: &AppContext, items: Vec<String>, ordered: bool) -> Box<dyn Element> {
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0);
        for (i, item) in items.iter().enumerate() {
            let prefix = if ordered {
                format!("{}. ", i + 1)
            } else {
                "• ".to_string()
            };
            column = column.with_child(
                Text::new(format!("{}{}", prefix, item))
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            );
        }
        column.finish()
    }

    fn render_block_quote(
        &self,
        app: &AppContext,
        text: String,
        padding: f32,
        radius: f32,
    ) -> Box<dyn Element> {
        Container::new(
            Text::new(text)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_padding(EdgeInsets::new(
            padding / 4.0,
            padding / 2.0,
            padding / 4.0,
            padding / 2.0,
        ))
        .with_corner_radius(radius)
        .finish()
    }
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
    use crate::elements::{AppContext, LayoutContext};
    use crate::geometry::vec2f;

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
}
