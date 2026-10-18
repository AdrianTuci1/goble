use super::*;

use crate::state::PaneSession;
use crate::ui::{Pane, PaneKind, Space};

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

    /// A turn's events belong to the pane that started it, not to whichever
    /// pane has focus when they arrive. Two panes each own a conversation; the
    /// turn starts on A, the focus moves to B, and the events A's harness
    /// reports must leave B untouched.
    #[test]
    fn chat_events_follow_the_pane_that_started_the_turn_after_focus_moves() {
        let (mut root, bus, _dir) = root_with_bus();
        let desktop = root.desktop.clone().expect("the root has a desktop");
        let conversation_a = desktop.create_chat("A", None, None).expect("create A");
        let conversation_b = desktop.create_chat("B", None, None).expect("create B");
        // The question is in the store, as the harness persists it, so the
        // refresh that follows its event keeps the card standing.
        desktop
            .store_clone()
            .insert_pending_ask(
                "ask-a",
                &conversation_a,
                None,
                "Deploy now?",
                r#"["yes","no"]"#,
                "pending",
                "2026-10-18T00:00:00Z",
                "2026-10-18T00:00:00Z",
            )
            .expect("persist the pending question");

        {
            let state = root.state_rc();
            let mut s = state.borrow_mut();
            s.spaces = vec![
                Space::new("A", Pane::Leaf { id: 1, kind: PaneKind::Chat }),
                Space::new("B", Pane::Leaf { id: 2, kind: PaneKind::Chat }),
            ];
            s.pane_sessions.insert(
                2,
                PaneSession {
                    conversation_id: conversation_b.clone(),
                    path: "/b".to_string(),
                    ..Default::default()
                },
            );
            s.pane_sessions
                .get_mut(&1)
                .expect("pane A has a session")
                .conversation_id = conversation_a.clone();
            // The turn starts on A...
            s.begin_turn(1);
            // ...and the user moves the focus to B.
            s.pane_runtime.entry(2).or_default().agent_state =
                crate::state::PaneAgentState::Idle;
            s.active_space = 1;
            s.active_pane_id = 2;
        }

        bus.emit(
            "chat:command_proposed",
            serde_json::json!({
                "chat_id": conversation_a,
                "id": "call-a",
                "candidates": ["git status"],
                "cwd": "/workspace",
            }),
        );
        bus.emit(
            "chat:ask_user",
            serde_json::json!({
                "chat_id": conversation_a,
                "question": "Deploy now?",
                "quick_replies": ["yes", "no"],
            }),
        );
        bus.emit(
            "chat:turn_finished",
            serde_json::json!({ "chat_id": conversation_a, "status": "success" }),
        );

        root.drain_events();

        let state = root.state_rc();
        let s = state.borrow();
        assert_eq!(
            s.pane_id_for_conversation(&conversation_a),
            Some(1),
            "A's conversation is bound to A"
        );
        assert_eq!(
            s.pane_agent_state(1),
            crate::state::PaneAgentState::NeedsInput,
            "A is waiting on the approval and the question it reported"
        );
        let a = s.pane_runtime.get(&1).expect("A's runtime");
        assert!(a.pending_command.is_some(), "A holds its approval");
        assert!(a.pending_ask.is_some(), "A holds its question");
        assert!(!a.busy, "the finished turn settled on A");

        assert_eq!(
            s.pane_agent_state(2),
            crate::state::PaneAgentState::Idle,
            "B's state is exactly what it was before A's events arrived"
        );
        let b = s.pane_runtime.get(&2).expect("B's runtime");
        assert!(b.pending_command.is_none(), "B has no approval");
        assert!(b.pending_ask.is_none(), "B has no question");
        assert!(b.messages.is_empty(), "B's transcript is untouched");

        assert_eq!(s.inbox.len(), 2, "the approval and the question were raised");
        assert!(
            s.inbox.iter().all(|item| item.pane_id == 1),
            "both inbox items belong to A"
        );
        assert!(
            s.inbox
                .iter()
                .all(|item| item.conversation_id == conversation_a),
            "and both name A's conversation"
        );
    }
