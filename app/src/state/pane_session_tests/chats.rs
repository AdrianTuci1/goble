use super::*;

    /// A sub-agent's conversation is not a sidebar conversation: the store's
    /// top-level listing leaves it out, the desktop cache loads only parents,
    /// and its own messages still load under its id for the child view.
    #[test]
    fn a_child_conversation_stays_out_of_the_sidebar_but_keeps_its_messages() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = DesktopState::new(
            goble_core::store::Store::open_in_memory().expect("in-memory store"),
            goble_desktop_service::ThreadStore::new(dir.path()).expect("thread store"),
        );
        let store = desktop.store_clone();
        let now = "2026-09-11T10:00:00Z";
        store
            .insert_chat("parent", "Parent", None, None, now, now)
            .expect("parent chat");
        store
            .insert_subagent_chat("child-1", "reviewer", "parent", None, None, now, now)
            .expect("child chat");
        store
            .insert_chat_message("m1", "child-1", "assistant", "CHILD MARKER", None, now)
            .expect("child row");

        desktop.load_from_store().expect("load the store");

        assert_eq!(
            desktop
                .list_chats()
                .into_iter()
                .map(|chat| chat.id)
                .collect::<Vec<_>>(),
            vec!["parent".to_string()],
            "only the top-level conversation is a sidebar card"
        );
        assert_eq!(
            store
                .list_chats()
                .unwrap()
                .into_iter()
                .map(|row| row.0)
                .collect::<Vec<_>>(),
            vec!["parent".to_string()],
            "the store's own listing leaves the child out"
        );
        assert_eq!(
            store
                .list_child_chats()
                .unwrap()
                .into_iter()
                .map(|row| row.0)
                .collect::<Vec<_>>(),
            vec!["child-1".to_string()],
            "the child is still reachable as a child"
        );
        let rows = desktop
            .list_chat_messages("child-1")
            .expect("the child's rows");
        assert!(
            rows.iter().any(|row| row.content == "CHILD MARKER"),
            "the child view still reads its own conversation"
        );
    }
