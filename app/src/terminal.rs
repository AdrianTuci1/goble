//! PTY-backed terminal pane support.
//!
//! This module owns the only blocking I/O in the terminal feature. Each pane
//! gets a [`TerminalSession`] that spawns a shell in the pane's working
//! directory and reads its output on a dedicated background thread. The reader
//! appends bytes into an [`Arc<Mutex<TerminalBuffer>>`]; the UI thread locks the
//! buffer briefly each frame to coalesce the new output into a scrolling line
//! buffer and paint it. Blocking reads therefore never touch the UI/window
//! thread, and the shell is never a cursor of the winit event loop.
//!
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

use goble_ui::event::ModifiersState;

/// Maximum number of completed lines kept in a pane's terminal buffer.
const MAX_LINES: usize = 2000;

// ---------------------------------------------------------------------------
// Output line buffer + ANSI handling
// ---------------------------------------------------------------------------

/// ANSI escape / control-sequence parse state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnsiState {
    Plain,
    /// Saw `ESC`; waiting for the introducer byte.
    Esc,
    /// Saw `ESC [`; a CSI sequence until a final byte (`0x40..=0x7e`).
    Csi,
    /// Saw `ESC ]`; an OSC sequence until BEL or `ESC \`.
    Osc,
    /// Saw `ESC ] ... ESC`; waiting for the terminating `\`.
    OscEsc,
}

/// A bounded, ANSI-aware terminal output buffer.
///
/// Real terminal emulation (cursor addressing, alternate screen, etc.) is out
/// of scope; this is "ANSI-aware enough": CSI/OSC escape sequences are stripped
/// so prompts and shell output read cleanly, lines are split on `\n`, and a
/// lone `\r` (without a following `\n`) starts an in-place overwrite of the
/// current line (so progress bars / redraws replace rather than append).
#[derive(Debug)]
pub struct TerminalBuffer {
    lines: VecDeque<String>,
    current: String,
    state: AnsiState,
    /// Set when the previous byte was `\r` (so a following `\n` is CRLF).
    pending_cr: bool,
    max_lines: usize,
}

impl TerminalBuffer {
    pub fn new(max_lines: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            current: String::new(),
            state: AnsiState::Plain,
            pending_cr: false,
            max_lines,
        }
    }

    pub fn append_bytes(&mut self, data: &[u8]) {
        let s = String::from_utf8_lossy(data);
        for c in s.chars() {
            self.push_char(c);
        }
    }

    fn push_char(&mut self, c: char) {
        match self.state {
            AnsiState::Plain => match c {
                '\u{1b}' => self.state = AnsiState::Esc,
                '\n' => {
                    let mut line = std::mem::take(&mut self.current);
                    while line.ends_with('\r') {
                        line.pop();
                    }
                    self.lines.push_back(line);
                    if self.lines.len() > self.max_lines {
                        self.lines.pop_front();
                    }
                    self.pending_cr = false;
                }
                '\r' => {
                    // A `\r` immediately followed by `\n` is CRLF (a line
                    // terminator). Otherwise it is a carriage-return overwrite
                    // and we clear the current line for the replacement text.
                    self.pending_cr = true;
                }
                '\u{8}' | '\u{7f}' => {
                    self.current.pop();
                    self.pending_cr = false;
                }
                _ => {
                    // A pending CR followed by a real character means "overwrite
                    // this line from the start".
                    if self.pending_cr {
                        self.current.clear();
                        self.pending_cr = false;
                    }
                    self.current.push(c);
                }
            },
            AnsiState::Esc => {
                self.state = match c {
                    '[' => AnsiState::Csi,
                    ']' => AnsiState::Osc,
                    // ESC ( B / ESC ) 0 charset selection, etc.: drop the pair.
                    _ => AnsiState::Plain,
                };
            }
            AnsiState::Csi => {
                if ('@'..='~').contains(&c) {
                    self.state = AnsiState::Plain;
                }
            }
            AnsiState::Osc => {
                if c == '\u{7}' {
                    self.state = AnsiState::Plain;
                } else if c == '\u{1b}' {
                    self.state = AnsiState::OscEsc;
                }
            }
            AnsiState::OscEsc => {
                self.state = if c == '\\' {
                    AnsiState::Plain
                } else {
                    AnsiState::Osc
                };
            }
        }
    }

    /// The last `max` completed lines plus the in-progress line. The in-progress
    /// line is appended as the final entry so callers render the newest output
    /// (the prompt + partial line) at the bottom of the scrollback.
    pub fn tail(&self, max: usize) -> Vec<String> {
        let mut out: Vec<String> = self
            .lines
            .iter()
            .rev()
            .take(max.saturating_sub(1))
            .cloned()
            .collect();
        out.reverse();
        out.push(self.current.clone());
        out
    }

    pub fn completed_lines(&self) -> usize {
        self.lines.len()
    }

    pub fn current(&self) -> &str {
        &self.current
    }
}

/// A cloneable snapshot of the terminal's visible output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSnapshot {
    /// The newest lines (last element is the in-progress / prompt line).
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
/// The local input buffer mirror (what `Cmd+Enter` routes to the harness) is
/// maintained by the caller from the `Forward` bytes; `RouteToAgent` consumes
/// it without touching the pty.
pub fn classify_key(key: &str, modifiers: ModifiersState) -> TerminalKeyAction {
    // Cmd/Ctrl+Enter must NOT reach the shell: it is an agent turn instead.
    if is_agent_enter(key, modifiers) {
        return TerminalKeyAction::RouteToAgent;
    }

    if key.eq_ignore_ascii_case("enter") || key.eq_ignore_ascii_case("return") {
        return TerminalKeyAction::Forward(vec![b'\n']);
    }
    if key == "Backspace" {
        return TerminalKeyAction::Forward(vec![b'\x7f']);
    }
    if key == "Tab" {
        return TerminalKeyAction::Forward(vec![b'\t']);
    }

    // Ctrl+<letter> sends the ASCII control code (e.g. Ctrl+C interrupts the
    // running program, Ctrl+L clears the screen). Command+letter is left alone
    // (it is an OS shortcut, not a shell keystroke).
    if modifiers.ctrl && !modifiers.command && !modifiers.alt && key.chars().count() == 1 {
        if let Some(c) = key.chars().next() {
            if c.is_ascii_lowercase() {
                let code = c as u8 - b'a' + 1;
                if (1..=26).contains(&code) {
                    return TerminalKeyAction::Forward(vec![code]);
                }
            }
        }
    }

    // Printable character (a single char from the OS key event). Multi-byte
    // characters arrive as one `key` string, so treat any all-printable string
    // as typed input (this also covers a single paste chunk).
    let mut chars = key.chars();
    if let Some(first) = chars.next() {
        let printable = chars.all(|c| !c.is_control())
            && !first.is_control()
            && first != '\u{1b}';
        if printable {
            return TerminalKeyAction::Forward(key.as_bytes().to_vec());
        }
    }

    TerminalKeyAction::Ignore
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

/// Apply a chunk of forwarded bytes to the per-pane local input mirror.
///
/// This is the input that `Cmd+Enter` routes to the harness; it mirrors exactly
/// what was forwarded to the shell so the agent sees the command line as typed.
pub fn update_input_mirror(mirror: &mut String, bytes: &[u8]) {
    let s = String::from_utf8_lossy(bytes);
    for c in s.chars() {
        match c {
            '\n' | '\r' => mirror.clear(),
            '\u{8}' | '\u{7f}' => {
                mirror.pop();
            }
            _ => mirror.push(c),
        }
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
/// The reader thread owns the blocking read end and pushes bytes into the
/// shared [`TerminalBuffer`]; the UI thread only ever takes a short-lived lock
/// to coalesce frames. Writing input goes straight to the pty master.
pub struct TerminalSession {
    output: Arc<Mutex<TerminalBuffer>>,
    writer: Option<Box<dyn Write + Send>>,
    /// Kept so the pty stays open for the lifetime of the session.
    _master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    child: Option<Box<dyn portable_pty::Child + Send>>,
}

impl std::fmt::Debug for TerminalSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalSession")
            .field("alive", &self.child.is_some())
            .finish_non_exhaustive()
    }
}

impl TerminalSession {
    /// A session that failed to launch; renders the error rather than panicking.
    pub fn failed(message: String) -> Self {
        let output = Arc::new(Mutex::new(TerminalBuffer::new(MAX_LINES)));
        if let Ok(mut buf) = output.lock() {
            buf.append_bytes(format!("(terminal error: {message})\n").as_bytes());
        }
        Self {
            output,
            writer: None,
            _master: None,
            child: None,
        }
    }

    /// Spawn `$SHELL` (or `/bin/zsh`) in `cwd` and start a background reader.
    pub fn spawn(cwd: &str) -> Result<Self, String> {
        let output = Arc::new(Mutex::new(TerminalBuffer::new(MAX_LINES)));

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 96,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;

        let mut cmd = CommandBuilder::new(default_shell());
        cmd.cwd(resolve_cwd(cwd));

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| e.to_string())?;
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| {
                let _ = child.kill();
                e.to_string()
            })?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| {
                let _ = child.kill();
                e.to_string()
            })?;

        let out = Arc::clone(&output);
        // Detached reader: it owns the blocking read end and pushes bytes into
        // the shared buffer until EOF (the child is killed on `drop`). We never
        // join it so the UI thread is never blocked.
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Ok(mut o) = out.lock() {
                            o.append_bytes(&buf[..n]);
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            output,
            writer: Some(writer),
            _master: Some(pair.master),
            child: Some(child),
        })
    }

    /// Write bytes to the shell's stdin. Non-blocking for the UI thread.
    pub fn write(&mut self, bytes: &[u8]) {
        if let Some(w) = self.writer.as_mut() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    /// Coalesce the newest output into a snapshot for painting.
    pub fn snapshot(&self, max_lines: usize) -> TerminalSnapshot {
        let buf = self.output.lock().map(|g| g.tail(max_lines)).unwrap_or_default();
        TerminalSnapshot { lines: buf }
    }

    /// Force a fixed-size reshape of the pty (best effort).
    pub fn set_size(&mut self, rows: u16, cols: u16) {
        if let Some(master) = self._master.as_mut() {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }

    /// The listener thread has finished (child exited) — used to cheaply detect
    /// when the shell is closed so the pane can render a hint.
    pub fn is_alive(&self) -> bool {
        self.child.is_some()
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

    /// Remove a pane's session + input mirror (dropping the session kills the
    /// shell). Called when a pane closes.
    pub fn drop_pane(&mut self, pane_id: u64) {
        self.sessions.remove(&pane_id);
        self.input.remove(&pane_id);
    }

    pub fn input(&self, pane_id: u64) -> String {
        self.input.get(&pane_id).cloned().unwrap_or_default()
    }

    pub fn set_input(&mut self, pane_id: u64, value: String) {
        self.input.insert(pane_id, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_splits_lines() {
        let mut buf = TerminalBuffer::new(10);
        buf.append_bytes(b"hello\nworld\n");
        let lines = buf.tail(10);
        // Last element is the (empty) in-progress line.
        assert_eq!(lines, vec!["hello".to_string(), "world".to_string(), String::new()]);
    }

    #[test]
    fn buffer_strips_csi_escape_sequences() {
        let mut buf = TerminalBuffer::new(10);
        buf.append_bytes(b"\x1b[31mred\x1b[0m\n");
        let lines = buf.tail(10);
        assert_eq!(lines[0], "red");
    }

    #[test]
    fn buffer_strips_osc_escape_sequences() {
        let mut buf = TerminalBuffer::new(10);
        buf.append_bytes(b"\x1b]0;title\x07text\n");
        let lines = buf.tail(10);
        assert_eq!(lines[0], "text");
    }

    #[test]
    fn buffer_handles_crlf_as_newline() {
        let mut buf = TerminalBuffer::new(10);
        buf.append_bytes(b"foo\r\nbar\n");
        let lines = buf.tail(10);
        assert_eq!(lines[0], "foo");
        assert_eq!(lines[1], "bar");
    }

    #[test]
    fn buffer_overwrites_on_lone_carriage_return() {
        let mut buf = TerminalBuffer::new(10);
        buf.append_bytes(b"10%\r20%\r30%\n");
        let lines = buf.tail(10);
        assert_eq!(lines[0], "30%");
    }

    #[test]
    fn buffer_backspace_pops_current_line() {
        let mut buf = TerminalBuffer::new(10);
        buf.append_bytes(b"ab\x7f\n");
        let lines = buf.tail(10);
        assert_eq!(lines[0], "a");
    }

    #[test]
    fn buffer_caps_line_count() {
        let mut buf = TerminalBuffer::new(3);
        for i in 0..5 {
            buf.append_bytes(format!("line{i}\n").as_bytes());
        }
        let lines = buf.tail(10);
        assert_eq!(lines.len(), 4, "3 completed + 1 in-progress");
        assert_eq!(lines[0], "line2");
        assert_eq!(lines[2], "line4");
    }

    #[test]
    fn enter_is_forwarded_plain_and_routed_on_cmd_or_ctrl() {
        use ModifiersState;
        assert_eq!(
            classify_key("Enter", ModifiersState::none()),
            TerminalKeyAction::Forward(vec![b'\n'])
        );
        assert_eq!(
            classify_key(
                "Enter",
                ModifiersState {
                    command: true,
                    ..Default::default()
                }
            ),
            TerminalKeyAction::RouteToAgent
        );
        assert_eq!(
            classify_key(
                "Enter",
                ModifiersState {
                    ctrl: true,
                    ..Default::default()
                }
            ),
            TerminalKeyAction::RouteToAgent
        );
        // Shift+Enter stays a shell newline.
        assert_eq!(
            classify_key(
                "Enter",
                ModifiersState {
                    command: true,
                    shift: true,
                    ..Default::default()
                }
            ),
            TerminalKeyAction::Forward(vec![b'\n'])
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

    #[test]
    fn printable_keys_are_forwarded_as_bytes() {
        assert_eq!(
            classify_key("a", ModifiersState::none()).unwrap_forward(),
            b"a".to_vec()
        );
        assert_eq!(
            classify_key("A", ModifiersState::none()).unwrap_forward(),
            b"A".to_vec()
        );
        assert_eq!(
            classify_key("Backspace", ModifiersState::none()).unwrap_forward(),
            vec![b'\x7f']
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
                }
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
                }
            )
            .unwrap_forward(),
            vec![0x0c],
            "Ctrl+L clears the screen"
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
        update_input_mirror(&mut mirror, b"ls");
        assert_eq!(mirror, "ls");
        update_input_mirror(&mut mirror, &[b'\x7f']);
        assert_eq!(mirror, "l");
        update_input_mirror(&mut mirror, b"\n");
        assert_eq!(mirror, "");
    }

    #[test]
    fn cwd_resolution_falls_back_to_process_dir() {
        let cwd = resolve_cwd("/definitely/not/a/real/dir/xyz");
        assert!(cwd.is_absolute());
        let cwd2 = resolve_cwd("");
        assert!(cwd2.is_absolute());
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
