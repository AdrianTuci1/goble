use chrono::Utc;
use goble_core::thread::{
    MessageId, Participant, ParticipantId, Reaction, ThreadError, ThreadId, ThreadMessage,
};

use super::ThreadStore;

impl ThreadStore {
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

    #[allow(clippy::too_many_arguments)]
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

    pub fn extract_mentions(content: &str) -> Vec<ParticipantId> {
        let mut mentions = Vec::new();
        for word in content.split_whitespace() {
            if let Some(stripped) = word.strip_prefix("@user:") {
                mentions.push(ParticipantId::user(stripped.trim_end_matches(|c: char| {
                    !c.is_alphanumeric() && c != '-' && c != '_'
                })));
            } else if let Some(stripped) = word.strip_prefix("@agent:") {
                mentions.push(ParticipantId::agent(stripped.trim_end_matches(
                    |c: char| !c.is_alphanumeric() && c != '-' && c != '_',
                )));
            } else if let Some(stripped) = word.strip_prefix("@") {
                let raw = stripped
                    .trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
                if !raw.is_empty() && !raw.contains(':') {
                    mentions.push(ParticipantId::agent(raw));
                }
            }
        }
        mentions
    }
}
