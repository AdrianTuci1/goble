use super::*;

    #[test]
    fn unbound_active_pane_falls_back_to_selected_conversation() {
        let mut state = UiState::mock();
        state.pane_sessions.get_mut(&1).unwrap().conversation_id.clear();
        state.selected_id = Some("sel".to_string());
        assert_eq!(state.pane_conversation_id(1).as_deref(), Some("sel"));
    }

    #[test]
    fn non_active_unbound_pane_has_no_conversation() {
        let mut state = UiState::mock();
        state.pane_sessions.get_mut(&1).unwrap().conversation_id.clear();
        // Pane 2 is not active and has no conversation -> None (not the fallback).
        assert_eq!(state.pane_conversation_id(2), None);
    }

    #[test]
    fn bind_pane_new_conversation_assigns_distinct_mock_id() {
        let mut state = UiState::mock();
        let pane1 = state.pane_sessions.get(&1).unwrap().conversation_id.clone();
        state.bind_pane_new_conversation(2, None);
        let pane2 = state.pane_sessions.get(&2).unwrap().conversation_id.clone();
        assert_eq!(pane2, "pane-2");
        assert_ne!(pane1, pane2, "two panes never share a conversation");
    }

    #[test]
    fn sync_active_view_reflects_active_pane() {
        let mut state = UiState::mock();
        state.pane_sessions.insert(
            1,
            PaneSession {
                conversation_id: "a".into(),
                draft: "draft1".into(),
                path: "/a".into(),
            },
        );
        state.pane_sessions.insert(
            2,
            PaneSession {
                conversation_id: "b".into(),
                draft: "draft2".into(),
                path: "/b".into(),
            },
        );
        state.pane_runtime.insert(
            1,
            PaneRuntime {
                messages: vec![ChatMessage::from_markdown(ChatRole::User, "m1")],
                pending_ask: None,
                queued_prompt: None,
                busy: false,
                ..PaneRuntime::default()
            },
        );
        state.pane_runtime.insert(
            2,
            PaneRuntime {
                messages: vec![ChatMessage::from_markdown(ChatRole::User, "m2")],
                pending_ask: None,
                queued_prompt: None,
                busy: true,
                ..PaneRuntime::default()
            },
        );

        state.active_pane_id = 1;
        state.sync_active_view();
        assert_eq!(state.selected_id.as_deref(), Some("a"));
        assert_eq!(state.composer_draft, "draft1");
        assert_eq!(state.composer_path, "/a");
        assert_eq!(state.chat_messages.len(), 1);
        assert!(!state.agent_busy);

        state.active_pane_id = 2;
        state.sync_active_view();
        assert_eq!(state.selected_id.as_deref(), Some("b"));
        assert_eq!(state.composer_draft, "draft2");
        assert_eq!(state.composer_path, "/b");
        assert!(state.agent_busy);
    }
