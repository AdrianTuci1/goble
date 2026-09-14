use super::*;

    /// S6's acceptance: entering from a sub-agent's transcript row shows the
    /// child's own conversation with the messages the store holds under the
    /// child's chat id, Esc returns to the parent's transcript, and the round
    /// trip can be made twice. The click and the key go through the mounted
    /// root, so the row, the relay, the state and the rebuilt frame are the
    /// app's own.
    #[test]
    fn entering_a_sub_agent_row_shows_the_childs_conversation_and_esc_returns() {
        use crate::emulator::Emulator;
        use crate::terminal::TerminalSession;
        use goble_desktop_service::{SubAgentFinishedEvent, SubAgentProgressEvent};
        use goble_terminal::blocks::BlockView;
        use goble_ui::event::ModifiersState;
        use goble_ui::geometry::vec2f;
        use goble_ui::render::RenderCommand;
        use goble_ui::test_util::render_element;

        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        // The parent's conversation: a line of its own, and the spawn call whose
        // row the user clicks.
        let parent = desktop
            .create_chat("Parent conversation", None, None)
            .expect("parent chat");
        desktop
            .add_chat_message(&parent, "user", "PARENT TRANSCRIPT MARKER")
            .expect("parent row");
        let store = desktop.store_clone();
        let now = "2026-09-11T10:00:00Z".to_string();
        store
            .insert_chat_message(
                "row-spawn",
                &parent,
                "assistant",
                "spawning a reviewer",
                Some(
                    r#"[{"id":"call_spawn","name":"spawn_subagent","arguments":{"subagent_type":"reviewer","description":"audit the migration"},"status":"finished","result":"the migration is safe"}]"#,
                ),
                &now,
            )
            .expect("spawn row");
        // The child's own conversation (S2): `parent_chat_id` set, and its rows
        // under its own id — one assistant line and one tool result, which the
        // transcript draws as a terminal block.
        store
            .insert_subagent_chat(
                "conv-child-1",
                "reviewer: audit the migration",
                &parent,
                None,
                None,
                &now,
                &now,
            )
            .expect("child chat");
        drop(store);
        desktop
            .add_chat_message("conv-child-1", "assistant", "CHILD CONVERSATION MARKER")
            .expect("child row");
        desktop
            .add_chat_message("conv-child-1", "tool", "call_child_1\nmigration is safe")
            .expect("child tool row");

        let root = RootView::new(&AppContext::default(), &desktop, None);
        let state = root.state_rc();
        {
            let mut s = state.borrow_mut();
            // No overlay may sit above the transcript and swallow the input.
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.settings_overlay_open = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.pane_sessions
                .get_mut(&1)
                .expect("the default pane session exists")
                .conversation_id = parent.clone();
            // A live session (detached: no pty) so the child's card has a block
            // list to be pushed into, exactly as A7 requires.
            s.terminal
                .borrow_mut()
                .sessions
                .insert(1, TerminalSession::with_emulator(Emulator::new(80, 24)));
            s.apply_subagent_spawned(&goble_desktop_service::SubAgentSpawnedEvent {
                chat_id: parent.clone(),
                subagent_id: "conv-child-1".into(),
                subagent_type: "reviewer".into(),
                description: "audit the migration".into(),
                parent_call_id: "call_spawn".into(),
                run_in_background: true,
            });
            s.apply_subagent_progress(&SubAgentProgressEvent {
                chat_id: parent.clone(),
                subagent_id: "conv-child-1".into(),
                status: "running".into(),
                activity: "reading the schema".into(),
                turns: 2,
                tool_calls: 4,
                tokens: 1234,
                duration_ms: 4500,
            });
            s.refresh_messages(&desktop);
        }

        let app = AppContext::default();
        let mut root: Box<dyn Element> = Box::new(root);
        // The pane's own area: the sidebar names the parent conversation and
        // quotes its last response, so the transcript is only what is drawn to
        // the right of it.
        let pane_texts = |root: &mut Box<dyn Element>| -> Vec<String> {
            render_element(root, vec2f(1024.0, 768.0), &app)
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::DrawText { origin, text, .. }
                        if origin.x >= crate::ui::SIDEBAR_WIDTH =>
                    {
                        Some(text.clone())
                    }
                    _ => None,
                })
                .collect()
        };
        let click_run = |root: &mut Box<dyn Element>, needle: &str| {
            let commands = render_element(root, vec2f(1024.0, 768.0), &app);
            let origin = commands
                .iter()
                .find_map(|c| match c {
                    RenderCommand::DrawText { origin, text, .. }
                        if text == needle && origin.x >= crate::ui::SIDEBAR_WIDTH =>
                    {
                        Some(*origin)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("the pane draws the {needle:?} run"));
            let position = origin + vec2f(2.0, 4.0);
            let mut ctx = EventContext::default();
            for event in [
                DispatchedEvent::MouseDown {
                    position,
                    button: 0,
                },
                DispatchedEvent::MouseUp {
                    position,
                    button: 0,
                },
            ] {
                root.dispatch_event(&event, &mut ctx, &app);
            }
        };
        let press_escape = |root: &mut Box<dyn Element>| {
            let mut ctx = EventContext::default();
            root.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: "Escape".to_string(),
                    modifiers: ModifiersState::none(),
                },
                &mut ctx,
                &app,
            )
        };
        let child_cards = |state: &std::cell::RefCell<UiState>| -> Vec<String> {
            state
                .borrow()
                .pane_terminal_view(1)
                .into_iter()
                .filter_map(|block| block.card.map(|card| card.conversation_id))
                .collect()
        };

        // The parent's transcript leads, and the child's rows are not in it.
        let texts = pane_texts(&mut root);
        assert!(
            texts.iter().any(|t| t.contains("PARENT TRANSCRIPT MARKER")),
            "the parent's transcript is on screen: {texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|t| t.contains("CHILD CONVERSATION MARKER")),
            "the child's conversation is not on screen yet: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "open child"),
            "the row carries the target that enters the child: {texts:?}"
        );

        // Trip one: the row's click shows the child's own conversation.
        click_run(&mut root, "open child");
        assert_eq!(
            state.borrow().pane_view(1),
            BlockView::Agent {
                conversation_id: "conv-child-1".to_string()
            },
            "entering the child switches the pane's filter to the child"
        );
        assert_eq!(
            child_cards(&state),
            vec!["conv-child-1".to_string()],
            "the child's card is pushed into the pane's block list"
        );
        let texts = pane_texts(&mut root);
        assert!(
            texts
                .iter()
                .any(|t| t.contains("CHILD CONVERSATION MARKER")),
            "the child's persisted messages are drawn: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("migration is safe")),
            "the child's tool output is drawn through the terminal block: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("PARENT TRANSCRIPT MARKER")),
            "the parent's transcript is off screen while the child is open: {texts:?}"
        );
        // The framed title: type, description, status and elapsed time.
        assert!(
            texts.iter().any(|t| t == "reviewer · audit the migration"),
            "the title names the child's type and description: {texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t == "running · reading the schema · 4.5s"),
            "the title carries the record's status, activity and elapsed time: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "Esc to return"),
            "the title says how to get back: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("Ask anything")),
            "the child's view has no composer of its own: {texts:?}"
        );

        // Esc returns to the parent's transcript, leaving the child's card.
        assert!(press_escape(&mut root), "the child view takes Escape");
        assert_eq!(
            state.borrow().pane_view(1),
            BlockView::Terminal,
            "Esc returns the pane to the shell it came from"
        );
        assert_eq!(
            child_cards(&state),
            vec!["conv-child-1".to_string()],
            "the child's card stays in the block list"
        );
        let texts = pane_texts(&mut root);
        assert!(
            texts.iter().any(|t| t.contains("PARENT TRANSCRIPT MARKER")),
            "the parent's transcript is back: {texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|t| t.contains("CHILD CONVERSATION MARKER")),
            "the child's transcript leaves with the child view: {texts:?}"
        );

        // The child ends while the parent's transcript is showing, so trip two
        // titles the view from a finished record.
        state
            .borrow_mut()
            .apply_subagent_finished(&SubAgentFinishedEvent {
                chat_id: parent.clone(),
                subagent_id: "conv-child-1".into(),
                status: "completed".into(),
                output: Some("the migration is safe".into()),
                error: None,
                duration_ms: 12_000,
                turns: 3,
                tool_calls: 5,
                tokens: 2048,
            });

        // Trip two: the same round trip again, through the same row.
        click_run(&mut root, "open child");
        let texts = pane_texts(&mut root);
        assert!(
            texts
                .iter()
                .any(|t| t.contains("CHILD CONVERSATION MARKER")),
            "the second trip shows the child's conversation again: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "completed in 12s"),
            "the title follows the record's new status: {texts:?}"
        );
        assert_eq!(
            child_cards(&state),
            vec!["conv-child-1".to_string()],
            "a second entry reuses the child's card rather than stacking another"
        );
        assert!(press_escape(&mut root), "the second trip takes Escape too");
        let texts = pane_texts(&mut root);
        assert!(
            texts.iter().any(|t| t.contains("PARENT TRANSCRIPT MARKER")),
            "and the parent's transcript is back again: {texts:?}"
        );
    }
