use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::{ChatMessage as ServiceChatMessage, DesktopState};
use goble_ui::elements::{
    AppContext, ChatHeader, ChatLayout, ChatMessage as UiChatMessage, ChatRole, ChatSidebar,
    Container, Element, EventContext, Fill, LayoutContext, PaintContext, Point, SizeConstraint,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::ChatView;

use crate::app::UiState;
use crate::state_api::{add_chat_message, run_harness, RunHarnessRequest};

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
        let mut messages: Vec<UiChatMessage> = chat_id_for_messages
            .as_ref()
            .and_then(|id| state.list_chat_messages(id).ok())
            .map(|msgs| msgs.iter().map(map_chat_message).collect())
            .unwrap_or_default();

        if let Some(ref id) = chat_id {
            if let Some(streaming) = ui_state.borrow().chat_streaming.get(id) {
                if !streaming.is_empty() {
                    messages.push(UiChatMessage::from_markdown(
                        goble_ui::elements::ChatRole::Assistant,
                        streaming.clone(),
                    ));
                }
            }
        }

        let state_for_send = Arc::clone(&state);
        let chat_id_for_send = chat_id.clone();
        let dirty_for_send = Rc::clone(&dirty);

        let chat_title = chat_id
            .as_ref()
            .and_then(|id| state.list_chats().iter().find(|c| c.id == *id).map(|c| c.title.clone()))
            .unwrap_or_else(|| "New chat".to_string());

        let right_sidebar_visible = Rc::new(RefCell::new(false));
        let header_sidebar_visible = Rc::clone(&right_sidebar_visible);
        let header_dirty = Rc::clone(&dirty);
        let header = ChatHeader::new(chat_title, app)
            .with_sidebar_toggle(
                *right_sidebar_visible.borrow(),
                move || {
                    let mut visible = header_sidebar_visible.borrow_mut();
                    *visible = !*visible;
                    *header_dirty.borrow_mut() = true;
                },
            )
            .finish();

        let chat = ChatView::new()
            .with_header(header)
            .with_messages(messages)
            .with_empty_state("New conversation", "Type a message below to get started.")
            .with_on_send(move |text| {
                let id = match chat_id_for_send {
                    Some(ref id) => id.clone(),
                    None => {
                        log::warn!("no chat selected; cannot send message");
                        return;
                    }
                };
                if let Err(e) = add_chat_message(&state_for_send, &id, "user", &text) {
                    log::error!("failed to add chat message: {}", e);
                }
                let chat = state_for_send.list_chats().into_iter().find(|c| c.id == id);
                let provider = chat.as_ref().and_then(|c| c.provider.clone()).unwrap_or_else(|| "openai".to_string());
                let model = chat.as_ref().and_then(|c| c.model.clone()).unwrap_or_else(|| "gpt-4o".to_string());
                if let Err(e) = run_harness(
                    &state_for_send,
                    RunHarnessRequest {
                        chat_id: id,
                        prompt: text,
                        provider,
                        model,
                    },
                ) {
                    log::error!("failed to run harness: {}", e);
                }
                *dirty_for_send.borrow_mut() = true;
            });

        let mut layout = ChatLayout::new(chat.finish());
        if *right_sidebar_visible.borrow() {
            layout = layout.with_right_sidebar(ChatSidebar::new(app).finish());
        }

        let content = Container::new(layout.finish())
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
