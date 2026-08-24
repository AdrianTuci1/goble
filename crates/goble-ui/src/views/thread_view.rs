use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::chat_content::{ChatAction, ChatMessage, ChatRole};
use crate::elements::{
    AppContext, Axis, ChatComposer, Container, CrossAxisAlignment, EdgeInsets, Element, Fill, Flex,
    GroupChatMessageGroup, LayoutContext, MainAxisAlignment, PaintContext, Point, Scrollable,
    SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

pub struct ThreadView {
    title: String,
    messages: Vec<ChatMessage>,
    on_send: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_action: Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    composer_value: Rc<RefCell<String>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ThreadView {
    pub fn new(title: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        Self {
            title: title.into(),
            messages,
            on_send: None,
            on_action: None,
            composer_value: Rc::new(RefCell::new(String::new())),
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_on_send<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_send = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_action<F: FnMut(ChatAction) + 'static>(mut self, callback: F) -> Self {
        self.on_action = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn rebuild(&mut self, app: &AppContext) {
        let padding = app.theme.spacing_px(SpacingToken::Md);
        let spacing = app.theme.spacing_px(SpacingToken::Md);

        let header = Container::new(
            Flex::row()
                .with_main_axis_alignment(MainAxisAlignment::Start)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    crate::elements::Text::new(self.title.clone())
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .finish(),
        )
        .with_padding(EdgeInsets::uniform(padding))
        .finish();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);
        column = column.with_child(header);

        let message_column = if self.messages.is_empty() {
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_child(
                    Text::new("No messages yet.")
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
        } else {
            let mut message_column = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(spacing);
            for group in group_messages_by_author(&self.messages) {
                let on_action = self.on_action.clone();
                let group_view = GroupChatMessageGroup::new(group)
                    .with_on_action(move |action| {
                        if let Some(cb) = on_action.as_ref() {
                            (cb.borrow_mut())(action);
                        }
                    })
                    .finish();
                message_column = message_column.with_child(group_view);
            }
            message_column
        };
        column =
            column.with_child(Scrollable::new(message_column.finish(), Axis::Vertical).finish());

        let current_value = self.composer_value.borrow().clone();
        let composer_value = self.composer_value.clone();
        let on_send = self.on_send.clone();
        let composer = ChatComposer::new()
            .with_value(current_value)
            .with_on_send(move |text| {
                *composer_value.borrow_mut() = String::new();
                if let Some(cb) = on_send.as_ref() {
                    (cb.borrow_mut())(text);
                }
            })
            .finish();
        column = column.with_child(composer);

        self.root = Some(
            Container::new(column.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
                .with_padding(EdgeInsets::uniform(padding))
                .finish(),
        );
    }
}

fn author_key(message: &ChatMessage) -> String {
    message
        .author_name
        .clone()
        .unwrap_or_else(|| role_label(message.role).to_string())
}

fn group_messages_by_author(messages: &[ChatMessage]) -> Vec<Vec<ChatMessage>> {
    if messages.is_empty() {
        return Vec::new();
    }

    let mut groups: Vec<Vec<ChatMessage>> = Vec::new();
    let mut current_key: String = author_key(&messages[0]);
    let mut current_group: Vec<ChatMessage> = vec![messages[0].clone()];

    for message in &messages[1..] {
        let key = author_key(message);
        if key == current_key {
            current_group.push(message.clone());
        } else {
            groups.push(current_group);
            current_key = key;
            current_group = vec![message.clone()];
        }
    }
    groups.push(current_group);
    groups
}

fn role_label(role: ChatRole) -> &'static str {
    match role {
        ChatRole::User => "You",
        ChatRole::Assistant => "Assistant",
    }
}

impl Element for ThreadView {
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
    use crate::elements::chat_content::{ChatFragment, ChatRole};
    use crate::elements::{AppContext, LayoutContext};
    use crate::geometry::vec2f;

    #[test]
    fn thread_view_layouts_with_messages() {
        let app = AppContext::default();
        let messages = vec![
            ChatMessage::new(
                ChatRole::Assistant,
                vec![ChatFragment::text("Thread message")],
            )
            .with_author_name("Ada"),
            ChatMessage::new(ChatRole::Assistant, vec![ChatFragment::text("Another")])
                .with_author_name("Ada"),
        ];
        let mut view = ThreadView::new("General", messages);
        let size = view.layout(
            SizeConstraint::loose(vec2f(600.0, 800.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn thread_view_groups_messages_by_author() {
        let messages = vec![
            ChatMessage::new(ChatRole::User, vec![ChatFragment::text("Hello")])
                .with_author_name("Ada"),
            ChatMessage::new(ChatRole::User, vec![ChatFragment::text("Hi")])
                .with_author_name("Ada"),
            ChatMessage::new(ChatRole::Assistant, vec![ChatFragment::text("Hey")])
                .with_author_name("Bot"),
        ];
        let groups = group_messages_by_author(&messages);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len(), 2);
        assert_eq!(groups[1].len(), 1);
    }
}
