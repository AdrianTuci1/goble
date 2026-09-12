use goble_terminal::blocks::BlockView;
use goble_terminal::{BlockId, HookEvent, OscEvent};

use crate::emulator::VisibleBlock;

use super::TerminalSession;

impl TerminalSession {
    /// The shell-integration hooks seen so far, oldest first.
    pub fn hooks(&mut self) -> Vec<HookEvent> {
        self.hooks.drain(..).collect()
    }

    /// The observed `OSC` events seen so far, oldest first.
    pub fn osc_events(&mut self) -> Vec<OscEvent> {
        self.osc.drain(..).collect()
    }

    /// Take the bell, if the program rang it.
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }

    /// The working directory the shell last reported, if it reports one.
    pub fn reported_cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    /// The blocks a view draws for this session, oldest first.
    ///
    /// The terminal view is the shell's own history — the agent's commands are
    /// real blocks in it too. A conversation's agent view is only the commands
    /// the agent ran for that conversation: the owner decided at `Preexec` is
    /// what tells the two apart, so a command the user typed is never in it.
    pub fn visible_blocks(&self, view: &BlockView) -> Vec<VisibleBlock> {
        self.state
            .lock()
            .map(|state| state.visible_blocks(view))
            .unwrap_or_default()
    }

    /// Push the card standing for `conversation_id` into this session's block
    /// list, returning the card's id (or `None` when the lock is poisoned).
    pub fn push_agent_view_block(&self, conversation_id: &str, label: &str) -> Option<BlockId> {
        self.state
            .lock()
            .ok()
            .map(|mut state| state.push_agent_view_block(conversation_id, label))
    }

    /// Whether this pane's shell has bootstrapped its integration: the
    /// `Bootstrapped` handshake B0's script sends before the shell can report a
    /// `Preexec`. Until it arrives a claim could never resolve.
    pub fn is_bootstrapped(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.bootstrapped())
            .unwrap_or(false)
    }

    /// Whether the program on this pty owns the whole screen right now (the
    /// alternate screen, i.e. a full-screen TUI). While it does, the pane's own
    /// rich input must not take the keys.
    pub fn is_alt_screen(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.is_alt_screen())
            .unwrap_or(false)
    }

    /// Whether a command is running in this session right now.
    pub fn has_running_command(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.has_running_command())
            .unwrap_or(false)
    }

    /// The listener thread has finished (child exited) — used to cheaply detect
    /// when the shell is closed so the pane can render a hint.
    pub fn is_alive(&self) -> bool {
        self.child.is_some()
    }
}
