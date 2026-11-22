//! The terminal cell-grid surface: alacritty runs painted straight into the
//! frame.
//!
//! The pane's content is an [`Element`] only because the window's root is one:
//! it is a single element that owns one [`GridFrame`], not a widget tree with a
//! goble-ui element per cell. Painting walks the frame's background runs,
//! glyphs and rules and writes render commands through the [`PaintContext`]'s
//! renderer, in the same batched order a terminal wants: every background rect
//! first, then every glyph, then the rules and the cursor.
//!
//! The frame comes from [`goble_terminal::Screen::frame`], which reads the
//! vendored `alacritty_terminal` grid directly. This module knows nothing about
//! alacritty and nothing about the block view: it is how a live screen is
//! painted in the bottom dock, the side panel's Terminal tab, and the main
//! pane whenever copy mode, find or the alternate screen needs cells.

use std::cell::RefCell;
use std::rc::Rc;

use goble_terminal::{CursorShape, GridFrame, ImagePlacement, Palette, Underline};
use goble_ui::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use goble_ui::geometry::{rectf, Vector2F};
use goble_ui::platform::text_atlas::{mono_advance, FontWeight};
use goble_ui::theme::FontFamily;
use goble_ui::ColorU;

use super::{FONT_SIZE, LINE_HEIGHT};

/// Thickness of an underline or a strikeout.
const RULE_THICKNESS: f32 = 1.0;
/// Thickness of a beam cursor.
const BEAM_THICKNESS: f32 = 2.0;
/// Bold cells switch to the bold face, the only other weight the bundled
/// monospace family carries.
const BOLD_WEIGHT: FontWeight = FontWeight::Bold;

/// The horizontal pitch of one cell, in points.
pub(super) fn cell_width() -> f32 {
    mono_advance(FONT_SIZE, FontWeight::Regular)
}

/// The vertical pitch of one row, in points.
pub(super) fn row_pitch() -> f32 {
    FONT_SIZE * LINE_HEIGHT
}

/// An inclusive span of grid cells on one row.
///
/// A selection or a search match that covers several rows is several spans,
/// one per row, which is how the painter fills them without a per-cell rect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CellSpan {
    pub(super) start: (usize, usize),
    pub(super) end: (usize, usize),
}

impl CellSpan {
    pub(super) const fn new(start: (usize, usize), end: (usize, usize)) -> Self {
        Self { start, end }
    }
}

/// The painted grid's box, in window coordinates.
#[derive(Debug, Clone, Copy)]
pub(super) struct GridGeometry {
    pub(super) origin: Vector2F,
    pub(super) columns: usize,
    pub(super) rows: usize,
}

impl GridGeometry {
    /// The cell a point falls in, or `None` when it is outside the grid.
    pub(super) fn cell_at(&self, position: Vector2F) -> Option<(usize, usize)> {
        let column_pitch = cell_width();
        let row_pitch = row_pitch();
        let x = position.x - self.origin.x;
        let y = position.y - self.origin.y;
        if x < 0.0 || y < 0.0 || column_pitch <= 0.0 || row_pitch <= 0.0 {
            return None;
        }
        let column = (x / column_pitch) as usize;
        let row = (y / row_pitch) as usize;
        (column < self.columns && row < self.rows).then_some((row, column))
    }
}

/// A live terminal viewport painted from [`GridFrame`] runs.
pub(super) struct GridSurface {
    frame: GridFrame,
    palette: Palette,
    font_size: f32,
    line_height: f32,
    show_cursor: bool,
    /// Whether the rows hug the bottom of the box. A shell's history grows
    /// upwards (copy mode over a prompt), while a full-screen program owns its
    /// own layout and stays where it painted.
    bottom_anchor: bool,
    selection: Vec<CellSpan>,
    selection_color: Option<ColorU>,
    highlights: Vec<CellSpan>,
    active_highlight: Option<usize>,
    copy_cursor: Option<(usize, usize)>,
    images: Vec<ImagePlacement>,
    image_namespace: String,
    cell_pixels: (f32, f32),
    size: Option<Vector2F>,
    origin: Option<Point>,
    /// Where the grid ended up, written as it paints so a pointer event maps to
    /// the cell the user is actually over.
    geometry: Option<Rc<RefCell<Option<GridGeometry>>>>,
}

impl GridSurface {
    pub(super) fn new(frame: GridFrame) -> Self {
        Self {
            frame,
            palette: Palette::xterm(),
            font_size: FONT_SIZE,
            line_height: LINE_HEIGHT,
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
            size: None,
            origin: None,
            geometry: None,
        }
    }

    pub(super) fn with_palette(mut self, palette: Palette) -> Self {
        self.palette = palette;
        self
    }

    pub(super) fn with_font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }

    pub(super) fn with_line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    pub(super) fn with_cursor_visible(mut self, visible: bool) -> Self {
        self.show_cursor = visible;
        self
    }

    pub(super) fn with_bottom_anchor(mut self, anchored: bool) -> Self {
        self.bottom_anchor = anchored;
        self
    }

    pub(super) fn with_selection(mut self, selection: Vec<CellSpan>) -> Self {
        self.selection = selection;
        self
    }

    pub(super) fn with_selection_color(mut self, color: ColorU) -> Self {
        self.selection_color = Some(color);
        self
    }

    pub(super) fn with_highlights(
        mut self,
        highlights: Vec<CellSpan>,
        active: Option<usize>,
    ) -> Self {
        self.highlights = highlights;
        self.active_highlight = active;
        self
    }

    pub(super) fn with_copy_cursor(mut self, cell: Option<(usize, usize)>) -> Self {
        self.copy_cursor = cell;
        self
    }

    pub(super) fn with_images(mut self, images: Vec<ImagePlacement>) -> Self {
        self.images = images;
        self
    }

    pub(super) fn with_image_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.image_namespace = namespace.into();
        self
    }

    pub(super) fn with_cell_pixels(mut self, width: f32, height: f32) -> Self {
        if width > 0.0 && height > 0.0 {
            self.cell_pixels = (width, height);
        }
        self
    }

    pub(super) fn with_geometry(mut self, geometry: Rc<RefCell<Option<GridGeometry>>>) -> Self {
        self.geometry = Some(geometry);
        self
    }

    fn color(&self, rgb: (u8, u8, u8)) -> ColorU {
        ColorU::new(rgb.0, rgb.1, rgb.2, 255)
    }

    fn cursor_fills_cell(&self, row: usize, column: usize) -> bool {
        let cursor = self.frame.cursor;
        self.show_cursor
            && cursor.visible
            && cursor.line == row
            && cursor.column == column
            && cursor.shape == CursorShape::Block
    }

    /// How many rows the content is pushed down so its last row sits on the
    /// box's last row. The cursor counts as content: a shell sits on the row
    /// after its last output, and that row must stay visible.
    fn bottom_offset_rows(&self, box_rows: usize) -> usize {
        let Some(last) = self
            .frame
            .rows
            .iter()
            .rposition(|row| !row.is_blank())
            .map(|last| last.max(self.frame.cursor.line))
        else {
            return 0;
        };
        box_rows.saturating_sub(last + 1)
    }

    fn origin_of(&self, origin: Vector2F, row: usize, column: usize) -> (f32, f32) {
        let column_pitch = cell_width();
        let pitch = self.font_size * self.line_height;
        (
            origin.x + column as f32 * column_pitch,
            origin.y + row as f32 * pitch,
        )
    }

    /// Backgrounds, one rect per contiguous run, then the block cursor's cell.
    fn paint_backgrounds(&self, renderer: &mut goble_ui::render::Renderer, origin: Vector2F) {
        let row_pitch = self.font_size * self.line_height;
        let column_pitch = cell_width();
        let background = self.palette.background;

        for (row_index, row) in self.frame.rows.iter().enumerate() {
            let top = origin.y + row_index as f32 * row_pitch;
            for run in &row.backgrounds {
                let color = self.palette.rgb(run.color);
                // The pane background is already painted underneath, so a
                // default-coloured run costs nothing.
                if color == background {
                    continue;
                }
                renderer.fill_rect(
                    rectf(
                        origin.x + run.start as f32 * column_pitch,
                        top,
                        run.len as f32 * column_pitch,
                        row_pitch,
                    ),
                    self.color(color),
                );
            }
            if self.cursor_fills_cell(row_index, self.frame.cursor.column) {
                renderer.fill_rect(
                    rectf(
                        origin.x + self.frame.cursor.column as f32 * column_pitch,
                        top,
                        column_pitch,
                        row_pitch,
                    ),
                    self.color(self.palette.cursor),
                );
            }
        }
    }

    /// Fill the cells of each span, one rect per row, clipped to the grid.
    fn paint_spans(
        &self,
        renderer: &mut goble_ui::render::Renderer,
        origin: Vector2F,
        spans: &[CellSpan],
        color: (u8, u8, u8),
    ) {
        let column_pitch = cell_width();
        let pitch = self.font_size * self.line_height;
        for span in spans {
            let (start_row, start_column) = span.start;
            let (end_row, end_column) = span.end;
            if start_row > end_row || start_row >= self.frame.rows.len() {
                continue;
            }
            let end_row = end_row.min(self.frame.rows.len() - 1);
            for row in start_row..=end_row {
                let columns = self.frame.columns;
                if columns == 0 {
                    continue;
                }
                // A span covering whole rows starts at the left edge after the
                // first and ends at the right edge before the last.
                let from = if row == start_row { start_column } else { 0 };
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
                    rectf(x, y, (to - from + 1) as f32 * column_pitch, pitch),
                    self.color(color),
                );
            }
        }
    }

    fn paint_overlays(&self, renderer: &mut goble_ui::render::Renderer, origin: Vector2F) {
        for (index, span) in self.highlights.iter().enumerate() {
            let color = if Some(index) == self.active_highlight {
                self.palette.cursor
            } else {
                self.palette.ansi[8]
            };
            self.paint_spans(renderer, origin, std::slice::from_ref(span), color);
        }
        // The selection sits over a match, so what the user chose stays
        // readable under the find bar.
        let band = self
            .selection_color
            .map(|color| (color.r, color.g, color.b))
            .unwrap_or(self.palette.ansi[4]);
        self.paint_spans(renderer, origin, &self.selection, band);
    }

    /// Images, one textured quad each, anchored to the cell they were placed
    /// on and painted over the backgrounds and under the glyphs.
    fn paint_images(&self, renderer: &mut goble_ui::render::Renderer, origin: Vector2F) {
        let column_pitch = cell_width();
        let pitch = self.font_size * self.line_height;
        for image in &self.images {
            if image.width == 0 || image.height == 0 {
                continue;
            }
            let x = origin.x + image.column as f32 * column_pitch;
            let y = origin.y + image.row as f32 * pitch;
            let width = match image.columns {
                Some(columns) => columns as f32 * column_pitch,
                None => image.width as f32 * column_pitch / self.cell_pixels.0,
            };
            let height = match image.rows {
                Some(rows) => rows as f32 * pitch,
                None => image.height as f32 * pitch / self.cell_pixels.1,
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

    /// One glyph command per non-blank cell. Per-cell glyphs are deliberate:
    /// the atlas caches one entry per string, so merging a changing line into
    /// whole-row strings would fill it every frame.
    fn paint_glyphs(&self, renderer: &mut goble_ui::render::Renderer, origin: Vector2F) {
        // A cell's ink can be a shade wider than its advance; the wrap width
        // only stops a single character breaking onto another line.
        let max_width = cell_width() * 2.0;

        for (row_index, row) in self.frame.rows.iter().enumerate() {
            for glyph in &row.glyphs {
                let text = glyph.text();
                if text.is_empty() {
                    continue;
                }
                let (mut foreground, background) = self.palette.cell_colors(&glyph.cell);
                if self.cursor_fills_cell(row_index, glyph.column) {
                    // The cursor repaints the cell, so its text is drawn in the
                    // colour the cursor did not paint with.
                    foreground = background;
                }
                let weight = if glyph.cell.attrs.bold {
                    BOLD_WEIGHT
                } else {
                    FontWeight::Regular
                };
                let (x, y) = self.origin_of(origin, row_index, glyph.column);
                renderer.draw_text_with_font(
                    Vector2F::new(x, y),
                    text,
                    self.font_size,
                    self.color(foreground),
                    max_width,
                    self.line_height,
                    weight,
                    FontFamily::Mono,
                    glyph.cell.attrs.italic,
                );
            }
        }
    }

    /// Underlines, strikeouts and the cursors that are not a filled block.
    fn paint_rules(&self, renderer: &mut goble_ui::render::Renderer, origin: Vector2F) {
        let column_pitch = cell_width();
        let pitch = self.font_size * self.line_height;

        for (row_index, row) in self.frame.rows.iter().enumerate() {
            for rule in &row.rules {
                let (foreground, _) = self.palette.cell_colors(&rule.cell);
                let (x, top) = self.origin_of(origin, row_index, rule.column);
                if rule.cell.attrs.underline != Underline::None {
                    let color = rule
                        .cell
                        .attrs
                        .underline_color
                        .map(|color| self.palette.rgb(color))
                        .unwrap_or(foreground);
                    let thickness = if rule.cell.attrs.underline == Underline::Double {
                        RULE_THICKNESS * 2.0
                    } else {
                        RULE_THICKNESS
                    };
                    renderer.fill_rect(
                        rectf(x, top + pitch - thickness, column_pitch, thickness),
                        self.color(color),
                    );
                }
                if rule.cell.attrs.strikeout {
                    renderer.fill_rect(
                        rectf(x, top + pitch * 0.55, column_pitch, RULE_THICKNESS),
                        self.color(foreground),
                    );
                }
            }
        }

        self.paint_cursor(renderer, origin);

        if let Some((row, column)) = self.copy_cursor {
            if row < self.frame.rows.len() && column < self.frame.columns {
                let (x, top) = self.origin_of(origin, row, column);
                renderer.stroke_rect(
                    rectf(x, top, column_pitch, pitch),
                    self.color(self.palette.cursor),
                    RULE_THICKNESS + 0.5,
                    0.0,
                );
            }
        }
    }

    /// The cursor's non-block shapes. A block cursor is painted under the
    /// glyphs instead, so its character stays legible.
    fn paint_cursor(&self, renderer: &mut goble_ui::render::Renderer, origin: Vector2F) {
        let cursor = self.frame.cursor;
        if !self.show_cursor || !cursor.visible || cursor.line >= self.frame.rows.len() {
            return;
        }
        if cursor.column >= self.frame.columns {
            return;
        }
        let column_pitch = cell_width();
        let pitch = self.font_size * self.line_height;
        let (x, top) = self.origin_of(origin, cursor.line, cursor.column);
        match cursor.shape {
            CursorShape::Beam => renderer.fill_rect(
                rectf(x, top, BEAM_THICKNESS, pitch),
                self.color(self.palette.cursor),
            ),
            CursorShape::Underline => renderer.fill_rect(
                rectf(
                    x,
                    top + pitch - BEAM_THICKNESS,
                    column_pitch,
                    BEAM_THICKNESS,
                ),
                self.color(self.palette.cursor),
            ),
            CursorShape::HollowBlock => renderer.stroke_rect(
                rectf(x, top, column_pitch, pitch),
                self.color(self.palette.cursor),
                RULE_THICKNESS,
                0.0,
            ),
            CursorShape::Block | CursorShape::Hidden => {}
        }
    }
}

impl Element for GridSurface {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        // The grid fills its pane: the background reaches the edges even where
        // the last row is short.
        let size = Vector2F::new(
            constraint.max.x.max(constraint.min.x),
            constraint.max.y.max(constraint.min.y),
        );
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
        // surface pushes down to the bottom of the box.
        let pitch = self.font_size * self.line_height;
        let box_rows = (size.y / pitch).floor().max(0.0) as usize;
        let offset = if self.bottom_anchor {
            self.bottom_offset_rows(box_rows) as f32 * pitch
        } else {
            0.0
        };
        let content_origin = Vector2F::new(origin.x, origin.y + offset);
        self.origin = Some(Point::from_vec2f(content_origin, Default::default()));
        if let Some(geometry) = &self.geometry {
            *geometry.borrow_mut() = Some(GridGeometry {
                origin: content_origin,
                columns: self.frame.columns,
                rows: self.frame.rows.len(),
            });
        }

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
