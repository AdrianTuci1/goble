use super::*;

    /// A persisted tool result is folded into the call it belongs to, not drawn
    /// as a segment of its own. The agent's call and its output are one
    /// continuous row in its reply; a terminal block — with its header, its copy
    /// button and its filter — belongs to a command the *user* ran, which the
    /// pane builds, and none is built here.
    #[test]
    fn a_persisted_tool_result_folds_into_the_call_it_belongs_to() {
        use goble_ui::elements::chat_content::ChatFragmentKind;

        let command = "printf '\\033[31mred\\033[0m\\n'";
        let output = "\u{1b}[31mred\u{1b}[0m";
        let arguments = serde_json::json!({ "command": command }).to_string();

        let assistant = goble_desktop_service::ChatMessage {
            id: "m1".into(),
            role: "assistant".into(),
            content: String::new(),
            tool_calls: Some(format!(
                r#"[{{"id":"call_1","name":"run_command","arguments":{arguments},"status":"finished"}}]"#
            )),
            created_at: "2026-09-10T00:00:00Z".into(),
        };
        let result = goble_desktop_service::ChatMessage {
            id: "m2".into(),
            role: "tool".into(),
            content: format!("call_1\n{output}"),
            tool_calls: None,
            created_at: "2026-09-10T00:00:01Z".into(),
        };

        let messages = MessageParseCache::default().resolve(&[assistant, result]);

        assert_eq!(
            messages.len(),
            1,
            "the result row is not a message of its own"
        );
        let calls = &messages[0].tool_calls;
        assert_eq!(calls.len(), 1, "the assistant row keeps its one call");
        assert_eq!(
            calls[0].result.as_deref(),
            Some(output),
            "the row's output is the call's result"
        );
        assert!(
            !messages
                .iter()
                .flat_map(|message| message.fragments.iter())
                .any(|fragment| matches!(fragment.kind, ChatFragmentKind::Terminal(_))),
            "no terminal block is built on the agent's path"
        );
    }

    /// A result row whose call the transcript does not carry still becomes a
    /// message — the output is real and must be readable — but it is drawn as
    /// the agent's own prose surface, never as a terminal block.
    #[test]
    fn an_orphan_tool_result_row_is_not_a_terminal_block() {
        use goble_ui::elements::chat_content::ChatFragmentKind;

        let orphan = goble_desktop_service::ChatMessage {
            id: "m9".into(),
            role: "tool".into(),
            content: "call_missing\nsome output".to_string(),
            tool_calls: None,
            created_at: "2026-09-10T00:00:00Z".into(),
        };

        let messages = MessageParseCache::default().resolve(&[orphan]);

        assert_eq!(messages.len(), 1);
        assert!(
            !matches!(messages[0].fragments[0].kind, ChatFragmentKind::Terminal(_)),
            "an orphan result row is not a terminal block, got {:?}",
            messages[0].fragments[0]
        );
        let ChatFragmentKind::CodeBlock { code, .. } = &messages[0].fragments[0].kind else {
            panic!(
                "an orphan result row draws its output, got {:?}",
                messages[0].fragments[0]
            );
        };
        assert_eq!(code, "some output", "the row's body is what it draws");
        assert!(
            !code.contains("call_missing"),
            "the call id line is not part of the body"
        );
    }

    /// A call whose result the store already carries keeps it: a result row that
    /// arrives later (or repeats) does not overwrite what the call recorded.
    #[test]
    fn a_folded_result_never_overwrites_the_calls_own_result() {
        let assistant = goble_desktop_service::ChatMessage {
            id: "m1".into(),
            role: "assistant".into(),
            content: String::new(),
            tool_calls: Some(
                r#"[{"id":"call_1","name":"run_command","arguments":{"command":"ls"},"status":"finished","result":"from the call"}]"#
                    .to_string(),
            ),
            created_at: "2026-09-10T00:00:00Z".into(),
        };
        let result = goble_desktop_service::ChatMessage {
            id: "m2".into(),
            role: "tool".into(),
            content: "call_1\nfrom the result row".to_string(),
            tool_calls: None,
            created_at: "2026-09-10T00:00:01Z".into(),
        };

        let messages = MessageParseCache::default().resolve(&[assistant, result]);
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].tool_calls[0].result.as_deref(),
            Some("from the call"),
            "the call's own recorded result wins"
        );
    }

    #[test]
    fn reasoning_events_reach_the_transcript_and_render_recessed_rows() {
        use goble_desktop_service::{ReasoningEvent, ReasoningPhase};
        use goble_ui::elements::AppContext;
        use goble_ui::render::RenderCommand;
        use goble_ui::test_util::render_element;
        use goble_ui::theme::ColorToken;
        use goble_ui::{ChatView, Element};

        let event = |phase, step, mode: &str, delta: &str, content: Option<&str>| ReasoningEvent {
            chat_id: "c1".into(),
            step,
            mode: mode.to_string(),
            delta: delta.to_string(),
            content: content.map(str::to_string),
            decision: None,
            phase,
        };

        let mut state = UiState::mock();
        state.apply_reasoning_event(&event(
            ReasoningPhase::Started,
            Some(0),
            "contemplating",
            "",
            None,
        ));
        state.apply_reasoning_event(&event(
            ReasoningPhase::Delta,
            None,
            "",
            "weighing options",
            None,
        ));
        state.apply_reasoning_event(&event(
            ReasoningPhase::Done,
            Some(0),
            "contemplating",
            "",
            Some("weighing options"),
        ));

        let rows = &state.pane_runtime.get(&1).unwrap().reasoning;
        assert_eq!(
            rows.len(),
            1,
            "the deltas accumulate into one reasoning step"
        );
        assert_eq!(rows[0].text, "weighing options");
        assert!(rows[0].done, "the done transition finalises the step");

        let messages = state.pane_runtime.get(&1).unwrap().messages.clone();
        assert!(
            messages
                .iter()
                .flat_map(|m| m.fragments.iter())
                .any(|f| matches!(
                    &f.kind,
                    goble_ui::ChatFragmentKind::Reasoning { text, .. } if text == "weighing options"
                )),
            "the reasoning step is overlaid on the transcript"
        );

        let app = AppContext::default();
        let render = |state: &UiState| -> Vec<RenderCommand> {
            let messages = state.pane_runtime.get(&1).unwrap().messages.clone();
            let mut view: Box<dyn goble_ui::Element> = ChatView::new()
                .with_messages(messages)
                .with_reasoning_expanded(state.reasoning_expanded.clone())
                .finish();
            render_element(&mut view, goble_ui::vec2f(600.0, 400.0), &app)
        };
        let has_text = |commands: &[RenderCommand], needle: &str| {
            commands
                .iter()
                .any(|c| matches!(c, RenderCommand::DrawText { text, .. } if text.contains(needle)))
        };

        let collapsed = render(&state);
        assert!(
            has_text(&collapsed, "Thinking"),
            "the reasoning header renders"
        );
        assert!(
            !has_text(&collapsed, "weighing options"),
            "the thinking body is collapsed by default"
        );
        let header_color = collapsed.iter().find_map(|c| match c {
            RenderCommand::DrawText { text, color, .. } if text.contains("Thinking") => {
                Some(*color)
            }
            _ => None,
        });
        assert_eq!(
            header_color,
            Some(app.theme.color(ColorToken::Muted)),
            "the reasoning row is recessed (muted)"
        );

        // Expanding the app-owned row shows the thinking body.
        state
            .reasoning_expanded
            .borrow_mut()
            .insert(reasoning_row_key("c1", 0), true);
        let expanded = render(&state);
        assert!(
            has_text(&expanded, "weighing options"),
            "an expanded reasoning row shows its body"
        );
    }

    /// A tool call's three-state fold is owned by the app, so a rebuilt
    /// transcript reads the fold the user chose instead of resetting to the
    /// collapsed default.
    #[test]
    fn a_tool_call_fold_survives_a_transcript_rebuild() {
        use goble_ui::elements::AppContext;
        use goble_ui::geometry::vec2f;
        use goble_ui::render::RenderCommand;
        use goble_ui::test_util::render_element;
        use goble_ui::{tool_fold_key, ChatView, Element, ToolCall, ToolDisplayMode};

        let app = AppContext::default();
        let call = ToolCall {
            id: "call_read".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path":"src/lib.rs"}"#.to_string(),
            status: ToolCallStatus::Finished,
            result: Some(
                (1..=12)
                    .map(|i| format!("line {i}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        };
        let messages =
            vec![ChatMessage::new(ChatRole::Assistant, Vec::new())
                .with_tool_calls(vec![call.clone()])];
        let state = UiState::mock();

        let render = |state: &UiState| -> Vec<RenderCommand> {
            let mut view: Box<dyn Element> = ChatView::new()
                .with_messages(messages.clone())
                .with_tool_fold(state.tool_fold.clone())
                .finish();
            render_element(&mut view, vec2f(600.0, 400.0), &app)
        };
        let has_text = |commands: &[RenderCommand], needle: &str| {
            // A highlighted read body arrives as several runs, so the needle is
            // matched against the whole drawn text, not one run.
            let drawn: String = commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            drawn.contains(needle)
        };

        let collapsed = render(&state);
        assert!(
            has_text(&collapsed, "src/lib.rs"),
            "a read starts folded, showing its path"
        );
        assert!(
            !has_text(&collapsed, "line 1"),
            "the body is hidden while folded"
        );

        // The fold the user chose (as `e` or a header click would set it) lives
        // in the app; a brand-new view over the same state reads it back.
        state
            .tool_fold
            .borrow_mut()
            .insert(tool_fold_key(&call, 0), ToolDisplayMode::Expanded);
        let expanded = render(&state);
        assert!(
            has_text(&expanded, "line 7"),
            "the rebuilt transcript reads the app-owned fold"
        );
    }
