//! Line-oriented ANSI rendering for captured output.
//!
//! A pane draws a command's output from the grid the emulator maintains. A
//! block in the transcript has no grid: it is a list of lines. This module runs
//! the same vendored `vte` parser the screen runs, through a line-oriented sink
//! instead of a grid, so an ANSI-coloured blob is drawn with the colours it
//! emitted instead of showing its escape sequences literally.
//!
//! Only what a line of text can carry is kept — the foreground colour, bold,
//! italic and underline. Everything else (cursor addressing, erases, modes, an
//! unknown final) is consumed and ignored, never dropped from the text around
//! it. The result is the same cell model the screen hands the renderer
//! ([`ScreenLine`]), so both paths describe a colour the same way.

use alacritty_terminal::vte::ansi::{Attr, Color, Handler, NamedColor, Processor, Rgb};

use crate::screen::{CellAttrs, ScreenCell, ScreenColor, ScreenLine, Underline};

/// Bounds against a pathological stream. Captured output is truncated upstream;
/// these only stop escape-code abuse from growing the line model without limit.
const MAX_ROWS: usize = 50_000;
const MAX_COLUMNS: usize = 8_192;

/// Render a raw output stream as styled lines.
///
/// Printable characters are kept with the attributes in force when they were
/// written; a carriage return, a backspace and a tab move within the line, and a
/// line feed starts the next one. A single trailing newline adds no blank line,
/// so this matches `str::lines()`.
pub fn render_lines(bytes: &[u8]) -> Vec<ScreenLine> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let mut sink = LineSink::new();
    let mut parser: Processor = Processor::new();
    parser.advance(&mut sink, bytes);
    sink.finish()
}

struct LineSink {
    rows: Vec<Vec<ScreenCell>>,
    row: usize,
    col: usize,
    fg: ScreenColor,
    bold: bool,
    italic: bool,
    underline: Underline,
}

impl LineSink {
    fn new() -> Self {
        Self {
            rows: vec![Vec::new()],
            row: 0,
            col: 0,
            fg: ScreenColor::Named(NamedColor::Foreground),
            bold: false,
            italic: false,
            underline: Underline::None,
        }
    }

    fn cell(&self, ch: char) -> ScreenCell {
        ScreenCell {
            ch,
            fg: self.fg,
            bg: ScreenColor::Named(NamedColor::Background),
            attrs: CellAttrs {
                bold: self.bold,
                italic: self.italic,
                underline: self.underline,
                ..CellAttrs::default()
            },
            zerowidth: Vec::new(),
            hyperlink: None,
        }
    }

    fn put(&mut self, ch: char) {
        if self.col >= MAX_COLUMNS {
            return;
        }
        if self.row >= self.rows.len() {
            self.rows.push(Vec::new());
        }
        let cell = self.cell(ch);
        let row = &mut self.rows[self.row];
        while row.len() <= self.col {
            row.push(ScreenCell::default());
        }
        row[self.col] = cell;
        self.col += 1;
    }

    fn newline(&mut self) {
        self.row += 1;
        self.col = 0;
        while self.rows.len() <= self.row {
            self.rows.push(Vec::new());
        }
        if self.row >= MAX_ROWS {
            // Keep the newest rows; the cap is only a guard, not a transcript.
            self.rows.remove(0);
            self.row -= 1;
        }
    }

    fn finish(mut self) -> Vec<ScreenLine> {
        if self.rows.last().is_some_and(|row| row.iter().all(is_blank)) {
            self.rows.pop();
        }
        self.rows
            .into_iter()
            .map(|mut cells| {
                while cells.last().is_some_and(is_blank) {
                    cells.pop();
                }
                ScreenLine { cells }
            })
            .collect()
    }
}

/// A cell that paints nothing: a space with no mark attached.
fn is_blank(cell: &ScreenCell) -> bool {
    cell.ch == ' ' && cell.zerowidth.is_empty()
}

impl Handler for LineSink {
    fn input(&mut self, c: char) {
        self.put(c);
    }

    fn linefeed(&mut self) {
        self.newline();
    }

    fn newline(&mut self) {
        self.newline();
    }

    fn carriage_return(&mut self) {
        self.col = 0;
    }

    fn backspace(&mut self) {
        self.col = self.col.saturating_sub(1);
    }

    fn put_tab(&mut self, count: u16) {
        let count = usize::from(count.max(1));
        self.col = (self.col / 8 + count) * 8;
    }

    fn terminal_attribute(&mut self, attr: Attr) {
        match attr {
            Attr::Reset => {
                self.fg = ScreenColor::Named(NamedColor::Foreground);
                self.bold = false;
                self.italic = false;
                self.underline = Underline::None;
            }
            Attr::Bold => self.bold = true,
            Attr::Italic => self.italic = true,
            Attr::Underline => self.underline = Underline::Single,
            Attr::DoubleUnderline => self.underline = Underline::Double,
            Attr::Undercurl => self.underline = Underline::Curly,
            Attr::DottedUnderline => self.underline = Underline::Dotted,
            Attr::DashedUnderline => self.underline = Underline::Dashed,
            Attr::CancelBold | Attr::CancelBoldDim => self.bold = false,
            Attr::CancelItalic => self.italic = false,
            Attr::CancelUnderline => self.underline = Underline::None,
            Attr::Foreground(color) => self.fg = map_color(color),
            // Background, dim, reverse, hidden, strikeout, blink and underline
            // colour are not drawn by a transcript line: ignored, but the text
            // they apply to is not.
            _ => {}
        }
    }
}

/// Map the parser's colour to the cell model's.
fn map_color(color: Color) -> ScreenColor {
    match color {
        Color::Named(named) => ScreenColor::Named(named),
        Color::Spec(Rgb { r, g, b }) => ScreenColor::Rgb(r, g, b),
        Color::Indexed(index) => ScreenColor::Indexed(index),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_texts(bytes: &[u8]) -> Vec<String> {
        render_lines(bytes).iter().map(ScreenLine::text).collect()
    }

    #[test]
    fn strips_sgr_to_plain_text() {
        assert_eq!(line_texts(b"\x1b[1m\x1b[36mbazel\x1b[0m"), vec!["bazel"]);
    }

    #[test]
    fn foreground_colours_are_kept_per_cell() {
        let lines = render_lines(b"\x1b[31mred\x1b[0m plain");
        assert_eq!(lines.len(), 1);
        let cells = &lines[0].cells;
        assert_eq!(cells[0].ch, 'r');
        assert!(matches!(cells[0].fg, ScreenColor::Named(NamedColor::Red)));
        // The reset returns the rest of the line to the default foreground.
        let last = cells.last().expect("a cell");
        assert!(matches!(
            last.fg,
            ScreenColor::Named(NamedColor::Foreground)
        ));
    }

    #[test]
    fn bold_italic_and_underline_are_carried() {
        let lines = render_lines(b"\x1b[1;3;4mx\x1b[22;23;24my");
        let cells = &lines[0].cells;
        assert!(cells[0].attrs.bold);
        assert!(cells[0].attrs.italic);
        assert_eq!(cells[0].attrs.underline, Underline::Single);
        assert!(!cells[1].attrs.bold);
        assert!(!cells[1].attrs.italic);
        assert_eq!(cells[1].attrs.underline, Underline::None);
    }

    #[test]
    fn truecolor_and_indexed_colours_are_kept() {
        let lines = render_lines(b"\x1b[38;2;1;2;3mr\x1b[38;5;200mi");
        let cells = &lines[0].cells;
        assert_eq!(cells[0].fg, ScreenColor::Rgb(1, 2, 3));
        assert_eq!(cells[1].fg, ScreenColor::Indexed(200));
    }

    #[test]
    fn carriage_return_overwrites_in_place() {
        assert_eq!(line_texts(b"aaaa\rbb"), vec!["bbaa"]);
    }

    #[test]
    fn trailing_newline_adds_no_blank_line() {
        assert_eq!(line_texts(b"a\n"), vec!["a"]);
        assert_eq!(line_texts(b"a\n\n"), vec!["a", ""]);
    }

    #[test]
    fn an_unrecognised_sequence_loses_no_text() {
        // Blink (ignored), a cursor position (not a line concept) and an unknown
        // private sequence all leave the words around them in place.
        assert_eq!(
            line_texts(b"\x1b[5mhidden\x1b[10;20H visible\x1b[?9999z !"),
            vec!["hidden visible !"]
        );
    }
}
