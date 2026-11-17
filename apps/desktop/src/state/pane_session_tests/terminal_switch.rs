//! Esc at an agent view turns a pane with no shell behind it into a terminal.
//!
//! A chat pane mounts the same agent surface a terminal pane's harness view
//! does, and its `esc for terminal` hint promises the same way out. The pane has
//! no pty, so the way out is a terminal of the pane's own: the leaf changes kind
//! in place, and the conversation stays bound for Cmd+Enter to open again.

use super::*;

#[test]
fn esc_at_a_chat_pane_makes_it_a_terminal() {
    let mut state = UiState::mock();
    let pane_id = state.active_pane_id;
    let conversation = state.pane_conversation_id(pane_id);
    assert!(matches!(
        state.spaces[0].leaf_kind(pane_id),
        Some(PaneKind::Chat)
    ));

    assert!(state.show_pane_terminal(pane_id));

    assert!(
        matches!(state.spaces[0].leaf_kind(pane_id), Some(PaneKind::Terminal)),
        "the pane is a terminal now: its agent view has a shell to return to"
    );
    assert_eq!(
        state.pane_conversation_id(pane_id),
        conversation,
        "the conversation stays bound, so Cmd+Enter opens its agent view again"
    );
    assert!(
        !state.pane_controls(pane_id).harness_mode,
        "the agent view closes with the switch: the pane shows its shell"
    );
    assert!(
        !state.composer_focused,
        "the shell, not the chat composer, is the typing surface"
    );
}

#[test]
fn esc_at_a_terminal_pane_leaves_it_alone() {
    let mut state = UiState::mock();
    let pane_id = state.active_pane_id;
    state.spaces[0].set_leaf_kind(pane_id, PaneKind::Terminal);

    assert!(
        !state.show_pane_terminal(pane_id),
        "a terminal pane is already the surface Esc offers"
    );
    assert!(matches!(
        state.spaces[0].leaf_kind(pane_id),
        Some(PaneKind::Terminal)
    ));
}
