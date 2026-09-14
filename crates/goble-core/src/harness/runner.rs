use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Output;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use goble_sandbox::{
    sandbox_for, PreparedCommand, Sandbox, SandboxError, SandboxGuard, SandboxProfile,
};

/// A command that the harness can execute directly from a tool call.
#[async_trait::async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, command: &str, args: &[String]) -> Result<String>;
}

/// A no-op / safe command runner for tests and environments where shell execution
/// is not available.
pub struct MockCommandRunner;

#[async_trait::async_trait]
impl CommandRunner for MockCommandRunner {
    async fn run(&self, command: &str, args: &[String]) -> Result<String> {
        Ok(format!("mock ran `{command}` with args {args:?}"))
    }
}

/// A sandboxed shell runner with an allowlist of commands and a per-command timeout.
///
/// The runner applies a configurable [`Sandbox`] around each command when one is
/// set. When no sandbox is configured — or the sandbox backend cannot run on
/// this platform — it executes the command directly, preserving the historical
/// unconfined behavior. The `allowed_commands` allow-list is always enforced
/// regardless of the sandbox configuration.
pub struct SandboxedCommandRunner {
    allowed_commands: HashSet<String>,
    timeout_seconds: u64,
    working_dir: PathBuf,
    sandbox: Option<Box<dyn Sandbox>>,
}

impl SandboxedCommandRunner {
    pub fn new(
        allowed: impl IntoIterator<Item = impl Into<String>>,
        timeout_seconds: u64,
        working_dir: PathBuf,
    ) -> Self {
        Self {
            allowed_commands: allowed.into_iter().map(Into::into).collect(),
            timeout_seconds,
            working_dir,
            sandbox: None,
        }
    }

    pub fn default_tools() -> Self {
        Self::new(
            [
                "echo", "cat", "ls", "pwd", "git", "cargo", "npm", "node", "python3", "rustc",
            ],
            60,
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        )
    }

    /// Configure an isolation backend. Pass `None` (the default) to run commands
    /// directly, exactly as when no sandbox was configured. When `Some`, commands
    /// are run inside the sandbox; if the sandbox backend is unavailable on this
    /// platform the runner degrades to direct execution rather than failing.
    pub fn with_sandbox(mut self, sandbox: Option<Box<dyn Sandbox>>) -> Self {
        self.sandbox = sandbox;
        self
    }

    /// Run `command`, applying the configured sandbox when present.
    ///
    /// When no sandbox is configured — or the sandbox backend cannot run on this
    /// platform — the command is executed directly, preserving the historical
    /// unconfined behavior. A command rejected by the sandbox's own allow-list is
    /// still refused rather than silently degraded.
    async fn run_prepared(&self, command: &PreparedCommand) -> Result<Output> {
        if let Some(sandbox) = &self.sandbox {
            match SandboxGuard::new(&**sandbox, command.clone()) {
                Ok(mut guard) => return guard.run_async().await,
                Err(SandboxError::NotInAllowList { command: denied }) => {
                    anyhow::bail!("command `{denied}` is not in the sandbox allow-list")
                }
                Err(SandboxError::UnsupportedLevel { .. })
                | Err(SandboxError::BackendUnavailable { .. }) => {
                    // The backend cannot apply this profile on this platform.
                    // Degrade to direct execution rather than failing the run.
                }
                Err(e) => return Err(e.into()),
            }
        }
        command
            .to_tokio_command()
            .output()
            .await
            .map_err(Into::into)
    }
}

/// The sandbox profile recommended for the harness's command runner.
///
/// The harness must keep the workspace directory *writable* so `write_file`,
/// `edit_file`, `delete_file`, `rename_file` and `git_commit` keep working, so
/// `read_only_fs` is left `false`. When hardened under `bwrap` this binds the
/// workspace read-write instead of `--ro-bind`ing it; everything outside the
/// workspace stays isolated by bwrap's empty mount tree. On Linux the network is
/// unshared (`no_network` -> `--unshare-net`) so a confined command cannot reach
/// out, and `no_new_privs` refuses privilege escalation. At the `Hardened` level
/// the allow-list is not enforced by the sandbox; the runner's own
/// `allowed_commands` gate is the real command allow-list, so the profile stays
/// gate-agnostic.
///
/// The composition roots (desktop service, native host, Tauri app, worker)
/// build [`SandboxedCommandRunner::default_tools`] and opt in with
/// `with_sandbox(harness_sandbox())`. On macOS `harness_sandbox()` returns a
/// Seatbelt-backed sandbox that really confines the *command* it runs (via
/// `sandbox-exec`), never the calling harness/app process. On platforms with no
/// hardening backend it returns `None` and the runner degrades to direct
/// execution, so dev/desktop behavior is unchanged there.
pub fn harness_sandbox_profile() -> SandboxProfile {
    let mut profile = SandboxProfile::hardened("harness");
    // Keep the workspace writable so file tools and git_commit keep working.
    profile.hardened_flags.read_only_fs = false;
    profile
}

/// Build the sandbox for [`harness_sandbox_profile`], or `None` when no backend
/// is available on this platform.
///
/// Availability is probed at construction: a hardened profile only hardens (and
/// therefore is only usable) when a real backend can apply it — bwrap on Linux,
/// Seatbelt (`sandbox-exec`) on macOS. When the backend cannot apply the profile
/// (e.g. an unavailable kernel call) we return `None` rather than claim to be
/// sandboxed. The `bwrap` executable must be installed on Linux for the returned
/// sandbox to actually confine a command; on macOS the `sandbox-exec` launcher
/// must be present, and confinement is applied to each spawned command — never the
/// harness/app process — so model calls stay unconfined. Callers can pass the
/// result straight to [`SandboxedCommandRunner::with_sandbox`].
pub fn harness_sandbox() -> Option<Box<dyn Sandbox>> {
    let sb = sandbox_for(harness_sandbox_profile());
    match sb.prepare(&goble_sandbox::PreparedCommand::new("echo")) {
        Ok(()) => Some(sb),
        Err(SandboxError::UnsupportedLevel { .. })
        | Err(SandboxError::BackendUnavailable { .. }) => None,
        Err(_) => Some(sb),
    }
}

#[async_trait::async_trait]
impl CommandRunner for SandboxedCommandRunner {
    async fn run(&self, command: &str, args: &[String]) -> Result<String> {
        if !self.allowed_commands.contains(command) {
            anyhow::bail!("command `{command}` is not in the allowed list");
        }
        let prepared = PreparedCommand::new(command)
            .args(args.iter().cloned())
            .cwd(self.working_dir.clone());
        let output = tokio::time::timeout(
            Duration::from_secs(self.timeout_seconds),
            self.run_prepared(&prepared),
        )
        .await
        .context("command timed out")?
        .context("failed to execute command")?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            anyhow::bail!("command failed: {stderr}");
        }
        Ok(format!("{stdout}{stderr}").trim().to_string())
    }
}

/// A pane's own shell, as the harness reaches it.
///
/// For an agent + terminal pane the agent's shell tool runs in the pane's own
/// session instead of the sandbox (`agent-terminal-bridge.md` §5). That is a
/// deliberate change of security posture: the command runs in the user's own
/// shell, with the user's environment and credentials, rather than under
/// [`SandboxedCommandRunner`]'s allow-list and 60 s timeout. The gates are the
/// choice of pane and the per-command approval, not the sandbox.
///
/// The implementation is the app's terminal pane: it writes the line to the
/// pty, holds the claim and hands back the block's output. The harness sees
/// only this trait, so it never depends on the pane's threading.
#[async_trait::async_trait]
pub trait PaneSession: Send + Sync {
    /// Run one command line in the pane's shell and return what it printed.
    ///
    /// `Err` when the pane could not run it or the command failed, so the tool
    /// call fails rather than hangs. The line is already composed (see
    /// [`shell_line`]); a command that is still running after a bounded wait is
    /// a failure, not a hang.
    async fn run_in_pane(&self, line: &str) -> Result<String>;
}

/// The shell tool's route for one agent turn: the pane's session when the pane
/// has a terminal, the sandbox when it does not.
///
/// This is the switch that makes agent + terminal mode real. It is installed as
/// the harness's one [`CommandRunner`], so every command tool (`run_command`
/// and the `git_*` tools) takes the same route; a pane with no terminal keeps
/// [`SandboxedCommandRunner`] exactly as before.
pub struct RoutedCommandRunner {
    /// The pane's own shell; `None` for a pane with no terminal.
    pane: Option<Arc<dyn PaneSession>>,
    sandbox: Arc<dyn CommandRunner>,
}

impl RoutedCommandRunner {
    /// Route to the sandbox until a pane session is attached.
    pub fn new(sandbox: Arc<dyn CommandRunner>) -> Self {
        Self {
            pane: None,
            sandbox,
        }
    }

    /// Route through `pane` when the turn's pane has a terminal.
    pub fn with_pane_session(mut self, pane: Option<Arc<dyn PaneSession>>) -> Self {
        self.pane = pane;
        self
    }

    /// Whether commands run in a pane's session rather than the sandbox.
    pub fn runs_in_pane(&self) -> bool {
        self.pane.is_some()
    }
}

#[async_trait::async_trait]
impl CommandRunner for RoutedCommandRunner {
    async fn run(&self, command: &str, args: &[String]) -> Result<String> {
        match &self.pane {
            Some(pane) => {
                let line =
                    shell_line(command, args).context("the agent asked for an empty command")?;
                pane.run_in_pane(&line).await
            }
            None => self.sandbox.run(command, args).await,
        }
    }
}

/// Compose the command line the pane's shell will run, or `None` for an empty
/// command.
///
/// The sandbox takes an argv; a shell takes one line. An argument containing a
/// character the shell would interpret is single-quoted, so it reaches the
/// command as the one word the model asked for.
pub(super) fn shell_line(command: &str, args: &[String]) -> Option<String> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    let mut line = command.to_string();
    for arg in args {
        line.push(' ');
        line.push_str(&quote_arg(arg));
    }
    Some(line)
}

/// Single-quote `arg` when the shell would otherwise change it.
fn quote_arg(arg: &str) -> String {
    let needs_quoting = arg.is_empty()
        || arg
            .chars()
            .any(|c| c.is_whitespace() || "'\"\\$`;&|<>()*?[]{}!~#".contains(c));
    if !needs_quoting {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}
