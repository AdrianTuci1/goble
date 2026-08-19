use crate::color::ColorU;
use crate::elements::{
    text::measure_text, AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::geometry::Vector2F;
use crate::theme::ColorToken;

const DEFAULT_FONT_SIZE: f32 = 12.0;
const DEFAULT_LINE_HEIGHT: f32 = 1.2;

/// Smaller secondary text for metadata, hints and timestamps.
pub struct Caption {
    text: String,
    font_size: f32,
    color: ColorU,
    line_height: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Caption {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            font_size: DEFAULT_FONT_SIZE,
            color: ColorU::default(),
            line_height: DEFAULT_LINE_HEIGHT,
            size: None,
            origin: None,
        }
    }

    pub fn with_font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    pub fn with_color(mut self, color: impl Into<ColorU>) -> Self {
        self.color = color.into();
        self
    }

    pub fn with_theme_color(mut self, token: ColorToken, app: &AppContext) -> Self {
        self.color = app.theme.color(token);
        self
    }

    pub fn with_line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

impl Element for Caption {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = measure_text(
            &self.text,
            self.font_size,
            self.line_height,
            constraint.max.x,
        );
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, _ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
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
    use crate::geometry::vec2f;

    #[test]
    fn caption_measures_non_zero() {
        let app = AppContext::default();
        let mut caption = Caption::new("Updated just now");
        let size = caption.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }
}
