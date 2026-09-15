//! What the composer's directory and branch pills do to a pane's shell: a
//! selection is the command the user would have typed, written into that pane's
//! own pty, and the pane's recorded directory follows so its pill moves with the
//! shell it just moved.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_terminal::hooks::{InitShellValue, PrecmdValue};
use goble_terminal::HookEvent;
use goble_ui::platform::WindowControl;

use super::*;
use crate::actions::make_actions;
use crate::emulator::Emulator;
use crate::media::MediaState;
use crate::state::UiState;
use crate::terminal::TerminalSession;
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

/// A `cd` typed at the pane's own prompt prints nothing, so the pane's directory
/// pill learns about it from the shell-integration channel: the `Precmd` hook
/// reports the directory its prompt is in, and the pane's recorded path — what
/// the pill draws — follows it. Nothing is typed into the pty to find out.
#[test]
fn a_reported_cwd_moves_the_panes_directory() {
    let (state, actions, captured, _dir) = harness();

    // The shell was started in one directory; `cd ..` moved it to `/b`, and the
    // prompt that followed reported where it now is.
    hook_cwd(&state, HookEvent::InitShell(InitShellValue {
        cwd: Some("/a".to_string()),
        ..Default::default()
    }));
    state.borrow_mut().adopt_reported_cwds();
    assert_eq!(state.borrow().composer_path, "/a");

    hook_cwd(&state, HookEvent::Precmd(PrecmdValue {
        pwd: Some("/b".to_string()),
        ..Default::default()
    }));
    state.borrow_mut().adopt_reported_cwds();

    let s = state.borrow();
    assert_eq!(
        s.composer_path, "/b",
        "the rich input's directory pill follows the pane's own shell"
    );
    assert_eq!(
        s.pane_sessions.get(&1).map(|session| session.path.clone()),
        Some("/b".to_string()),
        "the pane's recorded directory is what the pill draws"
    );
    assert_eq!(
        written(&captured),
        "",
        "learning the directory writes nothing into the pty"
    );
    let _ = actions;
}

/// The same report over OSC 7, the other carrier of a shell's working
/// directory, moves the same record.
#[test]
fn a_reported_cwd_over_osc_7_moves_the_panes_directory() {
    let (state, _actions, _captured, _dir) = harness();

    {
        let s = state.borrow_mut();
        let mut reg = s.terminal.borrow_mut();
        let session = reg.sessions.get_mut(&1).expect("the pane's session");
        feed_bytes(session, b"\x1b]7;file://host/b\x07");
    }
    state.borrow_mut().adopt_reported_cwds();

    assert_eq!(state.borrow().composer_path, "/b");
}

/// A directory chosen from the pill's own menu is already the pane's directory,
/// so the shell reporting the same path back leaves it — and the menu's write —
/// alone.
#[test]
fn a_report_of_the_directory_the_pane_already_has_changes_nothing() {
    let (state, actions, _captured, _dir) = harness();
    let home = tempfile::tempdir().expect("temp home");
    let target = home.path().to_string_lossy().to_string();

    (actions.on_select_dir.borrow_mut())(1, target.clone());
    hook_cwd(&state, HookEvent::Precmd(PrecmdValue {
        pwd: Some(target.clone()),
        ..Default::default()
    }));
    state.borrow_mut().adopt_reported_cwds();

    let s = state.borrow();
    assert_eq!(s.composer_path, target);
    assert!(!*s.pane_controls(1).dir_menu_open.borrow());
}

/// A directory chosen from the pill's own menu is the pane's directory the
/// moment it is chosen, and the pane's shell is told to `cd` there. Until that
/// `cd` runs the only directory the shell has reported is the one it was in
/// before — which must not take the choice back.
#[test]
fn a_directory_chosen_from_the_menu_is_not_reverted_while_its_cd_runs() {
    let (state, actions, captured, _dir) = harness();

    // The shell has been sitting in /a and its prompt reported it; the app has
    // not looked at that report yet.
    hook_cwd(&state, HookEvent::Precmd(PrecmdValue {
        pwd: Some("/a".to_string()),
        ..Default::default()
    }));

    (actions.on_select_dir.borrow_mut())(1, "/b".to_string());
    state.borrow_mut().adopt_reported_cwds();

    assert_eq!(
        state.borrow().composer_path,
        "/b",
        "the shell's older report does not take back the directory the user chose"
    );

    // The prompt that followed the `cd` reports where the shell now is.
    hook_cwd(&state, HookEvent::Precmd(PrecmdValue {
        pwd: Some("/b".to_string()),
        ..Default::default()
    }));
    state.borrow_mut().adopt_reported_cwds();

    let s = state.borrow();
    assert_eq!(s.composer_path, "/b");
    assert_eq!(
        s.pane_sessions.get(&1).map(|session| session.path.clone()),
        Some("/b".to_string())
    );
    assert_eq!(
        written(&captured),
        "cd '/b'\n",
        "the choice ran in the pane's own shell"
    );
}

/// A pane opened at a chosen directory keeps it while its shell starts. The
/// shell behind such a pane is started without the app naming a directory, so
/// the first directory it reports is the one it picked for itself — the process
/// cwd, usually the home directory — which is not where the pane was opened.
#[test]
fn a_shells_start_does_not_move_a_pane_opened_at_a_chosen_directory() {
    let (state, actions, _captured, _dir) = harness();
    let project = tempfile::tempdir().expect("temp project");
    let project = project.path().to_string_lossy().to_string();

    // The pane was opened at the project's directory, and a shell was started
    // behind it afterwards.
    (actions.on_select_dir.borrow_mut())(1, project.clone());
    {
        let s = state.borrow();
        s.terminal
            .borrow_mut()
            .sessions
            .insert(1, TerminalSession::started(Emulator::new(80, 24)));
    }

    hook_cwd(&state, HookEvent::InitShell(InitShellValue {
        cwd: Some("/Users/me".to_string()),
        ..Default::default()
    }));
    state.borrow_mut().adopt_reported_cwds();

    assert_eq!(
        state.borrow().composer_path,
        project,
        "a shell's start is not where the pane was opened"
    );

    // A `cd` typed at that shell's prompt still moves the pane.
    hook_cwd(&state, HookEvent::Precmd(PrecmdValue {
        pwd: Some(format!("{project}/src")),
        ..Default::default()
    }));
    state.borrow_mut().adopt_reported_cwds();

    assert_eq!(state.borrow().composer_path, format!("{project}/src"));
}

/// Feed a hook into pane 1's session and let the frame's pump see it.
fn hook_cwd(state: &Rc<RefCell<UiState>>, event: HookEvent) {
    let s = state.borrow_mut();
    let mut reg = s.terminal.borrow_mut();
    let session = reg.sessions.get_mut(&1).expect("the pane's session");
    run_hook(session, event);
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
