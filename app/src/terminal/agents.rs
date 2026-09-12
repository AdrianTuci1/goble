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
