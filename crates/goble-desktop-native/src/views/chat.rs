use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::{ChatMessage as ServiceChatMessage, DesktopState};
use goble_ui::elements::{
    AppContext, Button, ButtonVariant, ChatHeader, ChatLayout, ChatMessage as UiChatMessage,
    ChatRole, ChatSidebar, Container, CrossAxisAlignment, EdgeInsets, Element, EventContext, Fill,
    Flex, LayoutContext, PaintContext, Point, SizeConstraint, Text, TextInput,
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

const DEFAULT_PROVIDER: &str = "openai";

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

        let chat_id = chat_id.or_else(|| state.list_chats().first().map(|c| c.id.clone()));

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
            },
        };

        let chat_id_for_messages = chat_id.clone();
        let messages: Vec<UiChatMessage> = chat_id_for_messages
            .as_ref()
            .and_then(|id| state.list_chat_messages(id).ok())
            .map(|msgs| msgs.iter().map(map_chat_message).collect())
            .unwrap_or_default();

        let chat_opt = chat_id
            .as_ref()
            .and_then(|id| state.list_chats().into_iter().find(|c| &c.id == id));

        let provider = chat_opt
            .as_ref()
            .and_then(|c| c.provider.clone())
            .unwrap_or_else(|| DEFAULT_PROVIDER.to_string());
        let model = chat_opt
            .as_ref()
            .and_then(|c| c.model.clone())
            .unwrap_or_default();
        let api_key_present = state
            .get_llm_setting(&provider)
            .map(|s| !s.api_key.is_empty())
            .unwrap_or(false);
        let needs_api_key = chat_opt.is_some() && (model.is_empty() || !api_key_present);

        let state_for_send = Arc::clone(&state);
        let chat_id_for_send = chat_id.clone();
        let dirty_for_send = Rc::clone(&dirty);
        let chat_view = ChatView::new()
            .with_messages(messages)
            .with_model_options(
                vec![
                    "gpt-4o".to_string(),
                    "gpt-4o-mini".to_string(),
                    "claude-3-5-sonnet".to_string(),
                    "deepseek-chat".to_string(),
                ],
                if model.is_empty() {
                    None
                } else {
                    Some(model.clone())
                },
            )
            .with_runtime_options(
                vec![
                    "auto".to_string(),
                    "local".to_string(),
                    "tag".to_string(),
                    "worker".to_string(),
                ],
                Some("auto".to_string()),
            )
            .with_variant_options(
                vec![
                    "creative".to_string(),
                    "balanced".to_string(),
                    "precise".to_string(),
                ],
                Some("balanced".to_string()),
            )
            .with_on_model_change({
                let state = Arc::clone(&state);
                let dirty = Rc::clone(&dirty);
                let chat_id = chat_id.clone();
                let provider = provider.clone();
                move |model| {
                    if let Some(ref id) = chat_id {
                        if let Err(e) = state.set_chat_model(id, &provider, &model) {
                            log::error!("failed to set chat model: {}", e);
                        }
                        *dirty.borrow_mut() = true;
                    }
                }
            })
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
            })
            .finish();

        let chat_title = chat_opt
            .as_ref()
            .map(|c| c.title.clone())
            .unwrap_or_else(|| "New chat".to_string());
        let chat_sidebar_visible = ui_state.borrow().chat_sidebar_visible;
        let ui_state_for_toggle = Rc::clone(&ui_state);
        let dirty_for_toggle = Rc::clone(&dirty);
        let header = ChatHeader::new(chat_title, app)
            .with_sidebar_toggle(chat_sidebar_visible, move || {
                let visible = ui_state_for_toggle.borrow().chat_sidebar_visible;
                ui_state_for_toggle.borrow_mut().chat_sidebar_visible = !visible;
                *dirty_for_toggle.borrow_mut() = true;
            })
            .finish();

        let mut main_col = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(header);

        if chat_id.is_none() {
            let notice = Container::new(
                Flex::column()
                    .with_child(
                        Text::new("No chat available")
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .finish(),
            )
            .finish();
            main_col = main_col.with_child(notice);
        } else if needs_api_key {
            let api_provider = Rc::new(RefCell::new(provider.clone()));
            let api_model = Rc::new(RefCell::new(model.clone()));
            let api_key = Rc::new(RefCell::new(String::new()));
            let dirty_for_card = Rc::clone(&dirty);
            let state_for_card = Arc::clone(&state);
            let chat_id_for_card = chat_id.clone().unwrap();

            let make_row = |label: &str, value: Rc<RefCell<String>>| {
                let text = Text::new(label)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish();
                let initial = value.borrow().clone();
                let value_for_change = Rc::clone(&value);
                let input = TextInput::new()
                    .with_placeholder(label)
                    .with_value(initial)
                    .with_on_change(move |text| {
                        *value_for_change.borrow_mut() = text;
                    })
                    .finish();
                Flex::row()
                    .with_cross_axis_alignment(goble_ui::elements::CrossAxisAlignment::Center)
                    .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
                    .with_child(text)
                    .with_child(input)
                    .finish()
            };

            let provider_input = make_row("Provider", Rc::clone(&api_provider));
            let model_input = make_row("Model", Rc::clone(&api_model));
            let key_input = make_row("API key", Rc::clone(&api_key));

            let save = move || {
                let provider = api_provider.borrow().clone();
                let model = api_model.borrow().clone();
                let key = api_key.borrow().clone();
                if provider.is_empty() || model.is_empty() || key.is_empty() {
                    log::warn!("provider, model and API key are required");
                    return;
                }
                if let Err(e) = state_for_card.set_llm_setting(&provider, &key, None, &model, None)
                {
                    log::error!("failed to save LLM setting: {}", e);
                    return;
                }
                if let Err(e) = state_for_card.set_chat_model(&chat_id_for_card, &provider, &model)
                {
                    log::error!("failed to update chat model: {}", e);
                    return;
                }
                *dirty_for_card.borrow_mut() = true;
            };

            let card = Container::new(
                Flex::column()
                    .with_cross_axis_alignment(goble_ui::elements::CrossAxisAlignment::Stretch)
                    .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
                    .with_child(
                        Text::new("Configure API key to start chatting")
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_child(provider_input)
                    .with_child(model_input)
                    .with_child(key_input)
                    .with_child(
                        Button::new(
                            Text::new("Save")
                                .with_theme_color(ColorToken::Text, app)
                                .finish(),
                        )
                        .with_variant(ButtonVariant::Primary)
                        .with_on_click(save)
                        .finish(),
                    )
                    .finish(),
            )
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_padding(EdgeInsets::uniform(padding))
            .finish();
            main_col = main_col.with_child(card);
        }

        main_col = main_col.with_child(chat_view);

        let main = Container::new(main_col.finish())
            .with_background(Fill::Solid(bg))
            .with_padding(goble_ui::elements::EdgeInsets::uniform(padding))
            .finish();

        let mut info_col = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(app.theme.spacing_px(SpacingToken::Sm));
        match &chat_opt {
            Some(chat) => {
                info_col = info_col.with_child(
                    Text::new(format!("Title: {}", chat.title))
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                );
                let provider_text = chat
                    .provider
                    .clone()
                    .unwrap_or_else(|| "default".to_string());
                info_col = info_col.with_child(
                    Text::new(format!("Provider: {}", provider_text))
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                );
                let model_text = chat.model.clone().unwrap_or_else(|| "not set".to_string());
                info_col = info_col.with_child(
                    Text::new(format!("Model: {}", model_text))
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                );
            }
            None => {
                info_col = info_col.with_child(
                    Text::new("No chat selected")
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                );
            }
        }
        let info_content = Container::new(info_col.finish()).finish();

        let sidebar_tab = ui_state.borrow().chat_sidebar_tab;
        let ui_state_for_tab = Rc::clone(&ui_state);
        let dirty_for_tab = Rc::clone(&dirty);
        let right_sidebar = ChatSidebar::new(sidebar_tab)
            .with_info_content(info_content)
            .with_history_items(Vec::new())
            .with_on_change_tab(move |tab| {
                ui_state_for_tab.borrow_mut().chat_sidebar_tab = tab;
                *dirty_for_tab.borrow_mut() = true;
            })
            .finish();

        let content = if chat_sidebar_visible {
            ChatLayout::new(main)
                .with_right_sidebar(right_sidebar)
                .finish()
        } else {
            ChatLayout::new(main).finish()
        };

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
