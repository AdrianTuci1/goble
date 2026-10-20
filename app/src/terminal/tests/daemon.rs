//! Panes run on the PTY daemon: the registry reattaches to a persisted
//! session, dropping detaches while closing kills, and shell-integration hooks
//! arrive from the daemon's replay just as they do live.

use super::*;

use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_terminal::hooks::{encode_hook, PrecmdValue};
use goble_terminal::HookEvent;

use crate::state::UiState;

/// Two panes (as two app instances would be) can attach to one daemon session.
/// The second attach is a broadcast subscriber, not a takeover: both see live
/// output and dropping either leaves the other attached and the shell alive.
#[test]
fn a_second_client_attaches_without_stealing_or_killing_the_session() {
    let daemon = TestDaemon::start();
    let shell_cwd = tempfile::tempdir().expect("create a shell cwd");
    let cwd = shell_cwd.path().to_str().expect("a path");

    let mut first = daemon.open(1, cwd, None);
    let session_id = first
        .session_id()
        .expect("the first pane has a daemon session")
        .to_string();
    let mut second = daemon.open(1, cwd, Some(&session_id));
    assert_eq!(
        second.session_id(),
        Some(session_id.as_str()),
        "the second client attached to the existing session"
    );
    assert!(first.is_alive() && second.is_alive(), "neither attach killed it");
    assert_eq!(
        daemon
            .list()
            .iter()
            .filter(|session| session.alive)
            .count(),
        1,
        "both clients share one shell"
    );

    first.write(b"printf 'F5B-SHARED\\n'\n");
    assert!(
        wait_for(|| {
            first.pump();
            second.pump();
            first.snapshot(48).lines.iter().any(|line| line.contains("F5B-SHARED"))
                && second
                    .snapshot(48)
                    .lines
                    .iter()
                    .any(|line| line.contains("F5B-SHARED"))
        }),
        "both clients receive the same broadcast output"
    );

    // The second client leaving is a detach, not a kill, and not the first
    // client's detach either.
    drop(second);
    first.write(b"printf 'F5B-STILL-HERE\\n'\n");
    assert!(
        wait_for(|| {
            first.pump();
            first
                .snapshot(48)
                .lines
                .iter()
                .any(|line| line.contains("F5B-STILL-HERE"))
        }),
        "the first client stays attached after the second leaves; screen held:\n{}",
        first.snapshot(48).lines.join("\n")
    );
    assert_eq!(
        daemon
            .list()
            .iter()
            .filter(|session| session.alive)
            .count(),
        1,
        "the shell survived the second client's detach"
    );
}

/// The id a restored pane persisted is reused while the daemon still has that
/// session alive, so an app restart lands back in the same shell.
#[test]
fn ensure_session_reuses_the_persisted_daemon_session() {
    let daemon = TestDaemon::start();
    let shell_cwd = tempfile::tempdir().expect("create a shell cwd");
    let cwd = shell_cwd.path().to_str().expect("a path");

    // A session the previous run left behind: attach once, then detach.
    let first = daemon.open(1, cwd, None);
    let session_id = first.session_id().expect("a daemon session").to_string();
    drop(first);
    assert_eq!(
        daemon
            .list()
            .iter()
            .filter(|session| session.alive)
            .count(),
        1,
        "the detached shell is still running"
    );

    // Persist the id the way the pane state does, then restore it.
    let dir = tempfile::tempdir().expect("create a thread store dir");
    let desktop = DesktopState::new(
        Store::open_in_memory().expect("open in-memory store"),
        ThreadStore::new(dir.path()).expect("open thread store"),
    );
    let mut saved = UiState::mock();
    saved
        .pane_sessions
        .get_mut(&1)
        .expect("pane 1 has a session")
        .daemon_session_id = session_id.clone();
    saved.save_panes(&desktop);

    let mut restored = UiState::mock();
    restored.restore_panes(&desktop);
    restored.terminal.borrow_mut().use_socket(daemon.socket.clone());
    assert_eq!(
        restored
            .terminal
            .borrow()
            .session_ids
            .get(&1)
            .map(String::as_str),
        Some(session_id.as_str()),
        "the restored pane remembered its daemon session"
    );

    restored
        .terminal
        .borrow_mut()
        .ensure_session(1, cwd);
    let terminal = restored.terminal.borrow();
    let session = terminal.sessions.get(&1).expect("the pane has a session");
    assert_eq!(session.session_id(), Some(session_id.as_str()));
    assert!(session.is_alive(), "the reattached shell is live");
    assert_eq!(daemon.list().len(), 1, "no second shell was started");
}

/// A session the daemon no longer holds is replaced rather than reused: a
/// stale persisted id must not leave the pane attached to nothing.
#[test]
fn a_gone_daemon_session_gets_a_fresh_one() {
    let daemon = TestDaemon::start();
    let shell_cwd = tempfile::tempdir().expect("create a shell cwd");
    let cwd = shell_cwd.path().to_str().expect("a path");

    let mut reg = TerminalRegistry::default();
    reg.use_socket(daemon.socket.clone());
    reg.adopt_session_id(1, "session-that-is-gone".to_string());
    reg.ensure_session(1, cwd);

    let session = reg.sessions.get(&1).expect("the pane has a session");
    let fresh = session.session_id().expect("a fresh daemon session");
    assert_ne!(fresh, "session-that-is-gone");
    assert!(session.is_alive());
    let sessions = daemon.list();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, fresh);
}

/// Dropping a pane's session detaches: the daemon keeps the shell for the next
/// attach. This is what adoption into a viewer pane does, and what an app exit
/// does.
#[test]
fn drop_pane_detaches_rather_than_kills() {
    let daemon = TestDaemon::start();
    let shell_cwd = tempfile::tempdir().expect("create a shell cwd");
    let cwd = shell_cwd.path().to_str().expect("a path");

    let mut reg = TerminalRegistry::default();
    reg.use_socket(daemon.socket.clone());
    let session = daemon.open(1, cwd, None);
    let session_id = session.session_id().expect("a daemon session").to_string();
    reg.sessions.insert(1, session);

    reg.drop_pane(1);
    assert!(reg.sessions.is_empty());

    let sessions = daemon.list();
    assert_eq!(sessions.len(), 1, "the shell outlives the pane's session");
    assert_eq!(sessions[0].session_id, session_id);
    assert!(sessions[0].alive, "and is still running");
}

/// Closing a pane kills its shell. `UiState::forget_pane` is the close path
/// (pane close and tab close both use it).
#[test]
fn closing_a_pane_kills_its_daemon_session() {
    let daemon = TestDaemon::start();
    let shell_cwd = tempfile::tempdir().expect("create a shell cwd");
    let cwd = shell_cwd.path().to_str().expect("a path");

    let mut state = UiState::mock();
    state
        .terminal
        .borrow_mut()
        .use_socket(daemon.socket.clone());
    let session = daemon.open(1, cwd, None);
    let session_id = session.session_id().expect("a daemon session").to_string();
    state.terminal.borrow_mut().sessions.insert(1, session);

    state.forget_pane(1);
    assert!(
        !state.terminal.borrow().sessions.contains_key(&1),
        "the pane's session is gone with it"
    );
    assert!(
        wait_for(|| daemon
            .list()
            .iter()
            .all(|session| session.session_id != session_id)),
        "the daemon killed the shell instead of only detaching"
    );
}

/// Shell-integration hooks are bytes on the PTY stream. The daemon replays the
/// bytes produced before the pane attached, and the app's own parser turns them
/// into the handshake and the reported directory — the claim protocol and the
/// block list keep working through the daemon.
#[test]
fn shell_integration_hooks_reach_the_emulator_from_replayed_bytes() {
    let daemon = TestDaemon::start();
    let shell_cwd = tempfile::tempdir().expect("create a shell cwd");

    let hooks: Vec<u8> = [
        HookEvent::Bootstrapped(Default::default()),
        HookEvent::Precmd(PrecmdValue {
            pwd: Some("/replayed".to_string()),
            ..Default::default()
        }),
    ]
    .iter()
    .flat_map(encode_hook)
    .collect();
    let hook_file = shell_cwd.path().join("hooks.bin");
    std::fs::write(&hook_file, hooks).expect("write the hook bytes");

    // The shell emits the hooks and a plain marker, then stays alive. Waiting
    // for the marker proves the bytes reached the daemon before the pane
    // attached below, so the pane's own view comes from a replay.
    let info = daemon.create(
        "pane-replay",
        "/bin/sh",
        vec![
            "-c".to_string(),
            "cat \"$GOBLE_HOOKS\"; printf 'F1B-READY\\n'; exec sleep 30".to_string(),
        ],
        vec![(
            "GOBLE_HOOKS".to_string(),
            hook_file.display().to_string(),
        )],
        shell_cwd.path(),
    );
    daemon.wait_for_output(&info.session_id, b"F1B-READY");

    let mut session = daemon.open(
        7,
        shell_cwd.path().to_str().expect("a path"),
        Some(&info.session_id),
    );
    assert_eq!(
        session.session_id(),
        Some(info.session_id.as_str()),
        "the pane attached to the existing session"
    );

    let parsed = wait_for(|| {
        session.pump();
        session.is_bootstrapped() && session.reported_cwd() == Some("/replayed")
    });
    assert!(
        parsed,
        "the replayed hooks must reach the emulator; screen held:\n{}",
        session
            .snapshot(48)
            .lines
            .join("\n")
    );
    assert!(
        session
            .hooks()
            .iter()
            .any(|hook| matches!(hook, HookEvent::Bootstrapped(_))),
        "the bootstrapped handshake came through the replay"
    );
}

/// A pane that detaches and comes back sees the command it ran and that
/// command's output again: the daemon replays the bytes, and the fresh
/// emulator rebuilds the same block from the hooks in them.
#[test]
fn a_reattached_pane_rebuilds_the_command_and_its_output_from_the_replay() {
    use goble_terminal::blocks::BlockView;
    use goble_terminal::hooks::{CommandFinishedValue, PreexecValue};

    let daemon = TestDaemon::start();
    let shell_cwd = tempfile::tempdir().expect("create a shell cwd");

    let stream: Vec<u8> = [
        encode_hook(&HookEvent::Bootstrapped(Default::default())),
        encode_hook(&HookEvent::Precmd(PrecmdValue {
            pwd: Some("/replayed".to_string()),
            ..Default::default()
        })),
        encode_hook(&HookEvent::Preexec(PreexecValue {
            command: Some("echo persisted".to_string()),
        })),
        b"persisted output\r\n".to_vec(),
        encode_hook(&HookEvent::CommandFinished(CommandFinishedValue {
            exit_code: 0,
            ..Default::default()
        })),
    ]
    .concat();
    let script_file = shell_cwd.path().join("stream.bin");
    std::fs::write(&script_file, stream).expect("write the replay stream");

    let info = daemon.create(
        "pane-replay-output",
        "/bin/sh",
        vec![
            "-c".to_string(),
            "cat \"$GOBLE_HOOKS\"; printf 'F5C-READY\\n'; exec sleep 30".to_string(),
        ],
        vec![(
            "GOBLE_HOOKS".to_string(),
            script_file.display().to_string(),
        )],
        shell_cwd.path(),
    );
    daemon.wait_for_output(&info.session_id, b"F5C-READY");

    let mut session = daemon.open(
        8,
        shell_cwd.path().to_str().expect("a path"),
        Some(&info.session_id),
    );
    let rebuilt = wait_for(|| {
        session.pump();
        session.visible_blocks(&BlockView::Terminal).iter().any(|block| {
            block.command.contains("echo persisted") && block.output.contains("persisted output")
        })
    });
    assert!(
        rebuilt,
        "the replayed block keeps its command and output; blocks held: {:?}",
        session
            .visible_blocks(&BlockView::Terminal)
            .iter()
            .map(|block| (block.command.clone(), block.output.clone()))
            .collect::<Vec<_>>()
    );
}
