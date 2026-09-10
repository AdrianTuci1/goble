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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tokio::sync::oneshot;

use goble_core::harness::PaneSession;
use goble_terminal::blocks::BlockView;
use goble_terminal::hooks::PreexecValue;
use goble_terminal::{
    encode_mouse, BlockEvent, BlockId, BlockOwner, CursorState, HookEvent, Key, KeyEncoder,
    Modifiers, MouseAction, OscEvent, Palette, ScreenLine, TermMode, ToolResult,
};
use goble_ui::event::ModifiersState;

use crate::emulator::{Emulator, VisibleBlock};

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

/// The name a shell was invoked as, without its path: `bash`, `zsh`, ...
fn shell_name(shell: &str) -> &str {
    Path::new(shell)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(shell)
}

/// A per-session scratch directory for the integration script, so two panes
/// never share one. Removed when the session drops.
fn integration_dir() -> Option<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("goble-shell-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Build a pane's shell command, pointing a supported shell at its
/// shell-integration script. bash is given `--rcfile`; zsh reads
/// `$ZDOTDIR/.zshrc`. Each script then reads the user's own rc itself, because
/// both mechanisms replace the file the shell would normally read. An
/// unsupported shell is spawned unchanged and degrades to a plain terminal.
///
/// Returns the command and the scratch directory to remove on drop.
fn shell_command(shell: &str, cwd: PathBuf) -> (CommandBuilder, Option<PathBuf>) {
    let mut cmd = CommandBuilder::new(shell);
    cmd.cwd(cwd);

    let name = shell_name(shell);
    if name.starts_with("bash") {
        let Some(dir) = integration_dir() else {
            return (cmd, None);
        };
        let script = dir.join("goble.bashrc");
        if std::fs::write(&script, goble_terminal::integration::BASH).is_err() {
            return (cmd, None);
        }
        cmd.arg("--rcfile");
        cmd.arg(&script);
        (cmd, Some(dir))
    } else if name.starts_with("zsh") {
        let Some(dir) = integration_dir() else {
            return (cmd, None);
        };
        if std::fs::write(dir.join(".zshrc"), goble_terminal::integration::ZSH).is_err() {
            return (cmd, None);
        }
        if std::fs::write(dir.join(".zshenv"), goble_terminal::integration::ZSH_ENV).is_err() {
            return (cmd, None);
        }
        // Hand the script the directory of the user's own zshrc; ZDOTDIR now
        // points at ours, so it cannot find it on its own.
        if let Some(original) = std::env::var_os("ZDOTDIR")
            .filter(|value| !value.is_empty())
            .or_else(|| std::env::var_os("HOME"))
        {
            cmd.env("GOBLE_ORIG_ZDOTDIR", original);
        }
        cmd.env("ZDOTDIR", &dir);
        (cmd, Some(dir))
    } else {
        (cmd, None)
    }
}

/// How long a claim waits for the shell's `Preexec` before it is failed.
///
/// The wait is bounded on purpose: a shell that never runs the command (a
/// syntax error, a shell whose integration has not bootstrapped, a command the
/// line editor rejected) must fail the tool call rather than hang it.
pub const CLAIM_TIMEOUT: Duration = Duration::from_secs(30);

/// A command the app asked the pane's shell to run, waiting for the `Preexec`
/// that proves which command actually started.
///
/// Only a [`TerminalSession`] builds one: it is what the claim was placed for
/// plus the bounded wait, so a caller cannot fake a resolved claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCommandClaim {
    /// Who the block belongs to once the claim resolves.
    pub owner: BlockOwner,
    /// The command line that was written to the pty, compared against the
    /// `Preexec` the shell reports.
    pub command: String,
    /// When the bounded wait ends.
    deadline: Instant,
}

/// Why a claim could not be placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimError {
    /// The command line is empty; there is nothing for the shell to run.
    EmptyCommand,
    /// A claim is already waiting for its `Preexec`. The shell has one line
    /// editor, so a second claim would leave the first one unattributed.
    AlreadyPending,
}

/// What happened to a claim.
///
/// Rule three of the bridge: a claim attaches only to the command it wrote. It
/// is refused when a different command reaches the shell first, and it times
/// out when the shell never answers — it is never attached on a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimOutcome {
    /// The next `Preexec` was the claimed command: the block it starts belongs
    /// to the claim's owner.
    Resolved { owner: BlockOwner, command: String },
    /// A different command reached the shell first, so the claim is refused
    /// rather than attached to the wrong block.
    Refused {
        owner: BlockOwner,
        expected: String,
        actual: String,
    },
    /// No `Preexec` arrived within the bounded wait.
    TimedOut { owner: BlockOwner, expected: String },
}

impl ClaimOutcome {
    /// The owner the claim was placed for, so the caller can fail the matching
    /// tool call.
    pub fn owner(&self) -> &BlockOwner {
        match self {
            ClaimOutcome::Resolved { owner, .. }
            | ClaimOutcome::Refused { owner, .. }
            | ClaimOutcome::TimedOut { owner, .. } => owner,
        }
    }

    /// Whether the claim attached to the command it wrote.
    pub fn is_resolved(&self) -> bool {
        matches!(self, ClaimOutcome::Resolved { .. })
    }
}

/// How long the harness waits for the pane to answer a shell-tool command.
///
/// A pane that is never pumped (a background tab) must fail the tool call
/// rather than hang it. This is the outer bound, longer than the claim's own
/// bounded wait so a genuine refusal or timeout is reported by the pane first.
pub const PANE_TOOL_TIMEOUT: Duration = Duration::from_secs(35);

/// One command the harness asked the pane's shell to run.
struct PaneCommandRequest {
    /// The composed command line (see `RoutedCommandRunner`).
    line: String,
    /// The conversation that asked for it, for the block owner.
    conversation_id: String,
    /// Identifies the claim so its tool result can be matched back.
    call_id: String,
    /// Where the tool result is sent once the pane resolves the claim.
    reply: oneshot::Sender<Result<String, String>>,
}

/// The rendezvous between the harness and one pane's session.
///
/// The claim protocol allows one in-flight command per shell, so there is a
/// single request slot: a second submission while one waits is refused rather
/// than queued behind it.
#[derive(Default)]
struct PaneCommands {
    request: Option<PaneCommandRequest>,
}

/// The harness's handle to a pane's own shell (the P4 route).
///
/// The harness runs on the daemon's runtime while the pane's session is pumped
/// on the UI thread, so the handle only submits the command and waits; the
/// session claims it and hands back the block list's tool result. That is the
/// correlation model of `agent-terminal-bridge.md` §3 carried across the thread
/// boundary the two live on.
struct PaneCommandRunner {
    commands: Arc<Mutex<PaneCommands>>,
    /// The conversation this pane's turns belong to.
    conversation_id: String,
    /// Names each claim this runner makes, so results are matched, not guessed.
    next_call: AtomicU64,
}

impl PaneCommandRunner {
    /// A runner reaching the session that shares `commands`.
    fn new(commands: Arc<Mutex<PaneCommands>>, conversation_id: impl Into<String>) -> Self {
        Self {
            commands,
            conversation_id: conversation_id.into(),
            next_call: AtomicU64::new(1),
        }
    }
}

#[async_trait::async_trait]
impl PaneSession for PaneCommandRunner {
    async fn run_in_pane(&self, line: &str) -> anyhow::Result<String> {
        let (reply, answer) = oneshot::channel();
        {
            let mut commands = self
                .commands
                .lock()
                .map_err(|_| anyhow::anyhow!("the pane's command bridge is poisoned"))?;
            if commands.request.is_some() {
                anyhow::bail!("the pane is already running a command for the agent");
            }
            commands.request = Some(PaneCommandRequest {
                line: line.to_string(),
                conversation_id: self.conversation_id.clone(),
                call_id: format!(
                    "pane-call-{}",
                    self.next_call.fetch_add(1, Ordering::Relaxed)
                ),
                reply,
            });
        }
        match tokio::time::timeout(PANE_TOOL_TIMEOUT, answer).await {
            Ok(Ok(Ok(output))) => Ok(output),
            Ok(Ok(Err(message))) => anyhow::bail!(message),
            Ok(Err(_)) => anyhow::bail!("the pane dropped the command before answering"),
            Err(_) => anyhow::bail!(
                "the pane did not answer within {}s",
                PANE_TOOL_TIMEOUT.as_secs()
            ),
        }
    }
}

/// The tool result text for a claimed block's outcome.
///
/// A non-zero exit is a failed tool call, exactly as the sandboxed runner's is;
/// a command the shell moved past is not a hang — the result says it is still
/// running and carries the output that had arrived.
fn tool_result_text(result: &ToolResult) -> Result<String, String> {
    if result.is_failure() {
        return Err(format!(
            "command failed (exit {}): {}",
            result.exit_code().unwrap_or(-1),
            result.output
        ));
    }
    if result.is_still_running() {
        return Ok(format!(
            "{}\n(the command is still running; it did not report an exit code)",
            result.output
        ));
    }
    Ok(result.output.clone())
}

/// The claim id a verdict names, so its tool call can be failed.
fn owner_call_id(owner: &BlockOwner) -> String {
    match owner {
        BlockOwner::Agent { call_id, .. } => call_id.clone(),
        BlockOwner::User => String::new(),
    }
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
    /// Scratch directory holding this session's integration script; removed on
    /// drop. `None` when the shell has no integration or the file could not be
    /// written.
    integration_dir: Option<PathBuf>,
    /// A command the app claimed, waiting for its `Preexec` to resolve it.
    pending_claim: Option<PendingCommandClaim>,
    /// Claim verdicts the pane has not read yet, bounded like the hooks.
    claim_outcomes: VecDeque<ClaimOutcome>,
    /// The bounded wait a new claim gets; overridden in tests.
    claim_timeout: Duration,
    /// The harness's side of the shell-tool route for this pane. Commands are
    /// submitted here and answered from the block list's tool results.
    commands: Arc<Mutex<PaneCommands>>,
    /// Tool calls waiting for their claimed block's result, keyed by call id.
    awaiting: HashMap<String, oneshot::Sender<Result<String, String>>>,
    /// Claimed blocks whose tool result has arrived but whose caller has not
    /// been answered yet.
    tool_results: Vec<ToolResult>,
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
    pub(crate) fn with_emulator(emulator: Emulator) -> Self {
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
            integration_dir: None,
            pending_claim: None,
            claim_outcomes: VecDeque::new(),
            claim_timeout: CLAIM_TIMEOUT,
            commands: Arc::new(Mutex::new(PaneCommands::default())),
            awaiting: HashMap::new(),
            tool_results: Vec::new(),
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

        let (cmd, integration_dir) = shell_command(&default_shell(), resolve_cwd(cwd));

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
            integration_dir,
            pending_claim: None,
            claim_outcomes: VecDeque::new(),
            claim_timeout: CLAIM_TIMEOUT,
            commands: Arc::new(Mutex::new(PaneCommands::default())),
            awaiting: HashMap::new(),
            tool_results: Vec::new(),
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
        let (events, replies, hooks, osc, block_events) = {
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
                state.take_block_events(),
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
            // The claim is resolved (or refused) as the hooks arrive, so the
            // pane never has to hand its hook stream to somebody else first.
            if let HookEvent::Preexec(value) = &event {
                self.observe_preexec(value);
            }
            if self.hooks.len() == EVENT_BACKLOG {
                self.hooks.pop_front();
            }
            self.hooks.push_back(event);
        }
        // A block the agent claimed reaching a terminal state is that tool
        // call's result: its output and exit code answer the harness.
        for event in block_events {
            if let BlockEvent::ToolResult(result) = event {
                self.tool_results.push(result);
            }
        }
        // A claim the shell never answered is failed on the frame it expires
        // rather than left pending forever; the pane pumps every frame.
        self.expire_claim();
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

        // The harness's shell tool rides this same frame: submit, claim, and
        // hand back the block's result once the hooks have produced it.
        self.service_pane_commands();
    }

    /// Carry the harness's shell tool through this pane (P4).
    ///
    /// One submitted command is claimed for its tool call and written to the
    /// pty; a claim the shell refuses, or one that outlives the bounded wait,
    /// fails the tool call, and a claimed block's tool result answers it with
    /// its output and exit code. An unmapped result (a stale call) is dropped.
    fn service_pane_commands(&mut self) {
        let request = self
            .commands
            .lock()
            .ok()
            .and_then(|mut commands| commands.request.take());
        if let Some(request) = request {
            let owner = BlockOwner::Agent {
                conversation_id: request.conversation_id.clone(),
                call_id: request.call_id.clone(),
            };
            if !self.arm_block_claim(&owner) {
                let _ = request.reply.send(Err(
                    "the pane's shell is not ready to run an agent command".to_string(),
                ));
            } else if let Err(error) = self.claim_command(&request.line, owner) {
                let _ = request.reply.send(Err(format!(
                    "the pane could not claim the command: {error:?}"
                )));
            } else {
                self.awaiting.insert(request.call_id, request.reply);
            }
        }

        for outcome in self.claim_outcomes() {
            let failure = match &outcome {
                ClaimOutcome::Resolved { .. } => None,
                ClaimOutcome::Refused {
                    owner,
                    expected,
                    actual,
                } => Some((
                    owner_call_id(owner),
                    format!("the pane's shell ran `{actual}` first, not `{expected}`"),
                )),
                ClaimOutcome::TimedOut { owner, expected } => Some((
                    owner_call_id(owner),
                    format!("the pane's shell did not run `{expected}` in time"),
                )),
            };
            match failure {
                // A verdict for one of our tool calls fails it; the pane's own
                // record of the rest is left intact for whoever reads it.
                Some((call_id, message)) => match self.awaiting.remove(&call_id) {
                    Some(reply) => {
                        let _ = reply.send(Err(message));
                    }
                    None => self.claim_outcomes.push_back(outcome),
                },
                None => self.claim_outcomes.push_back(outcome),
            }
        }

        for result in std::mem::take(&mut self.tool_results) {
            let Some(reply) = self.awaiting.remove(&result.call_id) else {
                continue;
            };
            let _ = reply.send(tool_result_text(&result));
        }
    }

    /// Aim the block list's next `Preexec` at the active block, so the command
    /// the agent's tool call runs becomes a block that call owns.
    fn arm_block_claim(&mut self, owner: &BlockOwner) -> bool {
        match self.state.lock() {
            Ok(mut state) => state.claim_active_block(owner.clone()),
            Err(_) => false,
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

    /// The listener thread has finished (child exited) — used to cheaply detect
    /// when the shell is closed so the pane can render a hint.
    pub fn is_alive(&self) -> bool {
        self.child.is_some()
    }

    /// Ask the pane's shell to run `command` on behalf of an agent turn.
    ///
    /// The command is written to the pty and held as a claim: the next `Preexec`
    /// the shell reports decides whether it was the command we asked for. The
    /// claim never attaches on a guess — a `Preexec` for a different command
    /// refuses it, and one that never comes times out (see [`ClaimOutcome`]).
    pub fn claim_command(&mut self, command: &str, owner: BlockOwner) -> Result<(), ClaimError> {
        let command = command.trim();
        if command.is_empty() {
            return Err(ClaimError::EmptyCommand);
        }
        if self.pending_claim.is_some() {
            return Err(ClaimError::AlreadyPending);
        }
        // Enter, as the terminal sees it: a carriage return. A program in raw
        // mode does not read a line feed as "run this".
        let mut bytes = command.as_bytes().to_vec();
        bytes.push(b'\r');
        self.write(&bytes);
        self.pending_claim = Some(PendingCommandClaim {
            owner,
            command: command.to_string(),
            deadline: Instant::now() + self.claim_timeout,
        });
        Ok(())
    }

    /// The claim waiting for its `Preexec`, if any.
    pub fn pending_claim(&self) -> Option<&PendingCommandClaim> {
        self.pending_claim.as_ref()
    }

    /// Take the claim verdicts seen since the last call, oldest first.
    pub fn claim_outcomes(&mut self) -> Vec<ClaimOutcome> {
        self.claim_outcomes.drain(..).collect()
    }

    /// Resolve the pending claim against one `Preexec`.
    ///
    /// A `Preexec` with no claim is the user's command and touches nothing.
    /// With a claim, only the same command line resolves it; anything else —
    /// including a `Preexec` that does not report a command at all — refuses
    /// the claim, because attaching it to the wrong block is worse than failing
    /// the tool call.
    fn observe_preexec(&mut self, value: &PreexecValue) {
        let Some(claim) = self.pending_claim.take() else {
            return;
        };
        let actual = value.command.clone().unwrap_or_default();
        let outcome = if actual.trim() == claim.command {
            ClaimOutcome::Resolved {
                owner: claim.owner,
                command: claim.command,
            }
        } else {
            ClaimOutcome::Refused {
                owner: claim.owner,
                expected: claim.command,
                actual,
            }
        };
        self.push_claim_outcome(outcome);
    }

    /// Fail a claim that outlived its bounded wait.
    fn expire_claim(&mut self) {
        let Some(claim) = self.pending_claim.as_ref() else {
            return;
        };
        if Instant::now() < claim.deadline {
            return;
        }
        let claim = self.pending_claim.take().expect("checked above");
        self.push_claim_outcome(ClaimOutcome::TimedOut {
            owner: claim.owner,
            expected: claim.command,
        });
    }

    fn push_claim_outcome(&mut self, outcome: ClaimOutcome) {
        if self.claim_outcomes.len() == EVENT_BACKLOG {
            self.claim_outcomes.pop_front();
        }
        self.claim_outcomes.push_back(outcome);
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
        if let Some(dir) = self.integration_dir.take() {
            let _ = std::fs::remove_dir_all(dir);
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

    /// The harness's shell-tool route for a pane that has a terminal, or `None`
    /// for a pane that has none.
    ///
    /// A pane gets a session the first time it runs a command or paints a
    /// terminal; until then there is no shell to route through, so the turn
    /// keeps the sandboxed runner. Once a session exists the agent's commands
    /// run in it, visibly, with the user's environment and credentials.
    pub fn pane_session(
        &self,
        pane_id: u64,
        conversation_id: &str,
    ) -> Option<Arc<dyn PaneSession>> {
        let session = self.sessions.get(&pane_id)?;
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

#[cfg(test)]
mod tests {
    use super::*;

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

    // -- The claim protocol (P2) --------------------------------------------

    #[test]
    fn a_claim_writes_the_command_to_the_pty() {
        let mut session = detached();
        let captured = Arc::new(Mutex::new(Vec::new()));
        session.writer = Some(Box::new(CaptureWriter(Arc::clone(&captured))));

        session.claim_command("git status", agent_owner()).unwrap();

        assert_eq!(&*captured.lock().unwrap(), b"git status\r");
        assert_eq!(
            session.pending_claim().map(|c| c.command.as_str()),
            Some("git status")
        );
    }

    #[test]
    fn a_claimed_command_resolves_on_its_own_preexec() {
        let mut session = detached();
        let owner = agent_owner();
        session.claim_command("echo hi", owner.clone()).unwrap();

        run_hook(&mut session, preexec("echo hi"));

        assert!(session.pending_claim().is_none(), "the claim is consumed");
        assert_eq!(
            session.claim_outcomes(),
            vec![ClaimOutcome::Resolved {
                owner,
                command: "echo hi".to_string(),
            }]
        );
    }

    #[test]
    fn a_claim_that_never_gets_a_preexec_times_out() {
        let mut session = detached();
        let owner = agent_owner();
        // A zero bounded wait, so the claim expires on the next pump without
        // the test sleeping.
        session.claim_timeout = Duration::ZERO;
        session.claim_command("echo hi", owner.clone()).unwrap();

        session.pump();

        assert!(session.pending_claim().is_none(), "the claim is failed");
        assert_eq!(
            session.claim_outcomes(),
            vec![ClaimOutcome::TimedOut {
                owner,
                expected: "echo hi".to_string(),
            }]
        );
        // A late `Preexec` cannot revive a claim that already failed.
        run_hook(&mut session, preexec("echo hi"));
        assert!(session.claim_outcomes().is_empty());
    }

    #[test]
    fn a_claim_is_refused_when_a_user_command_arrives_first() {
        let mut session = detached();
        let owner = agent_owner();
        session.claim_command("echo agent", owner.clone()).unwrap();

        // The user hit enter before the agent's command reached the shell, so
        // the next `Preexec` is theirs.
        run_hook(&mut session, preexec("ls -la"));

        assert!(session.pending_claim().is_none(), "the claim is consumed");
        assert_eq!(
            session.claim_outcomes(),
            vec![ClaimOutcome::Refused {
                owner: owner.clone(),
                expected: "echo agent".to_string(),
                actual: "ls -la".to_string(),
            }]
        );

        // The agent's command running afterwards does not resurrect the claim:
        // the tool call already failed, and a guess is exactly what rule three
        // forbids.
        run_hook(&mut session, preexec("echo agent"));
        assert!(session.claim_outcomes().is_empty());
    }

    #[test]
    fn a_preexec_without_a_command_refuses_the_claim() {
        let mut session = detached();
        let owner = agent_owner();
        session.claim_command("echo hi", owner.clone()).unwrap();

        // The shell reported a command start but not what it was: it cannot be
        // matched, so it is refused rather than attached blind.
        run_hook(
            &mut session,
            HookEvent::Preexec(PreexecValue { command: None }),
        );

        assert_eq!(
            session.claim_outcomes(),
            vec![ClaimOutcome::Refused {
                owner,
                expected: "echo hi".to_string(),
                actual: String::new(),
            }]
        );
    }

    #[test]
    fn a_second_claim_while_one_is_pending_is_refused() {
        let mut session = detached();
        session.claim_command("echo one", agent_owner()).unwrap();

        assert_eq!(
            session.claim_command("echo two", agent_owner()),
            Err(ClaimError::AlreadyPending)
        );
        assert_eq!(
            session.claim_command("   ", agent_owner()),
            Err(ClaimError::EmptyCommand)
        );
        assert_eq!(
            session.pending_claim().map(|c| c.command.as_str()),
            Some("echo one"),
            "the refused claims changed nothing"
        );
    }

    #[test]
    fn an_unclaimed_preexec_leaves_no_outcome() {
        let mut session = detached();
        run_hook(&mut session, preexec("ls"));
        assert!(session.claim_outcomes().is_empty());
        assert!(session.pending_claim().is_none());
    }

    // -- Routing the harness's shell tool through the pane (P4) --------------

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

    #[test]
    fn an_agent_command_runs_in_the_pane_and_returns_its_output() {
        let mut session = detached();
        let captured = Arc::new(Mutex::new(Vec::new()));
        session.writer = Some(Box::new(CaptureWriter(Arc::clone(&captured))));
        // The shell bootstrap: a claim needs a block to land on.
        run_hook(&mut session, HookEvent::Bootstrapped(Default::default()));

        let answer = submit(&mut session, "echo hi");
        session.pump();
        assert_eq!(&*captured.lock().unwrap(), b"echo hi\r");
        assert!(session.pending_claim().is_some());

        // The shell echoes the line, reports the preexec, prints, and finishes.
        feed_bytes(&mut session, b"echo hi\r\n");
        run_hook(&mut session, preexec("echo hi"));
        feed_bytes(&mut session, b"hi\r\n");
        run_hook(&mut session, HookEvent::CommandFinished(Default::default()));

        assert_eq!(
            answer.blocking_recv().expect("the pane answers"),
            Ok("hi".to_string())
        );
    }

    #[test]
    fn a_command_the_shell_never_ran_fails_the_agent_call() {
        let mut session = detached();
        let captured = Arc::new(Mutex::new(Vec::new()));
        session.writer = Some(Box::new(CaptureWriter(Arc::clone(&captured))));
        run_hook(&mut session, HookEvent::Bootstrapped(Default::default()));

        let answer = submit(&mut session, "echo agent");
        session.pump();

        // The user's own command runs first: rule three refuses the claim
        // rather than attaching it to the wrong block, so the tool call fails.
        run_hook(&mut session, preexec("ls -la"));

        let result = answer.blocking_recv().expect("the pane answers");
        assert!(result.is_err(), "a refused claim fails the tool call");
    }

    #[test]
    fn a_pane_without_a_terminal_has_no_pane_session() {
        let reg = TerminalRegistry::default();
        assert!(reg.pane_session(1, "conv-1").is_none());
    }

    // -- The owner in the two views (P6) ------------------------------------

    /// The terminal view and a conversation's agent view are two filters over
    /// one block list. After a mix of commands the user typed and one the agent
    /// ran, the terminal view shows all of them and the agent view shows only
    /// the agent's block.
    #[test]
    fn the_two_views_show_the_right_blocks_after_a_mixed_sequence() {
        let mut reg = TerminalRegistry::default();
        reg.sessions.insert(1, detached());
        let session = reg.sessions.get_mut(&1).expect("the session is there");
        let captured = Arc::new(Mutex::new(Vec::new()));
        session.writer = Some(Box::new(CaptureWriter(Arc::clone(&captured))));
        run_hook(session, HookEvent::Bootstrapped(Default::default()));

        // The user types `ls`.
        run_hook(session, preexec("ls"));
        run_hook(session, HookEvent::CommandFinished(Default::default()));

        // The agent runs `echo hi` in the same shell, through P4's route.
        let answer = submit(session, "echo hi");
        session.pump();
        feed_bytes(session, b"echo hi\r\n");
        run_hook(session, preexec("echo hi"));
        feed_bytes(session, b"hi\r\n");
        run_hook(session, HookEvent::CommandFinished(Default::default()));
        assert_eq!(
            answer.blocking_recv().expect("the pane answers"),
            Ok("hi".to_string())
        );

        // The user types `pwd` afterwards.
        run_hook(session, preexec("pwd"));
        run_hook(session, HookEvent::CommandFinished(Default::default()));

        let terminal: Vec<String> = reg
            .visible_blocks(1, &BlockView::Terminal)
            .into_iter()
            .map(|block| block.command)
            .filter(|command| !command.is_empty())
            .collect();
        let agent = reg.visible_blocks(
            1,
            &BlockView::Agent {
                conversation_id: "conv-1".to_string(),
            },
        );

        // The user's `ls`, the agent's `echo hi` (a real block in the same
        // shell) and the user's `pwd`. The empty preamble/pending blocks are
        // not commands.
        assert_eq!(terminal, vec!["ls", "echo hi", "pwd"]);
        // Only the agent's command is in the conversation's view; the two the
        // user typed are not.
        assert_eq!(
            agent.iter().map(|b| b.command.as_str()).collect::<Vec<_>>(),
            vec!["echo hi"]
        );
        assert_eq!(
            agent[0].owner,
            BlockOwner::Agent {
                conversation_id: "conv-1".to_string(),
                call_id: "call-1".to_string(),
            }
        );
    }
}
