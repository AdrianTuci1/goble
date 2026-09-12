use chrono::Utc;
use goble_core::store::Store;

use super::{Chat, ChatMessage, DesktopState};

impl DesktopState {
    pub fn add_chat_log(&self, message: impl Into<String>) {
        self.add_log(message);
    }

    pub fn add_chat_message(&self, chat_id: &str, role: &str, content: &str) -> anyhow::Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();
        let tool_calls = if role == "tool" {
            serde_json::from_str::<Vec<serde_json::Value>>(content)
                .ok()
                .and_then(|v| serde_json::to_string(&v).ok())
        } else {
            None
        };
        self.store.lock().insert_chat_message(
            &id,
            chat_id,
            role,
            content,
            tool_calls.as_deref(),
            &created_at,
        )?;
        self.messages
            .lock()
            .entry(chat_id.to_string())
            .or_default()
            .push(ChatMessage {
                id,
                role: role.to_string(),
                content: content.to_string(),
                tool_calls: tool_calls.clone(),
                created_at,
            });
        self.emit("chat:updated", serde_json::json!({ "chat_id": chat_id }));
        Ok(())
    }

    pub fn list_chat_messages(&self, chat_id: &str) -> anyhow::Result<Vec<ChatMessage>> {
        let rows = self.store.lock().list_chat_messages(chat_id)?;
        Ok(rows
            .into_iter()
            .map(|(id, role, content, tool_calls, created_at)| ChatMessage {
                id,
                role,
                content,
                tool_calls,
                created_at,
            })
            .collect())
    }

    /// Return the single still-pending ask for a chat, if any. The harness
    /// persists questions it asked so the inline ask card survives a refresh or
    /// an app restart; answering clears it (status becomes `answered`).
    pub fn get_pending_ask(&self, chat_id: &str) -> anyhow::Result<Option<serde_json::Value>> {
        match self.store.lock().get_pending_ask(chat_id)? {
            Some((id, _chat_id, _mission_id, question, quick_replies, _status)) => {
                let quick: Vec<String> = quick_replies
                    .split('\n')
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                Ok(Some(serde_json::json!({
                    "id": id,
                    "question": question,
                    "quick_replies": quick,
                })))
            }
            None => Ok(None),
        }
    }

    pub fn store_clone(&self) -> Store {
        self.store.lock().clone()
    }

    pub fn create_chat(
        &self,
        title: &str,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> anyhow::Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.store
            .lock()
            .insert_chat(&id, title, provider, model, &now, &now)?;
        let chat = Chat {
            id: id.clone(),
            title: title.to_string(),
            provider: provider.map(|s| s.to_string()),
            model: model.map(|s| s.to_string()),
            agent_id: None,
            worker_id: None,
            workspace_routing: None,
            updated_at: now,
        };
        self.chats.lock().push(chat);
        self.emit("chats:updated", ());
        Ok(id)
    }

    pub fn set_chat_model(&self, id: &str, provider: &str, model: &str) -> anyhow::Result<()> {
        self.store.lock().set_chat_model(id, provider, model)?;
        if let Some(chat) = self.chats.lock().iter_mut().find(|c| c.id == id) {
            chat.provider = Some(provider.to_string());
            chat.model = Some(model.to_string());
        }
        self.emit("chats:updated", ());
        Ok(())
    }

    /// Persist where a conversation's agent should run (`"local"` / `"remote"`).
    pub fn set_chat_workspace_routing(
        &self,
        id: &str,
        routing: Option<&str>,
    ) -> anyhow::Result<()> {
        self.store.lock().set_chat_workspace_routing(id, routing)?;
        if let Some(chat) = self.chats.lock().iter_mut().find(|c| c.id == id) {
            chat.workspace_routing = routing.map(|s| s.to_string());
        }
        self.emit("chats:updated", ());
        Ok(())
    }

    /// Read where a conversation's agent should run, if the user chose.
    pub fn get_chat_workspace_routing(&self, id: &str) -> anyhow::Result<Option<String>> {
        self.store.lock().get_chat_workspace_routing(id)
    }

    pub fn list_chats(&self) -> Vec<Chat> {
        self.chats.lock().clone()
    }
}
