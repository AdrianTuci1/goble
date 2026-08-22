use crate::color::ColorU;
use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::Vector2F;
use crate::platform::text_atlas::{measure_text as measure_text_atlas, FontWeight};
use crate::theme::ColorToken;

const DEFAULT_FONT_SIZE: f32 = 14.0;
const DEFAULT_LINE_HEIGHT: f32 = 1.2;

/// Measure text using the bundled Roboto fonts when possible.
pub fn measure_text(text: &str, font_size: f32, line_height: f32, max_width: f32) -> Vector2F {
    measure_text_atlas(text, font_size, line_height, max_width, FontWeight::Regular)
}

/// A single-line or wrapped body text element.
pub struct Text {
    text: String,
    font_size: f32,
    color: ColorU,
    line_height: f32,
    max_lines: Option<usize>,
    weight: FontWeight,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Text {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            font_size: DEFAULT_FONT_SIZE,
            color: ColorU::default(),
            line_height: DEFAULT_LINE_HEIGHT,
            max_lines: None,
            weight: FontWeight::Regular,
            size: None,
            origin: None,
        }
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
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

    pub fn with_max_lines(mut self, max_lines: usize) -> Self {
        self.max_lines = Some(max_lines);
        self
    }

    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    pub fn color(&self) -> ColorU {
        self.color
    }
}

impl Default for Text {
    fn default() -> Self {
        Self::new("")
    }
}

impl Element for Text {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let mut size = measure_text_atlas(
            &self.text,
            self.font_size,
            self.line_height,
            constraint.max.x,
            self.weight,
        );
        if let Some(max_lines) = self.max_lines {
            let max_height = self.font_size * self.line_height * max_lines as f32;
            size.y = size.y.min(max_height);
        }
        size.x = size.x.max(constraint.min.x).min(constraint.max.x);
        size.y = size.y.max(constraint.min.y).min(constraint.max.y);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if let Some(size) = self.size {
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.draw_text_weighted(origin, self.text.clone(), self.font_size, self.color, size.x, self.weight);
            }
        }
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
    fn text_measures_empty_string() {
        let app = AppContext::default();
        let mut text = Text::new("");
        let size = text.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.y > 0.0);
        assert_eq!(size.x, 0.0);
    }

    #[test]
    fn text_measures_content() {
        let app = AppContext::default();
        let mut text = Text::new("hello").with_font_size(20.0);
        let size = text.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn text_wraps_at_max_width() {
        let app = AppContext::default();
        let mut text = Text::new("hello world").with_font_size(20.0);
        let size = text.layout(
            SizeConstraint::loose(vec2f(60.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x <= 60.0);
        assert!(size.y > 20.0 * DEFAULT_LINE_HEIGHT);
    }
}
