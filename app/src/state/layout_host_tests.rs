//! Layout selection and saved SSH host bindings, as the app persists them.

use super::*;

use std::sync::Arc;

use goble_core::ssh_command::SshSession;

use crate::ui::{PaneRect, SplitDir};

fn desktop() -> (Arc<DesktopState>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp thread store dir");
    let desktop = DesktopState::new(
        goble_core::store::Store::open_in_memory().expect("open in-memory store"),
        goble_desktop_service::ThreadStore::new(dir.path()).expect("open thread store"),
    );
    (desktop, dir)
}

/// A state whose active space holds three leaves: 1 and the two splits after
/// it. The counter is kept in step so a later split allocates fresh ids.
fn three_pane_state() -> UiState {
    let mut state = UiState::mock();
    let mut next = state.next_pane_id;
    let second = state.spaces[0]
        .split(1, SplitDir::Vertical, &mut next)
        .expect("pane 1 splits");
    state.spaces[0]
        .split(second, SplitDir::Horizontal, &mut next)
        .expect("the second pane splits");
    state.next_pane_id = next;
    state
}

fn session() -> SshSession {
    SshSession {
        host: "prod.example.com".to_string(),
        user: "deploy".to_string(),
        port: 2222,
        alias: None,
    }
}

#[test]
fn the_selected_layout_and_its_floating_rects_survive_a_restart() {
    let (desktop, _dir) = desktop();
    let mut state = three_pane_state();
    assert!(state.set_space_layout(0, LayoutKind::Floating, Some(desktop.as_ref())));
    let placed = PaneRect::new(0.25, 0.15, 0.5, 0.4);
    state.spaces[0].set_floating_rect(3, placed);
    state.save_panes(&desktop);

    let mut restored = UiState::mock();
    restored.restore_panes(&desktop);
    assert_eq!(restored.spaces[0].layout, LayoutKind::Floating);
    assert_eq!(restored.spaces[0].root.leaf_count(), 3, "no pane is lost");
    assert_eq!(restored.spaces[0].floating_rect(3), Some(placed));
    assert!(
        restored.spaces[0].floating_rect(1).is_some(),
        "every leaf comes back with a rect"
    );
}

#[test]
fn cycling_the_layout_recomputes_every_leaf_and_keeps_the_tree() {
    let mut state = three_pane_state();
    assert_eq!(state.spaces[0].layout, LayoutKind::Bsp);

    assert_eq!(state.cycle_space_layout(None), LayoutKind::MasterStack);
    assert_eq!(state.spaces[0].layout, LayoutKind::MasterStack);
    assert_eq!(state.spaces[0].root.leaf_count(), 3);

    assert_eq!(state.cycle_space_layout(None), LayoutKind::Floating);
    assert_eq!(state.spaces[0].layout, LayoutKind::Floating);
    assert_eq!(state.spaces[0].floating.len(), 3, "every leaf has a rect");

    // A split in the floating layout adds a rect; a close drops one.
    let mut next = state.next_pane_id;
    let fourth = state.spaces[0]
        .split(3, SplitDir::Vertical, &mut next)
        .expect("the pane splits");
    assert_eq!(state.spaces[0].floating.len(), 4);
    assert!(state.spaces[0].floating_rect(fourth).is_some());
    assert_eq!(state.spaces[0].close(fourth), Some(3));
    assert_eq!(state.spaces[0].floating.len(), 3);

    assert_eq!(state.cycle_space_layout(None), LayoutKind::Bsp);
    assert_eq!(state.spaces[0].layout, LayoutKind::Bsp);
    assert_eq!(state.spaces[0].root.leaf_count(), 3, "the tree is untouched");
}

#[test]
fn a_saved_host_binding_persists_with_the_pane_and_restores_the_session() {
    let (desktop, _dir) = desktop();
    let mut state = three_pane_state();
    let session = session();
    state.bind_ssh_pane(3, Some("prod".to_string()), session.clone());
    state.save_panes(&desktop);

    let mut restored = UiState::mock();
    restored.restore_panes(&desktop);
    let binding = restored
        .pane_ssh
        .get(&3)
        .expect("pane 3 remembers its saved host");
    assert_eq!(binding.host_name.as_deref(), Some("prod"));
    assert_eq!(binding.session.as_ref(), Some(&session));
    assert_eq!(
        restored.terminal.borrow().ssh_session(3),
        Some(&session),
        "the chip and shell route read the restored session"
    );
}

#[test]
fn ending_a_saved_hosts_session_keeps_the_reconnect_offer_but_a_typed_one_leaves_nothing() {
    let (desktop, _dir) = desktop();
    let mut state = three_pane_state();
    state.bind_ssh_pane(3, Some("prod".to_string()), session());
    state.bind_ssh_pane(
        1,
        None,
        SshSession {
            host: "adhoc.example.com".to_string(),
            user: "ada".to_string(),
            port: 22,
            alias: None,
        },
    );

    assert_eq!(state.note_ssh_ended(3), Some("prod".to_string()));
    assert_eq!(
        state.pane_ssh.get(&3).and_then(|b| b.reconnect.as_deref()),
        Some("prod"),
        "a named host is offered again"
    );
    assert!(state.terminal.borrow().ssh_session(3).is_none());

    assert_eq!(state.note_ssh_ended(1), None);
    assert!(
        !state.pane_ssh.contains_key(&1),
        "a typed session leaves no binding behind"
    );

    state.save_panes(&desktop);
    let mut restored = UiState::mock();
    restored.restore_panes(&desktop);
    let binding = restored.pane_ssh.get(&3).expect("the offer survives");
    assert_eq!(binding.reconnect.as_deref(), Some("prod"));
    assert!(binding.session.is_none());
    assert_eq!(
        restored.terminal.borrow().ssh_session(3),
        None,
        "an ended session is not restored as live"
    );
}

#[test]
fn a_line_typed_in_the_grid_binds_ssh_and_exit_offers_the_saved_host() {
    let mut state = three_pane_state();
    state.bind_ssh_pane(3, Some("prod".to_string()), session());

    state.note_shell_line(3, "exit");
    assert_eq!(
        state.ssh_reconnect_prompt,
        Some((3, "prod".to_string())),
        "the grid's exit offers the host the pane was opened from"
    );
    assert!(state.terminal.borrow().ssh_session(3).is_none());

    state.note_shell_line(3, "ssh deploy@prod.example.com");
    let bound = state
        .pane_ssh
        .get(&3)
        .expect("the typed line binds the pane");
    assert_eq!(bound.host_name, None, "a typed line names its own target");
    assert_eq!(
        bound.session.as_ref().map(|s| s.host.as_str()),
        Some("prod.example.com")
    );
}

#[test]
fn an_out_of_range_space_is_not_a_layout_change() {
    let mut state = UiState::mock();
    assert!(!state.set_space_layout(9, LayoutKind::Floating, None));
    assert_eq!(state.spaces[0].layout, LayoutKind::Bsp);
}
