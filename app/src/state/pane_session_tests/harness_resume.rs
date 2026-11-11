//! Opening and launching a conversation that ran inside a CLI harness: the
//! surface the tab gets, and how a TUI harness launch lands in agent mode.

use super::*;

/// The mock state plus a conversation that ran in `harness` under `directory`,
/// opened in a tab.
fn opened_state(harness: &str, directory: &str) -> UiState {
    let mut state = UiState::mock();
    state.conversations.push(
        ConversationEntry::new("resume-1", "Thread", "last reply", "now")
            .with_harness(harness)
            .with_directory(directory),
    );
    state.open_conversation_tab("resume-1".to_string(), None);
    state
}

/// The kind of content the leaf `pane_id` hosts.
fn pane_kind(state: &UiState, pane_id: u64) -> PaneKind {
    let mut leaves = Vec::new();
    for space in &state.spaces {
        space.root.leaves(&mut leaves);
    }
    leaves
        .into_iter()
        .find_map(|(id, kind)| (id == pane_id).then_some(kind))
        .expect("the pane is a leaf in the layout")
}

fn temp_directory() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create a temp directory");
    let path = dir.path().display().to_string();
    (dir, path)
}

#[test]
fn a_claude_conversation_opens_in_the_agent_view() {
    let (_dir, cwd) = temp_directory();
    let state = opened_state("claude", &cwd);
    let pane_id = state.active_pane_id;

    assert_eq!(
        pane_kind(&state, pane_id),
        PaneKind::Chat,
        "the thread is drawn by goble's agent surface, not by the harness's TUI"
    );
    assert!(
        state.pane_controls(pane_id).harness_mode,
        "the conversation's harness is open in the pane"
    );
    assert!(
        matches!(
            state.pane_view(pane_id),
            goble_terminal::blocks::BlockView::Agent { .. }
        ),
        "the pane draws the agent view"
    );
    assert!(
        !state.terminal.borrow().sessions.contains_key(&pane_id),
        "no harness process is launched behind the agent view"
    );
    assert_eq!(
        state
            .pane_sessions
            .get(&pane_id)
            .map(|session| session.path.as_str()),
        Some(cwd.as_str()),
        "the pane starts where the conversation worked"
    );
}

#[test]
fn a_provider_alias_opens_the_agent_view_too() {
    let (_dir, cwd) = temp_directory();
    let state = opened_state("anthropic", &cwd);

    assert_eq!(pane_kind(&state, state.active_pane_id), PaneKind::Chat);
    assert_eq!(
        state
            .pane_sessions
            .get(&state.active_pane_id)
            .map(|session| session.conversation_id.as_str()),
        Some("resume-1"),
        "the tab is bound to the conversation it opened"
    );
}

#[test]
fn an_unknown_provider_opens_the_agent_view() {
    let (_dir, cwd) = temp_directory();
    let state = opened_state("some-unknown-agent", &cwd);

    assert_eq!(pane_kind(&state, state.active_pane_id), PaneKind::Chat);
}

#[test]
fn a_home_directory_is_expanded_before_it_reaches_the_pane() {
    let state = opened_state("claude", "~/Projects/goble");
    let pane_id = state.active_pane_id;

    let expanded = crate::state::expand_path("~/Projects/goble");
    assert_eq!(
        state
            .pane_sessions
            .get(&pane_id)
            .map(|session| session.path.as_str()),
        Some(expanded.as_str()),
        "the pane's working directory is the real directory"
    );
    if let Some(home) = crate::state::home_directory() {
        assert_eq!(
            expanded,
            format!("{home}/Projects/goble"),
            "the tilde names the user's home"
        );
    }
}

#[test]
fn a_pane_names_the_cli_harness_of_its_conversation() {
    let (_dir, cwd) = temp_directory();
    let state = opened_state("claude", &cwd);

    assert_eq!(
        state
            .pane_conversation_harness(state.active_pane_id)
            .map(|spec| spec.id),
        Some("claude"),
        "the rich input names the harness that answers"
    );
}

#[test]
fn a_native_conversation_names_no_cli_harness() {
    let (_dir, cwd) = temp_directory();
    let state = opened_state("goble", &cwd);

    assert!(
        state
            .pane_conversation_harness(state.active_pane_id)
            .is_none(),
        "the native harness is the app's own, so its model stays switchable"
    );
}

#[test]
fn opening_the_agent_view_switches_a_bound_pane_into_agent_mode() {
    let mut state = UiState::mock();
    state.conversations.push(
        ConversationEntry::new("launch-1", "Launch", "last reply", "now")
            .with_harness("claude")
            .with_directory("/Users/me/Projects/goble"),
    );
    state
        .pane_sessions
        .entry(1)
        .or_default()
        .conversation_id = "launch-1".to_string();

    state.open_harness_agent_view(1);

    assert!(state.pane_controls(1).harness_mode, "the harness is open");
    assert!(
        matches!(
            state.pane_view(1),
            goble_terminal::blocks::BlockView::Agent { ref conversation_id }
                if conversation_id == "launch-1"
        ),
        "the view names the conversation the harness launched into"
    );
    assert!(
        state.composer_focused,
        "the agent input holds the keyboard the launch left"
    );
}

/// A harness launched from a terminal pane leaves its conversation bound to
/// that pane. Esc puts the pane back on the shell the harness runs in, and the
/// sidebar row is still the conversation's entry point: clicking it brings the
/// thread forward on its agent surface instead of leaving the pane in terminal
/// mode, where the next `!` line would run in the pane's own terminal.
#[test]
fn a_harness_conversation_left_on_its_shell_reopens_in_the_agent_view() {
    let mut state = UiState::mock();
    state.spaces = vec![Space::new(
        "Shell",
        Pane::Leaf {
            id: 1,
            kind: PaneKind::Terminal,
        },
    )];
    state.conversations.push(
        ConversationEntry::new("launch-1", "Launch", "last reply", "now")
            .with_harness("claude")
            .with_directory("/Users/me/Projects/goble"),
    );
    state
        .pane_sessions
        .get_mut(&1)
        .expect("pane 1 has a session")
        .conversation_id = "launch-1".to_string();

    // The launch opened the conversation's view, then Esc went back to the
    // shell the harness runs in.
    state.open_harness_agent_view(1);
    state.leave_agent_view(1);
    assert_eq!(
        state.pane_view(1),
        goble_terminal::blocks::BlockView::Terminal,
        "the premise: the pane is on its shell"
    );

    state.open_conversation_tab("launch-1".to_string(), None);

    assert_eq!(
        state.active_pane_id, 1,
        "the pane already on the conversation is the one brought forward"
    );
    assert!(
        state.pane_controls(1).harness_mode,
        "the row opens the conversation's harness"
    );
    assert!(
        matches!(
            state.pane_view(1),
            goble_terminal::blocks::BlockView::Agent { ref conversation_id }
                if conversation_id == "launch-1"
        ),
        "the row opens the conversation's agent view, not its terminal"
    );
}

#[test]
fn opening_the_agent_view_without_a_conversation_changes_nothing() {
    let mut state = UiState::mock();
    state.pane_sessions.entry(99).or_default();

    state.open_harness_agent_view(99);

    assert!(!state.pane_controls(99).harness_mode);
    assert_eq!(
        state.pane_view(99),
        goble_terminal::blocks::BlockView::Terminal
    );
}
