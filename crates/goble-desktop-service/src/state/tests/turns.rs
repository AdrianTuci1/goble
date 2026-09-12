use std::sync::Arc;

use goble_core::harness::ToolCallStatus;

use crate::state::turns::build_chat_turn;

use super::{
    command_suspension, command_suspension_with_runner, tmp_state, tool_messages, wait_for_bus_event,
};

#[test]
fn run_chat_turn_unregistered_harness_errors() {
    // A harness id that was never registered must not silently fall back to
    // the internal harness; run_chat_turn surfaces the missing registration.
    let (_dir, state) = tmp_state();
    let chat_id = state.create_chat("Demo", None, None).expect("create chat");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let err = state.run_chat_turn(
        &chat_id,
        "hi",
        "mock",
        "",
        "local",
        "default",
        &chat_id,
        None,
        Some("nope"),
    );
    assert!(err.is_err(), "an unregistered harness must error, not run locally");
}

#[test]
fn command_proposed_is_emitted_as_chat_command_proposed() {
    // The harness suspension crosses the daemon translator as a structured
    // event the app can render, instead of being dropped at the adapter.
    let (_dir, state) = tmp_state();
    let bus = Arc::new(crate::event_bus::CollectingEventBus::new());
    state.set_event_bus(bus.clone());
    let sid = goble_harness_types::SessionId::new("chat-1");

    state.translate_daemon_event(goble_daemon_protocol::DaemonEvent::CommandProposed {
        session_id: sid.clone(),
        id: "call-1".into(),
        candidates: vec!["git status".into(), "git diff --stat".into()],
        cwd: "/workspace".into(),
    });

    let events: Vec<_> = bus
        .events()
        .into_iter()
        .filter(|(name, _)| name == "chat:command_proposed")
        .map(|(_, payload)| payload)
        .collect();
    assert_eq!(events.len(), 1, "one chat:command_proposed per proposal");
    assert_eq!(events[0]["chat_id"], "chat-1");
    assert_eq!(events[0]["id"], "call-1");
    assert_eq!(events[0]["candidates"][0], "git status");
    assert_eq!(events[0]["cwd"], "/workspace");
}

/// A child's whole lifecycle crosses the daemon translator: the scripted
/// spawned → progress → finished sequence reaches the app as the three
/// named `chat:subagent_*` events with every payload field intact.
#[test]
fn a_sub_agent_lifecycle_sequence_is_emitted_with_its_fields_intact() {
    use goble_daemon_protocol::DaemonEvent as DE;
    let (_dir, state) = tmp_state();
    let bus = Arc::new(crate::event_bus::CollectingEventBus::new());
    state.set_event_bus(bus.clone());
    let sid = goble_harness_types::SessionId::new("chat-1");

    state.translate_daemon_event(DE::SubAgentSpawned {
        session_id: sid.clone(),
        chat_id: "chat-1".into(),
        subagent_id: "child-1".into(),
        subagent_type: "reviewer".into(),
        description: "review the diff".into(),
        parent_call_id: "call-1".into(),
        run_in_background: true,
    });
    state.translate_daemon_event(DE::SubAgentProgress {
        session_id: sid.clone(),
        chat_id: "chat-1".into(),
        subagent_id: "child-1".into(),
        status: "running".into(),
        activity: "running read_file".into(),
        turns: 2,
        tool_calls: 1,
        tokens: 120,
        duration_ms: 340,
    });
    state.translate_daemon_event(DE::SubAgentFinished {
        session_id: sid.clone(),
        chat_id: "chat-1".into(),
        subagent_id: "child-1".into(),
        status: "completed".into(),
        output: Some("all good".into()),
        error: None,
        duration_ms: 900,
        turns: 3,
        tool_calls: 2,
        tokens: 480,
    });

    let collected = bus.events();
    let pick = |name: &str| -> Vec<serde_json::Value> {
        collected
            .iter()
            .filter(|(event, _)| event == name)
            .map(|(_, payload)| payload.clone())
            .collect()
    };

    let spawned = pick("chat:subagent_spawned");
    assert_eq!(spawned.len(), 1);
    assert_eq!(spawned[0]["chat_id"], "chat-1");
    assert_eq!(spawned[0]["subagent_id"], "child-1");
    assert_eq!(spawned[0]["subagent_type"], "reviewer");
    assert_eq!(spawned[0]["description"], "review the diff");
    assert_eq!(spawned[0]["parent_call_id"], "call-1");
    assert_eq!(spawned[0]["run_in_background"], true);

    let progress = pick("chat:subagent_progress");
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0]["chat_id"], "chat-1");
    assert_eq!(progress[0]["subagent_id"], "child-1");
    assert_eq!(progress[0]["status"], "running");
    assert_eq!(progress[0]["activity"], "running read_file");
    assert_eq!(progress[0]["turns"], 2);
    assert_eq!(progress[0]["tool_calls"], 1);
    assert_eq!(progress[0]["tokens"], 120);
    assert_eq!(progress[0]["duration_ms"], 340);

    let finished = pick("chat:subagent_finished");
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0]["chat_id"], "chat-1");
    assert_eq!(finished[0]["subagent_id"], "child-1");
    assert_eq!(finished[0]["status"], "completed");
    assert_eq!(finished[0]["output"], "all good");
    assert_eq!(finished[0].get("error"), None, "a completion carries no error");
    assert_eq!(finished[0]["duration_ms"], 900);
    assert_eq!(finished[0]["turns"], 3);
    assert_eq!(finished[0]["tool_calls"], 2);
    assert_eq!(finished[0]["tokens"], 480);
}

#[test]
fn an_approved_command_runs_and_the_turn_finishes() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let (_dir, state, bus, chat_id, store) = command_suspension();
    assert!(
        tool_messages(&store, &chat_id).is_empty(),
        "nothing runs while the command is suspended"
    );

    let handle = state
        .resume_command_chat_turn(
            &chat_id,
            goble_core::harness::CommandDecision::Approve("echo hi".to_string()),
        )
        .expect("resume the suspended command");
    rt.block_on(handle).expect("the resumed turn settles");

    wait_for_bus_event(&bus, "chat:turn_finished");
    let tools = tool_messages(&store, &chat_id);
    assert!(
        tools.iter().any(|t| t.contains("mock ran `echo hi`")),
        "the approved command ran through the runner: {tools:?}"
    );
}

#[test]
fn a_rejected_command_fails_the_call_and_the_turn_finishes() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let (_dir, state, bus, chat_id, store) = command_suspension();

    let handle = state
        .resume_command_chat_turn(
            &chat_id,
            goble_core::harness::CommandDecision::Reject("too risky".to_string()),
        )
        .expect("resume the suspended command");
    rt.block_on(handle).expect("the rejected turn settles");

    wait_for_bus_event(&bus, "chat:turn_finished");
    let tools = tool_messages(&store, &chat_id);
    assert!(
        tools
            .iter()
            .any(|t| t.contains("rejected by user: too risky")),
        "the rejection is the result body: {tools:?}"
    );
    let failed: Vec<goble_core::harness::ChatToolCall> = store
        .list_chat_messages(&chat_id)
        .unwrap()
        .into_iter()
        .filter_map(|m| m.3)
        .flat_map(|json| {
            serde_json::from_str::<Vec<goble_core::harness::ChatToolCall>>(&json)
                .unwrap_or_default()
        })
        .collect();
    assert!(
        failed.iter().any(|c| c.status == ToolCallStatus::Error),
        "a rejected command is a failed tool call, read from its status: {failed:?}"
    );
    assert!(
        !tools.iter().any(|t| t.contains("mock ran")),
        "a rejected command never runs"
    );
}

/// R1's acceptance clause, as a test of its own: a turn containing a command
/// tool must never leave its session busy, whichever way the proposal exits.
/// `resume_command_chat_turn` subscribes the listener before issuing the
/// resume, so the returned handle completes only once that listener has seen
/// the turn's `TraceFinished`; a listener subscribed after a fast resume
/// would miss it and this test would time out with the session still live.
#[test]
fn a_command_turn_frees_the_session_on_every_exit_path() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();

    // The three ways a proposed command can end. The failing path uses the
    // real sandbox runner so an approved command that is refused by the
    // allow-list errors without spawning a process.
    struct ExitPath {
        label: &'static str,
        runner: Option<Arc<dyn goble_core::harness::CommandRunner>>,
        decision: goble_core::harness::CommandDecision,
    }
    let exits = vec![
        ExitPath {
            label: "approved",
            runner: None,
            decision: goble_core::harness::CommandDecision::Approve("echo hi".to_string()),
        },
        ExitPath {
            label: "rejected",
            runner: None,
            decision: goble_core::harness::CommandDecision::Reject("too risky".to_string()),
        },
        ExitPath {
            label: "approved-but-failing",
            runner: Some(Arc::new(
                goble_core::harness::SandboxedCommandRunner::default_tools(),
            )),
            decision: goble_core::harness::CommandDecision::Approve(
                "definitely-not-allowed".to_string(),
            ),
        },
    ];

    for ExitPath {
        label,
        runner,
        decision,
    } in exits
    {
        let (_dir, state, bus, chat_id, store) = command_suspension_with_runner(runner);
        let client = state.daemon_client();
        assert!(
            client.snapshot().iter().any(|r| r.session_id.0 == chat_id),
            "{label}: a suspended command keeps its session live"
        );

        let handle = state
            .resume_command_chat_turn(&chat_id, decision)
            .unwrap_or_else(|e| panic!("{label}: resume the suspended command: {e}"));
        rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), handle)
                .await
                .unwrap_or_else(|_| {
                    panic!("{label}: the resumed turn never settled, leaving the session busy")
                })
                .expect("the resumed turn's listener task joins");
        });
        wait_for_bus_event(&bus, "chat:turn_finished");

        // `TraceFinished` is emitted just before the daemon removes the
        // session, so wait for the removal rather than asserting on it in
        // the same instant the listener wakes. A session that is never
        // removed fails the bound rather than passing silently.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while client.snapshot().iter().any(|r| r.session_id.0 == chat_id) {
            assert!(
                std::time::Instant::now() < deadline,
                "{label}: a settled command turn must not leave the session busy"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let tool_rows = tool_messages(&store, &chat_id);
        match label {
            "rejected" => assert!(
                tool_rows.iter().any(|t| t.contains("rejected by user")),
                "a rejected command is recorded as a rejection: {tool_rows:?}"
            ),
            "approved-but-failing" => assert!(
                tool_rows
                    .iter()
                    .any(|t| t.contains("not in the allowed list")),
                "the approved-but-failing command reports its failure: {tool_rows:?}"
            ),
            _ => {}
        }
    }
}

#[test]
fn run_chat_turn_runs_registered_byoh_harness() {
    // A registered BYOH harness is resolved and driven by run_chat_turn
    // (instead of building + registering the internal harness): its reply
    // is streamed as an assistant delta on the daemon event bus.
    use std::sync::Arc;
    let (_dir, state) = tmp_state();
    let chat_id = state.create_chat("Demo", None, None).expect("create chat");
    state.register_harness(Arc::new(goble_harness_runtime::MockHarness::new(
        goble_harness_types::HarnessId::new("cli-fake"),
        "from byoh",
    )));

    let client = state.daemon_client();
    let mut rx = client.subscribe();
    let session_id = goble_harness_types::SessionId::new(&chat_id);

    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let handle = state
        .run_chat_turn(
            &chat_id,
            "Say hi",
            "mock",
            "",
            "local",
            "default",
            &chat_id,
            None,
            Some("cli-fake"),
        )
        .expect("run turn on a registered harness");
    rt.block_on(handle).expect("turn completes");

    rt.block_on(async {
        let mut saw_reply = false;
        while let Ok(ev) = rx.recv().await {
            match &ev {
                goble_daemon_protocol::DaemonEvent::AssistantDelta { delta, .. }
                    if delta == "from byoh" =>
                {
                    saw_reply = true;
                }
                goble_daemon_protocol::DaemonEvent::TraceFinished { session_id: sid, .. }
                    if sid == &session_id =>
                {
                    break;
                }
                _ => {}
            }
        }
        assert!(saw_reply, "the registered harness reply must be streamed");
    });
}

#[test]
fn tool_events_fill_and_clear_the_in_flight_map() {
    // A started call is held in the per-chat in-flight map and cleared on
    // finish/error, with the structured `chat:tool` event emitted for each
    // transition so the app can overlay it without a store re-read.
    let (_dir, state) = tmp_state();
    let bus = Arc::new(crate::event_bus::CollectingEventBus::new());
    state.set_event_bus(bus.clone());
    let sid = goble_harness_types::SessionId::new("chat-1");

    state.translate_daemon_event(goble_daemon_protocol::DaemonEvent::ToolCallStarted {
        session_id: sid.clone(),
        id: "t1".into(),
        name: "read_file".into(),
        arguments: serde_json::json!({"path": "src/lib.rs"}),
    });
    state.translate_daemon_event(goble_daemon_protocol::DaemonEvent::ToolCallStarted {
        session_id: sid.clone(),
        id: "t2".into(),
        name: "run_command".into(),
        arguments: serde_json::json!({"command": "ls"}),
    });

    let in_flight = state.in_flight_tool_calls("chat-1");
    assert_eq!(in_flight.len(), 2, "each started call gains an entry");
    assert!(in_flight
        .iter()
        .all(|c| c.status == ToolCallStatus::Running));
    assert!(
        bus.has_event("chat:tool"),
        "the app gets a structured chat:tool event"
    );

    state.translate_daemon_event(goble_daemon_protocol::DaemonEvent::ToolCallFinished {
        session_id: sid.clone(),
        id: "t1".into(),
        result: "ok".into(),
    });
    let in_flight = state.in_flight_tool_calls("chat-1");
    assert_eq!(in_flight.len(), 1, "a finished call clears its entry");
    assert_eq!(in_flight[0].id, "t2");

    state.translate_daemon_event(goble_daemon_protocol::DaemonEvent::ToolCallError {
        session_id: sid.clone(),
        id: "t2".into(),
        message: "boom".into(),
    });
    assert!(
        state.in_flight_tool_calls("chat-1").is_empty(),
        "an errored call clears its entry too"
    );

    let tool_events: Vec<_> = bus
        .events()
        .into_iter()
        .filter(|(name, _)| name == "chat:tool")
        .map(|(_, payload)| payload)
        .collect();
    assert_eq!(tool_events.len(), 4, "one chat:tool per transition");
    assert_eq!(tool_events[1]["id"], "t2");
    assert_eq!(tool_events[2]["result"], "ok");
    assert_eq!(tool_events[3]["status"], "error");
}

#[test]
fn reasoning_events_are_emitted_as_chat_reasoning() {
    // Every reasoning transition reaches the app as a structured
    // `chat:reasoning` event carrying the phase, the mode and the text.
    let (_dir, state) = tmp_state();
    let bus = Arc::new(crate::event_bus::CollectingEventBus::new());
    state.set_event_bus(bus.clone());
    let sid = goble_harness_types::SessionId::new("chat-1");

    state.translate_daemon_event(goble_daemon_protocol::DaemonEvent::ReasoningStarted {
        session_id: sid.clone(),
        step: 0,
        mode: "contemplating".into(),
    });
    state.translate_daemon_event(goble_daemon_protocol::DaemonEvent::ReasoningDelta {
        session_id: sid.clone(),
        delta: "weighing options".into(),
    });
    state.translate_daemon_event(goble_daemon_protocol::DaemonEvent::ReasoningDone {
        session_id: sid.clone(),
        step: 0,
        mode: "contemplating".into(),
        content: "weighing options".into(),
        decision: "\"execute\"".into(),
    });

    let events: Vec<_> = bus
        .events()
        .into_iter()
        .filter(|(name, _)| name == "chat:reasoning")
        .map(|(_, payload)| payload)
        .collect();
    assert_eq!(events.len(), 3, "one chat:reasoning per transition");
    assert_eq!(events[0]["phase"], "started");
    assert_eq!(events[0]["step"], 0);
    assert_eq!(events[0]["mode"], "contemplating");
    assert_eq!(events[1]["phase"], "delta");
    assert_eq!(events[1]["delta"], "weighing options");
    assert_eq!(events[2]["phase"], "done");
    assert_eq!(events[2]["content"], "weighing options");
    assert_eq!(events[2]["chat_id"], "chat-1");
}

#[test]
fn chat_turn_carries_selected_medium_and_project() {
    let turn = build_chat_turn(
        goble_harness_types::HarnessId::new("internal-c1"),
        goble_harness_types::SessionId::new("c1"),
        "hi",
        "vm",
        "projects/vm",
    );
    assert_eq!(
        turn.medium_id,
        goble_harness_types::MediumId::new("vm"),
        "the turn's medium must be the selected medium, not the local default"
    );
    assert_eq!(
        turn.project_id,
        goble_harness_types::ProjectId::new("projects/vm"),
        "the turn must be scoped to the selected medium's project, not 'default'"
    );
}
