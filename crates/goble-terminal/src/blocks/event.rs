use super::id::BlockId;
use super::tool::ToolResult;
use crate::screen::ScreenLine;

/// One visible row, tagged with the block it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockLine {
    pub block: BlockId,
    pub line: ScreenLine,
}

/// Something the block list did, for the caller to react to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockEvent {
    SessionInitialized {
        session_id: Option<String>,
        shell: Option<String>,
    },
    Bootstrapped,
    BlockStarted {
        id: BlockId,
    },
    BlockFinished {
        id: BlockId,
        exit_code: i32,
        failed: bool,
    },
    MetadataUpdated {
        id: BlockId,
    },
    InputBufferChanged {
        buffer: String,
        cursor: Option<usize>,
    },
    /// Blocks were dropped by `clear`.
    Cleared {
        removed: usize,
    },
    /// A claimed block reached a terminal state: its output and exit code are
    /// the tool result for the call that asked for the command.
    ToolResult(ToolResult),
}
