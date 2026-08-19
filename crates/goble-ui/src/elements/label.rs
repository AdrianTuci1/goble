use crate::color::ColorU;
use crate::elements::{
    text::measure_text, AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::geometry::Vector2F;
use crate::theme::ColorToken;

const DEFAULT_LINE_HEIGHT: f32 = 1.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LabelSize {
    #[default]
    Xs,
    Sm,
}

impl LabelSize {
    pub fn font_size(self) -> f32 {
        match self {
            LabelSize::Xs => 11.0,
            LabelSize::Sm => 12.0,
        }
    }
}

/// Uppercase, small, muted caption for section headers and form labels.
pub struct Label {
    text: String,
    size: LabelSize,
    color: ColorU,
    line_height: f32,
    measured_size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Label {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into().to_uppercase(),
            size: LabelSize::default(),
            color: ColorU::default(),
            line_height: DEFAULT_LINE_HEIGHT,
            measured_size: None,
            origin: None,
        }
    }

    pub fn with_size(mut self, size: LabelSize) -> Self {
        self.size = size;
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

    pub fn label_size(&self) -> LabelSize {
        self.size
    }
}

impl Element for Label {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let font_size = self.size.font_size();
        let size = measure_text(&self.text, font_size, self.line_height, constraint.max.x);
        self.measured_size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, _ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
    }

    fn size(&self) -> Option<Vector2F> {
        self.measured_size
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
    fn label_uppercases_text() {
        let label = Label::new("Section");
        assert_eq!(label.text(), "SECTION");
    }

    #[test]
    fn label_measures_non_zero() {
        let app = AppContext::default();
        let mut label = Label::new("Header");
        let size = label.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }
}
