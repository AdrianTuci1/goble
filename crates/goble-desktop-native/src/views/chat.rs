use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::{ChatMessage as ServiceChatMessage, DesktopState};
use goble_ui::elements::{
    AppContext, ChatMessage as UiChatMessage, ChatRole, Container, Element, EventContext, Fill,
    Flex, LayoutContext, PaintContext, Point, SizeConstraint, Text,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::ChatView;

use crate::app::UiState;

fn map_chat_message(msg: &ServiceChatMessage) -> UiChatMessage {
    let role = match msg.role.as_str() {
        "user" => ChatRole::User,
        _ => ChatRole::Assistant,
    };
    UiChatMessage::from_markdown(role, msg.content.clone()).with_timestamp(msg.created_at.clone())
}

pub struct ChatViewPanel {
    content: Box<dyn Element>,
}

impl ChatViewPanel {
    pub fn new(
        state: Arc<DesktopState>,
        ui_state: Rc<RefCell<UiState>>,
        dirty: Rc<RefCell<bool>>,
        app: &AppContext,
    ) -> Self {
        let chat_id = ui_state.borrow().selected_chat_id.clone();
        let bg = app.theme.color(ColorToken::Bg);
        let padding = app.theme.spacing_px(SpacingToken::Md);

        let chat_id = chat_id.or_else(|| {
            state.list_chats().first().map(|c| c.id.clone())
        });

        let chat_id = match chat_id {
            Some(id) => Some(id),
            None => match state.create_chat("New chat", None, None) {
                Ok(id) => {
                    ui_state.borrow_mut().selected_chat_id = Some(id.clone());
                    Some(id)
                }
                Err(e) => {
                    log::error!("failed to auto-create chat: {}", e);
                    None
                }
            }
        };

        let chat_id_for_messages = chat_id.clone();
        let messages: Vec<UiChatMessage> = chat_id_for_messages
            .as_ref()
            .and_then(|id| state.list_chat_messages(id).ok())
            .map(|msgs| msgs.iter().map(map_chat_message).collect())
            .unwrap_or_default();

        let state_for_send = Arc::clone(&state);
        let chat_id_for_send = chat_id.clone();
        let dirty_for_send = Rc::clone(&dirty);
        let mut chat = ChatView::new()
            .with_messages(messages)
            .with_on_send(move |text| {
                let id = match chat_id_for_send {
                    Some(ref id) => id.clone(),
                    None => {
                        log::warn!("no chat selected; cannot send message");
                        return;
                    }
                };
                if let Err(e) = state_for_send.add_chat_message(&id, "user", &text) {
                    log::error!("failed to add chat message: {}", e);
                }
                *dirty_for_send.borrow_mut() = true;
            });

        if chat_id.is_none() {
            chat = chat.with_header(
                Container::new(
                    Flex::column()
                        .with_child(
                            Text::new("No chat available")
                                .with_theme_color(ColorToken::Muted, app)
                                .finish(),
                        )
                        .finish(),
                )
                .finish(),
            );
        }

        let content = Container::new(chat.finish())
            .with_background(Fill::Solid(bg))
            .with_padding(goble_ui::elements::EdgeInsets::uniform(padding))
            .finish();

        Self { content }
    }
}

impl Element for ChatViewPanel {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.content.layout(constraint, ctx, app)
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.content.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.content.size()
    }

    fn origin(&self) -> Option<Point> {
        self.content.origin()
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.content.dispatch_event(event, ctx, app)
    }
}
