use super::*;

    #[test]
    fn live_tool_event_fills_and_clears_the_pane_in_flight_map() {
        let mut state = UiState::mock();
        let running = goble_desktop_service::ToolCallEvent {
            chat_id: "c1".into(),
            id: "t1".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({"path": "src/lib.rs"}),
            status: ToolCallStatus::Running,
            result: None,
        };
        state.apply_tool_event(&running);
        assert_eq!(
            state.live_work().tool_calls.len(),
            1,
            "a started call gains an in-flight entry for its pane"
        );
        let overlaid = state
            .pane_runtime
            .get(&1)
            .and_then(|rt| {
                rt.messages
                    .iter()
                    .flat_map(|m| m.tool_calls.iter())
                    .find(|c| c.id == "t1")
            });
        assert!(
            overlaid.is_some_and(|c| c.status == ToolCallStatus::Running),
            "the running call is overlaid on the transcript without a store re-read"
        );

        // A second call is tracked independently.
        state.apply_tool_event(&goble_desktop_service::ToolCallEvent {
            id: "t2".into(),
            name: "run_command".into(),
            ..running.clone()
        });
        assert_eq!(state.live_work().tool_calls.len(), 2);

        let finished = goble_desktop_service::ToolCallEvent {
            status: ToolCallStatus::Finished,
            result: Some("ok".into()),
            ..running.clone()
        };
        state.apply_tool_event(&finished);
        assert_eq!(
            state.live_work().tool_calls.len(),
            1,
            "a finished call clears its in-flight entry"
        );

        state.apply_tool_event(&goble_desktop_service::ToolCallEvent {
            status: ToolCallStatus::Error,
            result: Some("boom".into()),
            ..goble_desktop_service::ToolCallEvent {
                id: "t2".into(),
                ..running
            }
        });
        assert_eq!(
            state.live_work().tool_calls.len(),
            0,
            "an errored call clears its in-flight entry too"
        );
    }

    /// S5's row data: the live `chat:subagent_*` events open the child's record
    /// and move it, keyed by the parent tool call the transcript's row carries.
    #[test]
    fn sub_agent_events_feed_the_parents_rows() {
        let mut state = UiState::mock();
        state.apply_subagent_spawned(&goble_desktop_service::SubAgentSpawnedEvent {
            chat_id: "c1".into(),
            subagent_id: "conv-child-1".into(),
            subagent_type: "reviewer".into(),
            description: "audit the migration".into(),
            parent_call_id: "call_spawn".into(),
            run_in_background: true,
        });
        let rows = state.sub_agent_rows(1);
        assert_eq!(
            rows.get("call_spawn"),
            Some(&SubAgentRow {
                child_id: "conv-child-1".into(),
                subagent_type: "reviewer".into(),
                description: "audit the migration".into(),
                status: SubAgentRowStatus::Running,
                activity: String::new(),
                elapsed: Duration::ZERO,
                turns: 0,
                tool_calls: 0,
                tokens: 0,
                outcome: None,
                background: true,
            }),
            "the spawn opens the row the parent call's id resolves to"
        );

        state.apply_subagent_progress(&goble_desktop_service::SubAgentProgressEvent {
            chat_id: "c1".into(),
            subagent_id: "conv-child-1".into(),
            status: "running".into(),
            activity: "reading the schema".into(),
            turns: 2,
            tool_calls: 4,
            tokens: 1234,
            duration_ms: 4500,
        });
        let row = state.sub_agent_rows(1).remove("call_spawn").unwrap();
        assert_eq!(
            (
                row.status,
                row.activity.as_str(),
                row.elapsed,
                row.turns,
                row.tool_calls,
                row.tokens
            ),
            (
                SubAgentRowStatus::Running,
                "reading the schema",
                Duration::from_millis(4500),
                2,
                4,
                1234
            ),
            "progress moves the row's activity, counters and ticking elapsed time"
        );
        assert_eq!(
            state.pane_chat_snapshot()[&1]
                .sub_agents
                .get("call_spawn")
                .map(|r| r.activity.clone()),
            Some("reading the schema".to_string()),
            "the pane's snapshot carries the row to the transcript"
        );

        state.apply_subagent_finished(&goble_desktop_service::SubAgentFinishedEvent {
            chat_id: "c1".into(),
            subagent_id: "conv-child-1".into(),
            status: "failed".into(),
            output: None,
            error: Some("child turn failed".into()),
            duration_ms: 12000,
            turns: 3,
            tool_calls: 5,
            tokens: 2000,
        });
        let row = state.sub_agent_rows(1).remove("call_spawn").unwrap();
        assert_eq!(
            row.status,
            SubAgentRowStatus::Failed,
            "the finish settles the row's status"
        );
        assert_eq!(
            row.elapsed,
            Duration::from_secs(12),
            "the finish replaces the ticking clock with the child's duration"
        );
        assert_eq!(
            (row.outcome.as_deref(), row.activity.as_str()),
            (Some("child turn failed"), ""),
            "the finish carries the error and drops the activity label"
        );

        // The click's slot opens the child's own conversation in the pane (S6).
        state.open_sub_agent(1, "conv-child-1", None);
        let view = state
            .sub_agent_view(1)
            .expect("the row's open action enters the child view");
        assert_eq!(view.child_id, "conv-child-1");
        assert_eq!(
            view.row.as_ref().map(|r| r.status_line()),
            Some("failed in 12s · child turn failed".to_string()),
            "the child view carries the live record's row for its title"
        );
        assert!(state.close_sub_agent_view(1), "Esc leaves the child view");
        assert!(
            state.sub_agent_view(1).is_none(),
            "and the pane is back on its own conversation"
        );
    }

    /// The turn start is observed only when the app issues the turn, and it is
    /// dropped when the turn settles.
    #[test]
    fn begin_turn_records_the_start_and_finish_clears_it() {
        let mut state = UiState::mock();
        state.begin_turn(1);
        let work = state.live_work();
        assert!(work.turn_busy, "issuing a turn marks the pane busy");
        assert!(
            work.turn_started_at.is_some(),
            "the app observes the turn start it issued"
        );

        state.finish_turn(1);
        let work = state.live_work();
        assert!(!work.turn_busy, "a finished turn is no longer busy");
        assert!(
            work.turn_started_at.is_none(),
            "a finished turn keeps no start time"
        );
    }

    /// The accessor carries the pending approval and question so the chrome can
    /// say what the pane is waiting on, not only that it is busy.
    #[test]
    fn live_work_reports_pending_approvals_and_questions() {
        let mut state = UiState::mock();
        assert!(state.live_work().pending_approval.is_none());
        assert!(state.live_work().pending_question.is_none());

        state.apply_command_proposed(&goble_desktop_service::CommandProposedEvent {
            chat_id: "c1".into(),
            id: "call-9".into(),
            candidates: vec!["git status".into()],
            cwd: "/workspace".into(),
        });
        assert_eq!(
            state.live_work().pending_approval.as_deref(),
            Some("call-9")
        );

        if let Some(rt) = state.pane_runtime.get_mut(&1) {
            rt.pending_ask = Some(goble_ui::AskUserUi::new("Deploy?", vec![]));
        }
        assert_eq!(
            state.live_work().pending_question.as_deref(),
            Some("Deploy?")
        );
    }

    /// The footer's busy state reads the activity and the elapsed time from the
    /// pane's real live state: a running tool names its command, a pending
    /// approval names the wait, and the elapsed time comes from the observed
    /// turn start.
    #[test]
    fn the_footer_reads_the_panes_live_turn_status() {
        let mut state = UiState::mock();
        state.apply_tool_event(&goble_desktop_service::ToolCallEvent {
            chat_id: "c1".into(),
            id: "t1".into(),
            name: "run_command".into(),
            arguments: serde_json::json!({ "command": "cargo test" }),
            status: ToolCallStatus::Running,
            result: None,
        });
        state.begin_turn(1);

        let status = state.pane_chat_snapshot().get(&1).unwrap().turn_status.clone();
        match status {
            TurnStatus::Busy {
                activity,
                elapsed,
                work_count,
            } => {
                assert_eq!(
                    activity,
                    TurnActivity::Tool {
                        name: "run_command".into(),
                        command: Some("cargo test".into()),
                    },
                    "a running tool is drawn as its command"
                );
                assert!(
                    elapsed.is_some(),
                    "the observed turn start gives a real elapsed time"
                );
                assert_eq!(work_count, 1, "the running tool is the in-flight count");
            }
            other => panic!("a busy turn is Busy, got {other:?}"),
        }

        // A pending approval names the wait instead of the tool.
        state.apply_command_proposed(&goble_desktop_service::CommandProposedEvent {
            chat_id: "c1".into(),
            id: "call-9".into(),
            candidates: vec!["git status".into()],
            cwd: "/workspace".into(),
        });
        assert!(matches!(
            state.pane_chat_snapshot().get(&1).unwrap().turn_status,
            TurnStatus::Busy {
                activity: TurnActivity::WaitingOnApproval,
                ..
            }
        ));
    }

    /// The footer's idle state names each kind of work still in flight (the
    /// pane's calls and the workspace's running executions) and collapses to
    /// zero height when there is none.
    #[test]
    fn the_footer_names_the_kinds_still_running() {
        let mut state = UiState::mock();
        assert_eq!(
            state.pane_chat_snapshot().get(&1).unwrap().turn_status,
            TurnStatus::Idle,
            "nothing in flight is the zero-height state"
        );

        state.apply_agent_started("worker-a", "trace-1", "agent-1", "2026-09-11T10:00:00Z");
        assert_eq!(
            state.pane_chat_snapshot().get(&1).unwrap().turn_status,
            TurnStatus::StillRunning {
                kinds: vec![WorkKindCount {
                    kind: WorkKind::Execution,
                    count: 1,
                }]
            },
            "a running execution is named while the turn is idle"
        );

        state.apply_tool_event(&goble_desktop_service::ToolCallEvent {
            chat_id: "c1".into(),
            id: "t1".into(),
            name: "run_command".into(),
            arguments: serde_json::json!({ "command": "cargo test" }),
            status: ToolCallStatus::Running,
            result: None,
        });
        assert_eq!(
            state.pane_chat_snapshot().get(&1).unwrap().turn_status,
            TurnStatus::StillRunning {
                kinds: vec![
                    WorkKindCount {
                        kind: WorkKind::Command,
                        count: 1,
                    },
                    WorkKindCount {
                        kind: WorkKind::Execution,
                        count: 1,
                    },
                ]
            },
            "each kind is counted"
        );
    }

    /// A tool result's status is its call's persisted status, not a prefix in
    /// the result text: an errored call renders as an error, and a finished call
    /// whose printed text happens to say `ERROR:` still renders as a success.
    #[test]
    fn a_tool_result_reads_its_status_from_the_call_record() {
        use goble_ui::elements::chat_content::ChatFragmentKind;

        let assistant = |status: &str| goble_desktop_service::ChatMessage {
            id: "m1".into(),
            role: "assistant".into(),
            content: String::new(),
            tool_calls: Some(format!(
                r#"[{{"id":"call_1","name":"run_command","arguments":{{}},"status":"{status}","result":"boom"}}]"#
            )),
            created_at: "2026-09-10T00:00:00Z".into(),
        };
        let result = goble_desktop_service::ChatMessage {
            id: "m2".into(),
            role: "tool".into(),
            // The row names its call and carries the body; `ERROR:` here is
            // output text, not a status carrier.
            content: "call_1\nboom: ERROR: not found".into(),
            tool_calls: None,
            created_at: "2026-09-10T00:00:01Z".into(),
        };

        let status_of = |rows: &[goble_desktop_service::ChatMessage]| {
            let messages = MessageParseCache::default().resolve(rows);
            match &messages[1].fragments[0].kind {
                ChatFragmentKind::Terminal(data) => data.status,
                other => panic!("a tool row is a terminal block, got {other:?}"),
            }
        };

        assert_eq!(
            status_of(&[assistant("error"), result.clone()]),
            Some(TerminalStatus::Error),
            "the error status is read from the call record"
        );
        assert_eq!(
            status_of(&[assistant("finished"), result.clone()]),
            Some(TerminalStatus::Success),
            "a finished call is not turned into an error by its result text"
        );

        // A status that changes on the call row re-parses the result row it
        // belongs to, even though the result row's own text did not change.
        let mut cache = MessageParseCache::default();
        cache.resolve(&[assistant("error"), result.clone()]);
        assert_eq!(cache.parse_count(), 2);
        cache.resolve(&[assistant("finished"), result]);
        assert_eq!(
            cache.parse_count(),
            4,
            "a changed call status re-parses the result row it belongs to"
        );
    }
