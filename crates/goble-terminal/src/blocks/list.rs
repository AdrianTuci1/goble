use std::collections::VecDeque;

use super::block::Block;
use super::event::{BlockEvent, BlockLine};
use super::id::BlockId;
use super::state::{BlockOwner, BlockState, BlockView};
use super::tool::{ToolOutcome, ToolResult};
use super::visibility::BlockVisibility;
use crate::hooks::{HookEvent, InitShellValue, PrecmdValue, PreexecValue};
use crate::screen::ScreenSize;

/// Facts about the session the blocks belong to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionInfo {
    pub session_id: Option<String>,
    pub shell: Option<String>,
    pub host: Option<String>,
    pub cwd: Option<String>,
    /// Whether the shell draws its own prompt.
    pub honor_ps1: bool,
}

/// A command the app asked for, waiting for its `Preexec` to say which block it
/// runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingClaim {
    /// The block the command is expected to run in.
    pub block: BlockId,
    pub owner: BlockOwner,
}

/// The blocks of one session, oldest first, with exactly one of them active.
#[derive(Debug)]
pub struct BlockList {
    blocks: Vec<Block>,
    active: usize,
    size: ScreenSize,
    next_id: u64,
    session: SessionInfo,
    bootstrapped: bool,
    input_buffer: String,
    input_cursor: Option<usize>,
    /// Blocks that begin a fresh screen after a `clear`.
    gaps: Vec<BlockId>,
    /// A command the app claimed, resolved or refused by the next `Preexec`.
    pending_claim: Option<PendingClaim>,
    /// Conversations waiting for the next commands' `Preexec`, in the order
    /// the lines were written. The block a command runs in is only known when
    /// it starts — a fresh shell finishes its startup block first — so the
    /// visibility is applied at `Preexec`, not when the line is written.
    pending_conversations: VecDeque<String>,
}

impl BlockList {
    /// Start a list with one static block: whatever the shell prints before it
    /// is ready (the login banner, mostly) lands there.
    pub fn new(size: ScreenSize) -> Self {
        let preamble = Block::new(BlockId(1), 0, size, BlockState::Static);
        Self {
            blocks: vec![preamble],
            active: 0,
            size,
            next_id: 2,
            session: SessionInfo::default(),
            bootstrapped: false,
            input_buffer: String::new(),
            input_cursor: None,
            gaps: Vec::new(),
            pending_claim: None,
            pending_conversations: VecDeque::new(),
        }
    }

    /// File the block the next command starts in under `conversation_id`'s
    /// agent view. Applied at the command's `Preexec`, when the block it runs in
    /// is known.
    pub fn hold_next_command_for(&mut self, conversation_id: impl Into<String>) {
        self.pending_conversations.push_back(conversation_id.into());
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    /// A list always holds at least the active block.
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active_block(&self) -> &Block {
        &self.blocks[self.active]
    }

    pub fn active_block_mut(&mut self) -> &mut Block {
        let active = self.active;
        &mut self.blocks[active]
    }

    pub fn get(&self, id: BlockId) -> Option<&Block> {
        self.blocks.iter().find(|block| block.id == id)
    }

    pub fn session(&self) -> &SessionInfo {
        &self.session
    }

    pub fn bootstrapped(&self) -> bool {
        self.bootstrapped
    }

    /// The shell's current input buffer, as its line editor reports it.
    pub fn input_buffer(&self) -> &str {
        &self.input_buffer
    }

    pub fn input_cursor(&self) -> Option<usize> {
        self.input_cursor
    }

    /// Blocks a `clear` started a new screen with.
    pub fn gaps(&self) -> &[BlockId] {
        &self.gaps
    }

    /// Bytes the shell wrote after their block had already finished.
    pub fn dropped_bytes(&self) -> usize {
        self.blocks.iter().map(Block::dropped_bytes).sum()
    }

    /// The claim waiting for its `Preexec`, if any.
    pub fn pending_claim(&self) -> Option<&PendingClaim> {
        self.pending_claim.as_ref()
    }

    /// Claim the next `Preexec` for a command the agent asked for.
    ///
    /// The claim is aimed at `id`, the block the command is expected to run in:
    /// the active block, still waiting for a command. A claim aimed at any other
    /// block — a finished one, a conversation card, an id that does not exist —
    /// is refused rather than attached to a block it does not own. A later
    /// `Preexec` that lands on another block refuses the claim too.
    pub fn claim_next_pre_exec(&mut self, id: BlockId, owner: BlockOwner) -> bool {
        let active = self.active_block();
        if active.id() != id || active.state() != BlockState::BeforeExecution {
            return false;
        }
        self.pending_claim = Some(PendingClaim { block: id, owner });
        true
    }

    pub fn size(&self) -> ScreenSize {
        self.size
    }

    pub fn resize(&mut self, size: ScreenSize) {
        self.size = size;
        for block in &mut self.blocks {
            block.resize(size);
        }
    }

    /// Feed PTY bytes to the active block.
    pub fn feed(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.active_block_mut().feed(bytes);
    }

    /// Apply a shell-integration hook, returning what changed.
    pub fn apply(&mut self, event: HookEvent) -> Vec<BlockEvent> {
        match event {
            HookEvent::InitShell(value) => self.init_shell(value),
            HookEvent::Bootstrapped(_) => self.on_bootstrapped(),
            HookEvent::Precmd(value) => self.on_precmd(value),
            HookEvent::Preexec(value) => self.on_preexec(value),
            HookEvent::CommandFinished(value) => self.on_command_finished(value.exit_code),
            HookEvent::InputBuffer(value) => {
                self.input_buffer = value.buffer;
                self.input_cursor = value.cursor;
                vec![BlockEvent::InputBufferChanged {
                    buffer: self.input_buffer.clone(),
                    cursor: self.input_cursor,
                }]
            }
            HookEvent::Clear => self.on_clear(),
        }
    }

    fn init_shell(&mut self, value: InitShellValue) -> Vec<BlockEvent> {
        self.session.session_id = value.session_id;
        self.session.shell = value.shell;
        self.session.host = value.host;
        if value.cwd.is_some() {
            self.session.cwd = value.cwd;
        }
        if let Some(honor) = value.honor_ps1 {
            self.session.honor_ps1 = honor;
        }
        vec![BlockEvent::SessionInitialized {
            session_id: self.session.session_id.clone(),
            shell: self.session.shell.clone(),
        }]
    }

    fn on_bootstrapped(&mut self) -> Vec<BlockEvent> {
        if self.bootstrapped {
            return Vec::new();
        }
        self.bootstrapped = true;
        self.push_block();
        vec![BlockEvent::Bootstrapped]
    }

    fn on_precmd(&mut self, value: PrecmdValue) -> Vec<BlockEvent> {
        if value.pwd.is_some() {
            self.session.cwd = value.pwd.clone();
        }

        let mut events = Vec::new();
        // A prompt while a command is still running means the shell moved on:
        // the command keeps its own block, and this is a new one.
        if matches!(self.active_block().state(), BlockState::Executing) {
            let promoted = self.active;
            self.active_block_mut().promote_to_background();
            // No `CommandFinished` is coming for the promoted block, so its tool
            // result is produced here — the output that arrived, and the fact
            // that it is still running — instead of leaving the tool call to
            // wait for a hook it will never see.
            if let Some(result) = self.tool_result(promoted, ToolOutcome::StillRunning) {
                events.push(BlockEvent::ToolResult(result));
            }
            self.push_block();
        }

        let active = self.active_block_mut();
        active.set_metadata(&value);
        events.push(BlockEvent::MetadataUpdated { id: active.id() });
        events
    }

    fn on_preexec(&mut self, value: PreexecValue) -> Vec<BlockEvent> {
        // A claim only attaches to the block it was aimed at. It is consumed
        // either way, so a `Preexec` that lands on another block refuses the
        // claim and that command stays the user's.
        let claim = self.pending_claim.take();
        // A conversation held for the next command takes the block the command
        // actually starts in, which is not known until here.
        let conversation = self.pending_conversations.pop_front();
        let id = {
            let active = self.active_block_mut();
            if let Some(claim) = claim {
                if claim.block == active.id() {
                    active.set_owner(claim.owner);
                }
            }
            active.preexec(value.command.unwrap_or_default());
            active.id()
        };
        if let Some(conversation) = conversation {
            if let Some(block) = self.blocks.iter_mut().find(|block| block.id == id) {
                block.visibility = BlockVisibility::agent(conversation);
            }
        }
        vec![BlockEvent::BlockStarted { id }]
    }

    fn on_command_finished(&mut self, exit_code: i32) -> Vec<BlockEvent> {
        let finished = self.active;
        let active = self.active_block_mut();
        active.finish(exit_code);
        let event = BlockEvent::BlockFinished {
            id: active.id(),
            exit_code,
            failed: active.has_failed(),
        };
        // A block the agent claimed is that tool call's result: its output and
        // exit code go back to the caller, not only to the renderer.
        let result = self.tool_result(finished, ToolOutcome::Finished { exit_code });
        // The shell's `PROMPT_COMMAND` has already run, so the next block starts
        // here rather than at the next prompt.
        self.push_block();
        let mut events = vec![event];
        events.extend(result.map(BlockEvent::ToolResult));
        events
    }

    /// The tool result for a block the agent owns, or `None` when the block is
    /// the user's (there is no tool call to answer).
    fn tool_result(&self, index: usize, outcome: ToolOutcome) -> Option<ToolResult> {
        let block = &self.blocks[index];
        let BlockOwner::Agent {
            conversation_id,
            call_id,
        } = block.owner()
        else {
            return None;
        };
        Some(ToolResult {
            block: block.id(),
            conversation_id: conversation_id.clone(),
            call_id: call_id.clone(),
            command: block.command_text().to_string(),
            output: block.output_text(),
            outcome,
        })
    }

    fn on_clear(&mut self) -> Vec<BlockEvent> {
        let removed = self.active;
        if removed > 0 {
            self.blocks.drain(..removed);
            self.active = 0;
            for (index, block) in self.blocks.iter_mut().enumerate() {
                block.index = index;
            }
        }
        let active = self.active_block_mut();
        active.clear();
        let id = active.id();
        if !self.gaps.contains(&id) {
            self.gaps.push(id);
        }
        vec![BlockEvent::Cleared { removed }]
    }

    fn push_block(&mut self) {
        let block = Block::new(
            BlockId(self.next_id),
            self.blocks.len(),
            self.size,
            BlockState::BeforeExecution,
        );
        self.next_id += 1;
        self.blocks.push(block);
        self.active = self.blocks.len() - 1;
    }

    /// Place a block standing for a conversation at the point the conversation
    /// happened, without disturbing the active shell block: the card sits above
    /// the prompt waiting for the next command, so the command's output lands
    /// below the card and the card keeps its slot in the history. A command
    /// already running takes the card after itself — there is no prompt to
    /// insert above.
    pub fn push_agent_view_block(
        &mut self,
        conversation_id: impl Into<String>,
        label: impl Into<String>,
    ) -> BlockId {
        let id = BlockId(self.next_id);
        self.next_id += 1;
        let at = if self.active_block().state() == BlockState::BeforeExecution {
            self.active
        } else {
            self.blocks.len()
        };
        let block = Block::agent_view(id, at, self.size, conversation_id.into(), label.into());
        self.blocks.insert(at, block);
        for (index, block) in self.blocks.iter_mut().enumerate().skip(at) {
            block.index = index;
        }
        if at <= self.active {
            self.active += 1;
        }
        id
    }

    /// Make a block visible inside a conversation's agent view as well. Returns
    /// false when the block is unknown.
    pub fn associate_with_conversation(&mut self, id: BlockId, conversation_id: &str) -> bool {
        let Some(block) = self.blocks.iter_mut().find(|block| block.id == id) else {
            return false;
        };
        block.visibility.associate(conversation_id)
    }

    /// Replace a block's visibility wholesale. Returns false when the block is
    /// unknown.
    pub fn set_visibility(&mut self, id: BlockId, visibility: BlockVisibility) -> bool {
        let Some(block) = self.blocks.iter_mut().find(|block| block.id == id) else {
            return false;
        };
        block.visibility = visibility;
        true
    }

    /// The blocks a view draws, oldest first.
    ///
    /// This is the whole of the terminal/agent split: a conversation's own card
    /// is hidden while that conversation is open, because the view already is
    /// the conversation.
    pub fn visible_blocks(&self, view: &BlockView) -> Vec<&Block> {
        self.blocks
            .iter()
            .filter(|block| {
                if let BlockView::Agent { conversation_id } = view {
                    if block.conversation_id() == Some(conversation_id.as_str()) {
                        return false;
                    }
                }
                block.visibility.is_visible_in(view)
            })
            .collect()
    }

    /// Every visible row, oldest block first.
    pub fn lines(&self) -> Vec<BlockLine> {
        self.blocks
            .iter()
            .flat_map(|block| {
                block.lines().into_iter().map(move |line| BlockLine {
                    block: block.id,
                    line,
                })
            })
            .collect()
    }

    /// Every row a view draws, oldest block first.
    pub fn visible_lines(&self, view: &BlockView) -> Vec<BlockLine> {
        self.visible_blocks(view)
            .into_iter()
            .flat_map(|block| {
                block.lines().into_iter().map(move |line| BlockLine {
                    block: block.id,
                    line,
                })
            })
            .collect()
    }

    pub fn total_lines(&self) -> usize {
        self.lines().len()
    }

    /// The whole session as text, blocks separated by a blank line.
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .map(Block::text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}
