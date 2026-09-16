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

    /// What a turn on a pane would run on: the pane's own model first, then the
    /// window-global selection, then the configured default.
    #[test]
    fn a_panes_own_model_wins_over_the_windows_selection() {
        let mut state = UiState::mock();
        state.selected_model = "pane-choice".to_string();
        state.settings_llm_model = "configured".to_string();
        state.ensure_pane_controls();
        assert_eq!(state.pane_agent_model(1), "pane-choice");

        // A later window-wide pick does not move a pane that already chose.
        state.selected_model = "another".to_string();
        assert_eq!(state.pane_agent_model(1), "pane-choice");

        // A pane that never chose follows the window selection, and with no
        // selection at all the configured default is what a turn would use.
        assert_eq!(state.pane_agent_model(2), "another");
        state.selected_model.clear();
        assert_eq!(state.pane_agent_model(2), "configured");
    }

    /// A turn runs only with a key, a provider and a model. Anything missing
    /// means no conversation is created and nothing is written: the composer's
    /// notice band names what is missing instead.
    #[test]
    fn a_turn_runs_only_with_a_key_a_provider_and_a_model() {
        let mut state = UiState::mock();
        state.selected_model = "mock".to_string();
        state.settings_llm_provider = "openai".to_string();
        state.settings_llm_api_key = String::new();
        assert!(!state.can_run_agent_turn(1), "no key: nothing can answer");
        assert_eq!(state.llm_notice_heading(), "No API key configured");

        state.settings_llm_api_key = "sk-test".to_string();
        assert!(state.can_run_agent_turn(1));

        state.settings_llm_provider.clear();
        assert!(
            !state.can_run_agent_turn(1),
            "a key with no provider cannot run either"
        );
        assert_eq!(state.llm_notice_heading(), "No model configured");

        state.settings_llm_provider = "openai".to_string();
        state.selected_model.clear();
        state.settings_llm_model.clear();
        assert!(!state.can_run_agent_turn(1), "a key with no model cannot run");
    }

    /// A focus that moves to another pane ends the expansion: warp-new's rule
    /// (`pane_group/focus_state.rs` clears its maximized pane whenever the
    /// focused pane changes), so a pane that gave up the focus cannot keep the
    /// whole space. Focusing the pane that is already expanded leaves it alone,
    /// which is what makes the retract control a toggle rather than a way to
    /// re-expand.
    #[test]
    fn focusing_another_pane_ends_the_expansion() {
        let mut state = UiState::mock();
        state.active_pane_id = 1;
        state.maximized_pane = Some(1);

        state.focus_pane(1);
        assert_eq!(state.active_pane_id, 1);
        assert_eq!(
            state.maximized_pane,
            Some(1),
            "the pane that is already the expanded one keeps the space"
        );

        state.focus_pane(2);
        assert_eq!(state.active_pane_id, 2, "the focus moved");
        assert_eq!(
            state.maximized_pane, None,
            "and the pane it left cannot keep the whole space"
        );
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
