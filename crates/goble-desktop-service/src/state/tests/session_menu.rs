//! The sidebar session menu's durable support: the archived stamp and the
//! persisted Star/pin, and the id sets the sidebar's filter and Starred block
//! are built from.

use super::*;

/// Archiving writes a real store stamp, announces the change, and leaves the
/// conversation's row and messages intact: the sidebar hides it through its
/// filter, nothing about the conversation itself is deleted.
#[test]
fn archiving_a_conversation_persists_and_restores_through_the_store() {
    let (_dir, state) = tmp_state();
    let bus = Arc::new(crate::event_bus::CollectingEventBus::new());
    state.set_event_bus(bus.clone());
    let chat_id = state
        .create_chat("Redesign the sidebar", None, None)
        .expect("create chat");
    bus.take_events();

    state
        .set_chat_archived(&chat_id, true)
        .expect("archive the conversation");
    let archived_at = state
        .chat_archived(&chat_id)
        .expect("read the stamp")
        .expect("an archived conversation carries a stamp");
    assert!(!archived_at.is_empty(), "the stamp is a real timestamp");
    assert_eq!(
        state.archived_chat_ids().unwrap(),
        vec![chat_id.clone()],
        "the sidebar's filter reads it from the store"
    );
    assert!(
        state.list_chats().iter().any(|chat| chat.id == chat_id),
        "the conversation row itself is not removed"
    );
    assert!(
        bus.take_events().iter().any(|(name, _)| name == "chats:updated"),
        "the write is announced so the sidebar re-reads its archived set"
    );

    state
        .set_chat_archived(&chat_id, false)
        .expect("restore the conversation");
    assert_eq!(state.chat_archived(&chat_id).unwrap(), None);
    assert!(state.archived_chat_ids().unwrap().is_empty());
}

/// Starring persists the pin and reads back through the id set the Starred
/// block is seeded from: it survives an app restart instead of living only in
/// the running UI state.
#[test]
fn starring_a_conversation_persists_the_pin() {
    let (_dir, state) = tmp_state();
    let bus = Arc::new(crate::event_bus::CollectingEventBus::new());
    state.set_event_bus(bus.clone());
    let chat_id = state
        .create_chat("Sidebar subjects", None, None)
        .expect("create chat");
    bus.take_events();

    state
        .set_chat_starred(&chat_id, true)
        .expect("star the conversation");
    assert!(state.chat_starred(&chat_id).unwrap());
    assert_eq!(state.starred_chat_ids().unwrap(), vec![chat_id.clone()]);
    assert!(
        bus.take_events().iter().any(|(name, _)| name == "chats:updated"),
        "the write is announced so the Starred block re-reads it"
    );

    state
        .set_chat_starred(&chat_id, false)
        .expect("unstar the conversation");
    assert!(!state.chat_starred(&chat_id).unwrap());
    assert!(state.starred_chat_ids().unwrap().is_empty());
}
