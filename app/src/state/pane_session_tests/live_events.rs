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
                        arguments: serde_json::json!({ "command": "cargo test" }).to_string(),
                    },
                    "a running tool is drawn as the row its family gives it"
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

    /// The text the live footer draws for `status`, through the widget the app
    /// mounts with it.
    fn drawn_footer_texts(status: TurnStatus) -> Vec<String> {
        use goble_ui::elements::AppContext;
        use goble_ui::render::RenderCommand;
        use goble_ui::test_util::render_element;
        use goble_ui::{Element, TurnStatusFooter};

        let mut element: Box<dyn Element> = TurnStatusFooter::new(status).finish();
        render_element(
            &mut element,
            goble_ui::vec2f(600.0, 40.0),
            &AppContext::default(),
        )
        .into_iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { text, .. } => Some(text),
            _ => None,
        })
        .collect()
    }

    /// A busy turn whose prompt has been sent and whose model has not started
    /// streaming yet is waiting for the response — "Waiting for response…" on
    /// the line, not a Thinking the model is not doing.
    #[test]
    fn the_footer_waits_for_the_response_until_the_model_streams() {
        let mut state = UiState::mock();
        state.begin_turn(1);
        assert!(
            state.pane_runtime.get(&1).unwrap().reasoning.is_empty(),
            "the turn has produced no reasoning row yet"
        );

        let status = state.pane_chat_snapshot().get(&1).unwrap().turn_status.clone();
        assert!(
            matches!(
                status,
                TurnStatus::Busy {
                    activity: TurnActivity::WaitingForResponse,
                    ..
                }
            ),
            "no reasoning row yet is the wait for the response: {status:?}"
        );
        assert!(
            drawn_footer_texts(status)
                .iter()
                .any(|t| t == "Waiting for response…"),
            "the footer draws the wait for the response"
        );
    }

    /// The model's own phase is read from the reasoning rows this turn has
    /// produced: an open newest row is thinking, a closed one is the model
    /// writing its answer.
    #[test]
    fn the_footer_reads_thinking_and_responding_from_the_reasoning_rows() {
        use goble_desktop_service::{ReasoningEvent, ReasoningPhase};

        let event = |phase, step, mode: &str, delta: &str, content: Option<&str>| ReasoningEvent {
            chat_id: "c1".into(),
            step,
            mode: mode.to_string(),
            delta: delta.to_string(),
            content: content.map(str::to_string),
            decision: None,
            phase,
        };
        let activity = |state: &UiState| match state
            .pane_chat_snapshot()
            .get(&1)
            .unwrap()
            .turn_status
            .clone()
        {
            TurnStatus::Busy { activity, .. } => activity,
            other => panic!("a busy turn is Busy, got {other:?}"),
        };

        let mut state = UiState::mock();
        state.begin_turn(1);

        state.apply_reasoning_event(&event(
            ReasoningPhase::Started,
            Some(0),
            "contemplating",
            "",
            None,
        ));
        assert_eq!(
            activity(&state),
            TurnActivity::Thinking,
            "an open reasoning row is the model thinking"
        );

        state.apply_reasoning_event(&event(
            ReasoningPhase::Done,
            Some(0),
            "contemplating",
            "",
            Some("weighing options"),
        ));
        assert_eq!(
            activity(&state),
            TurnActivity::Responding,
            "the closed row means the model is writing its answer"
        );

        state.apply_reasoning_event(&event(
            ReasoningPhase::Started,
            Some(1),
            "contemplating",
            "",
            None,
        ));
        assert_eq!(
            activity(&state),
            TurnActivity::Thinking,
            "the newest row's state is the one the footer reports"
        );
    }

    /// A turn that spawned a child and is awaiting it names the child, not the
    /// spawn call: the call's own row would read `create_agent`, which says
    /// nothing about what the pane is waiting on.
    #[test]
    fn the_footer_waits_on_the_child_the_spawn_call_is_awaiting() {
        let spawned = |run_in_background: bool| goble_desktop_service::SubAgentSpawnedEvent {
            chat_id: "c1".into(),
            subagent_id: "conv-child-1".into(),
            subagent_type: "reviewer".into(),
            description: "audit the migration".into(),
            parent_call_id: "call_spawn_1".into(),
            run_in_background,
        };
        let awaiting = |state: &UiState| {
            let status = state
                .pane_chat_snapshot()
                .get(&1)
                .unwrap()
                .turn_status
                .clone();
            match status {
                TurnStatus::Busy { activity, .. } => activity,
                other => panic!("a busy turn is Busy, got {other:?}"),
            }
        };

        let mut state = UiState::mock();
        state.apply_tool_event(&goble_desktop_service::ToolCallEvent {
            chat_id: "c1".into(),
            id: "call_spawn_1".into(),
            name: "create_agent".into(),
            arguments: serde_json::json!({ "description": "audit the migration" }),
            status: ToolCallStatus::Running,
            result: None,
        });
        state.begin_turn(1);

        // The call is in flight before the child reports itself: the wait is
        // named without a record to read it from.
        assert_eq!(
            awaiting(&state),
            TurnActivity::WaitingOnSubAgent {
                description: None,
                activity: None,
            },
            "a spawn call awaiting a child that has not reported is the plain wait"
        );

        state.apply_subagent_spawned(&spawned(false));
        let activity = awaiting(&state);
        assert_eq!(
            activity,
            TurnActivity::WaitingOnSubAgent {
                description: Some("audit the migration".into()),
                activity: Some("initializing".into()),
            },
            "the child's own record names the wait once it exists"
        );
        assert!(
            drawn_footer_texts(state.pane_chat_snapshot().get(&1).unwrap().turn_status.clone())
                .iter()
                .any(|t| t == "audit the migration: initializing…"),
            "the footer draws the child and what it is doing"
        );
    }

    /// The still-running line counts the pane's live sub-agent children beside
    /// its commands and the workspace's executions; a child that has ended is
    /// not work in flight and does not appear.
    #[test]
    fn the_footer_names_a_running_sub_agent_and_ignores_an_ended_one() {
        let spawned = |child_id: &str, parent_call_id: &str| {
            goble_desktop_service::SubAgentSpawnedEvent {
                chat_id: "c1".into(),
                subagent_id: child_id.into(),
                subagent_type: "reviewer".into(),
                description: "audit the migration".into(),
                parent_call_id: parent_call_id.into(),
                run_in_background: true,
            }
        };
        let finished = |child_id: &str| goble_desktop_service::SubAgentFinishedEvent {
            chat_id: "c1".into(),
            subagent_id: child_id.into(),
            status: "completed".into(),
            output: Some("audit clean".into()),
            error: None,
            duration_ms: 12000,
            turns: 3,
            tool_calls: 5,
            tokens: 2000,
        };

        let mut state = UiState::mock();
        for (id, name) in [("t1", "run_command"), ("t2", "read_file")] {
            state.apply_tool_event(&goble_desktop_service::ToolCallEvent {
                chat_id: "c1".into(),
                id: id.into(),
                name: name.into(),
                arguments: serde_json::json!({ "command": "cargo test" }),
                status: ToolCallStatus::Running,
                result: None,
            });
        }
        state.apply_agent_started("worker-a", "trace-1", "agent-1", "2026-09-11T10:00:00Z");
        state.apply_subagent_spawned(&spawned("conv-child-1", "call_spawn_1"));
        state.apply_subagent_spawned(&spawned("conv-child-2", "call_spawn_2"));
        state.apply_subagent_finished(&finished("conv-child-2"));

        let status = state.pane_chat_snapshot().get(&1).unwrap().turn_status.clone();
        assert_eq!(
            status,
            TurnStatus::StillRunning {
                kinds: vec![
                    WorkKindCount {
                        kind: WorkKind::Command,
                        count: 2,
                    },
                    WorkKindCount {
                        kind: WorkKind::Execution,
                        count: 1,
                    },
                    WorkKindCount {
                        kind: WorkKind::SubAgent,
                        count: 1,
                    },
                ]
            },
            "the kinds read Command, Execution, SubAgent, and the ended child is not one"
        );
        let line = drawn_footer_texts(status)
            .into_iter()
            .find(|t| t.contains("still running"))
            .expect("the still-running line is drawn");
        assert_eq!(
            line,
            "2 commands still running · 1 execution still running · 1 sub-agent still running"
        );

        // A pane whose only child has ended has nothing in flight.
        let mut ended = UiState::mock();
        ended.apply_subagent_spawned(&spawned("conv-child-1", "call_spawn_1"));
        ended.apply_subagent_finished(&finished("conv-child-1"));
        assert_eq!(
            ended
                .pane_chat_snapshot()
                .get(&1)
                .unwrap()
                .turn_status,
            TurnStatus::Idle,
            "a completed child leaves the footer at zero height"
        );
    }

    /// A tool result's status is its call's persisted status, not a prefix in
    /// the result text: an errored call keeps its error, and a finished call
    /// whose printed text happens to say `ERROR:` is not turned into one. The
    /// status is read where the row is drawn — the mark's colour on the call's
    /// own row — and never inferred from the output.
    #[test]
    fn a_tool_result_reads_its_status_from_the_call_record() {
        use goble_core::harness::ToolCallStatus;

        let assistant = |status: &str| goble_desktop_service::ChatMessage {
            id: "m1".into(),
            role: "assistant".into(),
            content: String::new(),
            tool_calls: Some(format!(
                r#"[{{"id":"call_1","name":"run_command","arguments":{{}},"status":"{status}"}}]"#
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
            assert_eq!(messages.len(), 1, "the result row folds into its call");
            messages[0].tool_calls[0].status
        };

        assert_eq!(
            status_of(&[assistant("error"), result.clone()]),
            ToolCallStatus::Error,
            "the error status is read from the call record"
        );
        assert_eq!(
            status_of(&[assistant("finished"), result.clone()]),
            ToolCallStatus::Finished,
            "a finished call is not turned into an error by its result text"
        );

        // The folded body is the result row's, and it keeps the text verbatim:
        // nothing here reads the status out of it.
        let messages = MessageParseCache::default().resolve(&[assistant("finished"), result]);
        assert_eq!(
            messages[0].tool_calls[0].result.as_deref(),
            Some("boom: ERROR: not found"),
            "the row's body is folded in verbatim"
        );
    }
