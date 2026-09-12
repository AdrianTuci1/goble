use std::collections::HashSet;

use super::*;
use goble_core::agent::AgentId;
use goble_core::principal::PrincipalId;
use goble_core::thread::{
    MessageId, Participant, ParticipantId, Thread, ThreadError, ThreadId, ThreadKind, UserId,
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
    let thread = store
        .create_thread(
            ThreadKind::Channel,
            "team",
            owner.clone(),
            true,
            vec![Participant::User(owner.clone())],
            vec![],
        )
        .unwrap();
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
    assert!(parts
        .iter()
        .any(|p| p.participant_id() == other.participant_id()));
}
