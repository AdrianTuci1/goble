use super::*;

    /// The acceptance for C1: a scripted sequence of live events leaves the one
    /// accessor reporting exactly the work still in flight and nothing else.
    #[test]
    fn drain_events_leaves_the_live_accessor_reporting_what_is_in_flight() {
        let (mut root, bus, _dir) = root_with_bus();
        // Bind the active pane to a known conversation so the `chat:*` payloads
        // route to it.
        root.state_rc()
            .borrow_mut()
            .pane_sessions
            .get_mut(&1)
            .expect("the default pane session exists")
            .conversation_id = "conv-1".to_string();

        // A tool call starts, the turn it belonged to finishes, two worker
        // agents start (a1 then a2), a1 finishes, a2 reports its runtime state
        // and a tool result, and the executions page refreshes.
        bus.emit(
            "chat:tool",
            serde_json::json!({
                "chat_id": "conv-1",
                "id": "call_1",
                "name": "read_file",
                "arguments": {"path": "src/lib.rs"},
                "status": "running",
            }),
        );
        bus.emit(
            "chat:turn_finished",
            serde_json::json!({ "chat_id": "conv-1" }),
        );
        bus.emit(
            "agent:started",
            serde_json::json!({
                "worker_id": "worker-a",
                "trace_id": "trace-1",
                "agent_id": "agent-1",
                "started_at": "2026-09-11T10:00:00Z",
            }),
        );
        bus.emit(
            "agent:started",
            serde_json::json!({
                "worker_id": "worker-b",
                "trace_id": "trace-2",
                "agent_id": "agent-2",
                "started_at": "2026-09-11T10:00:01Z",
            }),
        );
        bus.emit(
            "agent:finished",
            serde_json::json!({
                "worker_id": "worker-a",
                "trace_id": "trace-1",
                "status": "Completed",
            }),
        );
        bus.emit(
            "agent:state_update",
            serde_json::json!({
                "worker_id": "worker-b",
                "trace_id": "trace-2",
                "state": {"version": 1, "checklist": [], "notes": ["working"], "self_feedback": []},
            }),
        );
        bus.emit(
            "agent:tool_result",
            serde_json::json!({
                "worker_id": "worker-b",
                "trace_id": "trace-2",
                "step_id": "step-1",
                "name": "run_command",
                "result": "ok",
            }),
        );
        bus.emit("executions:updated", serde_json::Value::Null);

        root.drain_events();

        let state = root.state_rc();
        let work = state.borrow().live_work();

        assert!(!work.turn_busy, "the finished turn is no longer busy");
        assert!(
            work.turn_started_at.is_none(),
            "a turn whose start was never observed reports no start time"
        );
        assert_eq!(
            work.tool_calls
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            vec!["call_1"],
            "the running call with no terminal event is still in flight"
        );
        assert!(work.pending_approval.is_none());
        assert!(work.pending_question.is_none());

        assert_eq!(work.executions.len(), 1, "only trace-2 is still running");
        let execution = &work.executions[0];
        assert_eq!(execution.trace_id, "trace-2");
        assert_eq!(execution.agent_id, "agent-2");
        assert_eq!(execution.worker_id, "worker-b");
        assert_eq!(execution.status, "running");
        assert_eq!(
            execution.started_at, "2026-09-11T10:00:01Z",
            "the start time is the service's own, not the app's arrival time"
        );
        assert!(
            execution.runtime_state.is_some(),
            "agent:state_update reached the running execution"
        );
        assert!(
            execution.last_activity.is_some(),
            "agent:tool_result reached the running execution"
        );
    }
