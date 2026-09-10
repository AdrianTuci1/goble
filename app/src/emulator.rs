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

use goble_terminal::{
    CursorState, HookEvent, HookTap, OscEvent, OscTap, Palette, Screen, ScreenConfig, ScreenEvent,
    ScreenLine, ScreenQuery, ScreenSize, TermMode,
};

/// Lines of scrollback a pane keeps above the viewport.
const SCROLLBACK: usize = 10_000;

/// The VT state of one pane.
pub struct Emulator {
    screen: Screen,
    hooks: HookTap,
    osc: OscTap,
    hook_events: Vec<HookEvent>,
    osc_events: Vec<OscEvent>,
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
            replies: Vec::new(),
            columns,
            screen_lines,
        }
    }

    /// Interpret one chunk of pty output.
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut staged = Vec::with_capacity(bytes.len());
        let hooks = self.hooks.tap(bytes, &mut staged);
        self.hook_events.extend(hooks);

        let mut passthrough = Vec::with_capacity(staged.len());
        let osc = self.osc.tap(&staged, &mut passthrough);
        self.osc_events.extend(osc);

        self.screen.feed(&passthrough);
    }

    /// Reshape the grid. Content reflows, so a pane resize never loses text.
    pub fn resize(&mut self, columns: usize, screen_lines: usize) {
        if (columns, screen_lines) == (self.columns, self.screen_lines) {
            return;
        }
        self.columns = columns;
        self.screen_lines = screen_lines;
        self.screen.resize(ScreenSize::new(columns, screen_lines));
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
