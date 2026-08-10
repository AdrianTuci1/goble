use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::agent::AgentId;
use crate::principal::PrincipalId;

/// A unified participant id used for mentions, reactions, and membership.
/// It is a tagged string of the form `user:<uuid>` or `agent:<uuid>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ParticipantId(pub String);

impl ParticipantId {
    pub fn user(id: impl Into<String>) -> Self {
        Self(format!("user:{}", id.into()))
    }

    pub fn agent(id: impl Into<String>) -> Self {
        Self(format!("agent:{}", id.into()))
    }

    pub fn kind(&self) -> &str {
        self.0.split(':').next().unwrap_or("unknown")
    }

    pub fn raw_id(&self) -> &str {
        self.0.split(':').nth(1).unwrap_or(&self.0)
    }
}

impl fmt::Display for ParticipantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A `UserId` is the public identity of a human participant. It is stored as the
/// raw UUID for backwards compatibility with the existing `PrincipalId` concept.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub String);

impl UserId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn from_principal(id: PrincipalId) -> Self {
        Self(id.0)
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A participant in a thread can be either a local user or an agent.
/// Agents are treated exactly like users for membership, mentions, and replies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Participant {
    User(UserId),
    Agent(AgentId),
}

impl Participant {
    pub fn participant_id(&self) -> ParticipantId {
        match self {
            Participant::User(u) => ParticipantId::user(&u.0),
            Participant::Agent(a) => ParticipantId::agent(&a.0),
        }
    }

    pub fn is_agent(&self) -> bool {
        matches!(self, Participant::Agent(_))
    }

    pub fn is_user(&self) -> bool {
        matches!(self, Participant::User(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadId(pub String);

impl ThreadId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageId(pub String);

impl MessageId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadKind {
    /// A one-to-one assistant chat, owned by the current user.
    Chat,
    /// A public or private channel with many participants.
    Channel,
    /// A direct thread with exactly two participants (user or agent).
    Direct,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub id: ThreadId,
    pub kind: ThreadKind,
    pub title: String,
    pub owner_id: UserId,
    pub is_private: bool,
    pub participants: Vec<Participant>,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Thread {
    pub fn new(
        id: ThreadId,
        kind: ThreadKind,
        title: impl Into<String>,
        owner_id: UserId,
        is_private: bool,
        participants: Vec<Participant>,
        tags: Vec<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            kind,
            title: title.into(),
            owner_id,
            is_private,
            participants,
            tags,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn has_participant(&self, participant_id: &ParticipantId) -> bool {
        self.participants
            .iter()
            .any(|p| &p.participant_id() == participant_id)
    }

    pub fn add_participant(&mut self, participant: Participant) -> Result<(), ThreadError> {
        let id = participant.participant_id();
        if self.has_participant(&id) {
            return Err(ThreadError::DuplicateParticipant(id));
        }
        self.participants.push(participant);
        self.updated_at = Utc::now();
        Ok(())
    }

    pub fn remove_participant(&mut self, participant_id: &ParticipantId) -> Result<(), ThreadError> {
        let before = self.participants.len();
        self.participants
            .retain(|p| &p.participant_id() != participant_id);
        if self.participants.len() == before {
            return Err(ThreadError::ParticipantNotFound(participant_id.clone()));
        }
        self.updated_at = Utc::now();
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadMessage {
    pub id: MessageId,
    pub thread_id: ThreadId,
    pub author: Participant,
    pub content: String,
    pub reply_to: Option<MessageId>,
    pub tags: Vec<String>,
    pub participant_mentions: Vec<ParticipantId>,
    pub reactions: Vec<Reaction>,
    pub attachments: Vec<Attachment>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ThreadMessage {
    pub fn new(
        thread_id: ThreadId,
        author: Participant,
        content: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: MessageId::generate(),
            thread_id,
            author,
            content: content.into(),
            reply_to: None,
            tags: Vec::new(),
            participant_mentions: Vec::new(),
            reactions: Vec::new(),
            attachments: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_reply_to(mut self, message_id: MessageId) -> Self {
        self.reply_to = Some(message_id);
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_mentions(mut self, mentions: Vec<ParticipantId>) -> Self {
        self.participant_mentions = mentions;
        self
    }

    pub fn has_reaction_from(&self, emoji: &str, participant_id: &ParticipantId) -> bool {
        self.reactions
            .iter()
            .any(|r| r.emoji == emoji && &r.participant_id == participant_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reaction {
    pub emoji: String,
    pub participant_id: ParticipantId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub blob_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadError {
    ThreadNotFound(ThreadId),
    MessageNotFound(MessageId),
    ParticipantNotFound(ParticipantId),
    DuplicateParticipant(ParticipantId),
    Unauthorized,
    InvalidDirectThreadParticipantCount(usize),
    ReplyToDifferentThread,
    ReplyToNotFound(MessageId),
}

impl fmt::Display for ThreadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThreadError::ThreadNotFound(id) => write!(f, "thread not found: {}", id.0),
            ThreadError::MessageNotFound(id) => write!(f, "message not found: {}", id.0),
            ThreadError::ParticipantNotFound(id) => write!(f, "participant not found: {}", id),
            ThreadError::DuplicateParticipant(id) => write!(f, "duplicate participant: {}", id),
            ThreadError::Unauthorized => write!(f, "unauthorized"),
            ThreadError::InvalidDirectThreadParticipantCount(n) => {
                write!(f, "direct thread requires exactly 2 participants, got {}", n)
            }
            ThreadError::ReplyToDifferentThread => write!(f, "reply must target the same thread"),
            ThreadError::ReplyToNotFound(id) => write!(f, "reply target not found: {}", id.0),
        }
    }
}

impl std::error::Error for ThreadError {}
