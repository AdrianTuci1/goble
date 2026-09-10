//! A terminal cell grid.
//!
//! Draws one [`ScreenLine`] per row in the bundled monospace face: first the
//! background of every row as runs of one colour, then one glyph per non-blank
//! cell, then the rules the cell attributes ask for (underline, strikeout) and
//! the non-block cursors on top.
//!
//! Three passes rather than one because the renderer batches by primitive kind:
//! interleaving a rect with every glyph would close a batch per cell, while
//! grouping them costs three draw calls for the whole grid.
//!
//! Glyph runs are *not* merged into whole-row strings, which would be cheaper in
//! commands but ruinous in atlas entries: the atlas caches a rasterised entry
//! per string and never evicts, so a terminal redrawing a changing line every
//! frame would fill it. Single-character entries are bounded by the font's
//! character set, so the atlas can never grow past it.

use crate::color::ColorU;
use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{rectf, Vector2F};
use crate::platform::text_atlas::{mono_advance, FontWeight};
use crate::render::Renderer;
use crate::theme::FontFamily;
use goble_terminal::{CursorShape, CursorState, Palette, ScreenCell, ScreenLine, Underline};

/// The font size a terminal grid uses unless told otherwise.
pub const DEFAULT_FONT_SIZE: f32 = 12.0;
/// Row pitch as a multiple of the font size.
pub const DEFAULT_LINE_HEIGHT: f32 = 1.35;
/// Thickness of an underline or a strikeout.
const RULE_THICKNESS: f32 = 1.0;
/// Thickness of a beam cursor.
const BEAM_THICKNESS: f32 = 2.0;
/// The glyphs are drawn with the regular face; bold cells switch to the bold
/// face, which is the only weight the bundled monospace family carries.
const BOLD_WEIGHT: FontWeight = FontWeight::Bold;

/// A grid of terminal cells.
pub struct TerminalGrid {
    rows: Vec<ScreenLine>,
    cursor: CursorState,
    palette: Palette,
    font_size: f32,
    line_height: f32,
    show_cursor: bool,
    columns: usize,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TerminalGrid {
    /// A grid for one screen's visible rows and its cursor.
    pub fn new(rows: Vec<ScreenLine>, cursor: CursorState) -> Self {
        let columns = rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
        Self {
            rows,
            cursor,
            palette: Palette::xterm(),
            font_size: DEFAULT_FONT_SIZE,
            line_height: DEFAULT_LINE_HEIGHT,
            show_cursor: true,
            columns,
            size: None,
            origin: None,
        }
    }

    pub fn with_palette(mut self, palette: Palette) -> Self {
        self.palette = palette;
        self
    }

    pub fn with_font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }

    pub fn with_line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    /// Whether the cursor is drawn. The active pane shows it; a background pane
    /// does not, which is how a terminal signals where typing goes.
    pub fn with_cursor_visible(mut self, visible: bool) -> Self {
        self.show_cursor = visible;
        self
    }

    /// The horizontal pitch of one cell, in points.
    pub fn column_pitch(font_size: f32, weight: FontWeight) -> f32 {
        mono_advance(font_size, weight)
    }

    /// The horizontal pitch at the weight the grid draws its text with, for a
    /// caller that has to decide how many columns fit before it builds a grid.
    pub fn cell_width(font_size: f32) -> f32 {
        mono_advance(font_size, FontWeight::Regular)
    }

    /// The vertical pitch of one row, in points.
    pub fn row_pitch(font_size: f32, line_height: f32) -> f32 {
        font_size * line_height
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// The cell a point falls in, in grid coordinates, or `None` when the point
    /// is outside the grid.
    ///
    /// Used to turn a mouse event into a report for the application; the
    /// geometry comes from the last paint, which is the same frame the pointer
    /// is being matched against.
    pub fn cell_at(&self, position: Vector2F) -> Option<(usize, usize)> {
        let origin = self.origin?.xy();
        let column_pitch = Self::column_pitch(self.font_size, FontWeight::Regular);
        let row_pitch = Self::row_pitch(self.font_size, self.line_height);
        if column_pitch <= 0.0 || row_pitch <= 0.0 {
            return None;
        }
        let x = position.x - origin.x;
        let y = position.y - origin.y;
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let column = (x / column_pitch) as usize;
        let row = (y / row_pitch) as usize;
        (column < self.columns && row < self.rows.len()).then_some((row, column))
    }

    /// Whether the cursor sits on this cell and is drawn as a solid block,
    /// which is the only shape that repaints the cell it is on. A hollow block
    /// is outlined instead, so its glyph keeps its own colour.
    fn cursor_fills_cell(&self, row: usize, column: usize) -> bool {
        self.show_cursor
            && self.cursor.visible
            && self.cursor.line == row
            && self.cursor.column == column
            && self.cursor.shape == CursorShape::Block
    }

    fn color(&self, rgb: (u8, u8, u8)) -> ColorU {
        ColorU::new(rgb.0, rgb.1, rgb.2, 255)
    }

    fn cell_colors(&self, cell: &ScreenCell) -> ((u8, u8, u8), (u8, u8, u8)) {
        self.palette.cell_colors(cell)
    }

    fn origin_of(&self, origin: Vector2F, row: usize, column: usize) -> (f32, f32) {
        (
            origin.x + column as f32 * Self::column_pitch(self.font_size, FontWeight::Regular),
            origin.y + row as f32 * Self::row_pitch(self.font_size, self.line_height),
        )
    }

    /// Rows of one background colour, merged into a single rect each.
    fn paint_backgrounds(&self, renderer: &mut Renderer, origin: Vector2F) {
        let row_pitch = Self::row_pitch(self.font_size, self.line_height);
        let column_pitch = Self::column_pitch(self.font_size, FontWeight::Regular);
        let background = self.palette.background;

        for (row_index, row) in self.rows.iter().enumerate() {
            let top = origin.y + row_index as f32 * row_pitch;
            let mut run: Option<((u8, u8, u8), usize, usize)> = None;

            for column in 0..=row.cells.len() {
                let color = row.cells.get(column).map(|cell| {
                    if self.cursor_fills_cell(row_index, column) {
                        self.palette.cursor
                    } else {
                        self.cell_colors(cell).1
                    }
                });

                match (run, color) {
                    (Some((current, start, end)), Some(color)) if color == current => {
                        run = Some((current, start, end + 1));
                    }
                    (Some((current, start, end)), next) => {
                        // The pane background is already painted underneath, so
                        // a default-coloured run costs nothing.
                        if current != background {
                            renderer.fill_rect(
                                rectf(
                                    origin.x + start as f32 * column_pitch,
                                    top,
                                    (end - start) as f32 * column_pitch,
                                    row_pitch,
                                ),
                                self.color(current),
                            );
                        }
                        run = next.map(|color| (color, column, column + 1));
                    }
                    (None, Some(color)) => run = Some((color, column, column + 1)),
                    (None, None) => {}
                }
            }
        }
    }

    /// One quad per non-blank cell.
    fn paint_glyphs(&self, renderer: &mut Renderer, origin: Vector2F) {
        // A cell's ink can be a shade wider than its advance, so the wrap width
        // is generous: it exists only to stop the layout from breaking a single
        // character onto another line.
        let max_width = Self::column_pitch(self.font_size, FontWeight::Regular) * 2.0;

        for (row_index, row) in self.rows.iter().enumerate() {
            for (column, cell) in row.cells.iter().enumerate() {
                if cell.attrs.wide_spacer {
                    // The left half drew the glyph; this cell is its other half.
                    continue;
                }
                let mut text = String::new();
                if cell.ch != ' ' || !cell.zerowidth.is_empty() {
                    text.push(cell.ch);
                    text.extend(cell.zerowidth.iter());
                }
                if text.is_empty() {
                    continue;
                }

                let (mut foreground, background) = self.cell_colors(cell);
                if self.cursor_fills_cell(row_index, column) {
                    // The cursor repaints the cell, so its text is drawn in the
                    // colour the cursor did not paint with.
                    foreground = background;
                }
                let weight = if cell.attrs.bold {
                    BOLD_WEIGHT
                } else {
                    FontWeight::Regular
                };
                let (x, y) = self.origin_of(origin, row_index, column);
                renderer.draw_text_with_font(
                    Vector2F::new(x, y),
                    text,
                    self.font_size,
                    self.color(foreground),
                    max_width,
                    self.line_height,
                    weight,
                    FontFamily::Mono,
                );
            }
        }
    }

    /// Underlines, strikeouts and the cursors that are not blocks.
    fn paint_rules(&self, renderer: &mut Renderer, origin: Vector2F) {
        let column_pitch = Self::column_pitch(self.font_size, FontWeight::Regular);
        let row_pitch = Self::row_pitch(self.font_size, self.line_height);

        for (row_index, row) in self.rows.iter().enumerate() {
            for (column, cell) in row.cells.iter().enumerate() {
                let (x, top) = self.origin_of(origin, row_index, column);
                let (foreground, _) = self.cell_colors(cell);

                if cell.attrs.underline != Underline::None {
                    let color = cell
                        .attrs
                        .underline_color
                        .map(|color| self.palette.rgb(color))
                        .unwrap_or(foreground);
                    let thickness = if cell.attrs.underline == Underline::Double {
                        RULE_THICKNESS * 2.0
                    } else {
                        RULE_THICKNESS
                    };
                    renderer.fill_rect(
                        rectf(x, top + row_pitch - thickness, column_pitch, thickness),
                        self.color(color),
                    );
                }

                if cell.attrs.strikeout {
                    renderer.fill_rect(
                        rectf(x, top + row_pitch * 0.55, column_pitch, RULE_THICKNESS),
                        self.color(foreground),
                    );
                }

                if self.show_cursor
                    && self.cursor.visible
                    && self.cursor.line == row_index
                    && self.cursor.column == column
                {
                    match self.cursor.shape {
                        CursorShape::Beam => renderer.fill_rect(
                            rectf(x, top, BEAM_THICKNESS, row_pitch),
                            self.color(self.palette.cursor),
                        ),
                        CursorShape::Underline => renderer.fill_rect(
                            rectf(
                                x,
                                top + row_pitch - BEAM_THICKNESS,
                                column_pitch,
                                BEAM_THICKNESS,
                            ),
                            self.color(self.palette.cursor),
                        ),
                        CursorShape::HollowBlock => renderer.stroke_rect(
                            rectf(x, top, column_pitch, row_pitch),
                            self.color(self.palette.cursor),
                            RULE_THICKNESS,
                            0.0,
                        ),
                        CursorShape::Block | CursorShape::Hidden => {}
                    }
                }
            }
        }
    }
}

impl Element for TerminalGrid {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        // The grid fills its pane: the background has to reach the edges even
        // where the last row is short.
        let size = Vector2F::new(constraint.max.x, constraint.max.y);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else {
            return;
        };
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };

        let area = rectf(origin.x, origin.y, size.x, size.y);
        renderer.fill_rect(area, self.color(self.palette.background));
        renderer.clip_rect(area);
        self.paint_backgrounds(renderer, origin);
        self.paint_glyphs(renderer, origin);
        self.paint_rules(renderer, origin);
        renderer.pop_clip();
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::RenderCommand;
    use goble_terminal::{CellAttrs, Screen, ScreenColor, ScreenSize};

    fn grid_from(bytes: &[u8], columns: usize, lines: usize) -> TerminalGrid {
        let mut screen = Screen::new(
            ScreenSize::new(columns, lines),
            goble_terminal::ScreenConfig::scrolling(0),
        );
        screen.feed(bytes);
        TerminalGrid::new(screen.lines(), screen.cursor())
    }

    fn painted(grid: &mut TerminalGrid) -> Vec<RenderCommand> {
        let mut ctx = PaintContext::new(crate::render::Renderer::default());
        grid.layout(
            SizeConstraint::loose(Vector2F::new(200.0, 100.0)),
            &mut LayoutContext,
            &AppContext::default(),
        );
        grid.paint(Vector2F::new(0.0, 0.0), &mut ctx, &AppContext::default());
        ctx.renderer.take().unwrap().commands().to_vec()
    }

    fn texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn rects(commands: &[RenderCommand]) -> Vec<(f32, f32, f32, f32, ColorU)> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. } => Some((
                    rect.origin.x,
                    rect.origin.y,
                    rect.size.width,
                    rect.size.height,
                    *color,
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn blank_cells_are_not_drawn() {
        let mut grid = grid_from(b"hi", 10, 2);
        let commands = painted(&mut grid);
        assert_eq!(texts(&commands), vec!["h", "i"]);
    }

    #[test]
    fn every_visible_row_is_drawn() {
        let mut grid = grid_from(b"one\r\ntwo\r\nthree", 10, 4);
        assert_eq!(grid.rows(), 4);
        assert_eq!(
            texts(&painted(&mut grid)),
            vec!["o", "n", "e", "t", "w", "o", "t", "h", "r", "e", "e"]
        );
    }

    #[test]
    fn the_pane_is_painted_and_clipped() {
        let mut grid = grid_from(b"x", 10, 2);
        let commands = painted(&mut grid);
        assert!(matches!(
            commands.first(),
            Some(RenderCommand::FillRect { .. })
        ));
        assert!(commands
            .iter()
            .any(|c| matches!(c, RenderCommand::ClipRect(_))));
        assert_eq!(
            commands.last().map(|c| matches!(c, RenderCommand::PopClip)),
            Some(true)
        );
    }

    #[test]
    fn a_background_run_becomes_one_rect() {
        let mut grid = grid_from(b"\x1b[41m   \x1b[0m", 10, 1);
        let commands = painted(&mut grid);
        // The pane, the three coloured cells as one run, and the block cursor
        // that sits after them.
        let rects = rects(&commands);
        assert_eq!(rects.len(), 3, "{rects:?}");
        let run = rects[1];
        assert_eq!(run.0, 0.0);
        assert_eq!(
            run.2,
            TerminalGrid::column_pitch(DEFAULT_FONT_SIZE, FontWeight::Regular) * 3.0
        );
        assert_eq!(run.4, ColorU::new(205, 0, 0, 255));
        assert_eq!(
            rects[2].2,
            TerminalGrid::column_pitch(DEFAULT_FONT_SIZE, FontWeight::Regular)
        );
    }

    #[test]
    fn the_default_background_is_not_repainted() {
        let mut grid = grid_from(b"plain text", 20, 1).with_cursor_visible(false);
        // Only the pane background: a cell that asked for the default colour
        // costs no rect at all.
        assert_eq!(rects(&painted(&mut grid)).len(), 1);
    }

    #[test]
    fn the_cursor_repaints_its_cell() {
        let mut grid = grid_from(b"ab", 10, 1);
        // The cursor follows the text, so it sits on the blank cell after it.
        assert_eq!(grid.cursor.column, 2);
        let commands = painted(&mut grid);
        let cursor_color = grid.palette.cursor;
        assert!(
            rects(&commands).iter().any(|(_, _, _, _, color)| *color
                == ColorU::new(cursor_color.0, cursor_color.1, cursor_color.2, 255)),
            "the block cursor is painted"
        );
    }

    #[test]
    fn an_inactive_pane_draws_no_cursor() {
        let mut grid = grid_from(b"ab", 10, 1).with_cursor_visible(false);
        let commands = painted(&mut grid);
        assert_eq!(rects(&commands).len(), 1, "only the pane background");
    }

    #[test]
    fn inverse_swaps_the_colours_the_application_asked_for() {
        let mut grid = grid_from(b"\x1b[7mX\x1b[0m", 4, 1);
        let commands = painted(&mut grid);
        // The pane background, then the inverted cell.
        let rects = rects(&commands);
        assert_eq!(rects.len(), 2);
        let palette = Palette::xterm();
        assert_eq!(
            rects[1].4,
            ColorU::new(
                palette.foreground.0,
                palette.foreground.1,
                palette.foreground.2,
                255
            )
        );
        // And the glyph is painted in the background colour.
        let glyph = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, color, .. } if text == "X" => Some(*color),
                _ => None,
            })
            .expect("the glyph");
        assert_eq!(
            glyph,
            ColorU::new(
                palette.background.0,
                palette.background.1,
                palette.background.2,
                255
            )
        );
    }

    #[test]
    fn an_underline_is_drawn_as_a_rule() {
        let mut grid = grid_from(b"\x1b[4mU\x1b[0m", 4, 1);
        let commands = painted(&mut grid);
        let row_pitch = TerminalGrid::row_pitch(DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT);
        assert!(
            rects(&commands)
                .iter()
                .any(|(_, y, _, height, _)| (*y - (row_pitch - 1.0)).abs() < 0.01 && *height == 1.0),
            "the underline sits at the bottom of the row: {:?}",
            rects(&commands)
        );
    }

    #[test]
    fn a_wide_character_is_drawn_once_and_its_spacer_is_skipped() {
        let mut grid = grid_from("日本".as_bytes(), 10, 1);
        assert_eq!(texts(&painted(&mut grid)), vec!["日", "本"]);
    }

    #[test]
    fn the_grid_dimensions_come_from_the_screen() {
        let grid = grid_from(b"abc", 12, 3);
        assert_eq!(grid.columns(), 12);
        assert_eq!(grid.rows(), 3);
    }

    #[test]
    fn a_point_maps_to_the_cell_under_it() {
        let mut grid = grid_from(b"abc", 10, 3);
        painted(&mut grid);
        let column = TerminalGrid::column_pitch(DEFAULT_FONT_SIZE, FontWeight::Regular);
        let row = TerminalGrid::row_pitch(DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT);
        assert_eq!(grid.cell_at(Vector2F::new(0.5, 0.5)), Some((0, 0)));
        assert_eq!(
            grid.cell_at(Vector2F::new(column * 2.0 + 1.0, row + 1.0)),
            Some((1, 2))
        );
        assert_eq!(grid.cell_at(Vector2F::new(-1.0, 0.0)), None);
        assert_eq!(grid.cell_at(Vector2F::new(column * 20.0, 0.0)), None);
    }

    #[test]
    fn a_hollow_cursor_keeps_the_glyph_readable() {
        // `DECSCUSR` cannot ask for a hollow block, so the shape is set
        // directly, the way a configured default would.
        let row = ScreenLine {
            cells: vec![ScreenCell {
                ch: 'X',
                ..ScreenCell::default()
            }],
        };
        let mut grid = TerminalGrid::new(
            vec![row],
            CursorState {
                line: 0,
                column: 0,
                visible: true,
                shape: CursorShape::HollowBlock,
            },
        );
        let commands = painted(&mut grid);
        // An outlined cursor is a stroke, not a fill, so the text keeps its own
        // colour instead of being repainted in the background colour.
        assert!(commands
            .iter()
            .any(|c| matches!(c, RenderCommand::StrokeRect { .. })));
        let palette = Palette::xterm();
        let glyph = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, color, .. } if text == "X" => Some(*color),
                _ => None,
            })
            .expect("the glyph");
        assert_eq!(
            glyph,
            ColorU::new(
                palette.foreground.0,
                palette.foreground.1,
                palette.foreground.2,
                255
            )
        );
        let cursor = ColorU::new(palette.cursor.0, palette.cursor.1, palette.cursor.2, 255);
        assert!(
            !rects(&commands)
                .iter()
                .any(|(_, _, _, _, color)| *color == cursor),
            "a hollow cursor does not fill its cell"
        );
    }

    #[test]
    fn attributes_do_not_disturb_a_plain_screen() {
        let mut grid = grid_from(b"ok", 4, 1).with_cursor_visible(false);
        let commands = painted(&mut grid);
        assert_eq!(rects(&commands).len(), 1);
        assert_eq!(texts(&commands), vec!["o", "k"]);
        assert_eq!(CellAttrs::default().underline, Underline::None);
        assert_eq!(ScreenColor::Indexed(1), ScreenColor::Indexed(1));
    }
}
