use crate::elements::{AppContext, Container, Element, Empty, Fill};
use crate::geometry::vec2f;
use crate::theme::ColorToken;

/// Height of the caret beam: one line box of the 12px body text.
pub const CARET_HEIGHT: f32 = 14.0;

/// The insertion beam of a focused text field.
///
/// The element tree is rebuilt every frame, so the caret is built into the
/// field's row (right after the text) instead of being paint-time state.
pub fn caret_beam(app: &AppContext) -> Box<dyn Element> {
    Container::new(
        Empty::new()
            .with_size(vec2f(1.0, CARET_HEIGHT))
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::Accent)))
    .finish()
}
