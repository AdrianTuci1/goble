use super::*;


    #[test]
    fn mock_pane_is_bound_to_its_own_conversation() {
        let state = UiState::mock();
        let session = state.pane_sessions.get(&1).expect("pane 1 has a session");
        assert_eq!(session.conversation_id, "c1");
        assert_eq!(state.pane_conversation_id(1).as_deref(), Some("c1"));
    }

    /// The two views start as the shell's own history: the preamble block is in
    /// the terminal view, and no conversation has a block until the agent runs
    /// one in this pane.
    #[test]
    fn the_pane_views_start_as_the_shells_own_history() {
        let state = UiState::mock();
        state
            .terminal
            .borrow_mut()
            .sessions
            .insert(1, TerminalSession::with_emulator(Emulator::new(80, 24)));

        let terminal = state.pane_terminal_view(1);
        assert_eq!(terminal.len(), 1, "the preamble block is shell history");
        assert!(terminal[0].command.is_empty());
        assert!(
            state.pane_agent_view(1, "c1").is_empty(),
            "no conversation has run a command in this pane"
        );
        assert!(
            state.pane_terminal_view(9).is_empty(),
            "a pane with no session has no view"
        );
    }

    /// S6's collision with A7's one-conversation-at-a-time filter, resolved:
    /// entering a child from *inside* the parent's own agent view switches the
    /// pane to the child, and leaving it puts the pane back on the parent's agent
    /// view — not on the shell, and not on a conversation with no way back.
    #[test]
    fn entering_a_child_from_the_parents_agent_view_returns_to_the_parent() {
        let mut state = UiState::mock();
        state
            .terminal
            .borrow_mut()
            .sessions
            .insert(1, TerminalSession::with_emulator(Emulator::new(80, 24)));
        state
            .enter_agent_view(1, "c1", "Parent conversation")
            .expect("the parent's card has a block list to go into");

        state.open_sub_agent(1, "conv-child-1", None);
        assert_eq!(
            state.pane_view(1),
            BlockView::Agent {
                conversation_id: "conv-child-1".to_string()
            },
            "the pane switches to the child's conversation"
        );
        assert!(state.close_sub_agent_view(1), "Esc leaves the child view");
        assert_eq!(
            state.pane_view(1),
            BlockView::Agent {
                conversation_id: "c1".to_string()
            },
            "and lands back on the parent's agent view, not on the shell"
        );
        let cards: Vec<String> = state
            .pane_terminal_view(1)
            .into_iter()
            .filter_map(|block| block.card.map(|card| card.conversation_id))
            .collect();
        assert_eq!(
            cards,
            vec!["c1".to_string(), "conv-child-1".to_string()],
            "both conversations keep a card in the terminal, so either is reachable again"
        );
        assert!(
            !state.close_sub_agent_view(1),
            "with no child view open Esc has nothing to leave"
        );
    }

    /// A child view belongs to the conversation its row was clicked in, so
    /// binding the pane to another conversation unmounts it rather than letting
    /// a child's composer-less transcript hide the newly-selected conversation.
    #[test]
    fn switching_conversation_unmounts_the_child_view() {
        let mut state = UiState::mock();
        state.open_sub_agent(1, "conv-child-1", None);
        assert_eq!(
            state.pane_view(1),
            BlockView::Agent {
                conversation_id: "conv-child-1".to_string()
            },
            "the pane is filtered to the child"
        );

        state.bind_active_pane_conversation("c2".to_string(), None);

        assert!(
            state.sub_agent_view(1).is_none(),
            "the child view does not follow the pane into another conversation"
        );
        assert_eq!(
            state.pane_view(1),
            BlockView::Terminal,
            "and the pane is back at the shell, showing the new conversation"
        );
    }
