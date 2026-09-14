    use super::*;
    use goble_ui::elements::chat_content::ChatFragment;

    #[test]
    fn detect_rdp_uri_with_trailing_text() {
        assert_eq!(
            detect_screen_link("open rdp://192.168.1.5:3389 now").as_deref(),
            Some("rdp://192.168.1.5:3389")
        );
    }

    #[test]
    fn detect_rdp_uri_trims_trailing_punctuation() {
        assert_eq!(
            detect_screen_link("Take over: rdp://host:3389.").as_deref(),
            Some("rdp://host:3389")
        );
    }

    #[test]
    fn detect_goble_desktop_uri() {
        assert_eq!(
            detect_screen_link("go to goble://desktop?user=me&host=h&port=5900 end")
                .as_deref(),
            Some("goble://desktop?user=me&host=h&port=5900")
        );
    }

    #[test]
    fn detect_screen_link_ignores_plain_text() {
        assert_eq!(detect_screen_link("no link here"), None);
        assert_eq!(detect_screen_link(""), None);
    }

    #[test]
    fn detect_screen_link_survives_long_prefix() {
        // A link after a long prose prefix is still detected (the URI length
        // must not be compared against its byte offset).
        let prefix: String = "a".repeat(40);
        let text = format!("{prefix} rdp://host:3389");
        assert_eq!(detect_screen_link(&text).as_deref(), Some("rdp://host:3389"));
    }

    #[test]
    fn screen_source_from_goble_query() {
        assert_eq!(
            screen_source_from_link("goble://desktop?user=me&host=myhost&port=5900").as_deref(),
            Some("me@myhost:5900")
        );
        assert_eq!(
            screen_source_from_link("goble://desktop?host=myhost&port=5900").as_deref(),
            Some("myhost:5900")
        );
        assert_eq!(
            screen_source_from_link("goble://desktop?host=myhost").as_deref(),
            Some("myhost")
        );
    }

    #[test]
    fn screen_source_from_rdp() {
        assert_eq!(
            screen_source_from_link("rdp://user@host:3389").as_deref(),
            Some("host:3389")
        );
        assert_eq!(screen_source_from_link("rdp://host").as_deref(), Some("host"));
    }

    #[test]
    fn screen_source_from_unknown_scheme_is_none() {
        assert_eq!(screen_source_from_link("https://example.com"), None);
    }

    #[test]
    fn message_screen_link_scans_assistant_output_only() {
        let assistant = ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Handing off: rdp://host:3389")],
        );
        assert_eq!(
            message_screen_link(&[assistant]).as_deref(),
            Some("rdp://host:3389")
        );
    }

    #[test]
    fn message_screen_link_ignores_user_messages() {
        let user = ChatMessage::new(
            ChatRole::User,
            vec![ChatFragment::text("use rdp://host:3389 please")],
        );
        assert_eq!(message_screen_link(&[user]), None);
    }

    #[test]
    fn message_screen_link_prefers_most_recent_assistant() {
        let older = ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("see rdp://oldhost:3389")],
        );
        let newer = ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("now rdp://newhost:3389")],
        );
        assert_eq!(
            message_screen_link(&[older, newer]).as_deref(),
            Some("rdp://newhost:3389")
        );
    }

    #[test]
    fn pane_chat_snapshot_surfaces_screen_link_from_message() {
        let mut state = UiState::mock();
        state.pane_sessions.insert(
            1,
            PaneSession {
                conversation_id: "a".into(),
                draft: "d1".into(),
                path: "/a".into(),
            },
        );
        state.pane_runtime.insert(
            1,
            PaneRuntime {
                messages: vec![ChatMessage::new(
                    ChatRole::Assistant,
                    vec![ChatFragment::text("desktop ready: goble://desktop?user=me&host=h&port=5900")],
                )],
                pending_ask: None,
                queued_prompt: None,
                busy: false,
                ..PaneRuntime::default()
            },
        );
        let snap = state.pane_chat_snapshot();
        assert_eq!(
            snap.get(&1).and_then(|s| s.screen_link.as_deref()),
            Some("goble://desktop?user=me&host=h&port=5900")
        );
        assert!(snap.get(&1).unwrap().inline_screen.is_none());
    }
