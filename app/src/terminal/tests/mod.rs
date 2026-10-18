//! Terminal-session behaviour: the pane's screen snapshot, the claim
//! protocol, the harness's shell-tool route and the per-pane registry. The
//! cases are grouped the way the module they exercise is: [`keys`], [`input`],
//! [`shell`], [`agents`], [`session`], [`claims`], [`pane_commands`] and
//! [`registry`]. The fixtures every group shares are declared here.

use std::io::Write;
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;

use goble_terminal::hooks::PreexecValue;
use goble_terminal::{BlockOwner, HookEvent};

use crate::emulator::Emulator;
use crate::terminal::claim::PaneCommandRequest;

use super::*;

#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use goble_pty::protocol::{decode_bytes, Request, ServerMessage, SessionInfo};
#[cfg(unix)]
use goble_pty::{Client, Server};

mod agents;
mod claims;
mod context_pills;
mod copy;
mod daemon;
mod input;
mod keys;
mod pane_commands;
mod registry;
mod session;
mod shell;
mod ssh_session;

    /// A session whose screen the test drives directly, with no pty behind it.
    fn detached() -> TerminalSession {
        TerminalSession::with_emulator(Emulator::new(80, 24))
    }

    /// A capture for the bytes a session writes to its pty.
    #[derive(Clone, Default)]
    struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn agent_owner() -> BlockOwner {
        BlockOwner::Agent {
            conversation_id: "conv-1".to_string(),
            call_id: "call-1".to_string(),
        }
    }

    fn preexec(command: &str) -> HookEvent {
        HookEvent::Preexec(PreexecValue {
            command: Some(command.to_string()),
        })
    }

    /// Feed a hook into the session's own pty tap, exactly as the shell would,
    /// and let the frame's `pump` see it.
    fn run_hook(session: &mut TerminalSession, event: HookEvent) {
        {
            let mut state = session.state.lock().unwrap();
            state.feed(&goble_terminal::hooks::encode_hook(&event));
        }
        session.pump();
    }

    /// Feed pty bytes and let the frame's `pump` see them, as the reader thread
    /// does for a real pane.
    fn feed_bytes(session: &mut TerminalSession, bytes: &[u8]) {
        {
            let mut state = session.state.lock().unwrap();
            state.feed(bytes);
        }
        session.pump();
    }

    /// The request the harness submits through a pane session.
    fn submit(
        session: &mut TerminalSession,
        line: &str,
    ) -> oneshot::Receiver<Result<String, String>> {
        let (reply, answer) = oneshot::channel();
        session.commands.lock().unwrap().request = Some(PaneCommandRequest {
            line: line.to_string(),
            conversation_id: "conv-1".to_string(),
            call_id: "call-1".to_string(),
            reply,
        });
        answer
    }

    /// Run `condition` until it holds, bounded. The daemon is a real process
    /// boundary even when it runs in this one, so events are awaited, never
    /// assumed to have already arrived.
    fn wait_for(mut condition: impl FnMut() -> bool) -> bool {
        for _ in 0..400 {
            if condition() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        false
    }

    /// An in-process PTY daemon on a temp socket, so the app tests exercise
    /// the real protocol without starting a daemon binary.
    #[cfg(unix)]
    struct TestDaemon {
        _dir: tempfile::TempDir,
        socket: PathBuf,
        server: Option<Server>,
    }

    #[cfg(unix)]
    impl TestDaemon {
        fn start() -> Self {
            // `default_shell` prefers `$SHELL`; the test process may not have
            // one, and the fallback is zsh. Use the bash this machine has.
            // Set once: the tests start daemons in parallel.
            static SHELL: std::sync::Once = std::sync::Once::new();
            SHELL.call_once(|| {
                if Path::new("/bin/bash").exists() {
                    std::env::set_var("SHELL", "/bin/bash");
                }
            });
            let dir = tempfile::tempdir().expect("create a daemon temp dir");
            let socket = dir.path().join("pty.sock");
            let server = Server::start(&socket).expect("start the PTY daemon");
            Self {
                _dir: dir,
                socket,
                server: Some(server),
            }
        }

        fn connect(&self) -> Client {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let mut client = Client::connect(&self.socket).expect("connect to the daemon");
            client
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .expect("set a read timeout");
            let id = client.next_id();
            let hello = client
                .request(Request::Hello {
                    id,
                    client_id: format!("test-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
                })
                .expect("hello the daemon");
            assert!(matches!(hello, ServerMessage::Ok { .. }), "{hello:?}");
            client
        }

        fn create(
            &self,
            pane_id: &str,
            program: &str,
            args: Vec<String>,
            env: Vec<(String, String)>,
            cwd: &Path,
        ) -> SessionInfo {
            let mut client = self.connect();
            let id = client.next_id();
            match client
                .request(Request::Create {
                    id,
                    pane_id: pane_id.to_string(),
                    cwd: cwd.display().to_string(),
                    cols: 80,
                    rows: 24,
                    shell: Some(program.to_string()),
                    args,
                    env,
                })
                .expect("create a daemon session")
            {
                ServerMessage::Ok {
                    session: Some(info),
                    ..
                } => info,
                other => panic!("create failed: {other:?}"),
            }
        }

        fn open(&self, pane_id: u64, cwd: &str, session_id: Option<&str>) -> TerminalSession {
            TerminalSession::spawn_at(&self.socket, pane_id, cwd, session_id)
                .expect("open a pane session through the daemon")
        }

        fn list(&self) -> Vec<SessionInfo> {
            let mut client = self.connect();
            let id = client.next_id();
            match client
                .request(Request::List { id })
                .expect("list daemon sessions")
            {
                ServerMessage::Sessions { sessions, .. } => sessions,
                other => panic!("list failed: {other:?}"),
            }
        }

        /// Attach from byte zero and wait until `needle` has been replayed.
        /// Used to prove output exists in the ring before the pane attaches.
        fn wait_for_output(&self, session_id: &str, needle: &[u8]) -> Vec<u8> {
            let mut client = self.connect();
            let id = client.next_id();
            client
                .request(Request::Attach {
                    id,
                    session_id: session_id.to_string(),
                    from_seq: 0,
                })
                .expect("attach to the daemon session");
            client
                .set_read_timeout(Some(std::time::Duration::from_millis(100)))
                .expect("set the polling timeout");
            let mut seen = Vec::new();
            for _ in 0..400 {
                match client.next_message() {
                    Ok(ServerMessage::Output { bytes_b64, .. }) => {
                        seen.extend(decode_bytes(&bytes_b64).expect("decode output"));
                        if seen.windows(needle.len()).any(|window| window == needle) {
                            return seen;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
                }
            }
            panic!(
                "timed out waiting for {:?}; saw {:?}",
                String::from_utf8_lossy(needle),
                String::from_utf8_lossy(&seen)
            );
        }
    }

    #[cfg(unix)]
    impl Drop for TestDaemon {
        fn drop(&mut self) {
            drop(self.server.take());
        }
    }
