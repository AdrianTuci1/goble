//! The paint model a terminal surface is drawn from.
//!
//! [`Screen::frame`](crate::screen::Screen::frame) reads the vendored
//! `alacritty_terminal` grid directly — the same `RenderableContent` iterator
//! Alacritty's own renderer consumes — and turns one viewport into
//! [`GridFrame`]: per row, contiguous runs of one background colour, one glyph
//! per non-blank cell, and the underline/strikeout rules the attributes ask
//! for. A painter therefore receives runs, not a widget tree: the backgrounds
//! of a row collapse to as many rects as it has colour changes, and the glyphs
//! stay per cell because the text atlas caches one rasterised entry per string
//! and a terminal redraws changing lines every frame.
//!
//! The model is deliberately plain data with no `alacritty_terminal` types in
//! it, so it can be asserted headlessly (see the tests below) and painted by
//! any host's renderer. `goble_terminal::screen` is the only place that knows
//! the emulator crate.

use crate::screen::{CursorState, ScreenCell, ScreenColor, ScreenLine, Underline};

/// One viewport of terminal cells, as the painter consumes it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GridFrame {
    /// The visible rows, top row first, each already grouped into runs.
    pub rows: Vec<FrameRow>,
    /// The cursor position within the viewport.
    pub cursor: CursorState,
    /// The grid's column count. Every row is this wide.
    pub columns: usize,
    /// Rows of scrollback above the viewport.
    pub history_size: usize,
    /// Rows scrolled back from the live bottom.
    pub display_offset: usize,
    /// Whether the program is on the alternate screen.
    pub alt_screen: bool,
}

impl GridFrame {
    /// An empty frame with a hidden cursor, as a session that has not drawn
    /// yet reports.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The frame's text, one row per line, trailing blanks trimmed.
    pub fn text(&self) -> String {
        self.rows
            .iter()
            .map(FrameRow::text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// One row's text, with trailing blanks trimmed.
    pub fn row_text(&self, row: usize) -> Option<String> {
        self.rows.get(row).map(FrameRow::text)
    }
}

/// One visible row, in paint order per kind: backgrounds first, then glyphs,
/// then the attribute rules.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameRow {
    /// Contiguous cells of one background colour, left to right. The colour
    /// already accounts for inverse video; hidden text is still its cell's
    /// glyph, drawn in this colour by the painter.
    pub backgrounds: Vec<BackgroundRun>,
    /// One glyph per non-blank, non-spacer cell, left to right.
    pub glyphs: Vec<Glyph>,
    /// Underlines and strikeouts, one per cell that asks for one.
    pub rules: Vec<CellRule>,
}

impl FrameRow {
    /// Group a row's cells into background runs, glyphs and rules.
    ///
    /// This is the `alacritty cell -> runs` step: only colour changes open a
    /// new background run, spacer cells attach no glyph, and a blank cell with
    /// no combining marks contributes nothing to the glyph list.
    pub fn from_cells(cells: &[ScreenCell]) -> Self {
        let mut backgrounds: Vec<BackgroundRun> = Vec::new();
        let mut glyphs = Vec::new();
        let mut rules = Vec::new();
        let mut run: Option<BackgroundRun> = None;

        for (column, cell) in cells.iter().enumerate() {
            // Inverse video paints the cell's foreground as its background.
            let background = if cell.attrs.inverse { cell.fg } else { cell.bg };
            match &mut run {
                // Runs are only merged while they are adjacent; a colour that
                // repeats after another one starts a new run.
                Some(current) if current.color == background && current.end() == column => {
                    current.len += 1;
                }
                _ => {
                    if let Some(finished) = run.take() {
                        backgrounds.push(finished);
                    }
                    run = Some(BackgroundRun {
                        start: column,
                        len: 1,
                        color: background,
                    });
                }
            }

            if !cell.attrs.wide_spacer && (cell.ch != ' ' || !cell.zerowidth.is_empty()) {
                glyphs.push(Glyph {
                    column,
                    cell: cell.clone(),
                });
            }

            if cell.attrs.underline != Underline::None || cell.attrs.strikeout {
                rules.push(CellRule {
                    column,
                    // The whole cell travels with the rule so a painter can
                    // resolve its effective colours through the palette, the
                    // same way it resolves a glyph's.
                    cell: cell.clone(),
                });
            }
        }
        if let Some(run) = run {
            backgrounds.push(run);
        }

        Self {
            backgrounds,
            glyphs,
            rules,
        }
    }

    /// Build a row from plain [`ScreenLine`] cells. The block view reads
    /// whole-screen rows, so it can paint through the same run model.
    pub fn from_line(line: &ScreenLine) -> Self {
        Self::from_cells(&line.cells)
    }

    /// The row's text, trailing blanks trimmed. A wide glyph advances two
    /// columns even though it has one glyph cell.
    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut next = 0usize;
        for glyph in &self.glyphs {
            while next < glyph.column {
                out.push(' ');
                next += 1;
            }
            out.push(glyph.cell.ch);
            out.extend(glyph.cell.zerowidth.iter());
            next = glyph.column + glyph.width();
        }
        out.truncate(out.trim_end_matches(' ').len());
        out
    }

    /// Whether no cell on the row carries a glyph.
    pub fn is_blank(&self) -> bool {
        self.glyphs.is_empty()
    }
}

/// A contiguous span of cells sharing one background colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackgroundRun {
    pub start: usize,
    pub len: usize,
    pub color: ScreenColor,
}

impl BackgroundRun {
    /// One past the run's last cell.
    pub fn end(&self) -> usize {
        self.start.saturating_add(self.len)
    }
}

/// One painted glyph: the cell plus where it sits on the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    pub column: usize,
    pub cell: ScreenCell,
}

impl Glyph {
    /// The glyph's text: its character plus any combining marks.
    pub fn text(&self) -> String {
        let mut text = String::new();
        text.push(self.cell.ch);
        text.extend(self.cell.zerowidth.iter());
        text
    }

    /// How many grid columns the glyph occupies (two for a wide character).
    pub fn width(&self) -> usize {
        if self.cell.attrs.wide {
            2
        } else {
            1
        }
    }
}

/// An underline or strikeout rule on one cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellRule {
    pub column: usize,
    pub cell: ScreenCell,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::{CursorShape, Screen, ScreenConfig, ScreenSize};
    use alacritty_terminal::vte::ansi::NamedColor;

    fn screen(columns: usize, lines: usize) -> Screen {
        Screen::new(
            ScreenSize::new(columns, lines),
            ScreenConfig::scrolling(100),
        )
    }

    #[test]
    fn text_and_backgrounds_become_runs() {
        let mut screen = screen(20, 3);
        screen.feed(b"\x1b[41mAB\x1b[0mC");
        let frame = screen.frame();

        assert_eq!(frame.rows[0].text(), "ABC");
        // The red background is one run of two cells, and the reset rest is a
        // second run: two colour changes, not three cells.
        assert_eq!(frame.rows[0].backgrounds.len(), 2, "{:?}", frame.rows[0]);
        assert_eq!(frame.rows[0].backgrounds[0].start, 0);
        assert_eq!(frame.rows[0].backgrounds[0].len, 2);
        assert_eq!(
            frame.rows[0].backgrounds[0].color,
            ScreenColor::Named(NamedColor::Red)
        );
        // Every non-blank cell is one glyph, in reading order.
        let glyphs: String = frame.rows[0]
            .glyphs
            .iter()
            .map(Glyph::text)
            .collect::<String>();
        assert_eq!(glyphs, "ABC");
        assert_eq!(frame.columns, 20);
        assert_eq!(frame.rows.len(), 3);
    }

    #[test]
    fn equal_colours_merge_only_while_adjacent() {
        let mut screen = screen(10, 2);
        // Two separate red cells with a default cell between them: two runs.
        screen.feed(b"\x1b[41mA\x1b[0m \x1b[41mB\x1b[0m");
        let backgrounds = &screen.frame().rows[0].backgrounds;
        // Red, default, red, then the default rest of the row.
        assert_eq!(backgrounds.len(), 4, "{backgrounds:?}");
        assert_eq!(backgrounds[0].len, 1);
        assert_eq!(backgrounds[1].start, 1);
        assert_eq!(backgrounds[2].start, 2);
        assert_eq!(backgrounds[3].start, 3);
    }

    #[test]
    fn wide_glyphs_take_two_columns_and_skip_their_spacer() {
        let mut screen = screen(10, 2);
        screen.feed("日本".as_bytes());
        let frame = screen.frame();
        assert_eq!(frame.rows[0].text(), "日本");
        let columns: Vec<usize> = frame.rows[0].glyphs.iter().map(|g| g.column).collect();
        assert_eq!(columns, vec![0, 2], "the spacer carries no glyph");
        assert_eq!(frame.rows[0].glyphs[0].width(), 2);
    }

    #[test]
    fn inverse_video_moves_the_cell_colour_into_the_background_run() {
        let mut screen = screen(10, 2);
        screen.feed(b"\x1b[7mINV\x1b[0m");
        let row = &screen.frame().rows[0];
        // The run's background is the cell's foreground (the default colour
        // family), which is what inverse video paints.
        assert!(row.backgrounds.iter().any(|run| matches!(
            run.color,
            ScreenColor::Named(
                NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground
            )
        )));
        // The glyph keeps the attribute so a painter can resolve its colours
        // through the palette.
        assert!(row.glyphs.iter().all(|glyph| glyph.cell.attrs.inverse));
    }

    #[test]
    fn underline_and_strikeout_survive_as_rules() {
        let mut screen = screen(10, 2);
        screen.feed(b"\x1b[4;9mX\x1b[0m");
        let row = &screen.frame().rows[0];
        assert_eq!(row.rules.len(), 1);
        assert_eq!(row.rules[0].column, 0);
        assert_eq!(row.rules[0].cell.attrs.underline, Underline::Single);
        assert!(row.rules[0].cell.attrs.strikeout);
    }

    #[test]
    fn the_frame_cursor_is_viewport_relative_and_carries_its_shape() {
        let mut screen = screen(20, 4);
        screen.feed(b"\x1b[3;5H\x1b[4 q");
        let frame = screen.frame();
        assert_eq!((frame.cursor.line, frame.cursor.column), (2, 4));
        assert!(frame.cursor.visible);
        assert_eq!(frame.cursor.shape, CursorShape::Underline);
    }

    #[test]
    fn scrollback_rows_are_not_part_of_the_viewport_frame() {
        let mut screen = screen(10, 2);
        screen.feed(b"one\r\ntwo\r\nthree\r\n");
        let frame = screen.frame();
        assert_eq!(frame.rows.len(), 2, "only the visible screen is painted");
        assert_eq!(frame.history_size, 2);
        assert_eq!(frame.display_offset, 0);
        assert_eq!(frame.text(), "three\n");
    }

    #[test]
    fn a_blank_screen_is_one_default_background_run_per_row() {
        let screen = screen(8, 3);
        let frame = screen.frame();
        assert_eq!(frame.rows.len(), 3);
        for row in &frame.rows {
            assert!(row.is_blank());
            assert_eq!(row.backgrounds.len(), 1);
            assert_eq!(row.backgrounds[0].len, 8);
        }
        assert!(frame.cursor.visible);
    }
}
