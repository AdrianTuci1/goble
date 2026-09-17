//! Where a submitted line leaves the pane's shell: an interactive `ssh` command
//! binds the pane to the host it names — `host`, `user@host`, `-p 2222 host` or
//! an `~/.ssh/config` alias — and `exit` returns it to a local shell. The line
//! still runs in the pane's own pty either way; only the session's account of
//! where it is moves.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use goble_core::ssh_command::SshSession;
use goble_core::ssh_hosts::current_user;
use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::platform::WindowControl;

use super::*;
use crate::actions::make_actions;
use crate::media::MediaState;
use crate::state::UiState;
use crate::ui::UiActions;

/// The fixture machine's `~/.ssh/config`.
const CONFIG: &str = "\
Host web
    HostName web.example.com
    User deploy
    Port 2222
";

/// A single-pane app whose shell is a capture instead of a pty and whose
/// `~/.ssh` is a fixture, so a submitted line's write and its binding are both
/// readable. The returned temp dirs must outlive the state.
struct Harness {
    state: Rc<RefCell<UiState>>,
    actions: UiActions,
    captured: Arc<Mutex<Vec<u8>>>,
    _store_dir: tempfile::TempDir,
    _home: tempfile::TempDir,
}

fn harness() -> Harness {
    let store_dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = Arc::new(DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(store_dir.path()).expect("thread store"),
    ));
    let home = tempfile::tempdir().expect("temp home");
    std::fs::create_dir_all(home.path().join(".ssh")).expect("fixture .ssh");
    std::fs::write(home.path().join(".ssh/config"), CONFIG).expect("fixture config");

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
        s.settings_ssh_home = Some(home.path().to_path_buf());
        let mut session = detached();
        session.writer = Some(Box::new(CaptureWriter(Arc::clone(&captured))));
        s.terminal.borrow_mut().sessions.insert(1, session);
        s.active_pane_id = 1;
    }
    Harness {
        state,
        actions,
        captured,
        _store_dir: store_dir,
        _home: home,
    }
}

impl Harness {
    /// Submit `command` at pane 1's own bar — the path the pane's rich input
    /// takes on Enter.
    fn submit(&mut self, command: &str) {
        (self.actions.on_run_shell_command.borrow_mut())(1, command.to_string());
    }

    fn binding(&self) -> Option<SshSession> {
        self.state
            .borrow()
            .terminal
            .borrow()
            .ssh_session(1)
            .cloned()
    }

    fn written(&self) -> String {
        String::from_utf8(self.captured.lock().unwrap().clone()).expect("the pty took text")
    }

    /// Everything the pane's session has on its screen, as one string.
    fn screen(&self) -> String {
        self.state
            .borrow()
            .terminal
            .borrow()
            .sessions
            .get(&1)
            .map(|session| {
                session
                    .view()
                    .rows
                    .iter()
                    .map(|row| row.text())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    }

    /// The digits the shell printed right after `prefix`, once they are on the
    /// screen — what `$$` expanded to. The wait is bounded; the caller says what
    /// the screen held when the shell never answered.
    fn wait_for_pid(&self, prefix: &str) -> Option<String> {
        for _ in 0..240 {
            if let Some(pid) = pid_after(&self.screen(), prefix) {
                return Some(pid);
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        None
    }
}

/// The digits a shell printed after `prefix` in `text`: the first occurrence
/// that is followed by digits, since the echoed input line carries the literal
/// `$$` and only the output carries what it expanded to.
fn pid_after(text: &str, prefix: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = text[from..].find(prefix) {
        let rest = &text[from + at + prefix.len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            return Some(digits);
        }
        from += at + prefix.len();
    }
    None
}

#[test]
fn an_interactive_ssh_line_binds_the_pane_to_the_host_it_names() {
    let mut h = harness();

    h.submit("ssh host");
    let bound = h.binding().expect("a bare host binds");
    assert_eq!(bound.host, "host");
    assert_eq!(bound.user, current_user());
    assert_eq!(bound.port, 22);
    assert_eq!(bound.alias, None);
    assert_eq!(
        h.written(),
        "ssh host\n",
        "the line still ran in the pane's own shell"
    );

    h.submit("ssh ada@server.example.com");
    let bound = h.binding().expect("user@host binds");
    assert_eq!(bound.host, "server.example.com");
    assert_eq!(bound.user, "ada");
    assert_eq!(bound.port, 22);

    h.submit("ssh -p 2222 host");
    let bound = h.binding().expect("-p binds");
    assert_eq!(bound.host, "host");
    assert_eq!(bound.port, 2222);
}

#[test]
fn an_alias_from_the_machines_config_binds_to_its_real_address() {
    let mut h = harness();

    h.submit("ssh web");

    let bound = h.binding().expect("an alias binds");
    assert_eq!(bound.host, "web.example.com");
    assert_eq!(bound.user, "deploy");
    assert_eq!(bound.port, 2222);
    assert_eq!(bound.alias.as_deref(), Some("web"));

    // What the line names itself wins over the block.
    h.submit("ssh -p 2200 ada@web");
    let bound = h.binding().expect("the alias is still the host");
    assert_eq!(bound.host, "web.example.com");
    assert_eq!(bound.user, "ada");
    assert_eq!(bound.port, 2200);
}

#[test]
fn a_line_that_only_mentions_ssh_binds_nothing() {
    let mut h = harness();

    for command in [
        // Another program entirely, however it is spelled.
        "scp -p 2222 file web:/tmp",
        "sftp web",
        "grep ssh /etc/hosts",
        "sudo ssh web",
        "echo \"ssh web\"",
        // ssh itself, asked not to start a session.
        "ssh -V",
        "ssh -o BatchMode=yes web uptime",
        "ssh web uptime",
    ] {
        h.submit(command);
        assert_eq!(h.binding(), None, "{command} is not a session");
    }

    // Every one of them ran in the pane's shell regardless.
    let written = h.written();
    assert!(written.contains("sudo ssh web"), "{written}");
    assert!(written.contains("scp -p 2222 file web:/tmp"), "{written}");
}

#[test]
fn exit_returns_the_pane_to_a_local_shell_and_other_lines_leave_it_alone() {
    let mut h = harness();

    h.submit("ssh web");
    assert!(h.binding().is_some(), "the pane is on the host");

    // A command typed at the remote shell says nothing about coming back, and
    // whether it ran locally or remotely cannot be read from the line.
    h.submit("ls -la");
    assert_eq!(
        h.binding().map(|session| session.host),
        Some("web.example.com".to_string()),
        "an ordinary command does not unbind the pane"
    );

    h.submit("exit");
    assert_eq!(h.binding(), None, "exit is the way back to the local shell");

    // And a second `exit` at an already-local shell changes nothing.
    h.submit("exit");
    assert_eq!(h.binding(), None);
}

/// S3: the identity the switch must not change, measured on a real pty. The
/// same shell process carries the pane into agent mode and back out — `$$` is
/// that shell's own pid, so the same digits before and after are the same
/// process, and the output it printed before the switch is still on the pane
/// afterwards. A restarted pty, a second session or a reconnected `ssh` would
/// show another pid or an empty screen.
#[test]
fn the_pane_keeps_its_own_shell_process_across_the_switch() {
    // The developer's own rc has nothing to do with the pane's shell and would
    // only add startup to the waits below. No other test in this binary starts
    // a shell.
    std::env::set_var("GOBLE_SKIP_USER_RC", "1");
    let mut h = harness();
    let dir = tempfile::tempdir().expect("temp dir");
    h.state.borrow_mut().terminal.borrow_mut().sessions.insert(
        1,
        TerminalSession::spawn(dir.path().to_str().expect("a path"))
            .expect("a shell to run the pane"),
    );

    // The pane is on a host, which is what the `ssh web` line leaves behind
    // (pinned by `an_interactive_ssh_line_binds_the_pane_to_the_host_it_names`).
    // There is no peer to connect to here, so the binding is set directly.
    let host = SshSession {
        host: "web.example.com".to_string(),
        user: "deploy".to_string(),
        port: 2222,
        alias: Some("web".to_string()),
    };
    h.state
        .borrow_mut()
        .terminal
        .borrow_mut()
        .set_ssh_session(1, Some(host.clone()));

    // The shell's own pid, printed by the shell itself.
    h.submit("echo S3-FIRST-$$");
    let first = h.wait_for_pid("S3-FIRST-").unwrap_or_else(|| {
        panic!(
            "the pane's shell never answered; its screen held:\n{}",
            h.screen()
        )
    });

    // Cmd+Enter's switch, and Esc's way back out: the app's own two actions.
    (h.actions.on_set_pane_harness_mode.borrow_mut())(1, true);
    assert!(
        h.state.borrow().pane_controls(1).harness_mode,
        "the pane is in agent mode"
    );
    assert!(
        h.screen().contains(&format!("S3-FIRST-{first}")),
        "the pane's earlier content survived the switch:\n{}",
        h.screen()
    );
    (h.actions.on_set_pane_harness_mode.borrow_mut())(1, false);
    assert!(
        !h.state.borrow().pane_controls(1).harness_mode,
        "Esc's switch put the pane back on its shell"
    );
    assert_eq!(
        h.binding().as_ref(),
        Some(&host),
        "the session is the one it was: the binding never moved"
    );

    // The same shell answers again, and it is the same process.
    h.submit("echo S3-SECOND-$$");
    let second = h.wait_for_pid("S3-SECOND-").unwrap_or_else(|| {
        panic!(
            "the pane's shell never answered again; its screen held:\n{}",
            h.screen()
        )
    });
    assert_eq!(
        second, first,
        "the same shell process carried the pane through the switch"
    );
    assert!(
        h.screen().contains(&format!("S3-FIRST-{first}")),
        "and the content from before the switch is still there:\n{}",
        h.screen()
    );
}

#[test]
fn closing_the_pane_drops_its_binding() {
    let mut h = harness();
    h.submit("ssh web");
    assert!(h.binding().is_some());

    h.state.borrow_mut().terminal.borrow_mut().drop_pane(1);

    assert_eq!(h.binding(), None, "the binding goes with the pane");
}
