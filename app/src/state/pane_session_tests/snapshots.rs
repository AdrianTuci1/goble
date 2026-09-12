use super::*;

    #[test]
    fn pane_chat_snapshot_has_distinct_per_pane_data() {
        let mut state = UiState::mock();
        state.pane_sessions.insert(
            1,
            PaneSession {
                conversation_id: "a".into(),
                draft: "d1".into(),
                path: "/a".into(),
            },
        );
        state.pane_sessions.insert(
            2,
            PaneSession {
                conversation_id: "b".into(),
                draft: "d2".into(),
                path: "/b".into(),
            },
        );
        state.pane_runtime.insert(
            1,
            PaneRuntime {
                messages: vec![ChatMessage::from_markdown(ChatRole::Assistant, "x")],
                pending_ask: None,
                queued_prompt: None,
                busy: false,
                ..PaneRuntime::default()
            },
        );
        state.pane_runtime.insert(
            2,
            PaneRuntime {
                messages: Vec::new(),
                pending_ask: None,
                queued_prompt: None,
                busy: false,
                ..PaneRuntime::default()
            },
        );
        let snap = state.pane_chat_snapshot();
        assert_eq!(snap.get(&1).map(|s| s.conversation_id.as_str()), Some("a"));
        assert_eq!(snap.get(&1).unwrap().messages.len(), 1);
        assert_eq!(snap.get(&1).unwrap().composer_path, "/a");
        assert_eq!(snap.get(&2).unwrap().composer_path, "/b");
        // Distinct draft/path per pane => independent sessions.
        assert_eq!(snap.get(&1).unwrap().composer_draft, "d1");
        assert_eq!(snap.get(&2).unwrap().composer_draft, "d2");
    }

    #[test]
    fn pane_controls_are_isolated_per_pane() {
        let mut state = UiState::mock();
        state.active_pane_id = 1;
        state.selected_model = "global-model".into();
        {
            let c1 = state.pane_controls_mut(1);
            c1.model = "m1".into();
            c1.auto_approve = true;
            c1.harness_mode = true;
            let flag = c1.model_menu_open.clone();
            *flag.borrow_mut() = true;
        }
        {
            let c2 = state.pane_controls_mut(2);
            c2.model = "m2".into();
        }
        // Two pty/agent panes in one workspace never share the composer state.
        assert_eq!(state.pane_controls(1).model, "m1");
        assert_eq!(state.pane_controls(2).model, "m2");
        assert!(state.pane_controls(1).auto_approve);
        assert!(!state.pane_controls(2).auto_approve);
        assert!(state.pane_controls(1).harness_mode);
        assert!(!state.pane_controls(2).harness_mode);
        let menu_1 = state.pane_controls(1).model_menu_open;
        let menu_2 = state.pane_controls(2).model_menu_open;
        assert!(*menu_1.borrow(), "pane 1's model menu is open");
        assert!(!*menu_2.borrow(), "pane 2's model menu stays closed");
        // A pane with no entry yet reads the window globals.
        assert_eq!(state.pane_controls(99).model, "global-model");
        // Every rendered pane gets an entry when the snapshot is prepared.
        state.spaces = vec![Space::new(
            "A",
            Pane::Split {
                id: 10,
                dir: crate::ui::SplitDir::Vertical,
                ratio: 0.5,
                first: Box::new(Pane::Leaf { id: 1, kind: PaneKind::Terminal }),
                second: Box::new(Pane::Leaf { id: 2, kind: PaneKind::Terminal }),
            },
        )];
        state.ensure_pane_controls();
        assert!(state.pane_controls.contains_key(&1));
        assert!(state.pane_controls.contains_key(&2));
    }

    #[test]
    fn refresh_reparses_only_the_changed_message() {
        let dir = tempfile::tempdir().expect("create temp thread store dir");
        let desktop = DesktopState::new(
            goble_core::store::Store::open_in_memory().expect("open in-memory store"),
            goble_desktop_service::ThreadStore::new(dir.path()).expect("open thread store"),
        );
        let chat_id = desktop
            .create_chat("Stream", None, None)
            .expect("create chat");
        let store = desktop.store_clone();
        let now = "2026-09-10T00:00:00Z";
        store
            .insert_chat_message("m1", &chat_id, "user", "hello", None, now)
            .expect("insert user row");
        store
            .insert_chat_message("m2", &chat_id, "assistant", "first ", None, now)
            .expect("insert assistant row");

        let mut state = UiState::mock();
        state.pane_sessions.get_mut(&1).unwrap().conversation_id = chat_id.clone();
        state.selected_id = Some(chat_id.clone());
        let parses = |s: &UiState| s.pane_runtime.get(&1).unwrap().parse_cache.parse_count();

        state.refresh_messages(&desktop);
        assert_eq!(parses(&state), 2, "the first load parses every row once");
        assert_eq!(state.pane_runtime.get(&1).unwrap().messages.len(), 2);

        // A frame with no new delta must re-parse nothing.
        state.refresh_messages(&desktop);
        assert_eq!(parses(&state), 2, "an unchanged refresh parses nothing");

        // One delta lands on the last row; only that row is re-parsed.
        store
            .append_chat_message_content("m2", "delta")
            .expect("append delta");
        state.refresh_messages(&desktop);
        assert_eq!(parses(&state), 3, "only the changed row is re-parsed");

        let msgs = &state.pane_runtime.get(&1).unwrap().messages;
        assert_eq!(msgs.len(), 2, "both rows are still in the transcript");
        assert_eq!(
            msgs[0].fragments,
            ChatMessage::from_markdown(ChatRole::User, "hello").fragments
        );
        assert_eq!(
            msgs[1].fragments,
            ChatMessage::from_markdown(ChatRole::Assistant, "first delta").fragments,
            "the delta is reflected in the re-parsed message"
        );
    }
