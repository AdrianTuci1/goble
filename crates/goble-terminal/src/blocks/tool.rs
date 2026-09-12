use super::id::BlockId;

/// How a claimed command ended.
///
/// A block the agent owns reaches a terminal state in exactly one of two ways:
/// the shell reports its exit code, or the shell moves on while it still runs.
/// The tool result carries which one happened, so the caller never has to wait
/// for a `CommandFinished` that is not coming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolOutcome {
    /// The command finished and the shell reported this exit code. A non-zero
    /// code is a failed command; `130` is one interrupted with Ctrl-C.
    Finished { exit_code: i32 },
    /// The shell drew its next prompt while the command still ran, so no exit
    /// code is coming. The result carries the output that had arrived; it is
    /// not a hang, and it is not a failure.
    StillRunning,
}

/// A claimed block's result: what the command printed and how it ended.
///
/// This is the tool result for the call that claimed the block — the same block
/// the agent view renders, read as output text plus an exit code rather than as
/// a second copy of the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    /// The block the result came from.
    pub block: BlockId,
    /// The conversation that asked for the command.
    pub conversation_id: String,
    /// The harness tool call this block is the result of.
    pub call_id: String,
    /// The command line the shell ran.
    pub command: String,
    /// Everything the command printed.
    pub output: String,
    pub outcome: ToolOutcome,
}

impl ToolResult {
    /// The command's exit code, when it has one.
    pub fn exit_code(&self) -> Option<i32> {
        match self.outcome {
            ToolOutcome::Finished { exit_code } => Some(exit_code),
            ToolOutcome::StillRunning => None,
        }
    }

    /// The command finished with a non-zero exit code. A command that is still
    /// running has not failed.
    pub fn is_failure(&self) -> bool {
        self.exit_code().is_some_and(|code| code != 0)
    }

    /// No exit code is coming: the shell moved on while the command ran.
    pub fn is_still_running(&self) -> bool {
        self.outcome == ToolOutcome::StillRunning
    }
}
