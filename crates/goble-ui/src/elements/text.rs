use crate::color::ColorU;
use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::Vector2F;
use crate::platform::text_atlas::{measure_text_family as measure_text_atlas, FontWeight};
use crate::theme::{ColorToken, FontFamily};

const DEFAULT_FONT_SIZE: f32 = 12.0;
const DEFAULT_LINE_HEIGHT: f32 = 1.2;

/// Measure text using the bundled Roboto fonts when possible.
pub fn measure_text(text: &str, font_size: f32, line_height: f32, max_width: f32) -> Vector2F {
    measure_text_atlas(
        text,
        font_size,
        line_height,
        max_width,
        FontWeight::Regular,
        FontFamily::System,
        false,
    )
}

/// A single-line or wrapped body text element.
pub struct Text {
    text: String,
    font_size: f32,
    color: ColorU,
    line_height: f32,
    max_lines: Option<usize>,
    weight: FontWeight,
    font_family: FontFamily,
    /// The width the text was last measured against. Drawing breaks a run at a
    /// different width than the measurement did, so the line count of the drawn
    /// block follows this number and not the measured size.
    wrap_width: f32,
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
            font_family: FontFamily::System,
            wrap_width: f32::INFINITY,
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

    pub fn with_font_family(mut self, family: FontFamily) -> Self {
        self.font_family = family;
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
            self.font_family,
            false,
        );
        if let Some(max_lines) = self.max_lines {
            let max_height = self.font_size * self.line_height * max_lines as f32;
            size.y = size.y.min(max_height);
        }
        size.x = size.x.max(constraint.min.x).min(constraint.max.x);
        size.y = size.y.max(constraint.min.y).min(constraint.max.y);
        self.wrap_width = constraint.max.x;
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if self.size.is_none() {
            return;
        }
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        renderer.draw_text_with_font(
            origin,
            self.text.clone(),
            self.font_size,
            self.color,
            // The drawing wrap width is the one the measurement used, so the
            // drawn run breaks exactly where the box was sized to break: a box
            // measured at 160 and drawn at 161 takes one line fewer than the
            // box reserves.
            self.wrap_width,
            self.line_height,
            self.weight,
            self.font_family,
            false,
        );
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
    use crate::render::RenderCommand;

    /// The (text, size, wrap width) of the run the element drew.
    fn drawn_run(element: &mut Box<dyn Element>, width: f32, app: &AppContext) -> (String, Vector2F, f32) {
        let commands = crate::test_util::render_element(element, vec2f(width, 4000.0), app);
        let size = element.size().expect("the element laid out");
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    text, max_width, ..
                } => Some((text.clone(), size, *max_width)),
                _ => None,
            })
            .expect("the text draws one run")
    }

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
        let mut text = Text::new("hello").with_font_size(12.0);
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
        let mut text = Text::new("hello world").with_font_size(12.0);
        let size = text.layout(
            SizeConstraint::loose(vec2f(60.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x <= 60.0);
        assert!(size.y > 20.0 * DEFAULT_LINE_HEIGHT);
    }

    /// A narrower constraint re-wraps the paragraph, so the box comes down to
    /// the constraint and gets taller.
    #[test]
    fn a_narrower_constraint_rewraps_the_text() {
        let app = AppContext::default();
        let paragraph = "one two three four five six seven eight nine ten";
        let mut wide = Text::new(paragraph);
        let wide_size = wide.layout(
            SizeConstraint::loose(vec2f(400.0, 4000.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut narrow = Text::new(paragraph);
        let narrow_size = narrow.layout(
            SizeConstraint::loose(vec2f(120.0, 4000.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(
            wide_size.x > narrow_size.x,
            "the box narrows with the constraint: {} then {}",
            wide_size.x,
            narrow_size.x
        );
        assert!(narrow_size.x <= 120.0);
        assert!(
            narrow_size.y > wide_size.y,
            "the narrower box holds more lines"
        );
    }

    /// The run is drawn at the width the box was measured at. Drawing it at the
    /// measured size instead breaks it wherever that size falls, which is one
    /// line more or less than the box holds — the block then spills over the
    /// content below it, or leaves its last line's worth of space empty.
    #[test]
    fn wrapped_text_is_drawn_at_the_width_it_was_measured_at() {
        let app = AppContext::default();
        let paragraph =
            "The agent wrote a long paragraph of prose that has to reflow on every resize";
        for width in [120.0_f32, 163.0, 240.0, 331.0] {
            let mut element: Box<dyn Element> = Box::new(Text::new(paragraph));
            let (drawn, size, max_width) = drawn_run(&mut element, width, &app);
            assert_eq!(
                max_width, width,
                "the run is drawn at the width its box was measured at"
            );
            let drawn_height = measure_text_atlas(
                &drawn,
                DEFAULT_FONT_SIZE,
                DEFAULT_LINE_HEIGHT,
                max_width,
                FontWeight::Regular,
                FontFamily::System,
                false,
            )
            .y;
            assert!(
                drawn_height <= size.y + 0.5,
                "the drawn block is {drawn_height} tall in a {width} wide box that reserves {}",
                size.y
            );
        }
    }
}
