use std::time::{Duration, Instant};

use super::id::BlockId;
use super::state::{BlockKind, BlockOwner, BlockState};
use super::visibility::BlockVisibility;
use crate::hooks::PrecmdValue;
use crate::screen::{Screen, ScreenConfig, ScreenLine, ScreenSize};

/// What the shell told us about where a command was run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockMetadata {
    pub pwd: Option<String>,
    pub git_branch: Option<String>,
    pub rprompt: Option<String>,
    pub session_id: Option<String>,
    pub virtual_env: Option<String>,
    pub conda_env: Option<String>,
    pub node_version: Option<String>,
}

/// One command and its output, or one conversation.
#[derive(Debug)]
pub struct Block {
    /// Matched against a claim by the list.
    pub(super) id: BlockId,
    /// Renumbered by the list when `clear` drops earlier blocks.
    pub(super) index: usize,
    kind: BlockKind,
    /// Read and replaced by the list.
    pub(super) visibility: BlockVisibility,
    owner: BlockOwner,
    state: BlockState,
    /// The prompt and the typed command.
    command: Screen,
    /// Everything the command printed.
    output: Screen,
    command_text: String,
    exit_code: Option<i32>,
    metadata: BlockMetadata,
    honor_ps1: bool,
    started_at: Option<Instant>,
    finished_at: Option<Instant>,
    /// Bytes that arrived after the block was finished and were discarded.
    dropped_bytes: usize,
}

impl Block {
    pub(super) fn new(id: BlockId, index: usize, size: ScreenSize, state: BlockState) -> Self {
        Self {
            id,
            index,
            kind: BlockKind::Shell,
            visibility: BlockVisibility::terminal(),
            owner: BlockOwner::User,
            state,
            command: Screen::new(size, ScreenConfig::block()),
            output: Screen::new(size, ScreenConfig::block()),
            command_text: String::new(),
            exit_code: None,
            metadata: BlockMetadata::default(),
            honor_ps1: false,
            started_at: None,
            finished_at: None,
            dropped_bytes: 0,
        }
    }

    /// A block standing for a conversation. It owns no screens: the app renders
    /// the conversation, and this block is what reserves its place in the list
    /// and what the terminal shows once the agent view is left.
    pub(super) fn agent_view(
        id: BlockId,
        index: usize,
        size: ScreenSize,
        conversation_id: String,
        label: String,
    ) -> Self {
        Self {
            kind: BlockKind::AgentView {
                conversation_id,
                label,
            },
            // Visible in the terminal so the card can be clicked to come back.
            // `visible_blocks` hides it again while its own agent view is open.
            visibility: BlockVisibility::terminal(),
            ..Self::new(id, index, size, BlockState::Static)
        }
    }

    pub fn id(&self) -> BlockId {
        self.id
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn kind(&self) -> &BlockKind {
        &self.kind
    }

    pub fn visibility(&self) -> &BlockVisibility {
        &self.visibility
    }

    /// Who ran this block's command.
    pub fn owner(&self) -> &BlockOwner {
        &self.owner
    }

    pub(super) fn set_owner(&mut self, owner: BlockOwner) {
        self.owner = owner;
    }

    /// The conversation this block stands for, if it is an agent-view block.
    pub fn conversation_id(&self) -> Option<&str> {
        match &self.kind {
            BlockKind::Shell => None,
            BlockKind::AgentView {
                conversation_id, ..
            } => Some(conversation_id),
        }
    }

    /// What the card says, if this block is an agent-view block.
    pub fn label(&self) -> Option<&str> {
        match &self.kind {
            BlockKind::Shell => None,
            BlockKind::AgentView { label, .. } => Some(label),
        }
    }

    pub fn state(&self) -> BlockState {
        self.state
    }

    pub fn command(&self) -> &Screen {
        &self.command
    }

    pub fn output(&self) -> &Screen {
        &self.output
    }

    pub fn command_mut(&mut self) -> &mut Screen {
        &mut self.command
    }

    pub fn output_mut(&mut self) -> &mut Screen {
        &mut self.output
    }

    /// The command as the shell reported it, without the prompt.
    pub fn command_text(&self) -> &str {
        &self.command_text
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Only an executed command can have failed; an unfinished or empty block is
    /// not a failure.
    pub fn has_failed(&self) -> bool {
        self.exit_code.is_some_and(|code| code != 0)
    }

    pub fn metadata(&self) -> &BlockMetadata {
        &self.metadata
    }

    /// Whether the shell draws its own prompt. Off means we hide it and draw our
    /// own header from [`Block::metadata`].
    pub fn honor_ps1(&self) -> bool {
        self.honor_ps1
    }

    pub fn is_done(&self) -> bool {
        self.state.is_done()
    }

    pub fn duration(&self) -> Option<Duration> {
        Some(self.finished_at?.duration_since(self.started_at?))
    }

    /// How long the command has been running so far. `None` before it starts;
    /// a finished command reads [`Block::duration`] instead, since this keeps
    /// growing after the command is done.
    pub fn elapsed(&self) -> Option<Duration> {
        (self.state == BlockState::Executing).then(|| self.started_at.map(|start| start.elapsed()))?
    }

    /// Bytes discarded because the block was already finished.
    pub fn dropped_bytes(&self) -> usize {
        self.dropped_bytes
    }

    /// Route PTY bytes to the screen that currently owns them.
    pub fn feed(&mut self, bytes: &[u8]) {
        // A conversation block renders from the app's own conversation model, so
        // anything the PTY writes has no screen to land on. It is never the
        // active block; counting the bytes keeps that visible if it ever is.
        if !self.kind.is_shell() {
            self.dropped_bytes += bytes.len();
            return;
        }
        match self.state {
            // Before a command runs the shell is drawing the prompt and echoing
            // what is typed, so those bytes are the command line.
            BlockState::BeforeExecution => self.command.feed(bytes),
            BlockState::Executing | BlockState::Background | BlockState::Static => {
                self.output.feed(bytes)
            }
            BlockState::DoneWithExecution | BlockState::DoneWithNoExecution => {
                self.dropped_bytes += bytes.len();
            }
        }
    }

    /// The shell is about to run this block's command.
    pub fn preexec(&mut self, command: String) {
        if matches!(self.state, BlockState::Executing) {
            return;
        }
        self.command_text = command;
        self.state = BlockState::Executing;
        self.started_at = Some(Instant::now());
    }

    /// The shell moved on while this command was still running: it will never
    /// report an exit code for it.
    pub fn promote_to_background(&mut self) {
        if matches!(self.state, BlockState::Executing) {
            self.state = BlockState::Background;
        }
    }

    /// The command finished. Both screens freeze, so nothing the shell writes
    /// from here on can join this block.
    pub fn finish(&mut self, exit_code: i32) {
        self.exit_code = Some(exit_code);
        self.finished_at = Some(Instant::now());
        self.state = if self.command_text.trim().is_empty() && !self.has_output() {
            BlockState::DoneWithNoExecution
        } else {
            BlockState::DoneWithExecution
        };
        self.freeze();
    }

    pub fn freeze(&mut self) {
        self.command.freeze();
        self.output.freeze();
    }

    fn has_output(&self) -> bool {
        !self.output.text().is_empty()
    }

    /// Drop what the command printed, keeping the command line itself. This is
    /// what the `clear` builtin resolves to for the block it runs in.
    pub fn clear(&mut self) {
        self.output.reset();
    }

    pub fn resize(&mut self, size: ScreenSize) {
        self.command.resize(size);
        self.output.resize(size);
    }

    pub fn set_metadata(&mut self, value: &PrecmdValue) {
        self.metadata = BlockMetadata {
            pwd: value.pwd.clone(),
            git_branch: value.git_branch.clone(),
            rprompt: value.rprompt.clone(),
            session_id: value.session_id.clone(),
            virtual_env: value.virtual_env.clone(),
            conda_env: value.conda_env.clone(),
            node_version: value.node_version.clone(),
        };
        if let Some(honor) = value.honor_ps1 {
            self.honor_ps1 = honor;
        }
    }

    /// The prompt and command rows, without blank padding.
    pub fn command_lines(&self) -> Vec<ScreenLine> {
        trimmed(self.command.content_lines())
    }

    /// The output rows, without blank padding.
    pub fn output_lines(&self) -> Vec<ScreenLine> {
        trimmed(self.output.content_lines())
    }

    /// Everything the block shows: command first, then output.
    pub fn lines(&self) -> Vec<ScreenLine> {
        let mut lines = self.command_lines();
        lines.extend(self.output_lines());
        lines
    }

    /// Rows the block will show.
    pub fn visible_lines(&self) -> usize {
        self.lines().len()
    }

    /// What the command printed, as text.
    ///
    /// This is the output screen's whole content, not only the rows visible in
    /// it, so a result handed to the agent keeps output that scrolled past the
    /// screen. It is deliberately not [`Block::text`], which also carries the
    /// command line the shell echoed.
    pub fn output_text(&self) -> String {
        self.output_lines()
            .iter()
            .map(ScreenLine::text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn text(&self) -> String {
        let mut lines: Vec<String> = self.lines().iter().map(ScreenLine::text).collect();
        while lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }
}

fn trimmed(lines: Vec<ScreenLine>) -> Vec<ScreenLine> {
    let mut lines = lines;
    while lines.last().is_some_and(ScreenLine::is_blank) {
        lines.pop();
    }
    while lines.first().is_some_and(ScreenLine::is_blank) {
        lines.remove(0);
    }
    lines
}
