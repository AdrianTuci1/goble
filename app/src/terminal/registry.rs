use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use goble_core::harness::PaneSession;
use goble_core::ssh_command::SshSession;
use goble_terminal::blocks::BlockView;
use goble_terminal::BlockId;

use crate::emulator::VisibleBlock;

use super::agents::{TerminalMode, TuiAgent};
use super::claim::PaneCommandRunner;
use super::session::TerminalSession;

/// Per-pane registry of live terminal sessions + the local input mirrors used
/// to route `Cmd+Enter` to the agent. Owned by the app state so panes can
/// split/close/persist it like any other pane.
#[derive(Debug, Default)]
pub struct TerminalRegistry {
    pub sessions: HashMap<u64, TerminalSession>,
    pub input: HashMap<u64, String>,
    /// Per-pane surface mode (shell vs. a TUI agent). Owned here so it survives
    /// the per-frame element rebuild and is dropped with the pane.
    pub modes: HashMap<u64, TerminalMode>,
    /// The SSH session each pane's shell is on, keyed by pane id, from the
    /// `ssh` line the user submitted in it. Absent for a pane at a local shell,
    /// so a pane whose command opened no session carries no binding.
    pub ssh_sessions: HashMap<u64, SshSession>,
    /// The daemon session each pane is attached to, keyed by pane id. Seeded
    /// from the persisted pane state at startup and kept current whenever a
    /// session is spawned or reattached, so the next save persists the id.
    pub session_ids: HashMap<u64, String>,
    /// The daemon socket to talk to. `None` uses the default socket and starts
    /// the daemon when nothing is listening; tests point this at an in-process
    /// daemon on a temp socket.
    pub socket: Option<PathBuf>,
    /// Per-pane copy mode, selection and find-bar state. Owned here for the
    /// same reason the sessions are: the pane's element tree is rebuilt every
    /// frame, and the state has to outlive it.
    pub ui: HashMap<u64, super::PaneTerminalState>,
}

impl TerminalRegistry {
    /// The host the pane's shell is on, or `None` when it is local.
    pub fn ssh_session(&self, pane_id: u64) -> Option<&SshSession> {
        self.ssh_sessions.get(&pane_id)
    }

    /// Move the pane's shell to [`SshSession`], or back to a local shell with
    /// `None`. What a submitted line means for the pane is read in
    /// `run_terminal_command`, which owns the command path; this is the storage.
    pub fn set_ssh_session(&mut self, pane_id: u64, session: Option<SshSession>) {
        match session {
            Some(session) => {
                self.ssh_sessions.insert(pane_id, session);
            }
            None => {
                self.ssh_sessions.remove(&pane_id);
            }
        }
    }

    /// Point this registry at an existing daemon instead of the default socket.
    pub fn use_socket(&mut self, socket: impl Into<PathBuf>) {
        self.socket = Some(socket.into());
    }

    /// Remember the daemon session a restored pane was last attached to, so
    /// the next [`Self::ensure_session`] reattaches rather than starting over.
    pub fn adopt_session_id(&mut self, pane_id: u64, session_id: String) {
        if !session_id.is_empty() {
            self.session_ids.insert(pane_id, session_id);
        }
    }

    pub fn ensure_session(&mut self, pane_id: u64, cwd: &str) {
        if self.sessions.contains_key(&pane_id) {
            return;
        }
        let persisted = self.session_ids.get(&pane_id).map(String::as_str);
        let spawned = match &self.socket {
            #[cfg(unix)]
            Some(socket) => TerminalSession::spawn_at(socket, pane_id, cwd, persisted),
            #[cfg(not(unix))]
            Some(_) => TerminalSession::spawn_for(pane_id, cwd, persisted),
            None => TerminalSession::spawn_for(pane_id, cwd, persisted),
        };
        match spawned {
            Ok(session) => {
                if let Some(session_id) = session.session_id() {
                    self.session_ids.insert(pane_id, session_id.to_string());
                }
                self.sessions.insert(pane_id, session);
            }
            Err(e) => {
                log::warn!("failed to spawn terminal for pane {pane_id}: {e}");
                self.sessions.insert(pane_id, TerminalSession::failed(e));
            }
        }
    }

    /// Start a fresh shell for a pane whose previous one is gone (a saved-host
    /// pane reconnecting after `exit`). The old daemon session id is dropped so
    /// the spawn never reattaches the shell that just exited.
    pub fn restart_session(&mut self, pane_id: u64, cwd: &str) {
        self.sessions.remove(&pane_id);
        self.session_ids.remove(&pane_id);
        self.ensure_session(pane_id, cwd);
    }

    /// Resize a pane's live session to `cols` × `rows`, keeping the font cell
    /// metrics the pane last reported. Returns false when the pane has no
    /// session.
    pub fn resize(&mut self, pane_id: u64, cols: u16, rows: u16) -> bool {
        match self.sessions.get_mut(&pane_id) {
            Some(session) => {
                session.set_grid_size(rows, cols);
                true
            }
            None => false,
        }
    }

    /// Remove a pane's session, detaching it from the daemon. The shell keeps
    /// running; this is the path a pane takes when it stops being a shell pane,
    /// not when it closes.
    pub fn drop_pane(&mut self, pane_id: u64) {
        self.sessions.remove(&pane_id);
        self.input.remove(&pane_id);
        self.modes.remove(&pane_id);
        self.ssh_sessions.remove(&pane_id);
        self.ui.remove(&pane_id);
    }

    /// Close a pane's shell for good: kill its daemon session, then drop every
    /// entry keyed by the pane. This is the pane-close path.
    pub fn kill_pane(&mut self, pane_id: u64) {
        if let Some(mut session) = self.sessions.remove(&pane_id) {
            session.kill();
        }
        self.session_ids.remove(&pane_id);
        self.input.remove(&pane_id);
        self.modes.remove(&pane_id);
        self.ssh_sessions.remove(&pane_id);
        self.ui.remove(&pane_id);
    }

    pub fn input(&self, pane_id: u64) -> String {
        self.input.get(&pane_id).cloned().unwrap_or_default()
    }

    pub fn set_input(&mut self, pane_id: u64, value: String) {
        self.input.insert(pane_id, value);
    }

    pub fn mode(&self, pane_id: u64) -> TerminalMode {
        self.modes.get(&pane_id).cloned().unwrap_or_default()
    }

    /// Whether a real program owns the pane's screen: a TUI agent running
    /// native-first, or any full-screen program in the alternate screen. While
    /// it does, the pane's own rich input steps aside and the program takes the
    /// keys (native-first), exactly as it does when the agent owns the pty.
    pub fn program_owns_screen(&self, pane_id: u64) -> bool {
        if matches!(self.mode(pane_id), TerminalMode::Agent(_)) {
            return true;
        }
        self.sessions
            .get(&pane_id)
            .map(|session| session.is_alt_screen())
            .unwrap_or(false)
    }

    /// Whether any pane's shell is running a command right now. A running
    /// command means output is still arriving, which is one of the reasons the
    /// window keeps repainting.
    pub fn any_running_command(&self) -> bool {
        self.sessions
            .values()
            .any(|session| session.has_running_command())
    }

    /// Whether the pane's shell is busy with a command of its own — the state
    /// that refuses the switch into agent mode (`open_pane_harness`).
    ///
    /// A pane bound to an SSH session is not busy in this sense: the running
    /// command is the session itself — the local `ssh` process — while the
    /// remote shell it put the user in is idle at a prompt. That pane is the one
    /// the switch exists for, and the reference treats a long-running command
    /// the same way: its `SetInputModeAgent` hands the command to the agent
    /// (tag-in) instead of refusing it, and the refusal is the path that would
    /// leave the command behind. Reading the binding here is what keeps the
    /// switch from refusing the session it was asked to attach the agent to.
    pub fn running_a_command(&self, pane_id: u64) -> bool {
        if self.ssh_sessions.contains_key(&pane_id) {
            return false;
        }
        self.sessions
            .get(&pane_id)
            .map(|session| session.has_running_command())
            .unwrap_or(false)
    }

    /// The blocks a view draws in `pane_id`, oldest first. A pane with no
    /// session has no blocks, so the result is empty.
    pub fn visible_blocks(&self, pane_id: u64, view: &BlockView) -> Vec<VisibleBlock> {
        self.sessions
            .get(&pane_id)
            .map(|session| session.visible_blocks(view))
            .unwrap_or_default()
    }

    /// Push the card standing for `conversation_id` into `pane_id`'s block list.
    /// A pane with no live session has no list to push into, so `None`.
    pub fn push_agent_view_block(
        &mut self,
        pane_id: u64,
        conversation_id: &str,
        label: &str,
    ) -> Option<BlockId> {
        self.sessions
            .get_mut(&pane_id)
            .and_then(|session| session.push_agent_view_block(conversation_id, label))
    }

    pub fn set_mode(&mut self, pane_id: u64, mode: TerminalMode) {
        self.modes.insert(pane_id, mode);
    }

    /// The harness's shell-tool route for a pane whose shell can answer, or
    /// `None` for a pane that has no terminal or whose shell has not
    /// bootstrapped.
    ///
    /// A pane gets a session the first time it runs a command or paints a
    /// terminal; until then there is no shell to route through, so the turn
    /// keeps the sandboxed runner. A shell that never bootstraps — anything
    /// outside bash/zsh, or one whose integration script failed to load —
    /// never reports a `Preexec`, so the pane offers no session and the turn
    /// keeps the sandboxed runner rather than having the claim time out.
    /// Once the shell has bootstrapped the agent's commands run in it,
    /// visibly, with the user's environment and credentials.
    pub fn pane_session(
        &self,
        pane_id: u64,
        conversation_id: &str,
    ) -> Option<Arc<dyn PaneSession>> {
        let session = self.sessions.get(&pane_id)?;
        if !session.is_bootstrapped() {
            return None;
        }
        Some(Arc::new(PaneCommandRunner::new(
            Arc::clone(&session.commands),
            conversation_id,
        )))
    }

    /// If the pane's current input line names a known TUI agent, switch the pane
    /// to agent mode (native-first) and return the detected agent. Called right
    /// before the submitted enter is forwarded to the shell.
    pub fn update_mode_from_input(&mut self, pane_id: u64) -> Option<TuiAgent> {
        let input = self.input(pane_id);
        if let Some(agent) = TuiAgent::detect(&input) {
            self.modes.insert(pane_id, TerminalMode::Agent(agent));
            Some(agent)
        } else {
            None
        }
    }

    /// Launch a TUI agent inside a pane's PTY: write `command + '\n'` so the
    /// agent takes over the terminal, then switch the pane to agent mode so goble
    /// keeps its native input. Unknown commands still become agent mode because
    /// the user explicitly asked to launch it.
    pub fn launch_agent(&mut self, pane_id: u64, command: &str) {
        let command = command.trim();
        if command.is_empty() {
            return;
        }
        if let Some(session) = self.sessions.get_mut(&pane_id) {
            let mut bytes = command.as_bytes().to_vec();
            // Enter, as the terminal sees it: a carriage return. A program in
            // raw mode does not read a line feed as "run this".
            bytes.push(b'\r');
            session.write(&bytes);
        }
        let agent = TuiAgent::detect(command).unwrap_or(TuiAgent::Other);
        self.modes.insert(pane_id, TerminalMode::Agent(agent));
    }
}
