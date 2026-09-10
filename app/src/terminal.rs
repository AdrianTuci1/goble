//! PTY-backed terminal pane support.
//!
//! This module owns the only blocking I/O in the terminal feature. Each pane
//! gets a [`TerminalSession`] that spawns a shell in the pane's working
//! directory and reads its output on a dedicated background thread. The reader
//! interprets that output into the pane's [`Emulator`](crate::emulator::Emulator)
//! — a real VT screen, not a line buffer — behind an `Arc<Mutex<..>>`; the UI
//! thread takes the lock for the few microseconds it needs to clone the visible
//! rows, the cursor and the input mode. Blocking reads therefore never touch the
//! UI/window thread, and the shell is never a cursor of the winit event loop.
//!
//! Keys are encoded for the program that is running (cursor-key style,
//! bracketed paste, mouse reporting), which is why [`classify_key`] needs the
//! session's [`TermMode`](goble_terminal::TermMode) and not just the keystroke.
//! The harness vs. shell key distinction is captured by [`classify_key`] /
//! [`is_agent_enter`]: plain Enter is forwarded to the pty (so it runs as a
//! terminal command in the shell), while Cmd/Ctrl+Enter is routed through the
//! agent turn path (see `crate::actions`).
//!
//! The warp-new *input-line* model — an input is a terminal command by default,
//! an agent prompt in agent mode, and a `!`-prefixed command forces a shell
//! command from agent mode — is implemented by [`classify_input`]/[`InputClass`].

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use goble_terminal::{
    encode_mouse, CursorState, HookEvent, Key, KeyEncoder, Modifiers, MouseAction, OscEvent,
    Palette, ScreenLine, TermMode,
};
use goble_ui::event::ModifiersState;

use crate::emulator::Emulator;

// ---------------------------------------------------------------------------
// TUI agent detection + terminal surface mode
// ---------------------------------------------------------------------------

/// A real command-line agent that runs inside the pane's PTY (codex, claude,
/// gemini, opencode, ...). When a pane is in [`TerminalMode::Agent`], goble
/// hands input to the agent's *native* line editor (native-first) instead of
/// routing it to the headless harness, so the user keeps the agent's full TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiAgent {
    Codex,
    Claude,
    Gemini,
    OpenCode,
    Cursor,
    Aider,
    /// An unrecognised binary launched explicitly (via the launch button); the
    /// pane still goes native-first so the requested agent owns the line.
    Other,
}

impl TuiAgent {
    /// The canonical command used to launch this agent.
    pub fn command(self) -> &'static str {
        match self {
            TuiAgent::Codex => "codex",
            TuiAgent::Claude => "claude",
            TuiAgent::Gemini => "gemini",
            TuiAgent::OpenCode => "opencode",
            TuiAgent::Cursor => "cursor-agent",
            TuiAgent::Aider => "aider",
            TuiAgent::Other => "codex",
        }
    }

    /// Short label for the terminal pane's mode badge / launch menu.
    pub fn label(self) -> &'static str {
        match self {
            TuiAgent::Codex => "codex",
            TuiAgent::Claude => "claude",
            TuiAgent::Gemini => "gemini",
            TuiAgent::OpenCode => "opencode",
            TuiAgent::Cursor => "cursor-agent",
            TuiAgent::Aider => "aider",
            TuiAgent::Other => "agent",
        }
    }

    /// Detect a TUI agent from the first command token, skipping wrapper tokens
    /// (`sudo`, `command`, `exec`, `npx`, `bunx`, `env VAR=...`). This mirrors
    /// warp-new's per-agent `command_prefix` model so `codex` / `claude` etc.
    /// launch an agent surface and take over the pty.
    pub fn detect(command: &str) -> Option<TuiAgent> {
        let first = first_command_token(command)?;
        match first {
            "codex" => Some(TuiAgent::Codex),
            "claude" => Some(TuiAgent::Claude),
            "gemini" => Some(TuiAgent::Gemini),
            "opencode" => Some(TuiAgent::OpenCode),
            "cursor-agent" | "cursor" => Some(TuiAgent::Cursor),
            "aider" => Some(TuiAgent::Aider),
            _ => None,
        }
    }

    /// All known agents, for building a launch menu.
    pub const ALL: [TuiAgent; 6] = [
        TuiAgent::Codex,
        TuiAgent::Claude,
        TuiAgent::Gemini,
        TuiAgent::OpenCode,
        TuiAgent::Cursor,
        TuiAgent::Aider,
    ];
}

/// The first whitespace-delimited token of a command line, with a bounded set
/// of "wrapper" tokens skipped and trailing path components stripped.
fn first_command_token(command: &str) -> Option<&str> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i];
        match tok {
            // Wrappers: skip and look one token further.
            "sudo" | "command" | "exec" | "npx" | "bunx" | "env" => {
                i += 1;
                continue;
            }
            // `env FOO=1 codex`: skip the assignment, find the binary.
            _ if tok.contains('=') => {
                i += 1;
                continue;
            }
            _ => {}
        }
        return Some(tok.rsplit('/').next().unwrap_or(tok));
    }
    None
}

/// The surface mode of a terminal pane.
///
/// [`TerminalMode::Shell`] is a plain shell. [`TerminalMode::Agent`] means a
/// real TUI agent is running inside the pty (launched by typing its command or
/// via the launch control), so goble keeps its native input and shows a badge.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TerminalMode {
    #[default]
    Shell,
    Agent(TuiAgent),
}

/// A cloneable snapshot of a session's visible output, for surfaces that show
/// terminal text without a cell grid (the chat transcript's inline block).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSnapshot {
    /// The newest visible lines, bottom row last, with trailing blanks dropped.
    pub lines: Vec<String>,
}

// ---------------------------------------------------------------------------
// Enter vs Cmd+Enter routing
// ---------------------------------------------------------------------------

/// What the terminal should do with a (key, modifier) pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalKeyAction {
    /// Forward these bytes to the pty (a normal shell keystroke).
    Forward(Vec<u8>),
    /// Cmd/Ctrl+Enter: route the pane's current input to the agent harness and
    /// do NOT write anything to the shell.
    RouteToAgent,
    /// Ignore (e.g. unresolvable or control keys we don't thread through).
    Ignore,
}

/// True when the modifiers are the submit-to-agent keybinding (Cmd or Ctrl, no
/// shift, not alt). This is the warp-new submit-to-local-agent binding: the
/// input is sent to the agent instead of being run as a shell command.
pub fn is_agent_submit(modifiers: ModifiersState) -> bool {
    (modifiers.command || modifiers.ctrl) && !modifiers.shift
}

/// True when the event is Cmd/Ctrl+Enter (command or ctrl, no shift, not alt).
pub fn is_agent_enter(key: &str, modifiers: ModifiersState) -> bool {
    let enter = key.eq_ignore_ascii_case("enter") || key.eq_ignore_ascii_case("return");
    enter && is_agent_submit(modifiers)
}

/// Classify a single key event into a terminal action.
///
/// The bytes for the running program are produced by the emulator crate's key
/// encoder, which knows the modes the program negotiated (application cursor
/// keys, bracketed paste, focus reporting). What is decided here is the policy
/// around it: Cmd/Ctrl+Enter becomes an agent turn instead of a keystroke, and
/// Command+anything is an OS shortcut rather than input to the pty.
///
/// The local input buffer mirror (what `Cmd+Enter` routes to the harness) is
/// maintained by the caller from the forwarded keys; `RouteToAgent` consumes it
/// without touching the pty.
pub fn classify_key(key: &str, modifiers: ModifiersState, mode: TermMode) -> TerminalKeyAction {
    // Cmd/Ctrl+Enter must NOT reach the shell: it is an agent turn instead.
    if is_agent_enter(key, modifiers) {
        return TerminalKeyAction::RouteToAgent;
    }

    // Command is the platform modifier: every other combination with it belongs
    // to a menu item or the system, never to a program in the pane.
    if modifiers.command {
        return TerminalKeyAction::Ignore;
    }

    let modifiers = terminal_modifiers(modifiers);
    if let Some(named) = named_key(key) {
        return TerminalKeyAction::Forward(KeyEncoder::encode(named, modifiers, mode));
    }

    // Text: a printable character, a chunk from an input method, or a paste
    // that arrived as one string. Control characters never travel this way.
    if !key.is_empty() && key.chars().all(|c| !c.is_control()) {
        let mut out = Vec::with_capacity(key.len());
        for c in key.chars() {
            KeyEncoder::encode_into(&mut out, Key::Char(c), modifiers, mode);
        }
        return TerminalKeyAction::Forward(out);
    }

    TerminalKeyAction::Ignore
}

/// The emulator's encoding of the modifiers attached to a key event.
pub fn terminal_modifiers(modifiers: ModifiersState) -> Modifiers {
    Modifiers::new(modifiers.shift, modifiers.alt, modifiers.ctrl)
}

/// The keys that are not plain text, in the string form the platform layer
/// delivers. `None` means the key is text (or is not a key we encode).
fn named_key(key: &str) -> Option<Key> {
    let named = match key {
        "Enter" | "Return" => Key::Enter,
        "Escape" => Key::Escape,
        "Backspace" => Key::Backspace,
        "Tab" => Key::Tab,
        "Delete" => Key::Delete,
        "Insert" => Key::Insert,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "ArrowUp" => Key::Up,
        "ArrowDown" => Key::Down,
        "ArrowLeft" => Key::Left,
        "ArrowRight" => Key::Right,
        _ => return key.strip_prefix('F')?.parse().ok().map(Key::Function),
    };
    Some(named)
}

/// The report for a pointer event, in the form the program negotiated: `None`
/// when it is not listening for that kind of report.
///
/// Modifiers are not carried: the platform layer does not put them on mouse
/// events, so a report cannot claim a modifier that was not observed.
pub fn mouse_report(action: MouseAction, cell: (usize, usize), mode: TermMode) -> Option<Vec<u8>> {
    let (row, column) = cell;
    encode_mouse(action, column, row, Modifiers::NONE, mode)
}

// ---------------------------------------------------------------------------
// Input-line classification (terminal command vs. agent prompt)
// ---------------------------------------------------------------------------

/// How a submitted input line should be handled.
///
/// This is the warp-new input model: the bottom prompt line is a terminal
/// command by default, and only sends to the agent on the submit keybinding
/// (Cmd/Ctrl+Enter) or in agent mode (an active agent conversation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputClass {
    /// Run this as a shell command in the pane's PTY session.
    TerminalCommand(String),
    /// Send this to the agent (start/continue the pane's agent conversation).
    AgentPrompt(String),
}

/// Classify a submitted input line.
///
/// A real parser for the warp-new input model, not a stub:
///
/// - In **terminal mode** (`agent_mode == false`) every non-empty input is a
///   terminal command, so `ls` / `git status` run in the pane's shell.
/// - In **agent mode** (`agent_mode == true`, i.e. the pane has an active
///   agent conversation) an input is an agent prompt unless it begins with
///   `!`, in which case it is a terminal command with the leading `!` stripped
///   (so `!foo` runs `foo`).
///
/// An input that is empty or whitespace-only yields `None` (nothing to do).
///
/// The classification is deliberately mode-driven rather than keyword-driven:
/// the only way to run a shell command from agent mode is the explicit `!`
/// prefix, matching the warp-new terminal command parser.
pub fn classify_input(input: &str, agent_mode: bool) -> Option<InputClass> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if agent_mode {
        if let Some(rest) = trimmed.strip_prefix('!') {
            let command = rest.trim();
            if command.is_empty() {
                None
            } else {
                Some(InputClass::TerminalCommand(command.to_string()))
            }
        } else {
            Some(InputClass::AgentPrompt(input.to_string()))
        }
    } else {
        Some(InputClass::TerminalCommand(input.to_string()))
    }
}

/// Apply a forwarded keystroke to the per-pane local input mirror.
///
/// This is the input that `Cmd+Enter` routes to the harness; it mirrors what
/// the user typed at the shell so the agent sees the command line as typed.
/// Only plain typing is mirrored: an escape sequence, an arrow key or a Ctrl
/// combination changes the shell's line in ways this mirror cannot follow, and
/// guessing would put text in the agent's mouth that the user never typed.
pub fn update_input_mirror(mirror: &mut String, key: &str, modifiers: ModifiersState) {
    if key == "Backspace" && !modifiers.command && !modifiers.ctrl && !modifiers.alt {
        mirror.pop();
        return;
    }
    if key.eq_ignore_ascii_case("enter") || key.eq_ignore_ascii_case("return") {
        mirror.clear();
        return;
    }
    if modifiers.command || modifiers.ctrl || modifiers.alt {
        return;
    }
    // A named key is not text, however printable its name is.
    if named_key(key).is_some() {
        return;
    }
    if !key.is_empty() && key.chars().all(|c| !c.is_control()) {
        mirror.push_str(key);
    }
}

// ---------------------------------------------------------------------------
// Child process / PTY session
// ---------------------------------------------------------------------------

/// The shell to spawn: `$SHELL` when set, else `/bin/zsh`.
pub fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
}

/// Resolve a pane's working directory: the per-session path when it exists,
/// else the process cwd (a sane fallback so a shell always launches).
pub fn resolve_cwd(cwd: &str) -> PathBuf {
    if !cwd.is_empty() {
        let p = Path::new(cwd);
        if p.is_dir() {
            return p.to_path_buf();
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

/// One PTY-backed shell session for a terminal pane.
///
/// The reader thread owns the blocking read end and interprets bytes into the
/// shared [`Emulator`]; the UI thread only ever takes a short-lived lock to
/// clone what it paints. Writing input goes straight to the pty master.
pub struct TerminalSession {
    state: Arc<Mutex<Emulator>>,
    writer: Option<Box<dyn Write + Send>>,
    /// Kept so the pty stays open for the lifetime of the session.
    _master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    child: Option<Box<dyn portable_pty::Child + Send>>,
    /// The grid size the screen and the pty were last told about.
    size: (u16, u16),
    /// Font metrics, for the reports that are measured in pixels. The pane owns
    /// the font and hands them over as it lays out.
    cell: (u16, u16),
    palette: Palette,
    title: Option<String>,
    /// Set when the program rang the bell; the pane takes it and plays it.
    bell: bool,
    /// Shell-integration events, bounded: the pane reads them, and a pane that
    /// never does must not grow the heap.
    hooks: VecDeque<HookEvent>,
    osc: VecDeque<OscEvent>,
    /// The last working directory the shell reported (`OSC 7`).
    cwd: Option<String>,
}

/// How many shell-integration events a session holds before dropping the
/// oldest. The pane consumes them every frame; this is only a bound.
const EVENT_BACKLOG: usize = 256;

/// Everything the pane needs to paint one frame of a session.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalViewState {
    /// The visible rows, top row first.
    pub rows: Vec<ScreenLine>,
    pub cursor: CursorState,
    /// How the program wants keys and pointer reports encoded.
    pub mode: TermMode,
    pub alt_screen: bool,
    /// Lines scrolled back from the bottom; 0 is the live screen.
    pub display_offset: usize,
    pub scrollback: usize,
    pub title: Option<String>,
    /// Whether any cell holds something. A session that has not drawn yet gets
    /// the pane's own hint line instead of a black rectangle.
    pub has_content: bool,
}

impl std::fmt::Debug for TerminalSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalSession")
            .field("alive", &self.child.is_some())
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl TerminalSession {
    /// A session that failed to launch; renders the error rather than panicking.
    pub fn failed(message: String) -> Self {
        let mut emulator = Emulator::new(80, 24);
        emulator.feed(format!("(terminal error: {message})\r\n").as_bytes());
        Self::with_emulator(emulator)
    }

    /// A session around a screen with no pty behind it: how a failed launch and
    /// a test both build one.
    fn with_emulator(emulator: Emulator) -> Self {
        Self {
            state: Arc::new(Mutex::new(emulator)),
            writer: None,
            _master: None,
            child: None,
            size: (24, 80),
            cell: (0, 0),
            palette: Palette::xterm(),
            title: None,
            bell: false,
            hooks: VecDeque::new(),
            osc: VecDeque::new(),
            cwd: None,
        }
    }

    /// Spawn `$SHELL` (or `/bin/zsh`) in `cwd` and start a background reader.
    pub fn spawn(cwd: &str) -> Result<Self, String> {
        let columns = 80;
        let screen_lines = 24;
        let state = Arc::new(Mutex::new(Emulator::new(columns, screen_lines)));

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: screen_lines as u16,
                cols: columns as u16,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;

        let mut cmd = CommandBuilder::new(default_shell());
        cmd.cwd(resolve_cwd(cwd));

        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| {
            let _ = child.kill();
            e.to_string()
        })?;
        let writer = pair.master.take_writer().map_err(|e| {
            let _ = child.kill();
            e.to_string()
        })?;

        let out = Arc::clone(&state);
        // Detached reader: it owns the blocking read end and interprets bytes
        // into the shared screen until EOF (the child is killed on `drop`). We
        // never join it so the UI thread is never blocked.
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Ok(mut o) = out.lock() {
                            o.feed(&buf[..n]);
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            state,
            writer: Some(writer),
            _master: Some(pair.master),
            child: Some(child),
            size: (screen_lines as u16, columns as u16),
            cell: (0, 0),
            palette: Palette::xterm(),
            title: None,
            bell: false,
            hooks: VecDeque::new(),
            osc: VecDeque::new(),
            cwd: None,
        })
    }

    /// Write bytes to the shell's stdin. Non-blocking for the UI thread.
    pub fn write(&mut self, bytes: &[u8]) {
        if let Some(w) = self.writer.as_mut() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    /// Take everything the screen produced since the last call and answer the
    /// questions it is waiting on.
    ///
    /// Called once per frame by the UI thread, which is where the font metrics
    /// the answers need are known. A program that asks the terminal a question
    /// (its colours, the text area size) waits for the reply, so this has to run
    /// even when nothing is being painted — the pane calls it every frame.
    pub fn pump(&mut self) {
        let (events, replies, hooks, osc) = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            let events = state.pump(&self.palette, self.cell);
            let replies = state.take_replies();
            (
                events,
                replies,
                state.take_hook_events(),
                state.take_osc_events(),
            )
        };

        for event in events {
            match event {
                goble_terminal::ScreenEvent::Title(title) => self.title = Some(title),
                goble_terminal::ScreenEvent::ResetTitle => self.title = None,
                goble_terminal::ScreenEvent::Bell => self.bell = true,
                _ => {}
            }
        }

        for event in hooks {
            if self.hooks.len() == EVENT_BACKLOG {
                self.hooks.pop_front();
            }
            self.hooks.push_back(event);
        }
        for event in osc {
            if let OscEvent::WorkingDirectory(path) = &event {
                self.cwd = Some(path.clone());
            }
            if self.osc.len() == EVENT_BACKLOG {
                self.osc.pop_front();
            }
            self.osc.push_back(event);
        }

        if !replies.is_empty() {
            self.write(&replies);
        }
    }

    /// This session's grid size, in cells.
    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    /// Reshape the screen and the pty, and remember the font metrics the
    /// pixel-sized reports need.
    pub fn set_size(&mut self, rows: u16, cols: u16, cell_width: u16, cell_height: u16) {
        self.cell = (cell_width, cell_height);
        if (rows, cols) == self.size || rows == 0 || cols == 0 {
            return;
        }
        self.size = (rows, cols);

        if let Ok(mut state) = self.state.lock() {
            state.resize(cols as usize, rows as usize);
        }
        if let Some(master) = self._master.as_mut() {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: cols.saturating_mul(cell_width),
                pixel_height: rows.saturating_mul(cell_height),
            });
        }
    }

    /// The palette the pane paints with, which is also what colour queries are
    /// answered from.
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    /// How the running program wants keys and pointer reports encoded.
    pub fn input_mode(&self) -> TermMode {
        self.state
            .lock()
            .map(|state| state.input_mode())
            .unwrap_or(TermMode::NONE)
    }

    /// Clone what the next frame paints: the rows, the cursor and the modes.
    pub fn view(&self) -> TerminalViewState {
        let Ok(state) = self.state.lock() else {
            return TerminalViewState::empty();
        };
        let rows = state.rows();
        let has_content = rows.iter().any(|row| !row.is_blank());
        TerminalViewState {
            rows,
            cursor: state.cursor(),
            mode: state.input_mode(),
            alt_screen: state.is_alt_screen(),
            display_offset: state.display_offset(),
            scrollback: state.history_size(),
            title: self.title.clone(),
            has_content,
        }
    }

    /// Scroll the display through the scrollback (`0` is the live bottom).
    pub fn scroll(&mut self, lines: i32) {
        if let Ok(mut state) = self.state.lock() {
            state.scroll(lines);
        }
    }

    /// Coalesce the newest output into a snapshot for painting.
    ///
    /// The last element is the bottom visible row, as a line buffer's would be;
    /// trailing blank rows are dropped so an idle shell contributes no block.
    pub fn snapshot(&self, max_lines: usize) -> TerminalSnapshot {
        let Ok(state) = self.state.lock() else {
            return TerminalSnapshot { lines: Vec::new() };
        };
        let mut lines: Vec<String> = state.rows().iter().map(|line| line.text()).collect();
        while lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        if lines.len() > max_lines {
            lines.drain(..lines.len() - max_lines);
        }
        TerminalSnapshot { lines }
    }

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

    /// The listener thread has finished (child exited) — used to cheaply detect
    /// when the shell is closed so the pane can render a hint.
    pub fn is_alive(&self) -> bool {
        self.child.is_some()
    }
}

impl TerminalViewState {
    /// The state of a session whose lock could not be taken: nothing to paint.
    pub fn empty() -> Self {
        Self {
            rows: Vec::new(),
            cursor: CursorState {
                line: 0,
                column: 0,
                visible: false,
                shape: goble_terminal::CursorShape::Block,
            },
            mode: TermMode::NONE,
            alt_screen: false,
            display_offset: 0,
            scrollback: 0,
            title: None,
            has_content: false,
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        // Kill the child so the pty closes; the detached reader thread sees EOF
        // and exits on its own. We never join it (that would block the UI
        // thread).
        if let Some(c) = self.child.as_mut() {
            let _ = c.kill();
        }
    }
}

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

    pub fn set_mode(&mut self, pane_id: u64, mode: TerminalMode) {
        self.modes.insert(pane_id, mode);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A session whose screen the test drives directly, with no pty behind it.
    fn detached() -> TerminalSession {
        TerminalSession::with_emulator(Emulator::new(80, 24))
    }

    #[test]
    fn a_snapshot_drops_the_blank_rows_below_the_prompt() {
        let session = detached();
        assert!(
            session.snapshot(48).lines.is_empty(),
            "an idle screen contributes no block"
        );
    }

    #[test]
    fn a_snapshot_keeps_the_newest_lines_up_to_the_limit() {
        let session = detached();
        {
            let mut state = session.state.lock().unwrap();
            state.feed(b"one\r\ntwo\r\nthree\r\nfour");
        }
        let lines = session.snapshot(3).lines;
        assert_eq!(lines, vec!["two", "three", "four"]);
    }

    #[test]
    fn a_snapshot_keeps_blank_rows_that_separate_content() {
        let session = detached();
        {
            let mut state = session.state.lock().unwrap();
            state.feed(b"top\r\n\r\nbottom");
        }
        assert_eq!(session.snapshot(48).lines, vec!["top", "", "bottom"]);
    }

    #[test]
    fn a_view_reports_the_cursor_and_the_modes() {
        let session = detached();
        assert!(!session.view().has_content);
        {
            let mut state = session.state.lock().unwrap();
            state.feed(b"\x1b[3;4Hhi\x1b[?25h\x1b[?1h");
        }
        let view = session.view();
        assert!(view.has_content);
        assert_eq!((view.cursor.line, view.cursor.column), (2, 5));
        assert!(view.cursor.visible);
        assert!(view.mode.app_cursor);
    }

    #[test]
    fn resizing_a_session_resizes_its_screen_and_remembers_the_metrics() {
        let mut session = detached();
        session.set_size(30, 100, 8, 16);
        assert_eq!(session.size(), (30, 100));
        assert_eq!(session.view().rows.len(), 30);
        assert!(session.view().rows.iter().all(|row| row.cells.len() == 100));
    }

    #[test]
    fn resizing_to_the_same_size_keeps_the_screen() {
        let mut session = detached();
        session.set_size(30, 100, 8, 16);
        {
            let mut state = session.state.lock().unwrap();
            state.feed(b"kept");
        }
        session.set_size(30, 100, 8, 16);
        assert_eq!(session.snapshot(48).lines, vec!["kept"]);
    }

    #[test]
    fn a_failed_session_renders_its_error_instead_of_panicking() {
        let session = TerminalSession::failed("no pty".to_string());
        let lines = session.snapshot(48).lines;
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("no pty"), "{lines:?}");
    }

    #[test]
    fn enter_is_forwarded_plain_and_routed_on_cmd_or_ctrl() {
        use ModifiersState;
        assert_eq!(
            classify_key("Enter", ModifiersState::none(), TermMode::NONE),
            TerminalKeyAction::Forward(b"\r".to_vec())
        );
        assert_eq!(
            classify_key(
                "Enter",
                ModifiersState {
                    command: true,
                    ..Default::default()
                },
                TermMode::NONE
            ),
            TerminalKeyAction::RouteToAgent
        );
        assert_eq!(
            classify_key(
                "Enter",
                ModifiersState {
                    ctrl: true,
                    ..Default::default()
                },
                TermMode::NONE
            ),
            TerminalKeyAction::RouteToAgent
        );
        // Shift+Enter is still a newline for the shell.
        assert_eq!(
            classify_key(
                "Enter",
                ModifiersState {
                    shift: true,
                    ..Default::default()
                },
                TermMode::NONE
            ),
            TerminalKeyAction::Forward(b"\r".to_vec())
        );
    }

    #[test]
    fn agent_enter_requires_command_or_ctrl_without_shift() {
        use ModifiersState;
        assert!(!is_agent_enter("Enter", ModifiersState::default()));
        assert!(is_agent_enter(
            "Enter",
            ModifiersState {
                command: true,
                ..Default::default()
            }
        ));
        assert!(is_agent_enter(
            "Enter",
            ModifiersState {
                ctrl: true,
                ..Default::default()
            }
        ));
        assert!(!is_agent_enter(
            "Enter",
            ModifiersState {
                command: true,
                shift: true,
                ..Default::default()
            }
        ));
        assert!(!is_agent_enter("Enter", ModifiersState::none()));
        assert!(!is_agent_enter("a", ModifiersState::default()));
    }

    /// Command belongs to the platform: everything bound to it except the
    /// submit-to-agent key must reach neither the shell nor a menu.
    #[test]
    fn command_combinations_other_than_enter_are_not_shell_input() {
        use ModifiersState;
        let command_c = ModifiersState {
            command: true,
            ..Default::default()
        };
        assert_eq!(
            classify_key("c", command_c, TermMode::NONE),
            TerminalKeyAction::Ignore
        );
        assert_eq!(
            classify_key("Backspace", command_c, TermMode::NONE),
            TerminalKeyAction::Ignore
        );
        // Ctrl+C by contrast is a keystroke: it interrupts the program.
        assert_eq!(
            classify_key(
                "c",
                ModifiersState {
                    ctrl: true,
                    ..Default::default()
                },
                TermMode::NONE
            ),
            TerminalKeyAction::Forward(vec![0x03])
        );
    }

    #[test]
    fn printable_keys_are_forwarded_as_bytes() {
        assert_eq!(
            classify_key("a", ModifiersState::none(), TermMode::NONE).unwrap_forward(),
            b"a".to_vec()
        );
        assert_eq!(
            classify_key("A", ModifiersState::none(), TermMode::NONE).unwrap_forward(),
            b"A".to_vec()
        );
        assert_eq!(
            classify_key("Backspace", ModifiersState::none(), TermMode::NONE).unwrap_forward(),
            vec![0x7f]
        );
        assert_eq!(
            classify_key(" ", ModifiersState::none(), TermMode::NONE).unwrap_forward(),
            b" ".to_vec()
        );
        // A multi-character key arrives from an input method or a paste.
        assert_eq!(
            classify_key("日本", ModifiersState::none(), TermMode::NONE).unwrap_forward(),
            "日本".as_bytes().to_vec()
        );
    }

    #[test]
    fn ctrl_letter_sends_ascii_control_code() {
        use ModifiersState;
        assert_eq!(
            classify_key(
                "c",
                ModifiersState {
                    ctrl: true,
                    ..Default::default()
                },
                TermMode::NONE
            )
            .unwrap_forward(),
            vec![0x03],
            "Ctrl+C should interrupt the shell, not type 'c'"
        );
        assert_eq!(
            classify_key(
                "l",
                ModifiersState {
                    ctrl: true,
                    ..Default::default()
                },
                TermMode::NONE
            )
            .unwrap_forward(),
            vec![0x0c],
            "Ctrl+L clears the screen"
        );
    }

    #[test]
    fn alt_prefixes_a_character_with_escape() {
        assert_eq!(
            classify_key(
                "x",
                ModifiersState {
                    alt: true,
                    ..Default::default()
                },
                TermMode::NONE
            )
            .unwrap_forward(),
            b"\x1bx".to_vec()
        );
    }

    /// The named keys are encoded for the program, not dropped: a shell reads
    /// arrows, Home/End, the paging keys and the function keys.
    #[test]
    fn named_keys_are_encoded_for_the_program() {
        let key =
            |key: &str| classify_key(key, ModifiersState::none(), TermMode::NONE).unwrap_forward();
        assert_eq!(key("ArrowUp"), b"\x1b[A".to_vec());
        assert_eq!(key("ArrowLeft"), b"\x1b[D".to_vec());
        assert_eq!(key("Home"), b"\x1b[H".to_vec());
        assert_eq!(key("End"), b"\x1b[F".to_vec());
        assert_eq!(key("Delete"), b"\x1b[3~".to_vec());
        assert_eq!(key("PageUp"), b"\x1b[5~".to_vec());
        assert_eq!(key("PageDown"), b"\x1b[6~".to_vec());
        assert_eq!(key("Insert"), b"\x1b[2~".to_vec());
        assert_eq!(key("F1"), b"\x1bOP".to_vec());
        assert_eq!(key("Tab"), b"\t".to_vec());
        assert_eq!(key("Escape"), b"\x1b".to_vec());
    }

    /// An application that took the cursor keys over gets the `SS3` form, which
    /// is the whole reason the mode is negotiated.
    #[test]
    fn application_cursor_mode_changes_the_arrow_encoding() {
        let app_cursor = TermMode {
            app_cursor: true,
            ..TermMode::NONE
        };
        assert_eq!(
            classify_key("ArrowUp", ModifiersState::none(), app_cursor).unwrap_forward(),
            b"\x1bOA".to_vec()
        );
        // With a modifier the parameterised form is used either way.
        assert_eq!(
            classify_key(
                "ArrowUp",
                ModifiersState {
                    shift: true,
                    ..Default::default()
                },
                app_cursor
            )
            .unwrap_forward(),
            b"\x1b[1;2A".to_vec()
        );
    }

    #[test]
    fn pointer_reports_follow_the_negotiated_mode() {
        use goble_terminal::MouseButton;
        use MouseAction;
        // No mouse mode: the application is not listening.
        assert!(mouse_report(
            MouseAction::Press(MouseButton::Left),
            (0, 0),
            TermMode::NONE
        )
        .is_none());

        let clicking = TermMode {
            mouse_click: true,
            sgr_mouse: true,
            ..TermMode::NONE
        };
        assert_eq!(
            mouse_report(MouseAction::Press(MouseButton::Left), (4, 7), clicking).unwrap(),
            b"\x1b[<0;8;5M".to_vec(),
            "SGR reports one-based column then line"
        );
        assert_eq!(
            mouse_report(MouseAction::Release(MouseButton::Left), (4, 7), clicking).unwrap(),
            b"\x1b[<0;8;5m".to_vec()
        );
        // Motion is only reported when the application asked for it.
        assert!(mouse_report(MouseAction::Move, (1, 1), clicking).is_none());
        let dragging = TermMode {
            mouse_drag: true,
            sgr_mouse: true,
            ..TermMode::NONE
        };
        assert_eq!(
            mouse_report(MouseAction::Drag(MouseButton::Left), (1, 1), dragging).unwrap(),
            b"\x1b[<32;2;2M".to_vec()
        );
        // A wheel is a press with no button, and needs no motion mode.
        assert_eq!(
            mouse_report(MouseAction::WheelUp, (0, 0), clicking).unwrap(),
            b"\x1b[<64;1;1M".to_vec()
        );
    }

    #[test]
    fn parser_defaults_everything_to_terminal_command() {
        // Terminal mode (no active agent conversation): even a multi-word
        // input runs as a shell command — `ls` / `git status` are commands.
        assert_eq!(
            classify_input("ls", false),
            Some(InputClass::TerminalCommand("ls".to_string()))
        );
        assert_eq!(
            classify_input("git status", false),
            Some(InputClass::TerminalCommand("git status".to_string()))
        );
    }

    #[test]
    fn parser_is_agent_prompt_in_agent_mode() {
        // Agent mode (an active agent conversation): free text is a prompt.
        assert_eq!(
            classify_input("hello world", true),
            Some(InputClass::AgentPrompt("hello world".to_string()))
        );
    }

    #[test]
    fn parser_bang_forces_terminal_command_in_agent_mode() {
        // Agent mode + leading `!` => a terminal command with the `!` stripped.
        assert_eq!(
            classify_input("!foo", true),
            Some(InputClass::TerminalCommand("foo".to_string()))
        );
        assert_eq!(
            classify_input("! git status", true),
            Some(InputClass::TerminalCommand("git status".to_string()))
        );
    }

    #[test]
    fn parser_rejects_blank_input() {
        assert_eq!(classify_input("", false), None);
        assert_eq!(classify_input("   ", true), None);
        // A bare `!` in agent mode has no command after it.
        assert_eq!(classify_input("!", true), None);
    }

    #[test]
    fn input_mirror_tracks_appends_backspace_and_clear() {
        let mut mirror = String::new();
        update_input_mirror(&mut mirror, "l", ModifiersState::none());
        update_input_mirror(&mut mirror, "s", ModifiersState::none());
        assert_eq!(mirror, "ls");
        update_input_mirror(&mut mirror, "Backspace", ModifiersState::none());
        assert_eq!(mirror, "l");
        update_input_mirror(&mut mirror, "Enter", ModifiersState::none());
        assert_eq!(mirror, "");
    }

    /// Only typing is mirrored: a key that changes the shell's line in a way
    /// this mirror cannot follow must not invent text for the agent.
    #[test]
    fn the_input_mirror_ignores_keys_it_cannot_follow() {
        let mut mirror = "ls".to_string();
        update_input_mirror(&mut mirror, "ArrowUp", ModifiersState::none());
        update_input_mirror(&mut mirror, "Escape", ModifiersState::none());
        update_input_mirror(&mut mirror, "Home", ModifiersState::none());
        update_input_mirror(&mut mirror, "PageUp", ModifiersState::none());
        assert_eq!(mirror, "ls");
        // Ctrl+C is a keystroke, but not a character: the shell's line is not
        // what the mirror holds any more.
        update_input_mirror(
            &mut mirror,
            "c",
            ModifiersState {
                ctrl: true,
                ..Default::default()
            },
        );
        assert_eq!(mirror, "ls");
        // A paste arrives as text and is mirrored whole.
        update_input_mirror(&mut mirror, "hi", ModifiersState::none());
        assert_eq!(mirror, "lshi");
    }

    #[test]
    fn cwd_resolution_falls_back_to_process_dir() {
        let cwd = resolve_cwd("/definitely/not/a/real/dir/xyz");
        assert!(cwd.is_absolute());
        let cwd2 = resolve_cwd("");
        assert!(cwd2.is_absolute());
    }

    #[test]
    fn detects_known_tui_agents_by_first_token() {
        assert_eq!(TuiAgent::detect("codex"), Some(TuiAgent::Codex));
        assert_eq!(TuiAgent::detect("claude"), Some(TuiAgent::Claude));
        assert_eq!(TuiAgent::detect("gemini --model x"), Some(TuiAgent::Gemini));
        assert_eq!(TuiAgent::detect("opencode"), Some(TuiAgent::OpenCode));
        assert_eq!(TuiAgent::detect("cursor-agent"), Some(TuiAgent::Cursor));
        assert_eq!(TuiAgent::detect("cursor"), Some(TuiAgent::Cursor));
        assert_eq!(TuiAgent::detect("aider"), Some(TuiAgent::Aider));
    }

    #[test]
    fn detection_skips_wrappers_and_paths() {
        assert_eq!(TuiAgent::detect("sudo codex"), Some(TuiAgent::Codex));
        assert_eq!(TuiAgent::detect("npx codex"), Some(TuiAgent::Codex));
        assert_eq!(TuiAgent::detect("env FOO=1 codex"), Some(TuiAgent::Codex));
        assert_eq!(
            TuiAgent::detect("/usr/local/bin/claude"),
            Some(TuiAgent::Claude)
        );
        assert_eq!(TuiAgent::detect("bunx gemini"), Some(TuiAgent::Gemini));
    }

    #[test]
    fn detection_rejects_shell_commands() {
        assert_eq!(TuiAgent::detect("ls"), None);
        assert_eq!(TuiAgent::detect("git status"), None);
        assert_eq!(TuiAgent::detect("cd /tmp"), None);
        assert_eq!(TuiAgent::detect(""), None);
    }

    #[test]
    fn registry_mode_defaults_to_shell() {
        let mut reg = TerminalRegistry::default();
        assert_eq!(reg.mode(1), TerminalMode::Shell);
        reg.set_mode(1, TerminalMode::Agent(TuiAgent::Codex));
        assert_eq!(reg.mode(1), TerminalMode::Agent(TuiAgent::Codex));
        reg.drop_pane(1);
        assert_eq!(reg.mode(1), TerminalMode::Shell);
    }

    #[test]
    fn update_mode_from_input_detects_and_settles() {
        let mut reg = TerminalRegistry::default();
        reg.set_input(1, "claude".to_string());
        let agent = reg.update_mode_from_input(1);
        assert_eq!(agent, Some(TuiAgent::Claude));
        assert_eq!(reg.mode(1), TerminalMode::Agent(TuiAgent::Claude));

        reg.set_input(1, "ls".to_string());
        assert_eq!(reg.update_mode_from_input(1), None);
        // A non-agent input never *downgrades* an already-agent pane; only a
        // detected agent switches the mode, so it stays agent-native here.
        assert_eq!(reg.mode(1), TerminalMode::Agent(TuiAgent::Claude));
    }

    #[test]
    fn launch_agent_sets_agent_mode_even_for_unknown_commands() {
        let mut reg = TerminalRegistry::default();
        // No session spawned in a unit test, but mode is still set.
        reg.launch_agent(1, "codex");
        assert_eq!(reg.mode(1), TerminalMode::Agent(TuiAgent::Codex));
        reg.launch_agent(2, "some-custom-agent");
        assert_eq!(reg.mode(2), TerminalMode::Agent(TuiAgent::Other));
        // Blank is a no-op.
        reg.launch_agent(3, "  ");
        assert_eq!(reg.mode(3), TerminalMode::Shell);
    }

    impl TerminalKeyAction {
        fn unwrap_forward(&self) -> Vec<u8> {
            match self {
                TerminalKeyAction::Forward(b) => b.clone(),
                other => panic!("expected Forward, got {other:?}"),
            }
        }
    }
}
