/// Who a block's command was run for.
///
/// The block is the same either way; the owner is what lets a conversation show
/// the commands it ran and leave the user's own out of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum BlockOwner {
    /// Typed by the user, or output with no command behind it.
    #[default]
    User,
    /// Run by the agent as part of a turn. `call_id` is the harness's tool-call
    /// id, so the block and its tool result are the same object.
    Agent {
        conversation_id: String,
        call_id: String,
    },
}

/// Where a block is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockState {
    /// A prompt is on screen and the command is being typed.
    BeforeExecution,
    /// A command is running.
    Executing,
    /// The command ran and finished.
    DoneWithExecution,
    /// The prompt was submitted empty; nothing ran.
    DoneWithNoExecution,
    /// The shell moved on while this command was still running, so no exit code
    /// is coming.
    Background,
    /// Output with no command behind it: shell startup, a message from the
    /// session, or anything written while no block owned the screen.
    Static,
}

impl BlockState {
    /// The block will not change again.
    pub fn is_done(self) -> bool {
        matches!(
            self,
            BlockState::DoneWithExecution | BlockState::DoneWithNoExecution
        )
    }

    /// Bytes from the PTY still belong to this block.
    pub fn accepts_output(self) -> bool {
        matches!(
            self,
            BlockState::BeforeExecution
                | BlockState::Executing
                | BlockState::Background
                | BlockState::Static
        )
    }
}

/// What a block holds.
///
/// A shell block is a command and the output it produced; the block list draws
/// it from its own two screens. An agent-view block holds no output at all: it
/// marks where a conversation happened, names the conversation, and carries the
/// label the renderer draws on the card. The conversation itself is rendered by
/// the app, not by the emulator, so nothing is ever fed to this block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    Shell,
    AgentView {
        conversation_id: String,
        /// What the card says when the conversation itself is not loaded (a
        /// restored list, a conversation that lives on another machine).
        label: String,
    },
}

impl BlockKind {
    pub fn is_shell(&self) -> bool {
        matches!(self, BlockKind::Shell)
    }
}

/// One of the views over a block list. Picking a view is the whole of "which
/// history am I looking at".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockView {
    Terminal,
    Agent { conversation_id: String },
}
