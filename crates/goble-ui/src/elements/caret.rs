use crate::color::ColorU;
use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{rectf, Vector2F};
use crate::theme::ColorToken;

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
/// The element takes no room in the row and rasterizes no text. It reports no
/// advance at all, so the line's own columns do not depend on where the caret
/// is, and it paints a single rectangle: a block fills the cell of the
/// character the insertion point sits before, an underline draws the rule under
/// that character, and a bar is the field's own beam.
///
/// A block is put into the row *before* the run it sits in, so its fill is
/// behind the text and the character it covers is that run's own glyph, at the
/// position the row already gives it. Redrawing the glyph here — as a run of
/// its own, which is what the caret used to do — placed it a couple of pixels
/// off its slot for every glyph whose ink overhangs its cell (a `j` in the
/// prose face, `A`, `x`, `p`, `#` and others in the oblique faces): a fresh
/// run is anchored on its own ink, while the same glyph inside the line is
/// anchored on the line's, so the letter under the caret jumped as the caret
/// crossed it. Painting the fill behind the text keeps one rasterization per
/// glyph and the row's own positions, which is the only way the text can be
/// perfectly still.
pub fn caret_overlay(
    app: &AppContext,
    shape: CaretShape,
    x: f32,
    cell: f32,
    height: f32,
) -> Box<dyn Element> {
    CaretOverlay {
        shape,
        x: x.max(0.0),
        cell: cell.max(0.0),
        height,
        color: app.theme.color(ColorToken::Focus),
        origin: None,
    }
    .finish()
}

struct CaretOverlay {
    shape: CaretShape,
    /// The cell's left edge, past the element's own origin. The element owns no
    /// advance, so this offset is not part of what the row lays out.
    x: f32,
    cell: f32,
    height: f32,
    color: ColorU,
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
        match self.shape {
            CaretShape::Bar => {
                renderer.fill_rect(rectf(x, origin.y, CARET_WIDTH, self.height), self.color);
            }
            CaretShape::Block => {
                renderer.fill_rect(rectf(x, origin.y, self.cell, self.height), self.color);
            }
            CaretShape::Underline => {
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
