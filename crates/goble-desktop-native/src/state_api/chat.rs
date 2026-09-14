use std::sync::Arc;

use goble_desktop_service::{Chat, ChatMessage, DesktopState};

pub fn create_chat(
    state: &Arc<DesktopState>,
    title: &str,
    provider: Option<&str>,
    model: Option<&str>,
) -> anyhow::Result<String> {
    state.create_chat(title, provider, model)
}

pub fn list_chats(state: &Arc<DesktopState>) -> Vec<Chat> {
    state.list_chats()
}

pub fn chat_messages(state: &Arc<DesktopState>, chat_id: &str) -> anyhow::Result<Vec<ChatMessage>> {
    state.list_chat_messages(chat_id)
}

pub fn add_chat_message(
    state: &Arc<DesktopState>,
    chat_id: &str,
    role: &str,
    content: &str,
) -> anyhow::Result<()> {
    state.add_chat_message(chat_id, role, content)
}

pub struct SetChatModelRequest {
    pub chat_id: String,
    pub provider: String,
    pub model: String,
}

pub fn set_chat_model(state: &Arc<DesktopState>, req: SetChatModelRequest) -> anyhow::Result<()> {
    state.set_chat_model(&req.chat_id, &req.provider, &req.model)
}
