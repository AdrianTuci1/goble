use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::chat_content::{ChatAction, ChatMessage};
use crate::elements::{
    AppContext, CrossAxisAlignment, Element, Flex, GroupChatMessage, LayoutContext, PaintContext,
    Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;

/// Renders a group of consecutive messages from the same author.
///
/// Only the first message shows the avatar, author name, and timestamp.
/// Subsequent messages are rendered compactly, indented to align with the
/// first message's content column.
pub struct GroupChatMessageGroup {
    messages: Vec<ChatMessage>,
    on_action: Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl GroupChatMessageGroup {
    pub fn new(messages: Vec<ChatMessage>) -> Self {
        Self {
            messages,
            on_action: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_on_action<F: FnMut(ChatAction) + 'static>(mut self, callback: F) -> Self {
        self.on_action = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn rebuild(&mut self, _app: &AppContext) {
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0);

        for (index, message) in self.messages.iter().enumerate() {
            let on_action = self.on_action.clone();
            let show_header = index == 0;
            let item = GroupChatMessage::new(message.clone())
                .with_show_header(show_header)
                .with_on_action(move |action| {
                    if let Some(cb) = on_action.as_ref() {
                        (cb.borrow_mut())(action);
                    }
                })
                .finish();
            column = column.with_child(item);
        }

        self.root = Some(column.finish());
    }
}

impl Element for GroupChatMessageGroup {
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
    fn group_layouts_multiple_messages() {
        let app = AppContext::default();
        let messages = vec![
            ChatMessage::new(ChatRole::User, vec![ChatFragment::text("First")])
                .with_author_name("Ada"),
            ChatMessage::new(ChatRole::User, vec![ChatFragment::text("Second")])
                .with_author_name("Ada"),
        ];
        let mut group = GroupChatMessageGroup::new(messages);
        let size = group.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }
}
