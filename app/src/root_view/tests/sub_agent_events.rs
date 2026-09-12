use super::*;

    /// S5's acceptance for the wire: the three `chat:subagent_*` events go
    /// through the real drain path and land as the parent transcript's row —
    /// the row's data is the live record, never the spawn call's arguments.
    #[test]
    fn sub_agent_events_reach_the_parents_transcript_row() {
        let (mut root, bus, _dir) = root_with_bus();
        root.state_rc()
            .borrow_mut()
            .pane_sessions
            .get_mut(&1)
            .expect("the default pane session exists")
            .conversation_id = "conv-1".to_string();

        bus.emit(
            "chat:subagent_spawned",
            serde_json::json!({
                "chat_id": "conv-1",
                "subagent_id": "conv-child-1",
                "subagent_type": "reviewer",
                "description": "audit the migration",
                "parent_call_id": "call_spawn",
                "run_in_background": true,
            }),
        );
        bus.emit(
            "chat:subagent_progress",
            serde_json::json!({
                "chat_id": "conv-1",
                "subagent_id": "conv-child-1",
                "status": "running",
                "activity": "reading the schema",
                "turns": 2,
                "tool_calls": 4,
                "tokens": 1234,
                "duration_ms": 4500,
            }),
        );
        root.drain_events();
        let snapshot = root.state_rc().borrow().pane_chat_snapshot();
        let running = snapshot[&1]
            .sub_agents
            .get("call_spawn")
            .expect("the spawn call carries its row")
            .clone();
        assert_eq!(
            (
                running.status,
                running.activity.as_str(),
                running.elapsed,
                running.description.as_str()
            ),
            (
                goble_ui::SubAgentRowStatus::Running,
                "reading the schema",
                std::time::Duration::from_millis(4500),
                "audit the migration"
            ),
            "the running row carries the activity and the elapsed time from the event"
        );

        bus.emit(
            "chat:subagent_finished",
            serde_json::json!({
                "chat_id": "conv-1",
                "subagent_id": "conv-child-1",
                "status": "completed",
                "output": "the migration is safe",
                "duration_ms": 43000,
                "turns": 3,
                "tool_calls": 5,
                "tokens": 2000,
            }),
        );
        root.drain_events();
        let snapshot = root.state_rc().borrow().pane_chat_snapshot();
        let finished = snapshot[&1]
            .sub_agents
            .get("call_spawn")
            .expect("the row survives the child finishing")
            .clone();
        assert_eq!(
            (
                finished.status,
                finished.elapsed,
                finished.outcome.as_deref()
            ),
            (
                goble_ui::SubAgentRowStatus::Completed,
                std::time::Duration::from_secs(43),
                Some("the migration is safe")
            ),
            "the finish settles the row's status, duration and outcome"
        );
    }
