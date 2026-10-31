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

    /// The harness writes one assistant row per execution step: the preamble
    /// with its calls, then a tool-only row per step. Drawn row by row that is
    /// one bubble per step; the chain merges into the reply that opened it, so
    /// the run header counts the whole chain and the prose stays on top.
    #[test]
    fn chained_tool_steps_merge_into_the_reply_that_opened_them() {
        let call = |id: &str, path: &str| {
            format!(
                r#"{{"id":"{id}","name":"read_file","arguments":{{"path":"{path}"}},"status":"finished"}}"#
            )
        };
        let row = |id: &str, content: &str, calls: Option<String>| {
            goble_desktop_service::ChatMessage {
                id: id.into(),
                role: "assistant".into(),
                content: content.into(),
                tool_calls: calls,
                created_at: "2026-09-10T00:00:00Z".into(),
            }
        };
        let rows = vec![
            row("m1", "Checking the module.", Some(format!("[{}]", call("call_1", "src/a.rs")))),
            row("m2", "", Some(format!("[{}]", call("call_2", "src/b.rs")))),
            row("m3", "", Some(format!("[{}]", call("call_3", "src/c.rs")))),
            row("m4", "All three are clean.", None),
        ];

        let messages = MessageParseCache::default().resolve(&rows);

        assert_eq!(
            messages.len(),
            2,
            "the tool-only steps are not bubbles of their own"
        );
        assert_eq!(
            messages[0].tool_calls.len(),
            3,
            "the chain's calls all belong to the reply that opened it"
        );
        assert!(
            messages[0]
                .fragments
                .iter()
                .any(|fragment| matches!(&fragment.kind, goble_ui::ChatFragmentKind::Text(text) if text.contains("Checking the module."))),
            "the opening prose leads the merged reply"
        );
        assert!(
            messages[1]
                .fragments
                .iter()
                .any(|fragment| matches!(&fragment.kind, goble_ui::ChatFragmentKind::Text(text) if text.contains("All three are clean."))),
            "the final answer stays its own message"
        );
    }

    /// A step that carries prose of its own starts a new reply: the next
    /// paragraph is where grok-build's agent message ends the run too.
    #[test]
    fn a_step_with_its_own_prose_starts_a_new_reply() {
        let row = |id: &str, content: &str, call_id: &str| goble_desktop_service::ChatMessage {
            id: id.into(),
            role: "assistant".into(),
            content: content.into(),
            tool_calls: Some(format!(
                r#"[{{"id":"{call_id}","name":"read_file","arguments":{{"path":"src/a.rs"}},"status":"finished"}}]"#
            )),
            created_at: "2026-09-10T00:00:00Z".into(),
        };
        let rows = vec![
            row("m1", "First I read it.", "call_1"),
            row("m2", "Then I read it again.", "call_2"),
        ];

        let messages = MessageParseCache::default().resolve(&rows);

        assert_eq!(messages.len(), 2, "prose owns its own reply");
        assert_eq!(messages[1].tool_calls.len(), 1);
    }

    /// The thinking block belongs to the turn's start: the harness plans before
    /// it executes, so the block sits right after the user's message instead of
    /// riding the assistant reply down under the tools it drove.
    #[test]
    fn the_reasoning_block_sits_at_the_start_of_the_turn() {
        use crate::state::transcript::{overlay_reasoning, reasoning_row_key};

        let mut messages = vec![
            ChatMessage::new(ChatRole::User, vec![ChatFragment::text("fix the test")]),
            ChatMessage::new(ChatRole::Assistant, vec![ChatFragment::text("Done.")]),
        ];
        let rows = vec![ReasoningRow {
            chat_id: "c1".into(),
            step: 0,
            mode: "contemplating".into(),
            text: "weighing the options".into(),
            done: true,
        }];

        overlay_reasoning(&mut messages, &rows);

        assert_eq!(messages.len(), 3, "one block is added");
        assert_eq!(messages[0].role, ChatRole::User);
        assert!(
            crate::state::transcript::is_reasoning_overlay(&messages[1]),
            "the thinking block follows the user's message"
        );
        let expected = reasoning_row_key("c1", 0);
        assert!(
            matches!(
                &messages[1].fragments[0].kind,
                goble_ui::ChatFragmentKind::Reasoning { key, text, .. }
                    if key == &expected && text == "weighing the options"
            ),
            "the block carries the app-owned key its fold is stored under"
        );
        assert!(
            messages[2]
                .fragments
                .iter()
                .any(|fragment| matches!(&fragment.kind, goble_ui::ChatFragmentKind::Text(text) if text.contains("Done."))),
            "the answer stays after the thinking"
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
            has_text(&collapsed, "Thought"),
            "the finished reasoning header renders"
        );
        assert!(
            !has_text(&collapsed, "weighing options"),
            "the thinking body is collapsed by default"
        );
        let header_color = collapsed.iter().find_map(|c| match c {
            RenderCommand::DrawText { text, color, .. } if text.contains("Thought") => {
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
            diff: None,
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
            has_text(&collapsed, "Read 1 file"),
            "a read starts folded under its run header"
        );
        assert!(
            !has_text(&collapsed, "src/lib.rs"),
            "the folded run hides the call's own row"
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

    /// The harness's own lookups (`search_store`, `credentials`,
    /// `user_guide`) never reach the transcript: their calls are dropped, the
    /// result folded into them goes with them, and a step that carried nothing
    /// else is not left as an empty reply.
    #[test]
    fn the_harnesss_internal_lookups_are_not_drawn() {
        let calls = serde_json::json!([
            {"id": "c1", "name": "credentials", "arguments": {"name": "openai"}, "status": "finished"},
            {"id": "c2", "name": "read_file", "arguments": {"path": "a.rs"}, "status": "finished"},
        ])
        .to_string();
        let assistant = goble_desktop_service::ChatMessage {
            id: "m1".into(),
            role: "assistant".into(),
            content: String::new(),
            tool_calls: Some(calls),
            created_at: "2026-09-10T00:00:00Z".into(),
        };
        let hidden_result = goble_desktop_service::ChatMessage {
            id: "m2".into(),
            role: "tool".into(),
            content: "c1\nsecret-key-name".into(),
            tool_calls: None,
            created_at: "2026-09-10T00:00:01Z".into(),
        };

        let messages = MessageParseCache::default().resolve(&[assistant, hidden_result]);

        assert_eq!(messages.len(), 1, "the reply is still one message");
        let calls = &messages[0].tool_calls;
        assert_eq!(calls.len(), 1, "only the visible call is kept");
        assert_eq!(calls[0].name, "read_file");
        assert!(
            !messages
                .iter()
                .flat_map(|message| message.fragments.iter())
                .any(|fragment| format!("{:?}", fragment.kind).contains("secret-key-name")),
            "the hidden call's output is not drawn as a row of its own"
        );

        // A step that carried nothing but a hidden call is not an empty bubble.
        let only_hidden = goble_desktop_service::ChatMessage {
            id: "m3".into(),
            role: "assistant".into(),
            content: String::new(),
            tool_calls: Some(
                serde_json::json!([
                    {"id": "c3", "name": "user_guide", "arguments": {"topic": "x"}, "status": "finished"}
                ])
                .to_string(),
            ),
            created_at: "2026-09-10T00:00:02Z".into(),
        };
        let messages = MessageParseCache::default().resolve(&[only_hidden]);
        assert!(
            messages.is_empty(),
            "a step with nothing to show is not a message: {messages:?}"
        );
    }
