use crate::color::ColorU;
use crate::elements::{
    AppContext, ConstrainedBox, Container, Element, Empty, Fill, Flex, LayoutContext,
    PaintContext, Point, SizeConstraint, Text,
};
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::platform::text_atlas::{measure_text_family, FontWeight};
use crate::theme::{ColorToken, FontFamily};

/// Height of the caret beam: one line box of the 12px body text.
pub const CARET_HEIGHT: f32 = 16.0;

/// Width of the caret beam. Deliberately wider than the terminal grid's own
/// caret: this is the pane's insertion point for the text the user is writing,
/// and it has to be legible at a glance against the rich-input bar. The beam is
/// painted past the insertion point rather than laid out between two glyphs
/// (see [`CaretBeam`]), so a wider one reaches further over the character after
/// it instead of pushing that character across.
pub const CARET_WIDTH: f32 = 4.0;

/// Width of a block caret's cell when there is no character to cover: the end
/// of the line, or an empty field. A block covers one character, and with a
/// proportional font the cell is the width that character itself measures.
pub const CARET_CELL_WIDTH: f32 = 8.0;

/// The size the field's own text is drawn at, which a caret cell is measured
/// at: the caret is one cell of that text.
const CARET_FONT_SIZE: f32 = 12.0;
const CARET_LINE_HEIGHT: f32 = 1.2;

/// A probe glyph wide enough that a run measured with it spans the pen.
const CELL_PROBE: &str = "W";

/// Thickness of the underline caret drawn under the character it sits on.
const CARET_UNDERLINE_HEIGHT: f32 = 2.0;

/// Which of vim's cursor shapes the editor draws. Insert mode draws a bar
/// (the rich input's own beam); normal and visual mode draw a block over the
/// character under the caret; replace mode draws an underline, as vim does.
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
    caret(app, CaretShape::Bar, None)
}

/// The same beam at a line height of the caller's own: a multi-line buffer's
/// rows are not the rich input's single line box. The bar takes no horizontal
/// advance at any height, so a row's text never moves as the caret moves.
pub fn caret_bar(app: &AppContext, height: f32) -> Box<dyn Element> {
    CaretBeam::new(CARET_WIDTH, height, app.theme.color(ColorToken::Focus)).finish()
}

/// Whether the caret draws the character it covers itself, and therefore takes
/// that character's own advance in the row it sits in: the text after a block
/// or an underline caret starts past the character it covers, so the character
/// is neither drawn twice nor pushed along by the caret. A beam covers nothing
/// and leaves the text to the row.
pub fn caret_covers_character(shape: CaretShape, under: Option<char>) -> bool {
    under.is_some() && shape != CaretShape::Bar
}

/// A character's own advance: where the next character starts after it.
///
/// A run reports the extent of its ink, which is a side bearing short of the
/// advance — and nothing at all for a space — so a cell that has to cover the
/// character it sits on, and leave the row where the text's own columns are,
/// measures the advance itself: the distance the probe glyph moves when it is
/// drawn after `text`.
fn advance(text: &str) -> f32 {
    let measured = |value: &str| {
        measure_text_family(
            value,
            CARET_FONT_SIZE,
            CARET_LINE_HEIGHT,
            f32::INFINITY,
            FontWeight::Regular,
            FontFamily::System,
            false,
        )
        .x
    };
    (measured(&format!("{text}{CELL_PROBE}")) - measured(CELL_PROBE)).max(0.0)
}

/// The width of the cell a block or an underline caret takes: the character's
/// own advance when there is one, and a fixed cell at the end of a line.
fn cell_width(under: Option<char>) -> f32 {
    match under {
        Some(ch) => advance(&ch.to_string()),
        None => CARET_CELL_WIDTH,
    }
}

/// The insertion point, drawn in the shape the editor's mode calls for.
///
/// `under` is the character the caret covers: a block caret paints it in the
/// background colour so it stays readable inside the caret cell, and an
/// underline caret gives it a slot to sit under. Both take the character's own
/// advance — what the row would have spent on it — so the text does not move
/// when the mode changes; both are ignored by the bar.
pub fn caret(app: &AppContext, shape: CaretShape, under: Option<char>) -> Box<dyn Element> {
    let color = app.theme.color(ColorToken::Focus);
    let focus = Fill::Solid(color);
    match shape {
        // The beam is painted where the text is, over the character it reaches:
        // it asks the row for no room at all, so the text never moves as the
        // caret moves. The renderer draws a run's rectangles before its glyphs,
        // so the character the beam covers stays drawn on the focus fill.
        CaretShape::Bar => CaretBeam::new(CARET_WIDTH, CARET_HEIGHT, color).finish(),
        CaretShape::Block => {
            // The cell is the character's own, so the letter under the caret
            // neither moves nor disappears: the cell takes the advance the row
            // would have spent on it, and the letter is redrawn in the cell's
            // background colour inside the fill. A space has no ink to draw but
            // keeps its advance, which is the cell the block covers.
            let cell: Box<dyn Element> = match under {
                Some(ch) => ConstrainedBox::new(
                    Text::new(ch.to_string())
                        .with_theme_color(ColorToken::Bg, app)
                        .finish(),
                )
                .with_min_width(cell_width(under))
                .finish(),
                None => Empty::new()
                    .with_size(vec2f(CARET_CELL_WIDTH, CARET_HEIGHT))
                    .finish(),
            };
            Container::new(cell).with_background(focus).finish()
        }
        CaretShape::Underline => {
            // The character keeps its own line box and its own colour; the caret
            // is a bar under it, so the text does not shift when the mode
            // changes.
            let slot: Box<dyn Element> = match under {
                Some(ch) => ConstrainedBox::new(
                    Text::new(ch.to_string())
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .with_min_width(cell_width(under))
                .finish(),
                None => Empty::new()
                    .with_size(vec2f(CARET_CELL_WIDTH, CARET_HEIGHT - CARET_UNDERLINE_HEIGHT))
                    .finish(),
            };
            let bar = Container::new(
                Empty::new()
                    .with_size(vec2f(cell_width(under), CARET_UNDERLINE_HEIGHT))
                    .finish(),
            )
            .with_background(focus)
            .finish();
            Flex::column()
                .with_child(slot)
                .with_child(bar)
                .finish()
        }
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
