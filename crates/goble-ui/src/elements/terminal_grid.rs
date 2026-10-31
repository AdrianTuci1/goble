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
use goble_terminal::{
    CursorShape, CursorState, ImagePlacement, Palette, ScreenCell, ScreenLine, Underline,
};

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

/// An inclusive span of grid cells on one or more rows.
///
/// The callers that draw selections and search matches build one range per
/// row, so a multi-row selection is simply several ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridRange {
    pub start: (usize, usize),
    pub end: (usize, usize),
}

impl GridRange {
    pub const fn new(start: (usize, usize), end: (usize, usize)) -> Self {
        Self { start, end }
    }
}

/// A grid of terminal cells.
pub struct TerminalGrid {
    rows: Vec<ScreenLine>,
    cursor: CursorState,
    palette: Palette,
    font_size: f32,
    line_height: f32,
    show_cursor: bool,
    /// Whether the rows hug the bottom of the box. A shell's history grows
    /// upwards from the input line below it (warp-new), while a full-screen
    /// program owns its own layout and must be drawn where it painted.
    bottom_anchor: bool,
    /// The mouse selection, one range per row.
    selection: Vec<GridRange>,
    /// The colour the selection is filled with. `None` keeps the palette's blue
    /// (ANSI 4), which is what a grid built outside an app theme paints with.
    selection_color: Option<ColorU>,
    /// Search matches, one range per row, and which one is current.
    highlights: Vec<GridRange>,
    active_highlight: Option<usize>,
    /// The copy-mode cursor, drawn as a hollow cell so the shell's own cursor
    /// can stay hidden.
    copy_cursor: Option<(usize, usize)>,
    /// Images placed by the terminal, anchored to cells.
    images: Vec<ImagePlacement>,
    /// The source prefix for the renderer's texture cache, so two panes using
    /// the same image id do not share a texture.
    image_namespace: String,
    /// The pane's font cell in device pixels, used to size an image that named
    /// no cell span from its pixel dimensions.
    cell_pixels: (f32, f32),
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
            bottom_anchor: false,
            selection: Vec::new(),
            selection_color: None,
            highlights: Vec::new(),
            active_highlight: None,
            copy_cursor: None,
            images: Vec::new(),
            image_namespace: "terminal".to_string(),
            cell_pixels: (8.0, 16.0),
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

    /// Whether the content hugs the bottom of the box. Off by default, so a
    /// block sized to its own rows is unaffected; the interactive pane turns it
    /// on so a short history sits above its input line, and a full-screen
    /// program leaves it off because its rows are its own layout.
    pub fn with_bottom_anchor(mut self, anchored: bool) -> Self {
        self.bottom_anchor = anchored;
        self
    }

    /// Paint a mouse selection, one cell range per row.
    pub fn with_selection(mut self, selection: Vec<GridRange>) -> Self {
        self.selection = selection;
        self
    }

    /// The colour the selection is filled with, so a pane's selection agrees
    /// with the other surfaces' bands. Unset, the grid keeps the palette's blue.
    pub fn with_selection_color(mut self, color: ColorU) -> Self {
        self.selection_color = Some(color);
        self
    }

    /// Paint search matches. `active` indexes the range in `highlights` that
    /// the find bar's current match covers, so it can be drawn in the accent.
    pub fn with_highlights(mut self, highlights: Vec<GridRange>, active: Option<usize>) -> Self {
        self.highlights = highlights;
        self.active_highlight = active;
        self
    }

    /// Draw a copy-mode cursor as a hollow cell, in the palette's cursor
    /// colour, so it is distinct from the shell's own caret.
    pub fn with_copy_cursor(mut self, cell: Option<(usize, usize)>) -> Self {
        self.copy_cursor = cell;
        self
    }

    /// Draw the terminal's images in the cells they were placed on. Each
    /// placement is a textured quad from the renderer's existing image
    /// pipeline; the image scrolls with the rows because its row was already
    /// mapped through the viewport's scroll offset.
    pub fn with_images(mut self, images: Vec<ImagePlacement>) -> Self {
        self.images = images;
        self
    }

    /// Namespace the renderer's per-source texture cache. Two panes draw the
    /// same kitty image id independently, so their textures must not collide.
    pub fn with_image_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.image_namespace = namespace.into();
        self
    }

    /// The font cell in device pixels, so an image that named no `c`/`r` span
    /// is drawn at its natural size instead of one point per pixel.
    pub fn with_cell_pixels(mut self, width: f32, height: f32) -> Self {
        if width > 0.0 && height > 0.0 {
            self.cell_pixels = (width, height);
        }
        self
    }

    /// The painted content origin, after the bottom anchor has moved it.
    pub fn content_origin(&self) -> Option<Vector2F> {
        self.origin.map(|point| point.xy())
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
    /// is being matched against. The origin is the *painted* origin, so a
    /// bottom-anchored screen maps to the cells the user sees.
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

    /// How many rows the content is pushed down so its last row sits on the
    /// box's last row: a short history hugs the input line below the pane
    /// instead of floating at the top of it. Zero once the screen is full.
    ///
    /// The cursor counts as content even on a blank row — a shell sits on the
    /// row after its last output, and that row must stay visible.
    fn bottom_offset_rows(&self, box_rows: usize) -> usize {
        let Some(last) = self
            .rows
            .iter()
            .rposition(|row| !row.is_blank())
            .map(|last| last.max(self.cursor.line))
        else {
            return 0;
        };
        box_rows.saturating_sub(last + 1)
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

    /// Fill the cells of each range, one run per row, clipped to the grid.
    fn paint_ranges(
        &self,
        renderer: &mut Renderer,
        origin: Vector2F,
        ranges: &[GridRange],
        color: (u8, u8, u8),
    ) {
        let column_pitch = Self::column_pitch(self.font_size, FontWeight::Regular);
        let row_pitch = Self::row_pitch(self.font_size, self.line_height);
        for range in ranges {
            let (start_row, start_column) = range.start;
            let (end_row, end_column) = range.end;
            if start_row > end_row || start_row >= self.rows.len() {
                continue;
            }
            let end_row = end_row.min(self.rows.len() - 1);
            for row in start_row..=end_row {
                let columns = self.rows[row].cells.len();
                if columns == 0 {
                    continue;
                }
                // A range spanning whole rows starts at the left edge of the
                // rows after the first and ends at the right edge before the
                // last.
                let from = if row == start_row {
                    start_column
                } else {
                    0
                };
                let to = if row == end_row {
                    end_column.min(columns - 1)
                } else {
                    columns - 1
                };
                if from > to {
                    continue;
                }
                let (x, y) = self.origin_of(origin, row, from);
                renderer.fill_rect(
                    rectf(
                        x,
                        y,
                        (to - from + 1) as f32 * column_pitch,
                        row_pitch,
                    ),
                    self.color(color),
                );
            }
        }
    }

    /// Selections and search matches, drawn over the cell backgrounds and
    /// under the glyphs.
    fn paint_overlays(&self, renderer: &mut Renderer, origin: Vector2F) {
        for (index, range) in self.highlights.iter().enumerate() {
            let color = if Some(index) == self.active_highlight {
                self.palette.cursor
            } else {
                self.palette.ansi[8]
            };
            self.paint_ranges(renderer, origin, std::slice::from_ref(range), color);
        }
        // A selection sits on top of a match, so the range the user chose is
        // still readable under the find bar.
        let band = self
            .selection_color
            .map(|color| (color.r, color.g, color.b))
            .unwrap_or(self.palette.ansi[4]);
        self.paint_ranges(renderer, origin, &self.selection, band);
    }

    /// Images as one textured quad each, anchored to the cursor cell they were
    /// placed on. A placed image is painted over the cell backgrounds and under
    /// the glyphs, so program output stays legible on top of it.
    fn paint_images(&self, renderer: &mut Renderer, origin: Vector2F) {
        let column_pitch = Self::column_pitch(self.font_size, FontWeight::Regular);
        let row_pitch = Self::row_pitch(self.font_size, self.line_height);
        for image in &self.images {
            if image.width == 0 || image.height == 0 {
                continue;
            }
            let x = origin.x + image.column as f32 * column_pitch;
            let y = origin.y + image.row as f32 * row_pitch;
            let width = match image.columns {
                Some(columns) => columns as f32 * column_pitch,
                None => image.width as f32 * column_pitch / self.cell_pixels.0,
            };
            let height = match image.rows {
                Some(rows) => rows as f32 * row_pitch,
                None => image.height as f32 * row_pitch / self.cell_pixels.1,
            };
            if width <= 0.0 || height <= 0.0 {
                continue;
            }
            let source = format!("{}-image-{}", self.image_namespace, image.id);
            renderer.draw_image(
                rectf(x, y, width, height),
                source,
                image.width,
                image.height,
                image.sequence,
                std::sync::Arc::clone(&image.rgba),
            );
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
                    cell.attrs.italic,
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

        if let Some((row, column)) = self.copy_cursor {
            if row < self.rows.len() && column < self.rows[row].cells.len() {
                let (x, top) = self.origin_of(origin, row, column);
                renderer.stroke_rect(
                    rectf(x, top, column_pitch, row_pitch),
                    self.color(self.palette.cursor),
                    RULE_THICKNESS + 0.5,
                    0.0,
                );
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
        let Some(size) = self.size else {
            return;
        };
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };

        // The rows are drawn from their own origin, which a bottom-anchored
        // screen pushes down to the bottom of the box.
        let row_pitch = Self::row_pitch(self.font_size, self.line_height);
        let box_rows = (size.y / row_pitch).floor().max(0.0) as usize;
        let offset = if self.bottom_anchor {
            self.bottom_offset_rows(box_rows) as f32 * row_pitch
        } else {
            0.0
        };
        let content_origin = Vector2F::new(origin.x, origin.y + offset);
        self.origin = Some(Point::from_vec2f(content_origin, Default::default()));

        let area = rectf(origin.x, origin.y, size.x, size.y);
        renderer.fill_rect(area, self.color(self.palette.background));
        renderer.clip_rect(area);
        self.paint_backgrounds(renderer, content_origin);
        self.paint_overlays(renderer, content_origin);
        self.paint_images(renderer, content_origin);
        self.paint_glyphs(renderer, content_origin);
        self.paint_rules(renderer, content_origin);
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
    fn a_terminal_image_is_a_textured_quad_on_its_cursor_cell() {
        let mut screen = Screen::new(
            ScreenSize::new(10, 2),
            goble_terminal::ScreenConfig::scrolling(0),
        );
        screen.feed(b"ab");
        // 1x1 red RGBA, placed two cells in, spanning two cells by one row.
        let pixels = [255u8, 0, 0, 255];
        screen.feed(b"\x1b_Ga=T,f=32,s=1,v=1,i=12,c=2,r=1;/wAA/w==\x1b\\");
        let images = screen.images();
        assert_eq!(images.len(), 1);

        let mut grid = TerminalGrid::new(screen.lines(), screen.cursor())
            .with_images(images)
            .with_image_namespace("pane-7");
        let commands = painted(&mut grid);
        let (rect, source, width, height, data) = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawImage {
                    rect,
                    source,
                    width,
                    height,
                    data,
                    ..
                } => Some((*rect, source.clone(), *width, *height, data.clone())),
                _ => None,
            })
            .expect("the image is drawn as a textured quad");
        assert_eq!(source, "pane-7-image-12");
        assert_eq!((width, height), (1, 1));
        assert_eq!(&*data, &pixels);
        let pitch = TerminalGrid::column_pitch(DEFAULT_FONT_SIZE, FontWeight::Regular);
        assert!((rect.origin.x - pitch * 2.0).abs() < 0.01, "{rect:?}");
        assert!((rect.size.width - pitch * 2.0).abs() < 0.01, "{rect:?}");
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
    fn a_bottom_anchored_screen_hugs_the_box_bottom() {
        let row_pitch = TerminalGrid::row_pitch(DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT);
        let mut grid = grid_from(b"abc", 10, 3).with_bottom_anchor(true);
        let commands = painted(&mut grid);
        // One used row in a box several rows tall: the row is pushed to the
        // bottom of the box, so its origin is the last row's, not the first's.
        let expected = (100.0 / row_pitch).floor() * row_pitch - row_pitch;
        let glyph = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { origin, text, .. } if text == "a" => Some(origin.y),
                _ => None,
            })
            .expect("the glyph");
        assert!(
            (glyph - expected).abs() < 0.01,
            "glyph at {glyph}, expected {expected}"
        );
        // Points follow the content, not the box: the empty band above it is
        // not a cell, and the content's own first row is.
        assert_eq!(grid.cell_at(Vector2F::new(0.5, 0.5)), None);
        assert_eq!(grid.cell_at(Vector2F::new(0.5, glyph + 0.5)), Some((0, 0)));
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

    /// A grid built outside the app theme falls back to the palette's blue for
    /// its selection; the find bar's active match keeps the cursor colour.
    #[test]
    fn a_selection_and_a_search_match_are_painted_in_the_palette() {
        let column = TerminalGrid::column_pitch(DEFAULT_FONT_SIZE, FontWeight::Regular);
        let palette = Palette::xterm();
        let mut grid = grid_from(b"hello world", 20, 1)
            .with_cursor_visible(false)
            .with_selection(vec![GridRange::new((0, 0), (0, 4))])
            .with_highlights(vec![GridRange::new((0, 6), (0, 10))], Some(0));
        let commands = painted(&mut grid);
        let rects = rects(&commands);
        assert!(
            rects.iter().any(|(_, _, width, _, color)| {
                *color == ColorU::new(palette.ansi[4].0, palette.ansi[4].1, palette.ansi[4].2, 255)
                    && (*width - column * 5.0).abs() < 0.01
            }),
            "the selection is five cells in the palette's blue: {rects:?}"
        );
        assert!(
            rects.iter().any(|(_, _, width, _, color)| {
                *color == ColorU::new(
                    palette.cursor.0,
                    palette.cursor.1,
                    palette.cursor.2,
                    255,
                ) && (*width - column * 5.0).abs() < 0.01
            }),
            "the active match is five cells in the accent: {rects:?}"
        );
    }

    /// A grid the app builds is handed the theme's selection band, so a pane's
    /// selection is the same desaturated accent as the block and transcript
    /// surfaces instead of the palette's blue.
    #[test]
    fn a_configured_selection_colour_replaces_the_palette_blue() {
        let column = TerminalGrid::column_pitch(DEFAULT_FONT_SIZE, FontWeight::Regular);
        let palette = Palette::xterm();
        let band = ColorU::new(0x4d, 0x4d, 0x4d, 255);
        let mut grid = grid_from(b"hello", 20, 1)
            .with_cursor_visible(false)
            .with_selection(vec![GridRange::new((0, 0), (0, 4))])
            .with_selection_color(band);
        let commands = painted(&mut grid);
        let rects = rects(&commands);
        assert!(
            rects.iter().any(|(_, _, width, _, color)| {
                *color == band && (*width - column * 5.0).abs() < 0.01
            }),
            "the selection is five cells in the configured colour: {rects:?}"
        );
        assert!(
            !rects.iter().any(|(_, _, _, _, color)| {
                *color == ColorU::new(palette.ansi[4].0, palette.ansi[4].1, palette.ansi[4].2, 255)
            }),
            "the palette's blue is not painted once a colour is configured: {rects:?}"
        );
    }

    #[test]
    fn a_copy_mode_cursor_is_a_hollow_cell() {
        let mut grid = grid_from(b"hi", 4, 1)
            .with_cursor_visible(false)
            .with_copy_cursor(Some((0, 1)));
        let commands = painted(&mut grid);
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, RenderCommand::StrokeRect { .. })),
            "the copy cursor is outlined: {commands:?}"
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
