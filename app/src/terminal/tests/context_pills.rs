//! What the composer's directory and branch pills do to a pane's shell: a
//! selection is the command the user would have typed, written into that pane's
//! own pty, and the pane's recorded directory follows so its pill moves with the
//! shell it just moved.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::platform::WindowControl;

use super::*;
use crate::actions::make_actions;
use crate::media::MediaState;
use crate::state::UiState;
use crate::ui::UiActions;

/// A single-pane app whose shell is a capture instead of a pty, so the bytes a
/// selection writes are readable.
fn harness() -> (
    Rc<RefCell<UiState>>,
    UiActions,
    Arc<Mutex<Vec<u8>>>,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = Arc::new(DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    ));
    let state = Rc::new(RefCell::new(UiState::from_desktop(&desktop)));
    let media = Rc::new(RefCell::new(MediaState::mock()));
    let actions = make_actions(
        Rc::clone(&state),
        Some(Arc::clone(&desktop)),
        media,
        WindowControl::default(),
        Rc::new(RefCell::new(1.0)),
    );
    let captured = Arc::new(Mutex::new(Vec::new()));
    {
        let mut s = state.borrow_mut();
        let mut session = detached();
        session.writer = Some(Box::new(CaptureWriter(Arc::clone(&captured))));
        s.terminal.borrow_mut().sessions.insert(1, session);
        s.active_pane_id = 1;
    }
    (state, actions, captured, dir)
}

fn written(captured: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8(captured.lock().unwrap().clone()).expect("the pty took text")
}

/// Picking a directory is `cd`, run where the pane's commands run: the absolute
/// path means it lands in the chosen directory whatever the shell's own cwd was,
/// and the pane's record of where it is moves with it.
#[test]
fn the_directory_pill_runs_cd_in_the_panes_own_shell() {
    let (state, actions, captured, _dir) = harness();
    let home = tempfile::tempdir().expect("temp home");
    let target = home.path().join("My Project");
    std::fs::create_dir(&target).unwrap();
    let target = target.to_string_lossy().to_string();

    (actions.on_select_dir.borrow_mut())(1, target.clone());

    assert_eq!(written(&captured), format!("cd '{target}'\n"));
    let s = state.borrow();
    assert_eq!(
        s.pane_sessions.get(&1).map(|session| session.path.clone()),
        Some(target),
        "the pane's directory follows the shell it moved"
    );
    assert!(
        !*s.pane_controls(1).dir_menu_open.borrow(),
        "the menu that made the choice is closed"
    );
}

/// A pane whose shell has not been started has nowhere to run `cd`, so the
/// choice is recorded for the shell it will start and for its composer, and
/// nothing is written.
#[test]
fn a_pane_with_no_shell_only_records_the_directory() {
    let (state, actions, captured, _dir) = harness();
    let home = tempfile::tempdir().expect("temp home");
    let target = home.path().to_string_lossy().to_string();
    state.borrow_mut().terminal.borrow_mut().sessions.remove(&1);

    (actions.on_select_dir.borrow_mut())(1, target.clone());

    assert_eq!(written(&captured), "");
    assert_eq!(
        state
            .borrow()
            .pane_sessions
            .get(&1)
            .map(|session| session.path.clone()),
        Some(target)
    );
}

/// Picking a branch is `git checkout`, run in the pane's shell, and the pill
/// moves to the branch the user asked for at once.
#[test]
fn the_branch_pill_checks_the_branch_out_in_the_panes_own_shell() {
    let (state, actions, captured, _dir) = harness();

    (actions.on_select_branch.borrow_mut())(1, "feature/x".to_string());

    assert_eq!(written(&captured), "git checkout 'feature/x'\n");
    let s = state.borrow();
    assert_eq!(s.pane_controls(1).branch, "feature/x");
    assert!(!*s.pane_controls(1).branch_menu_open.borrow());
    assert_eq!(s.composer_branch, "feature/x", "the active pane's pill");
}

/// With no shell there is nothing to check anything out in, so the pill is left
/// naming the branch the repository is really on rather than one that was never
/// checked out.
#[test]
fn a_pane_with_no_shell_does_not_move_its_branch() {
    let (state, actions, captured, _dir) = harness();
    state.borrow_mut().terminal.borrow_mut().sessions.remove(&1);
    let before = state.borrow().pane_controls(1).branch.clone();

    (actions.on_select_branch.borrow_mut())(1, "feature/x".to_string());

    assert_eq!(written(&captured), "");
    assert_eq!(
        state.borrow().pane_controls(1).branch,
        before,
        "the pill still names the branch the repository is on"
    );
}
