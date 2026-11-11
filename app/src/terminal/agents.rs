// ---------------------------------------------------------------------------
// TUI agent detection + terminal surface mode
// ---------------------------------------------------------------------------

use std::path::Path;

use goble_core::harness::{detect_harness, harness_by_id, HarnessSpec};

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
    /// `pi`, the π coding agent.
    Pi,
    /// The Grok CLI's agent TUI.
    Grok,
    /// Hermes' interaction CLI.
    Hermes,
    /// Google Antigravity's CLI (`agy`). It paints its TUI inline rather than
    /// taking the alternate screen, so detection is the only signal that the
    /// program owns the pane.
    Antigravity,
    /// Any other harness in the shared catalog (see
    /// [`goble_core::harness::catalog`]): the catalog names its icon, label and
    /// whether it paints a screen.
    Catalog(&'static HarnessSpec),
    /// An unrecognised binary launched explicitly (via the launch button); the
    /// pane still goes native-first so the requested agent owns the line.
    Other,
}

impl TuiAgent {
    /// The canonical command used to launch this agent.
    pub fn command(self) -> &'static str {
        if let Some(spec) = self.spec() {
            return spec.commands.first().copied().unwrap_or("codex");
        }
        "codex"
    }

    /// Short label for the terminal pane's mode badge and the pane's topbar.
    pub fn label(self) -> &'static str {
        if let Some(spec) = self.spec() {
            return spec.name;
        }
        "agent"
    }

    /// The harness id the icon is drawn from (`harness-<id>`).
    pub fn id(self) -> &'static str {
        match self {
            TuiAgent::Other => "goble",
            _ => self.spec().map_or("goble", |spec| spec.id),
        }
    }

    /// The catalog entry for this agent, if its harness is in the catalog.
    pub fn spec(self) -> Option<&'static HarnessSpec> {
        match self {
            TuiAgent::Codex => harness_by_id("codex"),
            TuiAgent::Claude => harness_by_id("claude"),
            TuiAgent::Gemini => harness_by_id("gemini"),
            TuiAgent::OpenCode => harness_by_id("opencode"),
            TuiAgent::Cursor => harness_by_id("cursor"),
            TuiAgent::Aider => harness_by_id("aider"),
            TuiAgent::Pi => harness_by_id("pi"),
            TuiAgent::Grok => harness_by_id("grok"),
            TuiAgent::Hermes => harness_by_id("hermes"),
            TuiAgent::Antigravity => harness_by_id("antigravity"),
            TuiAgent::Catalog(spec) => Some(spec),
            TuiAgent::Other => None,
        }
    }

    /// Whether the program paints its own screen and therefore owns the pane's
    /// keys. A protocol-over-stdio harness leaves the command blocks and the
    /// rich input in place.
    pub fn owns_screen(self) -> bool {
        match self {
            TuiAgent::Other => true,
            _ => self.spec().is_some_and(|spec| spec.tui),
        }
    }

    /// Detect a harness from a command line through the shared catalog. Wrapper
    /// programs and paths are normalised by the catalog, so this covers every
    /// Paseo harness and not just the ones with a dedicated variant.
    pub fn detect(command: &str) -> Option<TuiAgent> {
        let spec = detect_harness(command)?;
        Some(match spec.id {
            "codex" => TuiAgent::Codex,
            "claude" => TuiAgent::Claude,
            "gemini" => TuiAgent::Gemini,
            "opencode" => TuiAgent::OpenCode,
            "cursor" => TuiAgent::Cursor,
            "aider" => TuiAgent::Aider,
            "pi" => TuiAgent::Pi,
            "grok" => TuiAgent::Grok,
            "hermes" => TuiAgent::Hermes,
            "antigravity" => TuiAgent::Antigravity,
            _ => TuiAgent::Catalog(spec),
        })
    }

    /// All agents with a dedicated variant, for building a launch menu.
    pub const ALL: [TuiAgent; 10] = [
        TuiAgent::Codex,
        TuiAgent::Claude,
        TuiAgent::Gemini,
        TuiAgent::OpenCode,
        TuiAgent::Cursor,
        TuiAgent::Aider,
        TuiAgent::Pi,
        TuiAgent::Grok,
        TuiAgent::Hermes,
        TuiAgent::Antigravity,
    ];
}

/// Whether the program `command` runs is installed on this machine.
///
/// A pane only switches to a harness's surface when its program is really
/// there: a program that is missing is the shell's own error (`zsh: command not
/// found: pi`), and opening the harness surface for it would tell the user the
/// harness was installed and running.
///
/// Wrappers, flags and `VAR=value` assignments are skipped the way
/// [`detect_harness`] skips them. A package runner (`npx`, `bunx`, `uvx`,
/// `pnpm`, `yarn`) is the program itself: it fetches the package the line
/// names, so `npx -y codex` is installed exactly when `npx` is.
pub fn command_installed(command: &str) -> bool {
    let Some(program) = program_to_check(command) else {
        return false;
    };
    // A path is checked as given, so a relative one resolves against goble's
    // own working directory; a bare name is looked up on `PATH`.
    if program.contains(['/', '\\']) {
        return is_executable_file(Path::new(program));
    }
    on_path(program)
}

/// The program a line runs: the first token left once its wrappers, flags and
/// assignments are skipped.
pub(super) fn program_to_check(command: &str) -> Option<&str> {
    command
        .split_whitespace()
        .take(8)
        .find(|token| !token.starts_with('-') && !is_assignment(token) && !is_wrapper(token))
}

/// Whether `token` is a wrapper that runs the program after it.
fn is_wrapper(token: &str) -> bool {
    matches!(token, "sudo" | "command" | "exec" | "env")
}

/// Whether `token` is a shell variable assignment (`FOO=1`).
fn is_assignment(token: &str) -> bool {
    token.split_once('=').is_some_and(|(key, _)| {
        !key.is_empty()
            && key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    })
}

/// Whether the bare name `program` is in one of the directories on `PATH`.
fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| is_executable_file(&dir.join(program)))
}

/// Whether `path` is a file this machine can execute. Unix asks for any execute
/// bit, the same test the shell makes when it looks a command up.
#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
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
