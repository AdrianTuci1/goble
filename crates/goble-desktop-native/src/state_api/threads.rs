use std::sync::Arc;

use goble_core::thread::{MessageId, Participant, ParticipantId, ThreadId, ThreadKind, UserId};
use goble_desktop_service::{DesktopState, ThreadMessageSummary, ThreadSummary};

pub fn list_threads(state: &Arc<DesktopState>) -> Vec<ThreadSummary> {
    state
        .thread_store()
        .list_threads_with_read_status()
        .into_iter()
        .map(|(t, read_at)| ThreadSummary {
            last_read_at: read_at.map(|dt| dt.to_rfc3339()),
            ..ThreadSummary::from(t)
        })
        .collect()
}

pub struct CreateThreadRequest {
    pub kind: ThreadKind,
    pub title: String,
    pub is_private: bool,
    pub participants: Vec<Participant>,
    pub tags: Vec<String>,
}

pub fn create_thread(
    state: &Arc<DesktopState>,
    req: CreateThreadRequest,
) -> anyhow::Result<ThreadSummary> {
    let owner_id = state
        .thread_store()
        .get_profile()
        .map(|p| UserId(p.id.to_string()))
        .unwrap_or_else(UserId::generate);
    let thread = state
        .thread_store()
        .create_thread(
            req.kind,
            req.title,
            owner_id,
            req.is_private,
            req.participants,
            req.tags,
        )
        .map(ThreadSummary::from)?;
    state.emit("threads:updated", ());
    Ok(thread)
}

pub fn delete_thread(state: &Arc<DesktopState>, thread_id: &str) -> bool {
    state
        .thread_store()
        .delete_thread(&ThreadId(thread_id.to_string()))
}

pub struct AddThreadParticipantRequest {
    pub thread_id: String,
    pub participant: Participant,
}

pub fn add_thread_participant(
    state: &Arc<DesktopState>,
    req: AddThreadParticipantRequest,
) -> anyhow::Result<()> {
    state
        .thread_store()
        .add_participant(&ThreadId(req.thread_id), req.participant)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit("threads:updated", ());
    Ok(())
}

pub struct RemoveThreadParticipantRequest {
    pub thread_id: String,
    pub participant_id: String,
}

pub fn remove_thread_participant(
    state: &Arc<DesktopState>,
    req: RemoveThreadParticipantRequest,
) -> anyhow::Result<()> {
    state
        .thread_store()
        .remove_participant(
            &ThreadId(req.thread_id),
            &ParticipantId(req.participant_id),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit("threads:updated", ());
    Ok(())
}

pub fn get_thread_participants(
    state: &Arc<DesktopState>,
    thread_id: &str,
) -> anyhow::Result<Vec<Participant>> {
    state
        .thread_store()
        .list_participants(&ThreadId(thread_id.to_string()))
        .map_err(|e| anyhow::anyhow!("{e}"))
}

pub fn get_thread_messages(
    state: &Arc<DesktopState>,
    thread_id: &str,
) -> anyhow::Result<Vec<ThreadMessageSummary>> {
    state
        .thread_store()
        .list_messages(&ThreadId(thread_id.to_string()))
        .map(|messages| messages.into_iter().map(ThreadMessageSummary::from).collect())
        .map_err(|e| anyhow::anyhow!("{e}"))
}

pub struct PostMessageRequest {
    pub thread_id: String,
    pub content: String,
    pub reply_to: Option<String>,
    pub tags: Vec<String>,
    pub mentions: Vec<String>,
    pub trace_id: Option<String>,
}

pub fn post_thread_message(
    state: &Arc<DesktopState>,
    req: PostMessageRequest,
) -> anyhow::Result<ThreadMessageSummary> {
    let author = state
        .thread_store()
        .get_profile()
        .map(|p| Participant::User(UserId(p.id.to_string())))
        .unwrap_or_else(|| Participant::User(UserId::generate()));
    let reply_to = req.reply_to.map(MessageId);
    let mentions: Vec<ParticipantId> = req.mentions.into_iter().map(ParticipantId).collect();
    let thread_id = ThreadId(req.thread_id.clone());
    let message = state
        .thread_store()
        .post_message(
            &thread_id,
            author,
            req.content,
            reply_to,
            req.tags,
            mentions,
            req.trace_id,
        )
        .map(ThreadMessageSummary::from)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit(
        "thread:message:created",
        serde_json::json!({
            "thread_id": req.thread_id.clone(),
            "message": message.clone(),
        }),
    );
    state.emit(
        "thread:messages:updated",
        serde_json::json!({ "thread_id": req.thread_id }),
    );
    Ok(message)
}

pub fn mark_thread_read(state: &Arc<DesktopState>, thread_id: &str) -> anyhow::Result<()> {
    state
        .thread_store()
        .mark_thread_read(&ThreadId(thread_id.to_string()))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit(
        "thread:updated",
        serde_json::json!({ "thread_id": thread_id }),
    );
    Ok(())
}

pub struct UpdateThreadMessageRequest {
    pub thread_id: String,
    pub message_id: String,
    pub content: String,
}

pub fn update_thread_message(
    state: &Arc<DesktopState>,
    req: UpdateThreadMessageRequest,
) -> anyhow::Result<ThreadMessageSummary> {
    let me = state
        .thread_store()
        .get_profile()
        .ok_or_else(|| anyhow::anyhow!("profile not set"))?;
    let msg = state
        .thread_store()
        .update_message(
            &ThreadId(req.thread_id.clone()),
            &MessageId(req.message_id.clone()),
            &ParticipantId::user(me.id.0.clone()),
            req.content,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit(
        "thread:messages:updated",
        serde_json::json!({ "thread_id": req.thread_id }),
    );
    Ok(ThreadMessageSummary::from(msg))
}

pub struct DeleteThreadMessageRequest {
    pub thread_id: String,
    pub message_id: String,
}

pub fn delete_thread_message(
    state: &Arc<DesktopState>,
    req: DeleteThreadMessageRequest,
) -> anyhow::Result<()> {
    let me = state
        .thread_store()
        .get_profile()
        .ok_or_else(|| anyhow::anyhow!("profile not set"))?;
    state
        .thread_store()
        .delete_message(
            &ThreadId(req.thread_id.clone()),
            &MessageId(req.message_id.clone()),
            &ParticipantId::user(me.id.0.clone()),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit(
        "thread:messages:updated",
        serde_json::json!({ "thread_id": req.thread_id }),
    );
    Ok(())
}

pub struct ReactionRequest {
    pub thread_id: String,
    pub message_id: String,
    pub emoji: String,
}

fn thread_participant_id(state: &Arc<DesktopState>) -> ParticipantId {
    state
        .thread_store()
        .get_profile()
        .map(|p| ParticipantId::user(p.id.to_string()))
        .unwrap_or_else(|| ParticipantId::user(UserId::generate().to_string()))
}

pub fn add_thread_reaction(
    state: &Arc<DesktopState>,
    req: ReactionRequest,
) -> anyhow::Result<()> {
    let participant_id = thread_participant_id(state);
    state
        .thread_store()
        .add_reaction(
            &ThreadId(req.thread_id.clone()),
            &MessageId(req.message_id),
            participant_id,
            req.emoji,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit(
        "thread:messages:updated",
        serde_json::json!({ "thread_id": req.thread_id }),
    );
    Ok(())
}

pub fn remove_thread_reaction(
    state: &Arc<DesktopState>,
    req: ReactionRequest,
) -> anyhow::Result<()> {
    let participant_id = thread_participant_id(state);
    state
        .thread_store()
        .remove_reaction(
            &ThreadId(req.thread_id.clone()),
            &MessageId(req.message_id),
            &participant_id,
            &req.emoji,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit(
        "thread:messages:updated",
        serde_json::json!({ "thread_id": req.thread_id }),
    );
    Ok(())
}

pub struct InviteUserByPublicKeyRequest {
    pub thread_id: String,
    pub public_key_pem: String,
    pub name: String,
}

pub fn invite_user_by_public_key(
    state: &Arc<DesktopState>,
    req: InviteUserByPublicKeyRequest,
) -> anyhow::Result<Participant> {
    let participant = state
        .thread_store()
        .invite_user_by_public_key(
            &ThreadId(req.thread_id.clone()),
            req.public_key_pem,
            req.name,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit("threads:updated", ());
    Ok(participant)
}

pub fn migrate_legacy_chats_to_threads(
    state: &Arc<DesktopState>,
) -> anyhow::Result<Vec<ThreadSummary>> {
    state
        .migrate_legacy_chats_to_threads()
        .map(|summaries| {
            state.emit("threads:updated", ());
            summaries
        })
        .map_err(|e| anyhow::anyhow!("{e}"))
}
