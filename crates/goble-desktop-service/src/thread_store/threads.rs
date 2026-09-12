use chrono::{DateTime, Utc};
use goble_core::thread::{
    Participant, ParticipantId, Thread, ThreadError, ThreadId, ThreadKind, UserId,
};

use super::ThreadStore;

impl ThreadStore {
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

    pub fn list_participants(&self, thread_id: &ThreadId) -> Result<Vec<Participant>, ThreadError> {
        self.get_thread(thread_id).map(|t| t.participants)
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
}
