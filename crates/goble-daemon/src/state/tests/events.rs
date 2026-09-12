use super::*;

use crate::state::events::map_event;

#[test]
fn map_drops_host_discovery_frames() {
    assert!(map_event(HarnessServerEvent::Ready).is_none());
    assert!(matches!(
        map_event(HarnessServerEvent::Done {
            session_id: SessionId::new("s1")
        }),
        Some(DaemonEvent::Done { .. })
    ));
}

#[test]
fn map_carries_sub_agent_spawned() {
    let mapped = map_event(HarnessServerEvent::SubAgentSpawned {
        session_id: SessionId::new("s1"),
        chat_id: "chat-1".to_string(),
        subagent_id: "child-1".to_string(),
        subagent_type: "reviewer".to_string(),
        description: "review the diff".to_string(),
        parent_call_id: "call-1".to_string(),
        run_in_background: true,
    });
    assert!(matches!(
        mapped,
        Some(DaemonEvent::SubAgentSpawned {
            session_id,
            chat_id,
            subagent_id,
            subagent_type,
            description,
            parent_call_id,
            run_in_background: true,
        }) if session_id == SessionId::new("s1")
            && chat_id == "chat-1"
            && subagent_id == "child-1"
            && subagent_type == "reviewer"
            && description == "review the diff"
            && parent_call_id == "call-1"
    ));
}

#[test]
fn map_carries_sub_agent_progress() {
    let mapped = map_event(HarnessServerEvent::SubAgentProgress {
        session_id: SessionId::new("s1"),
        chat_id: "chat-1".to_string(),
        subagent_id: "child-1".to_string(),
        status: "running".to_string(),
        activity: "running read_file".to_string(),
        turns: 2,
        tool_calls: 1,
        tokens: 120,
        duration_ms: 340,
    });
    assert!(matches!(
        mapped,
        Some(DaemonEvent::SubAgentProgress {
            chat_id,
            subagent_id,
            status,
            activity,
            turns: 2,
            tool_calls: 1,
            tokens: 120,
            duration_ms: 340,
            ..
        }) if chat_id == "chat-1"
            && subagent_id == "child-1"
            && status == "running"
            && activity == "running read_file"
    ));
}

#[test]
fn map_carries_sub_agent_finished() {
    let mapped = map_event(HarnessServerEvent::SubAgentFinished {
        session_id: SessionId::new("s1"),
        chat_id: "chat-1".to_string(),
        subagent_id: "child-1".to_string(),
        status: "completed".to_string(),
        output: Some("all good".to_string()),
        error: None,
        duration_ms: 900,
        turns: 3,
        tool_calls: 2,
        tokens: 480,
    });
    assert!(matches!(
        mapped,
        Some(DaemonEvent::SubAgentFinished {
            session_id,
            chat_id,
            subagent_id,
            status,
            output: Some(output),
            error: None,
            duration_ms: 900,
            turns: 3,
            tool_calls: 2,
            tokens: 480,
        }) if session_id == SessionId::new("s1")
            && chat_id == "chat-1"
            && subagent_id == "child-1"
            && status == "completed"
            && output == "all good"
    ));
}
