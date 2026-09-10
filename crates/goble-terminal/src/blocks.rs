//! The command-block model.
//!
//! A block is one command and everything it printed, kept as one unit so the
//! renderer can draw a command and its output together and never lets output
//! from one command bleed into another.
//!
//! Blocks are driven by shell-integration hooks (see [`crate::hooks`]), not by
//! the VT stream: a prompt inside a command's output does not start a block, and
//! a command that prints nothing at all still ends one. The lifecycle is
//!
//! ```text
//! BeforeExecution --Preexec--> Executing --CommandFinished--> Done...*
//! ```
//!
//! Every finished block is frozen ([`Screen::freeze`]) so that anything the
//! shell writes afterwards — the next prompt, a background job's output — stays
//! out of it. The block that follows is created by `CommandFinished` itself, the
//! same way the shell's own `PROMPT_COMMAND` runs before the next prompt.
//!
//! A block draws its screens' whole grids (`Screen::content_lines`), not their
//! viewports, so output taller than the window is kept rather than scrolled
//! away. The block list lays those rows out; it is not itself the scrollback.

use std::fmt;
use std::time::{Duration, Instant};

use crate::hooks::{HookEvent, InitShellValue, PrecmdValue, PreexecValue};
use crate::screen::{Screen, ScreenConfig, ScreenLine, ScreenSize};

/// Identifies a block for as long as it is on screen, so the renderer can key
/// its layout on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u64);

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "b{}", self.0)
    }
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

/// One command and its output.
#[derive(Debug)]
pub struct Block {
    id: BlockId,
    index: usize,
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
    fn new(id: BlockId, index: usize, size: ScreenSize, state: BlockState) -> Self {
        Self {
            id,
            index,
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

    pub fn id(&self) -> BlockId {
        self.id
    }

    pub fn index(&self) -> usize {
        self.index
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

    /// Bytes discarded because the block was already finished.
    pub fn dropped_bytes(&self) -> usize {
        self.dropped_bytes
    }

    /// Route PTY bytes to the screen that currently owns them.
    pub fn feed(&mut self, bytes: &[u8]) {
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

/// One visible row, tagged with the block it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockLine {
    pub block: BlockId,
    pub line: ScreenLine,
}

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

/// Something the block list did, for the renderer to react to.
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
        }
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

        // A prompt while a command is still running means the shell moved on:
        // the command keeps its own block, and this is a new one.
        if matches!(self.active_block().state(), BlockState::Executing) {
            self.active_block_mut().promote_to_background();
            self.push_block();
        }

        let active = self.active_block_mut();
        active.set_metadata(&value);
        vec![BlockEvent::MetadataUpdated { id: active.id() }]
    }

    fn on_preexec(&mut self, value: PreexecValue) -> Vec<BlockEvent> {
        let active = self.active_block_mut();
        active.preexec(value.command.unwrap_or_default());
        vec![BlockEvent::BlockStarted { id: active.id() }]
    }

    fn on_command_finished(&mut self, exit_code: i32) -> Vec<BlockEvent> {
        let active = self.active_block_mut();
        active.finish(exit_code);
        let event = BlockEvent::BlockFinished {
            id: active.id(),
            exit_code,
            failed: active.has_failed(),
        };
        // The shell's `PROMPT_COMMAND` has already run, so the next block starts
        // here rather than at the next prompt.
        self.push_block();
        vec![event]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::{BootstrappedValue, CommandFinishedValue, InputBufferValue};

    fn size() -> ScreenSize {
        ScreenSize::new(40, 6)
    }

    fn init_shell() -> HookEvent {
        HookEvent::InitShell(InitShellValue {
            session_id: Some("s1".into()),
            shell: Some("zsh".into()),
            host: Some("mac".into()),
            cwd: Some("/work".into()),
            honor_ps1: Some(false),
        })
    }

    fn precmd(cwd: &str) -> HookEvent {
        HookEvent::Precmd(PrecmdValue {
            pwd: Some(cwd.into()),
            git_branch: Some("main".into()),
            rprompt: None,
            session_id: Some("s1".into()),
            virtual_env: None,
            conda_env: None,
            node_version: None,
            honor_ps1: Some(false),
        })
    }

    fn preexec(command: &str) -> HookEvent {
        HookEvent::Preexec(PreexecValue {
            command: Some(command.into()),
        })
    }

    fn finished(exit_code: i32) -> HookEvent {
        HookEvent::CommandFinished(CommandFinishedValue {
            exit_code,
            next_block_id: None,
        })
    }

    /// A list that has bootstrapped and drawn its first prompt.
    fn booted() -> BlockList {
        let mut list = BlockList::new(size());
        list.apply(init_shell());
        list.apply(HookEvent::Bootstrapped(BootstrappedValue {
            version: Some("1".into()),
        }));
        list.apply(precmd("/work"));
        list
    }

    #[test]
    fn starts_with_a_static_preamble() {
        let mut list = BlockList::new(size());
        assert_eq!(list.len(), 1);
        assert_eq!(list.active_block().state(), BlockState::Static);
        list.feed(b"Last login: today\n");
        assert_eq!(list.active_block().text(), "Last login: today");
        assert_eq!(list.active_block().command_text(), "");
    }

    #[test]
    fn bootstrapping_starts_the_first_command_block() {
        let mut list = BlockList::new(size());
        list.feed(b"banner\n");
        list.apply(init_shell());
        let events = list.apply(HookEvent::Bootstrapped(BootstrappedValue::default()));
        assert_eq!(events, vec![BlockEvent::Bootstrapped]);
        assert!(list.bootstrapped());
        assert_eq!(list.len(), 2);
        assert_eq!(list.active_block().state(), BlockState::BeforeExecution);
        // The banner stayed behind in the preamble.
        assert_eq!(list.text(), "banner");
        // Bootstrapping twice does not add a block.
        assert!(list
            .apply(HookEvent::Bootstrapped(BootstrappedValue::default()))
            .is_empty());
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn runs_a_command_from_prompt_to_exit_code() {
        let mut list = booted();
        assert_eq!(list.session().cwd.as_deref(), Some("/work"));

        // The shell's line editor echoes what is typed, then the command runs.
        list.feed(b"$ ls -la\r\n");
        let events = list.apply(preexec("ls -la"));
        let id = list.active_block().id();
        assert_eq!(events, vec![BlockEvent::BlockStarted { id }]);
        assert_eq!(list.active_block().state(), BlockState::Executing);
        assert_eq!(list.active_block().command_text(), "ls -la");

        list.feed(b"total 0\r\nfile.txt\r\n");
        let events = list.apply(finished(0));

        assert_eq!(
            events,
            vec![BlockEvent::BlockFinished {
                id,
                exit_code: 0,
                failed: false
            }]
        );
        let block = list.get(id).expect("the finished block");
        assert_eq!(block.state(), BlockState::DoneWithExecution);
        assert_eq!(block.exit_code(), Some(0));
        assert!(!block.has_failed());
        assert_eq!(block.text(), "$ ls -la\ntotal 0\nfile.txt");
        assert!(block.duration().is_some());
        // A new block is already active, so the next prompt has somewhere to go.
        assert_eq!(list.active_block().state(), BlockState::BeforeExecution);
        assert_ne!(list.active_block().id(), id);
    }

    #[test]
    fn a_non_zero_exit_code_is_a_failure() {
        let mut list = booted();
        list.apply(preexec("false"));
        list.apply(finished(1));
        assert!(list.blocks()[1].has_failed());
        assert_eq!(list.blocks()[1].exit_code(), Some(1));
    }

    #[test]
    fn metadata_is_recorded_on_the_block() {
        let list = booted();
        let block = list.active_block();
        assert_eq!(block.metadata().pwd.as_deref(), Some("/work"));
        assert_eq!(block.metadata().git_branch.as_deref(), Some("main"));
        assert!(!block.honor_ps1());
    }

    #[test]
    fn output_before_a_command_belongs_to_the_command_line() {
        let mut list = booted();
        // The shell echoes the typed line before the command runs.
        list.feed(b"$ ls");
        assert_eq!(list.active_block().command().text(), "$ ls");
        assert_eq!(list.active_block().output().text(), "");

        list.apply(preexec("ls"));
        list.feed(b"file.txt");
        assert_eq!(list.active_block().command().text(), "$ ls");
        assert_eq!(list.active_block().output().text(), "file.txt");
    }

    #[test]
    fn a_finished_block_never_grows_again() {
        let mut list = booted();
        let id = list.active_block().id();
        list.apply(preexec("echo hi"));
        list.feed(b"hi\r\n");
        list.apply(finished(0));
        let before = list.get(id).unwrap().text();

        // The next prompt, and output from a background job, arrive afterwards.
        list.feed(b"next prompt");
        assert_eq!(list.get(id).unwrap().text(), before);
        assert_eq!(
            list.get(id).unwrap().dropped_bytes(),
            0,
            "routed to the new block"
        );
        assert_eq!(list.active_block().command().text(), "next prompt");
    }

    #[test]
    fn a_frozen_block_keeps_the_shell_out_of_it() {
        let mut list = booted();
        let id = list.active_block().id();
        list.apply(preexec("printf out"));
        list.feed(b"out");
        list.apply(finished(0));
        let block = list.get(id).unwrap();
        assert!(block.output().is_frozen());
        assert_eq!(block.output().text(), "out");
        assert_eq!(block.output_lines().len(), 1);
    }

    #[test]
    fn a_block_keeps_output_taller_than_the_screen() {
        let mut list = booted();
        let id = list.active_block().id();
        list.apply(preexec("seq 1 50"));
        let output: String = (1..=50).map(|line| format!("{line}\r\n")).collect();
        list.feed(output.as_bytes());
        list.apply(finished(0));

        let block = list.get(id).unwrap();
        assert_eq!(
            block.output_lines().len(),
            50,
            "every row the command printed"
        );
        assert_eq!(
            block.output().history_size(),
            45,
            "50 rows plus the newline, in a 6 line screen"
        );
        assert_eq!(block.output_lines()[0].text(), "1");
        assert_eq!(block.output_lines()[49].text(), "50");
    }

    #[test]
    fn a_prompt_during_execution_promotes_the_command_to_background() {
        let mut list = booted();
        let running = list.active_block().id();
        list.apply(preexec("sleep 100 &"));

        let events = list.apply(precmd("/work"));
        let promoted = list.get(running).unwrap();
        assert_eq!(promoted.state(), BlockState::Background);
        assert!(promoted.exit_code().is_none());
        assert_eq!(events.len(), 1);
        assert_ne!(list.active_block().id(), running);
        assert_eq!(list.active_block().state(), BlockState::BeforeExecution);
    }

    #[test]
    fn an_empty_prompt_is_done_with_no_execution() {
        let mut list = booted();
        list.apply(preexec(""));
        list.apply(finished(0));
        assert_eq!(list.blocks()[1].state(), BlockState::DoneWithNoExecution);
        // Output without a command is still an execution.
        let mut list = booted();
        list.apply(preexec(""));
        list.feed(b"something");
        list.apply(finished(0));
        assert_eq!(list.blocks()[1].state(), BlockState::DoneWithExecution);
    }

    #[test]
    fn clear_drops_earlier_blocks_and_marks_a_gap() {
        let mut list = booted();
        for index in 0..3 {
            list.apply(preexec(&format!("echo {index}")));
            list.feed(format!("{index}\r\n").as_bytes());
            list.apply(finished(0));
        }
        assert_eq!(list.len(), 5);

        list.apply(preexec("clear"));
        list.feed(b"stale output");
        let events = list.apply(HookEvent::Clear);
        assert_eq!(events, vec![BlockEvent::Cleared { removed: 4 }]);
        assert_eq!(list.len(), 1);
        assert_eq!(list.active_index(), 0);
        // The `clear` command itself stays visible; what it printed does not.
        assert_eq!(list.active_block().command_text(), "clear");
        assert_eq!(list.active_block().output().text(), "");
        assert_eq!(list.gaps(), &[list.active_block().id()]);

        // The block keeps running and can still finish normally.
        list.apply(finished(0));
        assert_eq!(list.blocks()[0].state(), BlockState::DoneWithExecution);
    }

    #[test]
    fn input_buffer_is_reported() {
        let mut list = booted();
        let events = list.apply(HookEvent::InputBuffer(InputBufferValue {
            buffer: "git st".into(),
            cursor: Some(6),
        }));
        assert_eq!(
            events,
            vec![BlockEvent::InputBufferChanged {
                buffer: "git st".into(),
                cursor: Some(6)
            }]
        );
        assert_eq!(list.input_buffer(), "git st");
        assert_eq!(list.input_cursor(), Some(6));
    }

    #[test]
    fn resize_reaches_every_block() {
        let mut list = booted();
        list.apply(preexec("echo wrap"));
        list.feed(b"a rather long line that has to wrap once the screen is narrow");
        list.apply(finished(0));
        list.resize(ScreenSize::new(20, 4));
        for block in list.blocks() {
            assert_eq!(block.command().size(), ScreenSize::new(20, 4));
            assert_eq!(block.output().size(), ScreenSize::new(20, 4));
        }
        let block = list.get(list.blocks()[1].id()).unwrap();
        assert!(
            block.text().contains("wrap"),
            "text after reflow: {:?}",
            block.text()
        );
    }

    #[test]
    fn lines_are_tagged_with_their_block() {
        let mut list = booted();
        list.feed(b"$ echo one\r\n");
        list.apply(preexec("echo one"));
        list.feed(b"one\r\n");
        list.apply(finished(0));
        let lines = list.lines();
        assert_eq!(lines.len(), list.total_lines());
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line.text(), "$ echo one");
        assert_eq!(lines[0].block, list.blocks()[1].id());
        assert_eq!(lines[1].line.text(), "one");
        assert_eq!(list.text(), "$ echo one\none");
    }

    #[test]
    fn unknown_hook_orders_do_not_panic() {
        // Hooks can arrive out of order after a shell reload; nothing here may
        // panic or invent a block.
        let mut list = BlockList::new(size());
        list.apply(finished(0));
        list.apply(preexec("orphan"));
        list.apply(HookEvent::Clear);
        list.apply(precmd("/"));
        list.feed(b"still alive");
        assert!(!list.is_empty());
        assert_eq!(list.active_index(), list.len() - 1);
    }

    #[test]
    fn ids_are_unique_and_ordered() {
        let mut list = booted();
        let mut ids: Vec<BlockId> = list.blocks().iter().map(Block::id).collect();
        for _ in 0..3 {
            list.apply(preexec("true"));
            list.apply(finished(0));
            ids.push(list.active_block().id());
        }
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "ids repeat: {ids:?}");
    }
}
