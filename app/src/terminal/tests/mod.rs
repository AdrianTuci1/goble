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

mod agents;
mod claims;
mod input;
mod keys;
mod pane_commands;
mod registry;
mod session;
mod shell;

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
