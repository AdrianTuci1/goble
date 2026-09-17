//! A2: re-attaching to a conversation's session instead of starting over.
//!
//! The run lives with the session, so the client asks the daemon for the
//! session's own state and replays the turns it missed: on an open, with
//! everything the session has, and on a return, with only the span the client
//! did not have. A conversation with no session is told so; no turn is started
//! for it. The harnesses here are registered mocks that write nothing to the
//! store, so the transcript an attach hands back can only be the session's own
//! recording — a store read would be empty.

use std::sync::Arc;

use goble_harness_runtime::MockHarness;
use goble_harness_types::{HarnessId, SessionId};

use crate::state::SessionAttach;

use super::{command_suspension, tmp_state};

/// Run one turn on a registered mock harness under `session_id`, and wait for
/// it to settle. The harness streams one reply and writes no store rows.
fn run_mock_turn(
    state: &Arc<crate::state::DesktopState>,
    chat_id: &str,
    prompt: &str,
    harness: &str,
    reply: &str,
) {
    state.register_harness(Arc::new(MockHarness::new(HarnessId::new(harness), reply)));
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let handle = state
        .run_chat_turn(
            chat_id,
            prompt,
            "mock",
            "",
            "local",
            "default",
            chat_id,
            None,
            Some(harness),
        )
        .expect("run the turn on the registered harness");
    rt.block_on(handle).expect("the turn settles");
}

/// The rows an attach handed back, as `(role, content)` pairs in order.
fn rows(attach: &SessionAttach) -> Vec<(String, String)> {
    match attach {
        SessionAttach::Attached(session) => session
            .messages
            .iter()
            .map(|row| (row.role.clone(), row.content.clone()))
            .collect(),
        SessionAttach::NoSession { chat_id } => panic!("no session for {chat_id}"),
    }
}

/// The assistant text a span of replayed events carries.
fn replayed_replies(attach: &SessionAttach) -> Vec<String> {
    match attach {
        SessionAttach::Attached(session) => session
            .events
            .iter()
            .filter_map(|event| match event {
                goble_daemon_protocol::DaemonEvent::AssistantDelta { delta, .. } => {
                    Some(delta.clone())
                }
                _ => None,
            })
            .collect(),
        SessionAttach::NoSession { chat_id } => panic!("no session for {chat_id}"),
    }
}

#[test]
fn an_open_on_a_conversation_with_history_replays_it() {
    let (_dir, state) = tmp_state();
    let chat_id = state.create_chat("Ship it remotely", None, None).unwrap();
    run_mock_turn(&state, &chat_id, "run the tests", "cli-a", "all green");

    // The harness wrote nothing: whatever the attach hands back is the session's
    // own recording, never this machine's rows about the conversation.
    assert!(
        state.list_chat_messages(&chat_id).unwrap().is_empty(),
        "the mock harness writes no store rows"
    );

    let attach = state.attach_chat_session(&chat_id, 0);
    let SessionAttach::Attached(session) = &attach else {
        panic!("the session ran a turn, so it is there: {attach:?}");
    };
    assert_eq!(session.session_id, chat_id, "the session's own id");
    assert_eq!(session.turns, 1, "one turn recorded");
    assert_eq!(session.from_turn, 0, "a fresh open replays from the start");
    assert!(!session.live, "the turn settled");
    assert_eq!(
        rows(&attach),
        vec![
            ("user".to_string(), "run the tests".to_string()),
            ("assistant".to_string(), "all green".to_string()),
        ],
        "the prompt the harness was handed and the reply it streamed"
    );
    assert_eq!(
        replayed_replies(&attach),
        vec!["all green".to_string()],
        "the session's recorded events are replayed"
    );
}

#[test]
fn a_drop_and_return_re_attaches_to_the_same_session_id() {
    let (_dir, state) = tmp_state();
    let chat_id = state.create_chat("Ship it remotely", None, None).unwrap();
    run_mock_turn(&state, &chat_id, "run the tests", "cli-a", "all green");

    let opened = state.attach_chat_session(&chat_id, 0);
    let SessionAttach::Attached(first) = &opened else {
        panic!("the session is there: {opened:?}");
    };
    let session_id = first.session_id.clone();
    let seen = first.turns;
    assert_eq!(seen, 1);

    // The client is away. The run does not live here: the session keeps going
    // without it, and a second turn lands on the same session.
    run_mock_turn(&state, &chat_id, "and again", "cli-b", "still green");

    let returned = state.attach_chat_session(&chat_id, seen);
    let SessionAttach::Attached(second) = &returned else {
        panic!("the session is still there: {returned:?}");
    };
    assert_eq!(
        second.session_id, session_id,
        "a re-attach rejoins the session the client left, never a new one"
    );
    assert_eq!(second.turns, 2);
    assert_eq!(second.from_turn, 1, "the span the client already had");
    assert_eq!(
        rows(&returned),
        vec![
            ("user".to_string(), "and again".to_string()),
            ("assistant".to_string(), "still green".to_string()),
        ],
        "only what the client missed is replayed"
    );
    assert_eq!(replayed_replies(&returned), vec!["still green".to_string()]);
}

#[test]
fn a_conversation_with_no_session_says_so_and_starts_nothing() {
    let (_dir, state) = tmp_state();
    let chat_id = state.create_chat("Never ran", None, None).unwrap();

    match state.attach_chat_session(&chat_id, 0) {
        SessionAttach::NoSession { chat_id: named } => {
            assert_eq!(named, chat_id, "the answer names the conversation");
        }
        other => panic!("a conversation with no session must say so, got {other:?}"),
    }
    assert!(
        state
            .attach_chat_session(&chat_id, 0)
            .session_id()
            .is_none(),
        "asking again must not start one either"
    );
    assert!(
        state.daemon_client().snapshot().is_empty(),
        "no turn was started for it"
    );
    assert!(
        state.list_chat_messages(&chat_id).unwrap().is_empty(),
        "and nothing was written into the conversation"
    );
}

/// The run outlives the client: a pane that opens on a session whose turn is
/// still in flight finds it live, with the turn's own prompt and the call it had
/// reached so far.
#[test]
fn an_open_on_a_still_running_session_finds_it_live() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let (_dir, state, _bus, chat_id, _store) = command_suspension();

    let attach = state.attach_chat_session(&chat_id, 0);
    let SessionAttach::Attached(session) = &attach else {
        panic!("a suspended turn keeps its session: {attach:?}");
    };
    assert!(
        session.live,
        "the turn is in flight, so the session is live"
    );
    assert_eq!(session.turns, 1);
    let drawn = rows(&attach);
    assert_eq!(
        drawn.first(),
        Some(&("user".to_string(), "run echo hi".to_string())),
        "the turn's own prompt is replayed: {drawn:?}"
    );
    let SessionId(sid) = SessionId::new(&chat_id);
    let assistant = session
        .messages
        .iter()
        .find(|row| row.role == "assistant")
        .expect("the running turn has an assistant row");
    let calls: Vec<goble_core::harness::ChatToolCall> = serde_json::from_str(
        assistant
            .tool_calls
            .as_deref()
            .expect("the suspended command is a recorded tool call"),
    )
    .unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "run_command");
    assert_eq!(
        calls[0].status,
        goble_core::harness::ToolCallStatus::Running,
        "the call the session was left holding stays running"
    );
    assert_eq!(assistant.id, format!("{sid}:turn-0:assistant"));
}
