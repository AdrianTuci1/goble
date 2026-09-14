use std::collections::HashMap;
use std::sync::Arc;

use goble_core::harness::PaneSession;
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
}

impl TerminalRegistry {
    pub fn ensure_session(&mut self, pane_id: u64, cwd: &str) {
        if self.sessions.contains_key(&pane_id) {
            return;
        }
        match TerminalSession::spawn(cwd) {
            Ok(session) => {
                self.sessions.insert(pane_id, session);
            }
            Err(e) => {
                log::warn!("failed to spawn terminal for pane {pane_id}: {e}");
                self.sessions.insert(pane_id, TerminalSession::failed(e));
            }
        }
    }

    /// Remove a pane's session + input mirror + mode (dropping the session kills
    /// the shell). Called when a pane closes.
    pub fn drop_pane(&mut self, pane_id: u64) {
        self.sessions.remove(&pane_id);
        self.input.remove(&pane_id);
        self.modes.remove(&pane_id);
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
