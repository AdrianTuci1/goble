use super::*;

    #[test]
    fn a_command_proposal_holds_the_pane_and_finishing_frees_it() {
        let mut state = UiState::mock();
        state.active_pane_id = 1;
        if let Some(rt) = state.pane_runtime.get_mut(&1) {
            rt.busy = true;
        }
        let event = goble_desktop_service::CommandProposedEvent {
            chat_id: "c1".to_string(),
            id: "call-1".to_string(),
            candidates: vec!["git status".to_string()],
            cwd: "/workspace".to_string(),
        };
        state.apply_command_proposed(&event);

        let rt = state.pane_runtime.get(&1).unwrap();
        assert_eq!(
            rt.pending_command.as_ref().map(|p| p.id.as_str()),
            Some("call-1"),
            "the proposal is held on the pane"
        );
        assert!(rt.command_selection.is_some(), "the selection is app-owned");
        assert!(rt.busy, "a suspended command does not free the pane");
        assert!(
            state
                .pane_chat_snapshot()
                .get(&1)
                .unwrap()
                .pending_command
                .is_some(),
            "the composer's snapshot carries the proposal"
        );

        // Submitting a decision clears the card; the turn settles separately.
        state.clear_command_proposal(1, "call-1");
        assert!(state
            .pane_runtime
            .get(&1)
            .unwrap()
            .pending_command
            .is_none());

        // The finished turn frees the pane and drops any stale proposal.
        state.apply_command_proposed(&event);
        state.finish_turn(1);
        let rt = state.pane_runtime.get(&1).unwrap();
        assert!(!rt.busy, "the finished turn must not leave the pane busy");
        assert!(rt.pending_command.is_none());
    }

    #[test]
    fn rebinding_without_a_store_drops_the_pending_approval() {
        let mut state = UiState::mock();
        state.active_pane_id = 1;
        {
            let rt = state.pane_runtime.entry(1).or_default();
            rt.messages
                .push(ChatMessage::from_markdown(ChatRole::User, "stale"));
            rt.pending_command = Some(CommandProposalUi::new(
                "call-1".to_string(),
                vec!["git status".to_string()],
                "/workspace".to_string(),
            ));
            rt.command_selection = Some(Rc::new(RefCell::new(0)));
        }

        state.bind_active_pane_conversation("c2".to_string(), None);

        let rt = state.pane_runtime.get(&1).unwrap();
        assert!(
            rt.messages.is_empty(),
            "the store-less rebind clears the transcript"
        );
        assert!(
            rt.pending_command.is_none(),
            "the store-less rebind drops the pending approval"
        );
        assert!(
            rt.command_selection.is_none(),
            "the store-less rebind drops the approval's selected candidate"
        );
    }

    #[test]
    fn ensure_pane_sessions_adds_missing_chat_sessions() {
        let mut state = UiState::mock();
        state.spaces.push(Space::new(
            "S2",
            Pane::Leaf { id: 5, kind: PaneKind::Chat },
        ));
        state.pane_sessions.remove(&5);
        state.ensure_pane_sessions(None);
        assert!(
            state.pane_sessions.contains_key(&5),
            "a chat pane gets a session even if it was missing"
        );
    }

    #[test]
    fn set_active_pane_path_updates_pane_and_global() {
        let mut state = UiState::mock();
        state.active_pane_id = 1;
        state.set_active_pane_path("/workspace/proj".to_string());
        assert_eq!(state.pane_sessions.get(&1).unwrap().path, "/workspace/proj");
        assert_eq!(state.composer_path, "/workspace/proj");
    }
