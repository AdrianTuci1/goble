use super::*;

use goble_ui::TokenUsage;

fn pane_on(conversation_id: &str) -> UiState {
    let mut state = UiState::mock();
    state.pane_sessions.insert(
        1,
        PaneSession {
            conversation_id: conversation_id.to_string(),
            ..PaneSession::default()
        },
    );
    state
}

/// A conversation's durable total — what its model calls reported before this
/// app run — reaches its pane's footer, which is what makes a conversation
/// reopened after a restart still show what it spent.
#[test]
fn a_stored_total_reaches_the_pane_that_shows_the_conversation() {
    let mut state = pane_on("c1");
    state.seed_conversation_usage(
        "c1",
        Some(TokenUsage {
            input: 1_200,
            cached: Some(900),
            output: 80,
        }),
    );

    let usage = state
        .pane_chat_snapshot()
        .get(&1)
        .expect("the pane's snapshot")
        .usage;
    assert_eq!((usage.input, usage.cached, usage.output), (1_200, Some(900), 80));
    assert_eq!(usage.total(), 1_280, "the total is input + output");
}

/// A conversation totalling live events keeps its running sum: the store holds
/// that same spend, so seeding it on top would count it twice.
#[test]
fn a_live_total_is_never_re_seeded_from_the_store() {
    let mut state = pane_on("c1");
    state.apply_token_usage(&crate::state::TokenUsagePayload {
        chat_id: "c1".to_string(),
        usage: goble_desktop_service::TokenUsageEvent {
            input: 300,
            cached: None,
            output: 20,
        },
    });

    // A refresh reads the store, which by now holds the same call.
    state.seed_conversation_usage(
        "c1",
        Some(TokenUsage {
            input: 300,
            cached: None,
            output: 20,
        }),
    );

    let usage = state
        .pane_chat_snapshot()
        .get(&1)
        .expect("the pane's snapshot")
        .usage;
    assert_eq!(usage.total(), 320, "the live call is counted once");
}

/// A conversation whose provider reported nothing gets no entry at all, so its
/// footer says so rather than drawing a zero.
#[test]
fn a_conversation_that_reported_nothing_gets_no_total() {
    let mut state = pane_on("c1");
    state.seed_conversation_usage("c1", None);

    assert!(
        state.conversation_usage.is_empty(),
        "nothing was reported, so nothing is totalled"
    );
    assert!(
        state
            .pane_chat_snapshot()
            .get(&1)
            .expect("the pane's snapshot")
            .usage
            .is_empty(),
        "the pane's footer draws no counts"
    );
}
