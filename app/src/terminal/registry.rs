use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(unix)]
use goble_pty::protocol::{Request, ServerMessage};
#[cfg(unix)]
use goble_pty::Client;

use goble_core::harness::{detect_harness, HarnessSpec, PaneSession};
use goble_core::ssh_command::SshSession;
use goble_terminal::blocks::BlockView;
use goble_terminal::BlockId;

use crate::emulator::VisibleBlock;

use super::agents::{self, TerminalMode, TuiAgent};
use super::claim::PaneCommandRunner;
use super::session::TerminalSession;

/// How a prompt and the Enter after it reach a CLI agent's stdin. The split
/// mirrors warp-new's rich input: agents whose paste-burst heuristics swallow a
/// quick Enter get a bracketed paste and a later Enter, and agents that read a
/// whole line get the text and the Enter as two writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AgentSubmit {
    Inline,
    DelayedEnter,
    BracketedPaste,
    BracketedPasteDelayedEnter,
}

impl AgentSubmit {
    pub(super) fn for_harness(id: &str) -> Self {
        match id {
            "codex" => AgentSubmit::BracketedPaste,
            "copilot" => AgentSubmit::BracketedPasteDelayedEnter,
            "claude" | "gemini" | "opencode" | "auggie" | "cursor" => AgentSubmit::DelayedEnter,
            _ => AgentSubmit::Inline,
        }
    }
}

/// `text` as a terminal bracketed paste, so the agent's editor inserts it
/// literally instead of reading each byte as a key.
fn bracketed_paste(text: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + 12);
    bytes.extend_from_slice(b"\x1b[200~");
    bytes.extend_from_slice(text);
    bytes.extend_from_slice(b"\x1b[201~");
    bytes
}

/// How long an agent command waits for its fresh shell to bootstrap before it
/// runs anyway. The block association is lost on a shell that never sends the
/// handshake, and the command still has to run.
const AGENT_COMMAND_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);

/// A `!` line waiting for its agent shell to be ready, with the conversation
/// that owns it.
#[derive(Debug)]
struct PendingAgentCommand {
    conversation_id: String,
    text: String,
    queued_at: std::time::Instant,
}

/// Per-pane registry of live terminal sessions + the local input mirrors used
/// to route `Cmd+Enter` to the agent. Owned by the app state so panes can
/// split/close/persist it like any other pane.
#[derive(Debug, Default)]
pub struct TerminalRegistry {
    pub sessions: HashMap<u64, TerminalSession>,
    /// The shell an agent view runs its `!` commands in, keyed by pane id.
    ///
    /// It is its own session, not the pane's terminal one: an agent conversation
    /// and the shell behind Esc are two surfaces, so a command typed in the
    /// conversation stays in it and a program it starts cannot paint over the
    /// terminal the pane returns to.
    pub agent_sessions: HashMap<u64, TerminalSession>,
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
    /// The daemon socket to talk to. `None` starts no shell at all, which is
    /// the default a test gets; the app sets the default socket through
    /// [`Self::for_app`] and a test daemon is pointed at with [`Self::use_socket`].
    pub socket: Option<PathBuf>,
    /// Whether this registry may start the daemon on that socket. Only the app
    /// owns the default socket; a test points at a daemon it runs itself, so it
    /// must never spawn one beside the user's.
    pub start_daemon: bool,
    /// Panes whose last launch failed. Their screen holds the error and no
    /// shell, so the next ensure gets a fresh try. A screen with no shell that
    /// the registry did not fail (a test's own) is left alone.
    failed: HashSet<u64>,
    /// Panes whose agent-shell launch failed, for the same retry reason as
    /// `failed` above. Kept apart so one surface's failure never blocks the
    /// other's launch.
    failed_agents: HashSet<u64>,
    /// `!` lines that arrived before their agent shell bootstrapped. The shell
    /// pushes a fresh block at the bootstrap hook, so a command associated with
    /// the block before it would lose the conversation it belongs to; it waits
    /// here for the session to become ready.
    pending_agent_commands: HashMap<u64, Vec<PendingAgentCommand>>,
    /// The program check the mode switch gates on. `None` asks the machine
    /// ([`agents::command_installed`]); a test pins it so its pane does not
    /// depend on what happens to be installed.
    command_installed: Option<fn(&str) -> bool>,
    /// Per-pane copy mode, selection and find-bar state. Owned here for the
    /// same reason the sessions are: the pane's element tree is rebuilt every
    /// frame, and the state has to outlive it.
    pub ui: HashMap<u64, super::PaneTerminalState>,
}

impl TerminalRegistry {
    /// The registry the running app uses: the default daemon socket.
    ///
    /// Test binaries keep the empty default. A test that renders the shell or
    /// runs an action must never start a real shell in the user's daemon, and
    /// the daemon keeps a detached session alive by design, so a leaked test
    /// shell would outlive the run.
    pub fn for_app() -> Self {
        let mut registry = Self::default();
        if cfg!(test) || std::env::var_os("GOBLE_TESTING").is_some() {
            return registry;
        }
        // The PTY daemon is Unix-only; on Windows the registry keeps no socket
        // and every session path reports the daemon as unavailable.
        #[cfg(unix)]
        {
            registry.socket = Some(goble_pty::resolve_socket_path());
            registry.start_daemon = true;
        }
        registry
    }

    /// Make sure the PTY daemon is listening before a pane asks it for a shell.
    ///
    /// The app starts the daemon itself: the socket is the app's default one,
    /// and a launch with no daemon running yet (the first run after boot, or a
    /// daemon that crashed) has nothing to attach to. A test's registry is not
    /// allowed to start a process, so it leaves this alone.
    #[cfg(unix)]
    fn ensure_daemon(&self) {
        if !self.start_daemon {
            return;
        }
        if let Err(error) = Client::connect_or_start() {
            log::warn!("failed to start the PTY daemon: {error:#}");
        }
    }

    #[cfg(not(unix))]
    fn ensure_daemon(&self) {}

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
        // A failed launch keeps its error on screen but no shell behind it. A
        // later ensure (the next launch, or reopening the pane) gets a fresh
        // try instead of the error staying forever.
        if self.sessions.contains_key(&pane_id) && !self.failed.contains(&pane_id) {
            return;
        }
        self.sessions.remove(&pane_id);
        self.failed.remove(&pane_id);
        self.ensure_daemon();
        let persisted = self.session_ids.get(&pane_id).map(String::as_str);
        let spawned = match &self.socket {
            #[cfg(unix)]
            Some(socket) => TerminalSession::spawn_at(socket, pane_id, cwd, persisted),
            #[cfg(not(unix))]
            Some(_) => TerminalSession::spawn_for(pane_id, cwd, persisted),
            None => Err("this registry has no PTY daemon socket".to_string()),
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
                self.failed.insert(pane_id);
                self.sessions.insert(pane_id, TerminalSession::failed(e));
            }
        }
    }

    /// Make sure the pane's agent view has a shell of its own in `cwd`.
    ///
    /// The session is never reattached from a persisted id and never shares the
    /// pane's terminal session: it is the conversation's shell, started the
    /// first time a `!` line runs in it, and it lives and dies with the pane's
    /// agent surface.
    pub fn ensure_agent_session(&mut self, pane_id: u64, cwd: &str) {
        if self.agent_sessions.contains_key(&pane_id) && !self.failed_agents.contains(&pane_id) {
            return;
        }
        self.agent_sessions.remove(&pane_id);
        self.failed_agents.remove(&pane_id);
        self.ensure_daemon();
        let spawned = match &self.socket {
            #[cfg(unix)]
            Some(socket) => TerminalSession::spawn_at(socket, pane_id, cwd, None),
            #[cfg(not(unix))]
            Some(_) => TerminalSession::spawn_for(pane_id, cwd, None),
            None => Err("this registry has no PTY daemon socket".to_string()),
        };
        match spawned {
            Ok(session) => {
                self.agent_sessions.insert(pane_id, session);
            }
            Err(e) => {
                log::warn!("failed to spawn the agent shell for pane {pane_id}: {e}");
                self.failed_agents.insert(pane_id);
                self.agent_sessions
                    .insert(pane_id, TerminalSession::failed(e));
            }
        }
    }

    /// The pane's agent-view shell, when it has one.
    pub fn agent_session(&self, pane_id: u64) -> Option<&TerminalSession> {
        self.agent_sessions.get(&pane_id)
    }

    /// Run `text` in the pane's agent shell, owned by `conversation_id`.
    ///
    /// A fresh shell has not bootstrapped yet, and the bootstrap hook pushes the
    /// block the command will run in. Associating the command before that would
    /// associate the block the shell starts in, which the command never uses, so
    /// the line is held until the session is ready.
    pub fn run_agent_command(
        &mut self,
        pane_id: u64,
        cwd: &str,
        conversation_id: &str,
        text: &str,
    ) {
        self.ensure_agent_session(pane_id, cwd);
        if self.failed_agents.contains(&pane_id) {
            return;
        }
        let ready = self
            .agent_sessions
            .get(&pane_id)
            .is_some_and(TerminalSession::is_bootstrapped);
        if ready {
            self.write_agent_command(pane_id, conversation_id, text);
            return;
        }
        self.pending_agent_commands
            .entry(pane_id)
            .or_default()
            .push(PendingAgentCommand {
                conversation_id: conversation_id.to_string(),
                text: text.to_string(),
                queued_at: std::time::Instant::now(),
            });
    }

    /// Write one `!` line into the agent shell, filed under its conversation.
    fn write_agent_command(&mut self, pane_id: u64, conversation_id: &str, text: &str) {
        if let Some(session) = self.agent_sessions.get_mut(&pane_id) {
            session.run_block_for_conversation(conversation_id);
            let mut line = text.trim_end().to_string();
            line.push('\n');
            session.write(line.as_bytes());
        }
    }

    /// Release the commands whose agent shell is ready, one batch per frame.
    fn flush_agent_commands(&mut self) {
        let now = std::time::Instant::now();
        let pane_ids: Vec<u64> = self.pending_agent_commands.keys().copied().collect();
        for pane_id in pane_ids {
            let ready = self
                .agent_sessions
                .get(&pane_id)
                .is_some_and(TerminalSession::is_bootstrapped);
            let overdue = self
                .pending_agent_commands
                .get(&pane_id)
                .and_then(|commands| commands.first())
                .is_some_and(|command| now.duration_since(command.queued_at) >= AGENT_COMMAND_GRACE);
            if !ready && !overdue {
                continue;
            }
            if let Some(commands) = self.pending_agent_commands.remove(&pane_id) {
                for command in commands {
                    self.write_agent_command(pane_id, &command.conversation_id, &command.text);
                }
            }
        }
    }

    /// Start a fresh shell for a pane whose previous one is gone (a saved-host
    /// pane reconnecting after `exit`). The old daemon session id is dropped so
    /// the spawn never reattaches the shell that just exited.
    pub fn restart_session(&mut self, pane_id: u64, cwd: &str) {
        self.sessions.remove(&pane_id);
        self.session_ids.remove(&pane_id);
        self.failed.remove(&pane_id);
        self.ensure_session(pane_id, cwd);
    }

    /// Resize a pane's live session to `cols` × `rows`, keeping the font cell
    /// metrics the pane last reported. Returns false when the pane has no
    /// session.
    pub fn resize(&mut self, pane_id: u64, cols: u16, rows: u16) -> bool {
        // The agent shell is the same pane's other surface, so it follows the
        // size the pane measured whenever it exists.
        if let Some(session) = self.agent_sessions.get_mut(&pane_id) {
            session.set_grid_size(rows, cols);
        }
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
        self.agent_sessions.remove(&pane_id);
        self.failed_agents.remove(&pane_id);
        self.pending_agent_commands.remove(&pane_id);
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
        if let Some(mut session) = self.agent_sessions.remove(&pane_id) {
            session.kill();
        }
        self.failed_agents.remove(&pane_id);
        self.pending_agent_commands.remove(&pane_id);
        self.session_ids.remove(&pane_id);
        self.input.remove(&pane_id);
        self.modes.remove(&pane_id);
        self.ssh_sessions.remove(&pane_id);
        self.ui.remove(&pane_id);
    }

    /// Close every pane's shell for good. The window calls this once it has
    /// left the event loop: dropping a session only detaches, so without this
    /// the daemon keeps every pane's shell alive with no app to reattach it.
    pub fn kill_all(&mut self) {
        let pane_ids: Vec<u64> = self
            .sessions
            .keys()
            .chain(self.agent_sessions.keys())
            .copied()
            .collect();
        for pane_id in pane_ids {
            self.kill_pane(pane_id);
        }
    }

    /// Close the daemon sessions the layout does not name.
    ///
    /// `keep` holds the `(pane id, session id)` the restored layout persisted,
    /// so a pane that is gone, and a second session that claims the same pane,
    /// are both closed. A crashed or force-quit run leaves its shells detached
    /// and the daemon keeps a detached session by design; the window that owns
    /// the control socket calls this at launch, before it starts the restored
    /// panes, so such a run does not accumulate them.
    #[cfg(unix)]
    pub fn kill_orphan_sessions(&mut self, keep: &HashSet<(String, String)>) {
        let Some(socket) = self.socket.clone() else {
            return;
        };
        let Ok(mut client) = Client::connect(socket) else {
            return;
        };
        let id = client.next_id();
        let Ok(ServerMessage::Sessions { sessions, .. }) = client.request(Request::List { id })
        else {
            return;
        };
        for session in sessions {
            if keep.contains(&(session.pane_id.clone(), session.session_id.clone())) {
                continue;
            }
            let id = client.next_id();
            let _ = client.request(Request::Kill {
                id,
                session_id: session.session_id,
            });
        }
    }

    #[cfg(not(unix))]
    pub fn kill_orphan_sessions(&mut self, _keep: &HashSet<(String, String)>) {}

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
    ///
    /// The agent mode is sticky — once a pane has launched an agent it stays
    /// native-first — so the mode alone only means the screen is owned while
    /// the shell still runs that agent. Once the agent exits (or the foreground
    /// command is another one, say `cargo build`), the pane goes back to its
    /// own shell and its rich input comes back.
    pub fn program_owns_screen(&self, pane_id: u64) -> bool {
        if self
            .sessions
            .get(&pane_id)
            .map(|session| session.is_alt_screen())
            .unwrap_or(false)
        {
            return true;
        }
        let running = self
            .sessions
            .get(&pane_id)
            .and_then(|session| session.running_command());
        // A reported command that names a TUI agent owns the screen whatever
        // the pane's mode says. `agy` paints inline instead of taking the
        // alternate screen, and a session reattached from the daemon starts in
        // Shell mode even while its agent is still running.
        if running
            .as_deref()
            .is_some_and(|command| TuiAgent::detect(command).is_some_and(TuiAgent::owns_screen))
        {
            return true;
        }
        if !matches!(self.mode(pane_id), TerminalMode::Agent(_)) {
            return false;
        }
        match running {
            // The foreground command is another program: the mode alone does
            // not mean the agent still has the screen.
            Some(_) => false,
            // The shell reported a running command but not its line: the
            // pane's mode is the only signal left.
            None => self.running_a_command(pane_id),
        }
    }

    /// The harness this pane is working with, for the pane's topbar and its
    /// icon: the agent its mode names, or the harness its foreground command
    /// names even when that harness speaks a protocol instead of taking the
    /// screen.
    pub fn pane_harness(&self, pane_id: u64) -> Option<&'static HarnessSpec> {
        if let TerminalMode::Agent(agent) = self.mode(pane_id) {
            if let Some(spec) = agent.spec() {
                return Some(spec);
            }
        }
        let running = self
            .sessions
            .get(&pane_id)
            .and_then(|session| session.running_command());
        running.as_deref().and_then(detect_harness)
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

    /// The cells behind the blocks `view` draws in `pane_id`, keyed by block id.
    /// A pane with no session has no blocks, so the result is empty.
    pub fn block_output_lines(
        &self,
        pane_id: u64,
        view: &BlockView,
    ) -> std::collections::HashMap<BlockId, Vec<goble_terminal::ScreenLine>> {
        self.sessions
            .get(&pane_id)
            .map(|session| session.block_output_lines(view))
            .unwrap_or_default()
    }

    /// The cursors of the blocks `view` draws that are still running, keyed by
    /// block id: what a block needs to draw the caret its command left.
    pub fn block_output_cursors(
        &self,
        pane_id: u64,
        view: &BlockView,
    ) -> std::collections::HashMap<BlockId, goble_terminal::CursorState> {
        self.sessions
            .get(&pane_id)
            .map(|session| session.block_output_cursors(view))
            .unwrap_or_default()
    }

    /// The blocks `view` draws in the pane's agent shell, oldest first. A pane
    /// that has not run a `!` line yet has no agent shell, so the result is
    /// empty.
    pub fn agent_visible_blocks(&self, pane_id: u64, view: &BlockView) -> Vec<VisibleBlock> {
        self.agent_sessions
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

    /// Pump every live session, painted or not.
    ///
    /// A pane's harness turn routes its shell tool through this pane
    /// ([`PaneCommandRunner`]), and the session's pump is what services that
    /// request. A pane drawing its agent view, or sitting in a tab that is not
    /// in front, still owns its shell: pumping only the painted shells would
    /// leave the call waiting until the pane showed its terminal again, and the
    /// tool would fail with "the pane did not answer".
    pub fn pump_sessions(&mut self) {
        for session in self.sessions.values_mut() {
            session.pump();
        }
        // The agent view's own shell is a real session too: its pump answers
        // the program's queries and streams its blocks even while the pane
        // draws the conversation over it.
        for session in self.agent_sessions.values_mut() {
            session.pump();
        }
        // A shell that just bootstrapped can take the `!` line that was waiting
        // for it.
        self.flush_agent_commands();
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

    /// The harness's shell-tool route for a pane's agent surface: the pane's
    /// *agent* shell, which is the one `!` lines run in, never the shell the
    /// terminal pane shows.
    ///
    /// The two are separate sessions on purpose: Escape returns the pane to
    /// its terminal, and the commands a turn ran there must not be in that
    /// shell's history — nor may a harness the agent launched take the
    /// terminal's screen. The agent shell is spawned in `cwd` on first use.
    /// `None` while it has not bootstrapped keeps the turn on the sandboxed
    /// runner, the same contract as [`Self::pane_session`].
    pub fn agent_pane_session(
        &mut self,
        pane_id: u64,
        cwd: &str,
        conversation_id: &str,
    ) -> Option<Arc<dyn PaneSession>> {
        self.ensure_agent_session(pane_id, cwd);
        let session = self.agent_sessions.get(&pane_id)?;
        if !session.is_bootstrapped() {
            return None;
        }
        Some(Arc::new(PaneCommandRunner::new(
            Arc::clone(&session.commands),
            conversation_id,
        )))
    }

    /// Pin the program check for this registry, so a test's pane switches (or
    /// refuses to switch) without depending on what the machine has.
    #[cfg(test)]
    pub(super) fn assume_installed(&mut self, check: fn(&str) -> bool) {
        self.command_installed = Some(check);
    }

    /// Whether the program `command` runs is installed on this machine.
    fn command_exists(&self, command: &str) -> bool {
        self.command_installed.unwrap_or(agents::command_installed)(command)
    }

    /// The TUI agent a line launches: the one it names, when this machine has
    /// the program. A line whose program is missing is not an agent line — the
    /// shell runs it and prints its own `command not found`.
    fn detected_agent(&self, command: &str) -> Option<TuiAgent> {
        TuiAgent::detect(command)
            .filter(|agent| agent.owns_screen())
            .filter(|_| self.command_exists(command))
    }

    /// If the pane's current input line names a known TUI agent that is
    /// installed here, switch the pane to agent mode (native-first) and return
    /// the detected agent. Called right before the submitted enter is forwarded
    /// to the shell, which is what reports a program that is not there.
    pub fn update_mode_from_input(&mut self, pane_id: u64) -> Option<TuiAgent> {
        let input = self.input(pane_id);
        let agent = self.detected_agent(&input)?;
        self.modes.insert(pane_id, TerminalMode::Agent(agent));
        Some(agent)
    }

    /// The same switch for a command line submitted from the rich input rather
    /// than typed into the grid: the bar is the pane's usual command surface, so
    /// `codex` typed there must flip the pane exactly as `codex` typed at the
    /// prompt does.
    pub fn note_command(&mut self, pane_id: u64, command: &str) -> Option<TuiAgent> {
        let agent = self.detected_agent(command)?;
        self.modes.insert(pane_id, TerminalMode::Agent(agent));
        Some(agent)
    }

    /// Hand `text` to the CLI agent running in the pane's pty, the way warp-new's
    /// rich input does: the prompt goes to the agent's own stdin and its own
    /// line editor, so the agent keeps its native screen and its conversation.
    ///
    /// Each harness reads a prompt and the Enter after it at its own pace, so
    /// the delivery is the strategy warp-new maps per agent: an inline submit, a
    /// bracketed paste, or the paste/text now and the Enter from a later frame.
    /// A leading `!` or `&` is a mode switch the agent has to see on its own,
    /// so that byte goes first.
    pub fn submit_agent_prompt(&mut self, pane_id: u64, text: &str) {
        use std::time::Duration;

        let text = text.trim_end();
        if text.is_empty() || !self.sessions.contains_key(&pane_id) {
            return;
        }
        let harness = self.pane_harness(pane_id).map(|spec| spec.id).unwrap_or("");
        let strategy = AgentSubmit::for_harness(harness);
        let bytes = text.as_bytes().to_vec();
        // `!` and `&` are the agents' own mode switches (bash mode, background
        // mode); sending them in the same read as the command makes the agent
        // interpret the text before the mode changed.
        if bytes.len() > 1 && matches!(bytes[0], b'!' | b'&') {
            self.write_to_pane(pane_id, &bytes[..1]);
            let rest = bytes[1..].to_vec();
            self.queue_agent_submit(pane_id, rest, strategy, Duration::from_millis(50));
        } else {
            self.queue_agent_submit(pane_id, bytes, strategy, Duration::ZERO);
        }
        self.set_input(pane_id, String::new());
    }

    fn write_to_pane(&mut self, pane_id: u64, bytes: &[u8]) {
        if let Some(session) = self.sessions.get_mut(&pane_id) {
            session.write(bytes);
        }
    }

    fn write_to_pane_after(&mut self, pane_id: u64, delay: std::time::Duration, bytes: Vec<u8>) {
        if let Some(session) = self.sessions.get_mut(&pane_id) {
            session.write_after(delay, bytes);
        }
    }

    fn queue_agent_submit(
        &mut self,
        pane_id: u64,
        bytes: Vec<u8>,
        strategy: AgentSubmit,
        delay: std::time::Duration,
    ) {
        use std::time::Duration;

        let (text, enter_after) = match strategy {
            AgentSubmit::Inline => {
                let mut inline = bytes;
                inline.push(b'\r');
                self.write_to_pane_after(pane_id, delay, inline);
                return;
            }
            AgentSubmit::DelayedEnter => (bytes, Some(Duration::from_millis(50))),
            AgentSubmit::BracketedPaste => {
                (bracketed_paste(&bytes), Some(Duration::from_millis(50)))
            }
            AgentSubmit::BracketedPasteDelayedEnter => {
                (bracketed_paste(&bytes), Some(Duration::from_millis(300)))
            }
        };
        self.write_to_pane_after(pane_id, delay, text);
        if let Some(enter_after) = enter_after {
            self.write_to_pane_after(pane_id, delay + enter_after, b"\r".to_vec());
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
