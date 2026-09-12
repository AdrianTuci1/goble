use crate::elements::{AppContext, Container, Element, Empty, Fill, Flex, Text};
use crate::geometry::vec2f;
use crate::theme::ColorToken;

/// Height of the caret beam: one line box of the 12px body text.
pub const CARET_HEIGHT: f32 = 16.0;

/// Width of the caret beam. Deliberately wider than the terminal grid's own
/// caret: this is the pane's insertion point for the text the user is writing,
/// and it has to be legible at a glance against the rich-input bar.
pub const CARET_WIDTH: f32 = 3.0;

/// Width of a block caret's cell. A block cursor covers one character; with a
/// proportional font that is the width the character itself measures, and the
/// constant covers the characters that measure nothing (a space, or the end of
/// the line).
pub const CARET_CELL_WIDTH: f32 = 8.0;

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

/// The insertion point, drawn in the shape the editor's mode calls for.
///
/// `under` is the character the caret covers: a block caret paints it in the
/// background colour so it stays readable inside the accent cell, and an
/// underline caret gives it a slot to sit under. Both are ignored by the bar.
pub fn caret(app: &AppContext, shape: CaretShape, under: Option<char>) -> Box<dyn Element> {
    let accent = Fill::Solid(app.theme.color(ColorToken::Accent));
    match shape {
        CaretShape::Bar => Container::new(
            Empty::new()
                .with_size(vec2f(CARET_WIDTH, CARET_HEIGHT))
                .finish(),
        )
        .with_background(accent)
        .finish(),
        CaretShape::Block => {
            // A blank cell (a space, or the end of the line) still has to be
            // visible, so it is drawn at the cell's own width.
            let cell: Box<dyn Element> = match under.filter(|c| !c.is_whitespace()) {
                Some(ch) => Text::new(ch.to_string())
                    .with_theme_color(ColorToken::Bg, app)
                    .finish(),
                None => Empty::new()
                    .with_size(vec2f(CARET_CELL_WIDTH, CARET_HEIGHT))
                    .finish(),
            };
            Container::new(cell).with_background(accent).finish()
        }
        CaretShape::Underline => {
            // The character keeps its own line box; the accent is a bar under
            // it, so the text does not shift when the mode changes.
            let slot: Box<dyn Element> = match under {
                Some(ch) if !ch.is_whitespace() => Text::new(ch.to_string())
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
                _ => Empty::new()
                    .with_size(vec2f(CARET_CELL_WIDTH, CARET_HEIGHT - CARET_UNDERLINE_HEIGHT))
                    .finish(),
            };
            let bar = Container::new(
                Empty::new()
                    .with_size(vec2f(CARET_CELL_WIDTH, CARET_UNDERLINE_HEIGHT))
                    .finish(),
            )
            .with_background(accent)
            .finish();
            Flex::column()
                .with_child(slot)
                .with_child(bar)
                .finish()
        }
    }
}
