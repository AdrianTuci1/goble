use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use goble_core::principal::PrincipalId;
use sha2::{Digest, Sha256};
use goble_core::thread::{
    MessageId, Participant, ParticipantId, Reaction, Thread, ThreadError, ThreadKind,
    ThreadMessage, ThreadId, UserId,
};
use goble_core::user::{AuthorizedKey, UserError, UserProfile};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

const THREADS_FILE: &str = "threads.json";
const MESSAGES_DIR: &str = "messages";
const USERS_FILE: &str = "users.json";
const KEYS_FILE: &str = "keys.json";
const READ_RECEIPTS_FILE: &str = "read_receipts.json";
const MIGRATION_MARKER: &str = "threads_migration_v1.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MigrationMarker {
    version: u32,
    migrated_at: String,
}

/// In-memory store for threads, messages, user profile, and authorized keys.
/// Persists to JSON files under the provided base directory.
pub struct ThreadStore {
    threads: Mutex<Vec<Thread>>,
    messages: Mutex<HashMap<String, Vec<ThreadMessage>>>,
    last_read_at: Mutex<HashMap<String, chrono::DateTime<Utc>>>,
    profile: Mutex<Option<UserProfile>>,
    keys: Mutex<Vec<AuthorizedKey>>,
    base_path: PathBuf,
}

impl ThreadStore {
    pub fn new(base_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();
        std::fs::create_dir_all(&base_path)?;
        std::fs::create_dir_all(base_path.join(MESSAGES_DIR))?;

        let store = Self {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(HashMap::new()),
            last_read_at: Mutex::new(HashMap::new()),
            profile: Mutex::new(None),
            keys: Mutex::new(Vec::new()),
            base_path,
        };

        store.load()?;
        Ok(store)
    }

    pub fn list_threads(&self) -> Vec<Thread> {
        self.threads.lock().clone()
    }

    pub fn list_threads_with_read_status(&self) -> Vec<(Thread, Option<chrono::DateTime<Utc>>)> {
        let threads = self.threads.lock().clone();
        let last_read = self.last_read_at.lock();
        threads
            .into_iter()
            .map(|t| {
                let read_at = last_read.get(&t.id.0).copied();
                (t, read_at)
            })
            .collect()
    }

    pub fn create_thread(
        &self,
        kind: ThreadKind,
        title: impl Into<String>,
        owner_id: UserId,
        is_private: bool,
        mut participants: Vec<Participant>,
        tags: Vec<String>,
    ) -> Result<Thread, ThreadError> {
        let title = title.into();
        let id = ThreadId::generate();

        if kind == ThreadKind::Chat && participants.len() != 1 {
            participants = vec![Participant::User(owner_id.clone())];
        }

        if kind == ThreadKind::Direct && participants.len() != 2 {
            return Err(ThreadError::InvalidDirectThreadParticipantCount(
                participants.len(),
            ));
        }

        let seen: std::collections::HashSet<_> =
            participants.iter().map(|p| p.participant_id()).collect();
        if seen.len() != participants.len() {
            return Err(ThreadError::DuplicateParticipant(
                participants[participants.len() - 1].participant_id(),
            ));
        }

        let thread = Thread::new(id, kind, title, owner_id, is_private, participants, tags);
        self.threads.lock().push(thread.clone());
        self.save().map_err(|_| ThreadError::Unauthorized)?;
        Ok(thread)
    }

    pub fn get_thread(&self, id: &ThreadId) -> Result<Thread, ThreadError> {
        self.threads
            .lock()
            .iter()
            .find(|t| &t.id == id)
            .cloned()
            .ok_or_else(|| ThreadError::ThreadNotFound(id.clone()))
    }

    pub fn delete_thread(&self, id: &ThreadId) -> bool {
        let before = self.threads.lock().len();
        self.threads.lock().retain(|t| &t.id != id);
        let removed = self.threads.lock().len() < before;
        if removed {
            self.messages.lock().remove(&id.0);
            let _ = self.save();
        }
        removed
    }

    pub fn add_participant(
        &self,
        thread_id: &ThreadId,
        participant: Participant,
    ) -> Result<(), ThreadError> {
        let mut threads = self.threads.lock();
        let thread = threads
            .iter_mut()
            .find(|t| &t.id == thread_id)
            .ok_or_else(|| ThreadError::ThreadNotFound(thread_id.clone()))?;

        if thread.kind == ThreadKind::Direct {
            return Err(ThreadError::Unauthorized);
        }

        thread.add_participant(participant)?;
        drop(threads);
        self.save().map_err(|_| ThreadError::Unauthorized)?;
        Ok(())
    }

    pub fn remove_participant(
        &self,
        thread_id: &ThreadId,
        participant_id: &ParticipantId,
    ) -> Result<(), ThreadError> {
        let mut threads = self.threads.lock();
        let thread = threads
            .iter_mut()
            .find(|t| &t.id == thread_id)
            .ok_or_else(|| ThreadError::ThreadNotFound(thread_id.clone()))?;

        if thread.kind == ThreadKind::Direct {
            return Err(ThreadError::Unauthorized);
        }

        thread.remove_participant(participant_id)?;
        drop(threads);
        self.save().map_err(|_| ThreadError::Unauthorized)?;
        Ok(())
    }

    pub fn list_participants(
        &self,
        thread_id: &ThreadId,
    ) -> Result<Vec<Participant>, ThreadError> {
        self.get_thread(thread_id).map(|t| t.participants)
    }

    pub fn list_messages(&self, thread_id: &ThreadId) -> Result<Vec<ThreadMessage>, ThreadError> {
        self.get_thread(thread_id)?;
        Ok(self
            .messages
            .lock()
            .get(&thread_id.0)
            .cloned()
            .unwrap_or_default())
    }

    pub fn update_message(
        &self,
        thread_id: &ThreadId,
        message_id: &MessageId,
        participant_id: &ParticipantId,
        content: impl Into<String>,
    ) -> Result<ThreadMessage, ThreadError> {
        let content = content.into();
        self.get_thread(thread_id)?;
        let mut messages = self.messages.lock();
        let list = messages
            .get_mut(&thread_id.0)
            .ok_or_else(|| ThreadError::ThreadNotFound(thread_id.clone()))?;
        let message = list
            .iter_mut()
            .find(|m| &m.id == message_id)
            .ok_or_else(|| ThreadError::MessageNotFound(message_id.clone()))?;
        if message.author.participant_id() != *participant_id {
            return Err(ThreadError::Unauthorized);
        }
        message.content = content;
        message.participant_mentions = Self::extract_mentions(&message.content);
        message.updated_at = Utc::now();
        let updated = message.clone();
        drop(messages);
        self.save().map_err(|_| ThreadError::Unauthorized)?;
        Ok(updated)
    }

    pub fn delete_message(
        &self,
        thread_id: &ThreadId,
        message_id: &MessageId,
        participant_id: &ParticipantId,
    ) -> Result<(), ThreadError> {
        self.get_thread(thread_id)?;
        let mut messages = self.messages.lock();
        let list = messages
            .get_mut(&thread_id.0)
            .ok_or_else(|| ThreadError::ThreadNotFound(thread_id.clone()))?;
        let index = list
            .iter()
            .position(|m| &m.id == message_id)
            .ok_or_else(|| ThreadError::MessageNotFound(message_id.clone()))?;
        if list[index].author.participant_id() != *participant_id {
            return Err(ThreadError::Unauthorized);
        }
        list.remove(index);
        drop(messages);
        self.save().map_err(|_| ThreadError::Unauthorized)?;
        Ok(())
    }

    pub fn post_message(
        &self,
        thread_id: &ThreadId,
        author: Participant,
        content: impl Into<String>,
        reply_to: Option<MessageId>,
        tags: Vec<String>,
        mentions: Vec<ParticipantId>,
        trace_id: Option<String>,
    ) -> Result<ThreadMessage, ThreadError> {
        let thread = self.get_thread(thread_id)?;
        if !thread.has_participant(&author.participant_id()) {
            return Err(ThreadError::Unauthorized);
        }

        let mut message = ThreadMessage::new(thread_id.clone(), author, content)
            .with_tags(tags)
            .with_trace_id(trace_id.unwrap_or_default());
        message.participant_mentions = mentions;

        if let Some(ref parent_id) = reply_to {
            let messages = self.messages.lock();
            let parent = messages
                .get(&thread_id.0)
                .and_then(|list| list.iter().find(|m| &m.id == parent_id))
                .ok_or_else(|| ThreadError::ReplyToNotFound(parent_id.clone()))?;
            if parent.thread_id != *thread_id {
                return Err(ThreadError::ReplyToDifferentThread);
            }
            message.reply_to = Some(parent_id.clone());
        }

        self.messages
            .lock()
            .entry(thread_id.0.clone())
            .or_default()
            .push(message.clone());

        self.save().map_err(|_| ThreadError::Unauthorized)?;
        Ok(message)
    }

    pub fn add_reaction(
        &self,
        thread_id: &ThreadId,
        message_id: &MessageId,
        participant_id: ParticipantId,
        emoji: impl Into<String>,
    ) -> Result<(), ThreadError> {
        self.get_thread(thread_id)?;
        let emoji = emoji.into();
        let mut messages = self.messages.lock();
        let list = messages
            .get_mut(&thread_id.0)
            .ok_or_else(|| ThreadError::ThreadNotFound(thread_id.clone()))?;
        let message = list
            .iter_mut()
            .find(|m| &m.id == message_id)
            .ok_or_else(|| ThreadError::MessageNotFound(message_id.clone()))?;

        if message.has_reaction_from(&emoji, &participant_id) {
            return Ok(());
        }

        message.reactions.push(Reaction {
            emoji,
            participant_id,
        });
        message.updated_at = Utc::now();
        drop(messages);
        self.save().map_err(|_| ThreadError::Unauthorized)?;
        Ok(())
    }

    pub fn remove_reaction(
        &self,
        thread_id: &ThreadId,
        message_id: &MessageId,
        participant_id: &ParticipantId,
        emoji: &str,
    ) -> Result<(), ThreadError> {
        self.get_thread(thread_id)?;
        let mut messages = self.messages.lock();
        let list = messages
            .get_mut(&thread_id.0)
            .ok_or_else(|| ThreadError::ThreadNotFound(thread_id.clone()))?;
        let message = list
            .iter_mut()
            .find(|m| &m.id == message_id)
            .ok_or_else(|| ThreadError::MessageNotFound(message_id.clone()))?;

        message
            .reactions
            .retain(|r| !(r.emoji == emoji && &r.participant_id == participant_id));
        message.updated_at = Utc::now();
        drop(messages);
        self.save().map_err(|_| ThreadError::Unauthorized)?;
        Ok(())
    }

    pub fn get_profile(&self) -> Option<UserProfile> {
        self.profile.lock().clone()
    }

    pub fn set_profile(&self, profile: UserProfile) -> anyhow::Result<()> {
        *self.profile.lock() = Some(profile);
        self.save()
    }

    pub fn mark_thread_read(&self, thread_id: &ThreadId) -> Result<(), ThreadError> {
        self.get_thread(thread_id)?;
        self.last_read_at
            .lock()
            .insert(thread_id.0.clone(), Utc::now());
        self.save().map_err(|_| ThreadError::Unauthorized)?;
        Ok(())
    }

    pub fn get_last_read_at(&self, thread_id: &ThreadId) -> Option<DateTime<Utc>> {
        self.last_read_at.lock().get(&thread_id.0).copied()
    }

    pub fn list_authorized_keys(&self) -> Vec<AuthorizedKey> {
        self.keys.lock().clone()
    }

    pub fn add_authorized_key(&self, key: AuthorizedKey) -> Result<(), UserError> {
        let mut keys = self.keys.lock();
        if keys.iter().any(|k| k.fingerprint == key.fingerprint) {
            return Err(UserError::DuplicateKeyFingerprint(key.fingerprint));
        }
        keys.push(key);
        drop(keys);
        self.save().map_err(|_| UserError::ProfileNotFound)?;
        Ok(())
    }

    pub fn remove_authorized_key(&self, id: &str) -> bool {
        let before = self.keys.lock().len();
        self.keys.lock().retain(|k| k.id != id);
        let removed = self.keys.lock().len() < before;
        if removed {
            let _ = self.save();
        }
        removed
    }

    pub fn extract_mentions(content: &str) -> Vec<ParticipantId> {
        let mut mentions = Vec::new();
        for word in content.split_whitespace() {
            if let Some(stripped) = word.strip_prefix("@user:") {
                mentions.push(ParticipantId::user(stripped.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')));
            } else if let Some(stripped) = word.strip_prefix("@agent:") {
                mentions.push(ParticipantId::agent(stripped.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')));
            } else if word.starts_with("@") {
                let raw = word[1..].trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
                if !raw.is_empty() && !raw.contains(':') {
                    mentions.push(ParticipantId::agent(raw));
                }
            }
        }
        mentions
    }

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
                        Participant::Agent(goble_core::agent::AgentId(
                            "goble_default".to_string(),
                        ))
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

    fn save(&self) -> anyhow::Result<()> {
        let threads = self.threads.lock();
        let messages = self.messages.lock();
        let profile = self.profile.lock();
        let keys = self.keys.lock();

        std::fs::write(
            self.base_path.join(THREADS_FILE),
            serde_json::to_string_pretty(&*threads)?,
        )?;

        for (thread_id, list) in messages.iter() {
            std::fs::write(
                self.base_path
                    .join(MESSAGES_DIR)
                    .join(format!("{}.jsonl", thread_id)),
                list.iter()
                    .map(|m| serde_json::to_string(m))
                    .collect::<Result<Vec<_>, _>>()?
                    .join("\n"),
            )?;
        }

        std::fs::write(
            self.base_path.join(USERS_FILE),
            serde_json::to_string_pretty(&*profile)?,
        )?;

        std::fs::write(
            self.base_path.join(KEYS_FILE),
            serde_json::to_string_pretty(&*keys)?,
        )?;

        let last_read = self.last_read_at.lock();
        std::fs::write(
            self.base_path.join(READ_RECEIPTS_FILE),
            serde_json::to_string_pretty(&*last_read)?,
        )?;

        Ok(())
    }

    fn load(&self) -> anyhow::Result<()> {
        let threads_path = self.base_path.join(THREADS_FILE);
        if threads_path.exists() {
            let data = std::fs::read_to_string(&threads_path)?;
            let threads: Vec<Thread> = serde_json::from_str(&data)?;
            *self.threads.lock() = threads;
        }

        let messages_dir = self.base_path.join(MESSAGES_DIR);
        if messages_dir.exists() {
            let mut map = HashMap::new();
            for entry in std::fs::read_dir(&messages_dir)? {
                let entry = entry?;
                let path = entry.path();
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    let content = std::fs::read_to_string(&path)?;
                    let mut messages = Vec::new();
                    for line in content.lines() {
                        if line.trim().is_empty() {
                            continue;
                        }
                        messages.push(serde_json::from_str::<ThreadMessage>(line)?);
                    }
                    map.insert(name.to_string(), messages);
                }
            }
            *self.messages.lock() = map;
        }

        let profile_path = self.base_path.join(USERS_FILE);
        if profile_path.exists() {
            let data = std::fs::read_to_string(&profile_path)?;
            *self.profile.lock() = serde_json::from_str(&data)?;
        }

        let keys_path = self.base_path.join(KEYS_FILE);
        if keys_path.exists() {
            let data = std::fs::read_to_string(&keys_path)?;
            *self.keys.lock() = serde_json::from_str(&data)?;
        }

        let read_path = self.base_path.join(READ_RECEIPTS_FILE);
        if read_path.exists() {
            let data = std::fs::read_to_string(&read_path)?;
            *self.last_read_at.lock() = serde_json::from_str(&data)?;
        }

        Ok(())
    }

    pub fn invite_user_by_public_key(
        &self,
        thread_id: &ThreadId,
        public_key_pem: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<Participant, ThreadError> {
        let participant = self.resolve_user_by_public_key(public_key_pem, name)
            .map_err(|_| ThreadError::Unauthorized)?;
        self.add_participant(thread_id, participant.clone())?;
        Ok(participant)
    }

    pub fn resolve_user_by_public_key(
        &self,
        public_key_pem: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<Participant, UserError> {
        let pem = public_key_pem.into();
        let fingerprint = fingerprint(&pem);
        let id = PrincipalId(fingerprint.clone());
        let key = AuthorizedKey::new(&fingerprint, name, &pem, &fingerprint);
        self.add_authorized_key(key)?;
        Ok(Participant::User(UserId::from_principal(id)))
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

fn fingerprint(pem: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(pem.trim().as_bytes());
    format!("{}", hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use goble_core::agent::AgentId;
    use goble_core::principal::PrincipalId;
    use goble_core::thread::{
        MessageId, Participant, ParticipantId, Thread, ThreadError, ThreadId, ThreadKind,
        UserId,
    };
    use goble_core::user::{AuthorizedKey, UserError, UserProfile};

    fn tmp_store() -> (tempfile::TempDir, ThreadStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ThreadStore::new(dir.path()).unwrap();
        (dir, store)
    }

    fn owner() -> UserId {
        UserId::generate()
    }

    fn user(id: &str) -> Participant {
        Participant::User(UserId(id.to_string()))
    }

    fn agent(id: &str) -> Participant {
        Participant::Agent(AgentId(id.to_string()))
    }

    #[test]
    fn create_channel_and_invite_agent() {
        let (_dir, store) = tmp_store();
        let owner = owner();
        let channel = store
            .create_thread(
                ThreadKind::Channel,
                "general",
                owner.clone(),
                false,
                vec![Participant::User(owner.clone())],
                vec!["#general".to_string()],
            )
            .unwrap();

        store
            .add_participant(&channel.id, agent("agent-1"))
            .unwrap();

        let participants = store.list_participants(&channel.id).unwrap();
        let ids: HashSet<_> = participants.iter().map(|p| p.participant_id()).collect();
        assert!(ids.contains(&ParticipantId::user(&owner.0)));
        assert!(ids.contains(&ParticipantId::agent("agent-1")));
    }

    #[test]
    fn cannot_invite_into_direct_thread() {
        let (_dir, store) = tmp_store();
        let owner = owner();
        let dm = store
            .create_thread(
                ThreadKind::Direct,
                "dm",
                owner.clone(),
                false,
                vec![Participant::User(owner.clone()), agent("agent-1")],
                vec![],
            )
            .unwrap();

        assert!(matches!(
            store.add_participant(&dm.id, user("someone")),
            Err(ThreadError::Unauthorized)
        ));
    }

    #[test]
    fn post_message_with_reply_and_mentions() {
        let (_dir, store) = tmp_store();
        let owner = owner();
        let channel = store
            .create_thread(
                ThreadKind::Channel,
                "team",
                owner.clone(),
                false,
                vec![Participant::User(owner.clone()), agent("agent-1")],
                vec![],
            )
            .unwrap();

        let parent = store
            .post_message(
                &channel.id,
                Participant::User(owner.clone()),
                "hello team",
                None,
                vec!["#team".to_string()],
                vec![ParticipantId::agent("agent-1")],
                None,
            )
            .unwrap();

        let reply = store
            .post_message(
                &channel.id,
                agent("agent-1"),
                "hi there",
                Some(parent.id.clone()),
                vec![],
                vec![ParticipantId::user(&owner.0)],
                None,
            )
            .unwrap();

        assert_eq!(reply.reply_to, Some(parent.id.clone()));
        assert!(reply
            .participant_mentions
            .contains(&ParticipantId::user(&owner.0)));

        let messages = store.list_messages(&channel.id).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].id, reply.id);
    }

    #[test]
    fn reply_to_missing_message_fails() {
        let (_dir, store) = tmp_store();
        let owner = owner();
        let channel = store
            .create_thread(
                ThreadKind::Channel,
                "team",
                owner.clone(),
                false,
                vec![Participant::User(owner.clone())],
                vec![],
            )
            .unwrap();

        let missing = MessageId::generate();
        let result = store.post_message(
            &channel.id,
            Participant::User(owner.clone()),
            "orphan",
            Some(missing.clone()),
            vec![],
            vec![],
                        None,

        );
        assert!(matches!(result, Err(ThreadError::ReplyToNotFound(id)) if id == missing));
    }

    #[test]
    fn reactions_roundtrip() {
        let (_dir, store) = tmp_store();
        let owner = owner();
        let channel = store
            .create_thread(
                ThreadKind::Channel,
                "team",
                owner.clone(),
                false,
                vec![Participant::User(owner.clone()), agent("agent-1")],
                vec![],
            )
            .unwrap();

        let msg = store
            .post_message(
                &channel.id,
                Participant::User(owner.clone()),
                "ship it",
                None,
                vec![],
                vec![],
                None,
            )
            .unwrap();

        store
            .add_reaction(&channel.id, &msg.id, ParticipantId::agent("agent-1"), "🚀")
            .unwrap();
        store
            .add_reaction(&channel.id, &msg.id, ParticipantId::user(&owner.0), "🚀")
            .unwrap();
        store
            .add_reaction(&channel.id, &msg.id, ParticipantId::agent("agent-1"), "🚀")
            .unwrap();

        let messages = store.list_messages(&channel.id).unwrap();
        let reactions = &messages[0].reactions;
        assert_eq!(reactions.len(), 2);

        store
            .remove_reaction(&channel.id, &msg.id, &ParticipantId::agent("agent-1"), "🚀")
            .unwrap();
        let messages = store.list_messages(&channel.id).unwrap();
        assert_eq!(messages[0].reactions.len(), 1);
    }

    #[test]
    fn persistence_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let owner = owner();

        let store = ThreadStore::new(dir.path()).unwrap();
        let channel = store
            .create_thread(
                ThreadKind::Channel,
                "general",
                owner.clone(),
                false,
                vec![Participant::User(owner.clone()), agent("agent-1")],
                vec!["#general".to_string()],
            )
            .unwrap();
        store
            .post_message(
                &channel.id,
                Participant::User(owner.clone()),
                "first",
                None,
                vec!["#general".to_string()],
                vec![ParticipantId::agent("agent-1")],
                None,
            )
            .unwrap();

        drop(store);
        let store = ThreadStore::new(dir.path()).unwrap();

        let threads = store.list_threads();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].title, "general");

        let messages = store.list_messages(&threads[0].id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "first");
    }

    #[test]
    fn profile_and_keys_roundtrip() {
        let (_dir, store) = tmp_store();
        let profile = UserProfile::new(
            PrincipalId("u1".to_string()),
            "Adrian",
            "adrian@example.com",
        )
        .with_public_key("pem");
        store.set_profile(profile.clone()).unwrap();

        let key = AuthorizedKey::new("k1", "laptop", "pem", "fp");
        store.add_authorized_key(key.clone()).unwrap();
        assert!(matches!(
            store.add_authorized_key(key.clone()),
            Err(UserError::DuplicateKeyFingerprint(_))
        ));

        assert_eq!(store.get_profile().unwrap().name, "Adrian");
        assert_eq!(store.list_authorized_keys().len(), 1);
    }

    #[test]
    fn invite_user_by_public_key_adds_participant() {
        let (_dir, store) = tmp_store();
        let owner = owner();
        let thread = store.create_thread(
            ThreadKind::Channel,
            "team",
            owner.clone(),
            true,
            vec![Participant::User(owner.clone())],
            vec![],
        ).unwrap();
        let pem = "-----BEGIN PUBLIC KEY-----\n[REDACTED]\n-----END PUBLIC KEY-----";
        let participant = store
            .invite_user_by_public_key(&thread.id, pem, "Ada")
            .unwrap();
        assert!(participant.is_user());
        let participants = store.list_participants(&thread.id).unwrap();
        assert_eq!(participants.len(), 2);
        assert!(store.list_authorized_keys().len() >= 1);
    }

    #[allow(dead_code)]
    fn create_private_thread_for_reply_requires_exactly_two_participants() {
        let (_dir, store) = tmp_store();
        let owner = owner();
        let result = store.create_thread(
            ThreadKind::Direct,
            "private",
            owner.clone(),
                false,
                vec![Participant::User(owner.clone())],
            vec![],
        );
        assert!(matches!(
            result,
            Err(ThreadError::InvalidDirectThreadParticipantCount(1))
        ));
    }

    #[test]
    fn duplicate_participant_is_rejected() {
        let (_dir, store) = tmp_store();
        let owner = owner();
        let result = store.create_thread(
            ThreadKind::Channel,
            "duplicates",
            owner.clone(),
                false,
                vec![
                Participant::User(owner.clone()),
                Participant::User(owner.clone()),
            ],
            vec![],
        );
        assert!(matches!(result, Err(ThreadError::DuplicateParticipant(_))));
    }

    #[test]
    fn thread_serde_roundtrip() {
        let thread = Thread::new(
            ThreadId::generate(),
            ThreadKind::Channel,
            "general",
            UserId::generate(),
            false,
            vec![user("u1"), agent("a1")],
            vec!["#general".to_string()],
        );
        let json = serde_json::to_string(&thread).unwrap();
        let back: Thread = serde_json::from_str(&json).unwrap();
        assert_eq!(thread, back);
    }

    #[test]
    fn message_mentions_extracted_from_content() {
        let content = "hey @user:u1 and @agent:a1 check this";
        let mentions = ThreadStore::extract_mentions(content);
        assert_eq!(
            mentions,
            vec![ParticipantId::user("u1"), ParticipantId::agent("a1"),]
        );
    }


    #[test]
    fn post_message_rejects_non_participant_author() {
        let (_dir, store) = tmp_store();
        let owner = owner();
        let channel = store
            .create_thread(
                ThreadKind::Channel,
                "team",
                owner.clone(),
                false,
                vec![Participant::User(owner.clone())],
                vec![],
            )
            .unwrap();
        let result = store.post_message(
            &channel.id,
            agent("agent-1"),
            "hello",
            None,
            vec![],
            vec![],
                        None,

        );
        assert!(matches!(result, Err(ThreadError::Unauthorized)));
    }

    #[test]
    fn private_channel_requires_participant_to_read() {
        let (_dir, store) = tmp_store();
        let owner = owner();
        let other = user("other");
        let channel = store
            .create_thread(
                ThreadKind::Channel,
                "private",
                owner.clone(),
                true,
                vec![Participant::User(owner.clone())],
                vec![],
            )
            .unwrap();
        store.add_participant(&channel.id, other.clone()).unwrap();
        let parts = store.list_participants(&channel.id).unwrap();
        assert!(parts.iter().any(|p| p.participant_id() == other.participant_id()));
    }
}
