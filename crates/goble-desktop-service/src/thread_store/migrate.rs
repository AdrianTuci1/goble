use chrono::Utc;
use goble_core::thread::{
    MessageId, Participant, Thread, ThreadId, ThreadKind, ThreadMessage, UserId,
};
use serde::{Deserialize, Serialize};

use super::ThreadStore;

const MIGRATION_MARKER: &str = "threads_migration_v1.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MigrationMarker {
    version: u32,
    migrated_at: String,
}

impl ThreadStore {
    pub fn migrate_legacy_chats(
        &self,
        chats: Vec<LegacyChat>,
        owner_id: UserId,
    ) -> anyhow::Result<()> {
        let marker_path = self.base_path.join(MIGRATION_MARKER);
        if marker_path.exists() {
            return Ok(());
        }

        for chat in chats {
            let thread_id = ThreadId(chat.id.clone());
            let thread = Thread {
                id: thread_id.clone(),
                kind: ThreadKind::Chat,
                title: chat.title,
                owner_id: owner_id.clone(),
                is_private: false,
                participants: vec![Participant::User(owner_id.clone())],
                tags: Vec::new(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.threads.lock().push(thread);

            let messages: Vec<ThreadMessage> = chat
                .messages
                .into_iter()
                .map(|m| {
                    let role = m.role.to_lowercase();
                    let author = if role == "assistant" {
                        Participant::Agent(goble_core::agent::AgentId("goble_default".to_string()))
                    } else {
                        Participant::User(owner_id.clone())
                    };
                    ThreadMessage {
                        id: MessageId(m.id),
                        thread_id: thread_id.clone(),
                        author,
                        content: m.content,
                        reply_to: None,
                        tags: Vec::new(),
                        participant_mentions: Vec::new(),
                        reactions: Vec::new(),
                        attachments: Vec::new(),
                        trace_id: None,
                        created_at: m
                            .created_at
                            .parse::<chrono::DateTime<Utc>>()
                            .unwrap_or_else(|_| Utc::now()),
                        updated_at: Utc::now(),
                    }
                })
                .collect();
            self.messages.lock().insert(chat.id, messages);
        }

        self.save()?;
        let marker = MigrationMarker {
            version: 1,
            migrated_at: Utc::now().to_rfc3339(),
        };
        std::fs::write(marker_path, serde_json::to_string_pretty(&marker)?)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyChat {
    pub id: String,
    pub title: String,
    pub messages: Vec<LegacyChatMessage>,
}
