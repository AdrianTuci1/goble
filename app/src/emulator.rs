//! The VT state behind a terminal pane.
//!
//! One [`Emulator`] owns the screen (grid, cursor, modes, scrollback) and the
//! two byte taps that observe what the VT parser throws away: the
//! shell-integration hook channel and the OSC numbers (`7` working directory,
//! `133` prompt markers). A byte read from the pty goes through both taps and
//! then into the parser, so only our own envelopes are lifted out and every
//! other byte reaches the screen untouched.
//!
//! There is no I/O here. The reader thread holds the lock while it feeds and
//! the UI thread takes it for the time it needs to clone the visible rows, so
//! neither ever waits on the other for long. Answers the screen asks for (a
//! colour report, the text-area size) are queued as bytes for the session to
//! write back to the pty.

use goble_terminal::blocks::BlockView;
use goble_terminal::{
    Block, BlockEvent, BlockId, BlockList, BlockOwner, BlockState, CursorState, HookEvent, HookTap,
    OscEvent, OscTap, Palette, Screen, ScreenConfig, ScreenEvent, ScreenLine, ScreenQuery,
    ScreenSize, TermMode,
};
use std::time::Duration;

/// Lines of scrollback a pane keeps above the viewport.
const SCROLLBACK: usize = 10_000;

/// The card a conversation leaves behind in the terminal: the block that marks
/// where the conversation happened, named so a click can reopen its agent view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentViewCard {
    pub conversation_id: String,
    pub label: String,
}

/// One block as a view shows it: the command, who ran it and what it printed.
/// The owner is what puts an agent's command in its conversation's view and
/// leaves the user's own out of it; a conversation card is a block too, so it
/// carries its identity here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleBlock {
    pub id: BlockId,
    pub owner: BlockOwner,
    pub command: String,
    /// Everything the command printed, or `""` for a conversation card.
    pub output: String,
    /// Where the block is in its lifecycle, so a view can tell a command still
    /// running from one that finished instead of guessing from an exit flag.
    pub state: BlockState,
    /// The command's exit code once the shell reported it; `None` while it runs.
    pub exit_code: Option<i32>,
    /// Where the command ran, as the shell reported it. The view shortens it for
    /// display; the block itself keeps what the shell said.
    pub pwd: Option<String>,
    /// The git branch the command ran on, when the shell reported one.
    pub git_branch: Option<String>,
    /// How long the command took: its finished duration, or how long it has been
    /// running so far. `None` before the shell reported the command.
    pub duration: Option<Duration>,
    /// Set when this block is a conversation card rather than a shell block.
    pub card: Option<AgentViewCard>,
}

/// The VT state of one pane.
pub struct Emulator {
    screen: Screen,
    hooks: HookTap,
    osc: OscTap,
    hook_events: Vec<HookEvent>,
    osc_events: Vec<OscEvent>,
    /// The block list of this session: the command boundaries the hooks delimit,
    /// with each block's own screens. It is what the pane draws as its sections
    /// and what turns a claimed command into a tool result
    /// (`BlockEvent::ToolResult`); `screen` is still kept for what blocks cannot
    /// represent — the alternate screen of a full-screen program.
    blocks: BlockList,
    /// Block-level events the pane has not read yet (block starts/finishes and
    /// claimed commands' tool results).
    block_events: Vec<BlockEvent>,
    /// Bytes the screen cannot write itself and the session must send back.
    replies: Vec<u8>,
    columns: usize,
    screen_lines: usize,
}

impl std::fmt::Debug for Emulator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Emulator")
            .field("columns", &self.columns)
            .field("screen_lines", &self.screen_lines)
            .field("scrollback", &self.screen.history_size())
            .finish_non_exhaustive()
    }
}

impl Emulator {
    pub fn new(columns: usize, screen_lines: usize) -> Self {
        Self {
            screen: Screen::new(
                ScreenSize::new(columns, screen_lines),
                ScreenConfig::scrolling(SCROLLBACK),
            ),
            hooks: HookTap::new(),
            osc: OscTap::new(),
            hook_events: Vec::new(),
            osc_events: Vec::new(),
            blocks: BlockList::new(ScreenSize::new(columns, screen_lines)),
            block_events: Vec::new(),
            replies: Vec::new(),
            columns,
            screen_lines,
        }
    }

    /// Interpret one chunk of pty output.
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut staged = Vec::with_capacity(bytes.len());
        let hooks = self.hooks.tap(bytes, &mut staged);
        self.hook_events.extend(hooks.clone());

        let mut passthrough = Vec::with_capacity(staged.len());
        let osc = self.osc.tap(&staged, &mut passthrough);
        self.osc_events.extend(osc);

        // The blocks see the same bytes the screen does; the hooks then delimit
        // them into command/output blocks. Bytes are fed first so the shell's
        // echo of a typed line stays in the command screen, where the block
        // model puts it.
        self.blocks.feed(&passthrough);
        for hook in hooks {
            let events = self.blocks.apply(hook);
            // A `Preexec` is where the block's owner is decided. Once it is
            // known, an agent's command joins its conversation's view right
            // away, while a user's command — whose owner is `User` — stays out.
            for event in &events {
                if let BlockEvent::BlockStarted { id } = event {
                    self.associate_owner(*id);
                }
            }
            self.block_events.extend(events);
        }
        self.screen.feed(&passthrough);
    }

    /// Make a block the agent owns visible inside its conversation's agent
    /// view. A user-owned block names no conversation, so the terminal view
    /// keeps it to itself.
    fn associate_owner(&mut self, id: BlockId) {
        let conversation_id = match self.blocks.get(id).map(Block::owner) {
            Some(BlockOwner::Agent {
                conversation_id, ..
            }) => conversation_id.clone(),
            _ => return,
        };
        self.blocks
            .associate_with_conversation(id, &conversation_id);
    }

    /// Reshape the grid. Content reflows, so a pane resize never loses text.
    pub fn resize(&mut self, columns: usize, screen_lines: usize) {
        if (columns, screen_lines) == (self.columns, self.screen_lines) {
            return;
        }
        self.columns = columns;
        self.screen_lines = screen_lines;
        self.blocks.resize(ScreenSize::new(columns, screen_lines));
        self.screen.resize(ScreenSize::new(columns, screen_lines));
    }

    /// Whether the shell's integration has bootstrapped — B0's handshake hook,
    /// which is what makes the shell able to report a `Preexec` at all. The
    /// block list only offers a claimable block once it has arrived.
    pub fn bootstrapped(&self) -> bool {
        self.blocks.bootstrapped()
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    pub fn screen_lines(&self) -> usize {
        self.screen_lines
    }

    /// The visible rows, top row first.
    pub fn rows(&self) -> Vec<ScreenLine> {
        self.screen.lines()
    }

    pub fn cursor(&self) -> CursorState {
        self.screen.cursor()
    }

    /// How a key or a pointer report has to be encoded for the program that is
    /// running: cursor-key style, bracketed paste, focus reporting, mouse.
    pub fn input_mode(&self) -> TermMode {
        self.screen.input_mode()
    }

    pub fn is_alt_screen(&self) -> bool {
        self.screen.is_alt_screen()
    }

    /// Whether a command is running right now (a block in the executing state),
    /// so a pane knows output is still arriving and keeps repainting instead of
    /// idling between keystrokes.
    pub fn has_running_command(&self) -> bool {
        self.blocks
            .blocks()
            .iter()
            .any(|block| block.state() == BlockState::Executing)
    }

    /// Lines scrolled back from the bottom of the display.
    pub fn display_offset(&self) -> usize {
        self.screen.display_offset()
    }

    pub fn history_size(&self) -> usize {
        self.screen.history_size()
    }

    /// Scroll the display through the scrollback. 0 is the live bottom.
    pub fn scroll(&mut self, lines: i32) {
        self.screen.scroll_display(lines);
    }

    pub fn take_hook_events(&mut self) -> Vec<HookEvent> {
        std::mem::take(&mut self.hook_events)
    }

    /// Block-level events seen since the last call, oldest first. A claimed
    /// command's tool result arrives here as `BlockEvent::ToolResult`.
    pub fn take_block_events(&mut self) -> Vec<BlockEvent> {
        std::mem::take(&mut self.block_events)
    }

    /// The blocks a view draws, oldest first: the terminal's own view, or one
    /// conversation's agent view. The filter is the per-block visibility the
    /// owner set, so the two views agree on who ran what.
    pub fn visible_blocks(&self, view: &BlockView) -> Vec<VisibleBlock> {
        self.blocks
            .visible_blocks(view)
            .into_iter()
            .map(|block| VisibleBlock {
                id: block.id(),
                owner: block.owner().clone(),
                command: block.command_text().to_string(),
                output: block.output_text(),
                state: block.state(),
                exit_code: block.exit_code(),
                pwd: block.metadata().pwd.clone(),
                git_branch: block.metadata().git_branch.clone(),
                duration: block.duration().or_else(|| block.elapsed()),
                card: block
                    .conversation_id()
                    .map(|conversation_id| AgentViewCard {
                        conversation_id: conversation_id.to_string(),
                        label: block.label().unwrap_or_default().to_string(),
                    }),
            })
            .collect()
    }

    /// Push the card standing for `conversation_id` into the block list, or
    /// return the one already there.
    ///
    /// A round trip through the terminal enters a conversation more than once,
    /// so the card is reused rather than stacked: one conversation leaves one
    /// card in the list.
    pub fn push_agent_view_block(&mut self, conversation_id: &str, label: &str) -> BlockId {
        if let Some(existing) = self
            .blocks
            .blocks()
            .iter()
            .find(|block| block.conversation_id() == Some(conversation_id))
        {
            return existing.id();
        }
        self.blocks.push_agent_view_block(conversation_id, label)
    }

    /// Aim the next `Preexec` at the active block for `owner`, so the block the
    /// agent's command runs in belongs to that tool call.
    ///
    /// False when the active block cannot be claimed (the shell has not
    /// bootstrapped yet), so the caller fails the tool call instead of writing a
    /// command whose result could never be attributed.
    pub fn claim_active_block(&mut self, owner: BlockOwner) -> bool {
        let id = self.blocks.active_block().id();
        self.blocks.claim_next_pre_exec(id, owner)
    }

    pub fn take_osc_events(&mut self) -> Vec<OscEvent> {
        std::mem::take(&mut self.osc_events)
    }

    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    /// Collect the screen's events and answer everything it is waiting on.
    ///
    /// The answers are bytes for the pty, and the cell metrics come from the
    /// caller because they belong to the font, not to the emulator.
    pub fn pump(&mut self, palette: &Palette, cell: (u16, u16)) -> Vec<ScreenEvent> {
        let mut events = Vec::new();
        for event in self.screen.drain_events() {
            match event {
                // The screen already knows how to phrase these; it just cannot
                // write to the pty itself.
                ScreenEvent::PtyWrite(bytes) => self.replies.extend_from_slice(bytes.as_bytes()),
                // Announced only so the caller knows to look at the queue below.
                ScreenEvent::QueryPending => {}
                other => events.push(other),
            }
        }

        let queries = self.screen.drain_queries();
        for query in queries {
            if let Some(reply) = self.answer(&query, palette, cell) {
                self.replies.extend_from_slice(reply.as_bytes());
            }
        }

        events
    }

    fn answer(&self, query: &ScreenQuery, palette: &Palette, cell: (u16, u16)) -> Option<String> {
        if let Some(index) = query.color_index() {
            let index = u8::try_from(index).unwrap_or(u8::MAX);
            return query.color_reply(index as usize, palette.indexed(index));
        }
        if let Some(reply) = query.text_area_reply(
            self.columns as u16,
            self.screen_lines as u16,
            cell.0,
            cell.1,
        ) {
            return Some(reply);
        }
        // A clipboard read is answered with nothing: a program in the pane does
        // not get to read what the user copied elsewhere.
        query.clipboard_reply("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows_text(emulator: &Emulator) -> Vec<String> {
        emulator.rows().iter().map(|line| line.text()).collect()
    }

    #[test]
    fn text_and_escapes_land_on_the_grid() {
        let mut emulator = Emulator::new(20, 3);
        emulator.feed(b"\x1b[31mred\x1b[0m plain");
        assert_eq!(rows_text(&emulator)[0], "red plain");
    }

    #[test]
    fn cursor_positioning_is_interpreted_not_printed() {
        let mut emulator = Emulator::new(20, 3);
        emulator.feed(b"\x1b[2;3Hmid");
        assert_eq!(rows_text(&emulator)[1], "  mid");
        let cursor = emulator.cursor();
        assert_eq!((cursor.line, cursor.column), (1, 5));
    }

    #[test]
    fn a_hook_envelope_is_lifted_out_of_the_stream() {
        let mut emulator = Emulator::new(40, 3);
        let mut stream = goble_terminal::hooks::encode_hook(&HookEvent::Precmd(Default::default()));
        stream.extend_from_slice(b"after");
        emulator.feed(&stream);
        assert_eq!(emulator.take_hook_events().len(), 1);
        assert_eq!(rows_text(&emulator)[0], "after");
    }

    #[test]
    fn osc_seven_and_osc_133_are_observed_and_still_parsed() {
        let mut emulator = Emulator::new(40, 3);
        emulator.feed(b"\x1b]7;file://host/tmp/dir\x07\x1b]133;A\x07$ ");
        let events = emulator.take_osc_events();
        assert_eq!(events.len(), 2);
        // `text()` trims the trailing blank the shell left after the prompt.
        assert_eq!(rows_text(&emulator)[0], "$");
    }

    #[test]
    fn resize_reflows_instead_of_dropping_content() {
        let mut emulator = Emulator::new(40, 6);
        emulator.feed(b"one\r\ntwo\r\nthree");
        emulator.resize(20, 3);
        let text = rows_text(&emulator).join("\n");
        assert!(text.contains("one"), "{text:?}");
        assert!(text.contains("two"), "{text:?}");
        assert!(text.contains("three"), "{text:?}");
    }

    #[test]
    fn resize_to_the_same_size_is_a_no_op() {
        let mut emulator = Emulator::new(20, 3);
        emulator.feed(b"keep");
        emulator.resize(20, 3);
        assert_eq!(rows_text(&emulator)[0], "keep");
    }

    #[test]
    fn input_mode_follows_the_application() {
        let mut emulator = Emulator::new(20, 3);
        assert!(!emulator.input_mode().app_cursor);
        emulator.feed(b"\x1b[?1h\x1b[?2004h\x1b[?1000h\x1b[?1006h");
        let mode = emulator.input_mode();
        assert!(mode.app_cursor && mode.bracketed_paste);
        assert!(mode.mouse_click && mode.sgr_mouse);
    }

    #[test]
    fn a_text_area_request_is_answered_with_the_cell_metrics() {
        let mut emulator = Emulator::new(80, 24);
        emulator.feed(b"\x1b[14t");
        // 24 rows of 16pt and 80 columns of 8pt.
        emulator.pump(&Palette::xterm(), (8, 16));
        let reply = String::from_utf8(emulator.take_replies()).unwrap();
        assert_eq!(reply, "\x1b[4;384;640t");
    }

    #[test]
    fn a_colour_request_is_answered_from_the_palette() {
        let mut emulator = Emulator::new(80, 24);
        emulator.feed(b"\x1b]4;1;?\x07");
        emulator.pump(&Palette::xterm(), (8, 16));
        let reply = String::from_utf8(emulator.take_replies()).unwrap();
        // Index 1 is the xterm red, 205,0,0, reported as 16-bit channels.
        assert_eq!(reply, "\x1b]4;1;rgb:cdcd/0000/0000\x07");
    }

    #[test]
    fn the_alternate_screen_is_reported() {
        let mut emulator = Emulator::new(20, 3);
        emulator.feed(b"\x1b[?1049h");
        assert!(emulator.is_alt_screen());
        emulator.feed(b"\x1b[?1049l");
        assert!(!emulator.is_alt_screen());
    }
}
