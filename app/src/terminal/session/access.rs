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

    /// Record a directory the shell reported. A directory a shell starts in is
    /// where it was put, not somewhere it moved to, so it is retired as soon as
    /// it arrives and never offered as a move. A session with no shell behind it
    /// has no start of its own, so what it reports is all the app has.
    pub(super) fn record_reported_cwd(&mut self, path: String, start: bool) {
        if start && self.spawned {
            self.cwd_handed = Some(path.clone());
        }
        self.cwd = Some(path);
    }

    /// The directory the shell is in that the app has not been offered yet, when
    /// that report is a move.
    ///
    /// `None` while the shell says nothing new. A report the app has already
    /// been offered is not a move either: a `cd` the app itself typed — a
    /// directory picked from the pill's own menu — is still in flight, and the
    /// report the shell made before it says nothing about where the pane is
    /// going.
    pub fn take_cwd_move(&mut self) -> Option<String> {
        let reported = self.cwd.clone()?;
        if self.cwd_handed.as_deref() == Some(reported.as_str()) {
            return None;
        }
        self.cwd_handed = Some(reported.clone());
        Some(reported)
    }

    /// Retire every directory the shell has reported so far: the app has just
    /// set this pane's directory itself, so an older report is not where the
    /// pane is going. The shell's own report of the move follows it.
    pub fn retire_reported_cwds(&mut self) {
        if let Some(reported) = self.cwd.as_deref() {
            self.cwd_handed = Some(reported.to_string());
        }
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

    /// Make the block the shell is at a prompt in belong to `conversation_id`'s
    /// agent view alone: the command that block is about to run was typed inside
    /// the conversation, so it is the conversation's command and not part of the
    /// shell's own history (see `Emulator::run_block_for_conversation`). False
    /// when the shell is not waiting at a prompt.
    pub fn run_block_for_conversation(&mut self, conversation_id: &str) -> bool {
        self.state
            .lock()
            .map(|mut state| state.run_block_for_conversation(conversation_id))
            .unwrap_or(false)
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
