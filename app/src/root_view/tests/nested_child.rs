use super::*;

    /// S6's nested case: a child that spawned a child of its own. The row drawn
    /// inside the child view must be enterable like the parent's row is — a
    /// sub-agent is enterable, not merely visible — and leaving the nested view
    /// must land back on the conversation the first row was clicked in, so no
    /// conversation is stranded. Clicks and the key go through the mounted root.
    #[test]
    fn a_nested_child_row_inside_the_child_view_is_enterable_and_esc_returns_to_the_parent() {
        use crate::emulator::Emulator;
        use crate::terminal::TerminalSession;
        use goble_terminal::blocks::BlockView;
        use goble_ui::geometry::vec2f;
        use goble_ui::render::RenderCommand;
        use goble_ui::test_util::render_element;

        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let now = "2026-09-11T10:00:00Z".to_string();
        let parent = desktop
            .create_chat("Parent conversation", None, None)
            .expect("parent chat");
        desktop
            .add_chat_message(&parent, "user", "PARENT TRANSCRIPT MARKER")
            .expect("parent row");
        // The parent's transcript carries the spawn call whose row is clicked
        // first, and the child's own transcript carries a spawn call of its own.
        {
            let store = desktop.store_clone();
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
                .expect("parent spawn row");
            store
                .insert_chat_message(
                    "row-nested-spawn",
                    "conv-child-1",
                    "assistant",
                    "delegating a check",
                    Some(
                        r#"[{"id":"call_nested","name":"spawn_subagent","arguments":{"subagent_type":"explorer","description":"check the schema"},"status":"finished","result":"the schema checks out"}]"#,
                    ),
                    &now,
                )
                .expect("nested spawn row");
            // Two generations of children, each with its own conversation under
            // its own id (`S2`): the first owned by the parent, the second by
            // the first.
            for (id, title, parent_chat) in [
                ("conv-child-1", "reviewer: audit the migration", parent.as_str()),
                ("conv-child-2", "explorer: check the schema", "conv-child-1"),
            ] {
                store
                    .insert_subagent_chat(id, title, parent_chat, None, None, &now, &now)
                    .expect("child chat");
            }
        }
        desktop
            .add_chat_message("conv-child-1", "assistant", "CHILD CONVERSATION MARKER")
            .expect("child row");
        desktop
            .add_chat_message("conv-child-2", "assistant", "NESTED CONVERSATION MARKER")
            .expect("nested child row");

        let root = RootView::new(&AppContext::default(), &desktop, None);
        let state = root.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.settings_overlay_open = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.pane_sessions
                .get_mut(&1)
                .expect("the default pane session exists")
                .conversation_id = parent.clone();
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
            s.apply_subagent_spawned(&goble_desktop_service::SubAgentSpawnedEvent {
                chat_id: "conv-child-1".into(),
                subagent_id: "conv-child-2".into(),
                subagent_type: "explorer".into(),
                description: "check the schema".into(),
                parent_call_id: "call_nested".into(),
                run_in_background: true,
            });
            s.refresh_messages(&desktop);
        }

        let app = AppContext::default();
        let mut root: Box<dyn Element> = Box::new(root);
        // Everything the pane draws, excluding the sidebar (which quotes the
        // selected conversation's last response).
        fn pane_texts(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<String> {
            render_element(root, vec2f(1024.0, 768.0), app)
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
        }
        fn click_run(root: &mut Box<dyn Element>, app: &AppContext, needle: &str) {
            let commands = render_element(root, vec2f(1024.0, 768.0), app);
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
                root.dispatch_event(&event, &mut ctx, app);
            }
        }
        fn press_escape(root: &mut Box<dyn Element>, app: &AppContext) -> bool {
            let mut ctx = EventContext::default();
            root.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: "Escape".to_string(),
                    modifiers: goble_ui::event::ModifiersState::none(),
                },
                &mut ctx,
                app,
            )
        }
        let view_of = |state: &std::cell::RefCell<UiState>| state.borrow().pane_view(1);
        let cards = |state: &std::cell::RefCell<UiState>| -> Vec<String> {
            state
                .borrow()
                .pane_terminal_view(1)
                .into_iter()
                .filter_map(|block| block.card.map(|card| card.conversation_id))
                .collect()
        };

        // Trip one: the parent's row opens its child.
        click_run(&mut root, &app, "open child");
        assert_eq!(
            view_of(&state),
            BlockView::Agent {
                conversation_id: "conv-child-1".to_string()
            },
            "the pane switches to the first child"
        );
        let texts = pane_texts(&mut root, &app);
        assert!(
            texts
                .iter()
                .any(|t| t.contains("CHILD CONVERSATION MARKER")),
            "the first child's conversation is drawn: {texts:?}"
        );
        // The nested row lives in the child's own transcript, so the child view
        // has to hand the pane's live records to the renderer for it to be
        // enterable — without the record there is no open target at all.
        assert!(
            texts.iter().any(|t| t == "open child"),
            "the child's own sub-agent row is enterable from inside the child \
             view: {texts:?}"
        );

        // Trip two: that row opens the nested child's conversation.
        click_run(&mut root, &app, "open child");
        assert_eq!(
            view_of(&state),
            BlockView::Agent {
                conversation_id: "conv-child-2".to_string()
            },
            "the pane switches to the nested child"
        );
        let texts = pane_texts(&mut root, &app);
        assert!(
            texts
                .iter()
                .any(|t| t.contains("NESTED CONVERSATION MARKER")),
            "the nested child's persisted messages are drawn: {texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|t| t.contains("CHILD CONVERSATION MARKER")),
            "the first child's transcript is off screen while the nested one shows: {texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t == "explorer · check the schema"),
            "the title names the nested child: {texts:?}"
        );
        assert_eq!(
            cards(&state),
            vec!["conv-child-1".to_string(), "conv-child-2".to_string()],
            "both children keep a card, so neither conversation is stranded"
        );

        // Esc returns to the conversation the first row was clicked in, not to
        // the intermediate child the user passed through.
        assert!(
            press_escape(&mut root, &app),
            "the nested child view takes Escape"
        );
        assert_eq!(
            view_of(&state),
            BlockView::Terminal,
            "the pane's filter is back where the round trip started"
        );
        let texts = pane_texts(&mut root, &app);
        assert!(
            texts.iter().any(|t| t.contains("PARENT TRANSCRIPT MARKER")),
            "the parent's transcript is back: {texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|t| t.contains("NESTED CONVERSATION MARKER")),
            "and the nested child's transcript left with its view: {texts:?}"
        );
    }
