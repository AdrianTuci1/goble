use crate::color::ColorU;
use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{rectf, Vector2F};
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, FontFamily};

use super::vec2f;

/// Height of the caret: one line box of the 12px body text.
pub const CARET_HEIGHT: f32 = 16.0;

/// Width of the thin caret beam. It is the insertion point of a field that has
/// no character index of its own — a search or a rename — and of an empty
/// rich input, where it leads the placeholder. The beam is painted past the
/// insertion point rather than laid out between two glyphs (see
/// [`CaretBeam`]), so it reaches over the character after it instead of pushing
/// that character across.
pub const CARET_WIDTH: f32 = 4.0;

/// Width of a block caret's cell when there is no character to cover: the end
/// of the line, or an empty field. A block covers one character, and with a
/// proportional font the cell is the width that character itself advances.
pub const CARET_CELL_WIDTH: f32 = 8.0;

/// Thickness of the underline caret drawn under the character it sits on.
const CARET_UNDERLINE_HEIGHT: f32 = 2.0;

/// The line height a block's inverted glyph is rasterized with. A one-glyph run
/// lays its single line on the face's own ascent, which this multiplier does
/// not move — the fontdue baseline of a first line is the line's ascent — so
/// the glyph lands on the pen the row gave it; the multiplier only sizes the
/// blank box the atlas reserves around the ink.
const INK_LINE_HEIGHT: f32 = 1.0;

/// The shape a field's insertion point is drawn in. A block covers the
/// character after the insertion point, which is what the rich input and the
/// document editor draw while typing and in vim's normal and visual modes; an
/// underline marks the character replace mode overwrites; a bar is the thin
/// beam of a field that has no character index of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaretShape {
    #[default]
    Bar,
    Block,
    Underline,
}

/// The character a block caret covers, and the face the row draws it in.
///
/// A block is a terminal-style cursor: the cell is the caret's own colour and
/// the character on it is drawn in the background colour, so the letter reads
/// as inverted rather than shaded. The face travels with the character because
/// the block redraws it as a run of its own, and it has to be the very face the
/// row used — family, size, weight and italic — or the glyph would not land on
/// its own slot.
#[derive(Clone, Copy, Debug)]
pub struct CaretGlyph {
    pub ch: char,
    pub family: FontFamily,
    pub size: f32,
    pub weight: FontWeight,
    pub italic: bool,
}

/// The insertion point of a focused text field.
///
/// The element tree is rebuilt every frame, so the caret is built into the
/// field's row at the index it sits at, instead of being paint-time state.
pub fn caret_beam(app: &AppContext) -> Box<dyn Element> {
    CaretBeam::new(
        CARET_WIDTH,
        CARET_HEIGHT,
        app.theme.color(ColorToken::Focus),
    )
    .finish()
}

/// The same beam at a line height of the caller's own: a multi-line buffer's
/// rows are not the rich input's single line box. The bar takes no horizontal
/// advance at any height, so a row's text never moves as the caret moves.
pub fn caret_bar(app: &AppContext, height: f32) -> Box<dyn Element> {
    CaretBeam::new(CARET_WIDTH, height, app.theme.color(ColorToken::Focus)).finish()
}

/// The insertion point, painted in the shape the editor's mode calls for, one
/// cell at `x` past the element's own origin.
///
/// The element takes no room in the row. It reports no advance at all, so the
/// line's own columns do not depend on where the caret is, and it paints one
/// cell: a block fills the cell of the character the insertion point sits
/// before, an underline draws the rule under that character, and a bar is the
/// field's own beam.
///
/// A block is painted in two halves, because the row's text sits between them.
/// The fill goes into the row *before* the run it covers, so the row's glyphs
/// keep their one rasterization and their own positions; the character under
/// the cell is then redrawn *after* the run, in the background colour, by
/// [`caret_ink`]. That is what makes the block read as a terminal's cursor —
/// the cell is the caret's colour and the letter on it is the background's —
/// instead of a pale highlight behind a letter that keeps its own colour. The
/// redraw is safe because the platform anchors a one-glyph run on its own slot,
/// so the ink lands back on the very cell the line drew it in, and the caret
/// still reports no advance: nothing moves as the caret crosses a letter.
pub fn caret_overlay(
    app: &AppContext,
    shape: CaretShape,
    x: f32,
    cell: f32,
    height: f32,
) -> Box<dyn Element> {
    CaretOverlay {
        shape,
        phase: Phase::Fill,
        x: x.max(0.0),
        cell: cell.max(0.0),
        height,
        color: app.theme.color(ColorToken::Focus),
        ink: app.theme.color(ColorToken::Bg),
        glyph: None,
        origin: None,
    }
    .finish()
}

/// The other half of a block: the character the cell covers, redrawn in the
/// background colour at the very pen the row gave it and in the row's own face.
///
/// The caller puts this element into the row *after* the run the cell covers,
/// so its ink lands over the glyph the fill is behind. It occupies the same
/// cell as the fill and reports no advance, like every other caret half.
pub fn caret_ink(
    app: &AppContext,
    x: f32,
    cell: f32,
    height: f32,
    glyph: CaretGlyph,
) -> Box<dyn Element> {
    CaretOverlay {
        shape: CaretShape::Block,
        phase: Phase::Ink,
        x: x.max(0.0),
        cell: cell.max(0.0),
        height,
        color: app.theme.color(ColorToken::Focus),
        ink: app.theme.color(ColorToken::Bg),
        glyph: Some(glyph),
        origin: None,
    }
    .finish()
}

/// Which half of a block caret an overlay paints. The row's text sits between
/// the two: the fill is drawn before the run, the inverted character after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Fill,
    Ink,
}

struct CaretOverlay {
    shape: CaretShape,
    phase: Phase,
    /// The cell's left edge, past the element's own origin. The element owns no
    /// advance, so this offset is not part of what the row lays out.
    x: f32,
    cell: f32,
    height: f32,
    color: ColorU,
    /// The colour the covered character is redrawn in: the field's own
    /// background, which is what makes the cell read as inverted.
    ink: ColorU,
    /// The character under a block, with the face the row draws it in. `None`
    /// on the fill half, and at the end of a line or on a cell with no glyph of
    /// its own, where there is nothing to invert.
    glyph: Option<CaretGlyph>,
    origin: Option<Point>,
}

impl Element for CaretOverlay {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        // An overlay owns no advance: the row's text keeps its columns.
        vec2f(0.0, self.height)
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let x = origin.x + self.x;
        match (self.shape, self.phase) {
            (CaretShape::Bar, _) => {
                renderer.fill_rect(rectf(x, origin.y, CARET_WIDTH, self.height), self.color);
            }
            (CaretShape::Block, Phase::Fill) => {
                renderer.fill_rect(rectf(x, origin.y, self.cell, self.height), self.color);
            }
            (CaretShape::Block, Phase::Ink) => {
                // The cell is the caret's colour and the character on it is
                // the background's: the glyph keeps the pen the row gave it and
                // only its ink changes colour.
                if let Some(glyph) = self.glyph {
                    let mut buffer = [0u8; 4];
                    renderer.draw_text_with_font(
                        vec2f(x, origin.y),
                        glyph.ch.encode_utf8(&mut buffer).to_string(),
                        glyph.size,
                        self.ink,
                        f32::INFINITY,
                        INK_LINE_HEIGHT,
                        glyph.weight,
                        glyph.family,
                        glyph.italic,
                    );
                }
            }
            (CaretShape::Underline, _) => {
                renderer.fill_rect(
                    rectf(
                        x,
                        origin.y + self.height - CARET_UNDERLINE_HEIGHT,
                        self.cell,
                        CARET_UNDERLINE_HEIGHT,
                    ),
                    self.color,
                );
            }
        }
    }

    fn size(&self) -> Option<Vector2F> {
        Some(vec2f(0.0, self.height))
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

/// A caret that paints itself instead of taking room in the line it sits in.
///
/// A caret laid out between two runs adds its own width to the row: every move
/// of it would push the character after it across, which is the text moving
/// under the cursor. This element reports no advance at all and paints its bar
/// from its own origin rightwards, over the character the row draws next, so
/// the letter the bar reaches keeps its place and its ink.
struct CaretBeam {
    width: f32,
    height: f32,
    color: ColorU,
    origin: Option<Point>,
}

impl CaretBeam {
    fn new(width: f32, height: f32, color: ColorU) -> Self {
        Self {
            width,
            height,
            color,
            origin: None,
        }
    }
}

impl Element for CaretBeam {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        // Zero width: the painted bar is not part of what the row lays out, so
        // the row's own text keeps the positions it has without a caret.
        vec2f(0.0, self.height)
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rect(
                rectf(origin.x, origin.y, self.width, self.height),
                self.color,
            );
        }
    }

    fn size(&self) -> Option<Vector2F> {
        Some(vec2f(0.0, self.height))
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}
