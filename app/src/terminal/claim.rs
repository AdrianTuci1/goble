use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

use goble_core::harness::PaneSession;
use goble_terminal::{BlockOwner, ToolResult};

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
    pub(super) deadline: Instant,
}

/// Why a claim could not be placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimError {
    /// The command line is empty; there is nothing for the shell to run.
    EmptyCommand,
    /// A claim is already waiting for its `Preexec`. The shell has one line
    /// editor, so a second claim would leave the first one unattributed.
    AlreadyPending,
    /// The shell's integration has not bootstrapped, so it will never report a
    /// `Preexec` and the claim could only ever time out. Refused at once.
    NotBootstrapped,
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
pub(super) struct PaneCommandRequest {
    /// The composed command line (see `RoutedCommandRunner`).
    pub(super) line: String,
    /// The conversation that asked for it, for the block owner.
    pub(super) conversation_id: String,
    /// Identifies the claim so its tool result can be matched back.
    pub(super) call_id: String,
    /// Where the tool result is sent once the pane resolves the claim.
    pub(super) reply: oneshot::Sender<Result<String, String>>,
}

/// The rendezvous between the harness and one pane's session.
///
/// The claim protocol allows one in-flight command per shell, so there is a
/// single request slot: a second submission while one waits is refused rather
/// than queued behind it.
#[derive(Default)]
pub(super) struct PaneCommands {
    pub(super) request: Option<PaneCommandRequest>,
}

/// The harness's handle to a pane's own shell (the P4 route).
///
/// The harness runs on the daemon's runtime while the pane's session is pumped
/// on the UI thread, so the handle only submits the command and waits; the
/// session claims it and hands back the block list's tool result. That is the
/// correlation model of `agent-terminal-bridge.md` §3 carried across the thread
/// boundary the two live on.
pub(super) struct PaneCommandRunner {
    commands: Arc<Mutex<PaneCommands>>,
    /// The conversation this pane's turns belong to.
    conversation_id: String,
    /// Names each claim this runner makes, so results are matched, not guessed.
    next_call: AtomicU64,
}

impl PaneCommandRunner {
    /// A runner reaching the session that shares `commands`.
    pub(super) fn new(commands: Arc<Mutex<PaneCommands>>, conversation_id: impl Into<String>) -> Self {
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
pub(super) fn tool_result_text(result: &ToolResult) -> Result<String, String> {
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
pub(super) fn owner_call_id(owner: &BlockOwner) -> String {
    match owner {
        BlockOwner::Agent { call_id, .. } => call_id.clone(),
        BlockOwner::User => String::new(),
    }
}
