use crate::color::ColorU;
use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{vec2f, Vector2F};
use crate::platform::text_atlas::{measure_text_family, FontWeight};
use crate::theme::{ColorToken, FontFamily};

const DEFAULT_FONT_SIZE: f32 = 12.0;
const DEFAULT_LINE_HEIGHT: f32 = 1.4;
const DEFAULT_LANGUAGE_FONT_SIZE: f32 = 10.0;
const LANGUAGE_GAP: f32 = 2.0;

/// Monospaced inline or block code element.
///
/// Block code is drawn in the mono family and is **not** word-wrapped by
/// default, so a line keeps its indentation and its column alignment. A fenced
/// block's info string is carried as the language label and drawn above the
/// body in the same mono family; a caller that would rather place the label
/// itself can read it back with [`Code::language`].
///
/// A body may also arrive pre-highlighted, as the styled runs
/// [`crate::syntax::highlight`] returns; it is then painted run by run. The mono
/// family, the label and the no-wrap contract are the same either way.
pub struct Code {
    text: String,
    language: Option<String>,
    font_size: f32,
    color: ColorU,
    language_color: ColorU,
    line_height: f32,
    inline: bool,
    wrap: bool,
    highlighted: Option<Vec<crate::syntax::HighlightedLine>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Code {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            language: None,
            font_size: DEFAULT_FONT_SIZE,
            color: ColorU::default(),
            language_color: ColorU::default(),
            line_height: DEFAULT_LINE_HEIGHT,
            inline: false,
            // Code is pre-formatted: wrapping it would break indentation.
            wrap: false,
            highlighted: None,
            size: None,
            origin: None,
        }
    }

    /// Set the fence's language label. Only block code shows it.
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
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

    pub fn with_language_color(mut self, color: impl Into<ColorU>) -> Self {
        self.language_color = color.into();
        self
    }

    pub fn with_language_theme_color(mut self, token: ColorToken, app: &AppContext) -> Self {
        self.language_color = app.theme.color(token);
        self
    }

    pub fn with_line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    pub fn with_inline(mut self, inline: bool) -> Self {
        self.inline = inline;
        self
    }

    /// Opt in to wrapping long lines at the available width. Off by default.
    pub fn with_wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Highlight the body as `language` through [`crate::syntax::highlight`].
    /// The body is painted run by run in the runs' own colours. A language that
    /// cannot be resolved leaves the body in the plain colour it had before, so
    /// an unknown fence falls back rather than drawing nothing.
    pub fn with_highlight(mut self, language: &str) -> Self {
        self.highlighted = crate::syntax::highlight(&self.text, language);
        self
    }

    /// Carry pre-highlighted lines — the runs [`crate::syntax::highlight`]
    /// returns — when the caller already resolved the language.
    pub fn with_highlighted_lines(mut self, lines: Vec<crate::syntax::HighlightedLine>) -> Self {
        self.highlighted = Some(lines);
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The fence's language label, when one was supplied.
    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    pub fn wraps(&self) -> bool {
        self.wrap
    }

    /// Inline code has no label; block code shows it when the fence declared one.
    fn label(&self) -> Option<&str> {
        if self.inline {
            None
        } else {
            self.language.as_deref()
        }
    }

    /// Wrapping uses the constraint; pre-formatted block code is measured
    /// unbounded so a long line overflows instead of folding mid-expression.
    fn measure_max_width(&self, constraint: SizeConstraint) -> f32 {
        if self.wrap {
            constraint.max.x
        } else {
            f32::INFINITY
        }
    }

    fn label_size(&self, max_width: f32) -> Option<Vector2F> {
        self.label().map(|label| {
            measure_text_family(
                label,
                DEFAULT_LANGUAGE_FONT_SIZE,
                self.line_height,
                max_width,
                FontWeight::Regular,
                FontFamily::Mono,
                false,
            )
        })
    }
}

impl Element for Code {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let max_width = self.measure_max_width(constraint);
        let mut size = match &self.highlighted {
            Some(lines) => measure_highlighted_lines(lines, self.font_size, self.line_height),
            None => measure_text_family(
                &self.text,
                self.font_size,
                self.line_height,
                max_width,
                FontWeight::Regular,
                FontFamily::Mono,
                false,
            ),
        };
        if let Some(label_size) = self.label_size(max_width) {
            size.x = size.x.max(label_size.x);
            size.y += label_size.y + LANGUAGE_GAP;
        }
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let max_width = if self.wrap {
            size.x + 1.0
        } else {
            f32::INFINITY
        };
        let mut y = origin.y;
        if let Some(label) = self.label() {
            if let Some(label_size) = self.label_size(max_width) {
                renderer.draw_text_with_font(
                    vec2f(origin.x, y),
                    label.to_string(),
                    DEFAULT_LANGUAGE_FONT_SIZE,
                    self.language_color,
                    max_width,
                    self.line_height,
                    FontWeight::Regular,
                    FontFamily::Mono,
                    false,
                );
                y += label_size.y + LANGUAGE_GAP;
            }
        }
        if let Some(lines) = &self.highlighted {
            let line_height = self.font_size * self.line_height;
            for (index, runs) in lines.iter().enumerate() {
                let line_y = y + index as f32 * line_height;
                let mut x = origin.x;
                for run in runs {
                    let run_size = measure_text_family(
                        &run.text,
                        self.font_size,
                        self.line_height,
                        f32::INFINITY,
                        run.weight,
                        FontFamily::Mono,
                        run.italic,
                    );
                    renderer.draw_text_with_font(
                        vec2f(x, line_y),
                        run.text.clone(),
                        self.font_size,
                        run.color,
                        f32::INFINITY,
                        self.line_height,
                        run.weight,
                        FontFamily::Mono,
                        run.italic,
                    );
                    x += run_size.x;
                }
            }
            return;
        }
        renderer.draw_text_with_font(
            vec2f(origin.x, y),
            self.text.clone(),
            self.font_size,
            self.color,
            max_width,
            self.line_height,
            FontWeight::Regular,
            FontFamily::Mono,
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

/// Measure highlighted lines: the widest line's runs summed, one line height per
/// line. The runs are mono, so this matches the plain measurement of the same
/// characters while keeping the size consistent with the per-line painting.
fn measure_highlighted_lines(
    lines: &[crate::syntax::HighlightedLine],
    font_size: f32,
    line_height: f32,
) -> Vector2F {
    let mut width: f32 = 0.0;
    for runs in lines {
        let line_width: f32 = runs
            .iter()
            .map(|run| {
                measure_text_family(
                    &run.text,
                    font_size,
                    line_height,
                    f32::INFINITY,
                    run.weight,
                    FontFamily::Mono,
                    run.italic,
                )
                .x
            })
            .sum();
        width = width.max(line_width);
    }
    let height = lines.len().max(1) as f32 * font_size * line_height;
    vec2f(width.ceil(), height.ceil())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;
    use crate::render::{RenderCommand, Renderer};

    fn painted_runs(code: &mut Code, max_width: f32) -> Vec<(String, FontFamily)> {
        let app = AppContext::default();
        code.layout(
            SizeConstraint::loose(vec2f(max_width, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut paint_ctx = PaintContext::new(Renderer::new());
        code.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText {
                    text, font_family, ..
                } => Some((text, font_family)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn code_measures_non_zero() {
        let app = AppContext::default();
        let mut code = Code::new("fn main() {}");
        let size = code.layout(
            SizeConstraint::loose(Vector2F::new(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn code_draws_in_the_mono_family_and_keeps_indentation() {
        let source = "fn main() {\n    let x = 1;\n}";
        let mut code = Code::new(source).with_language("rust");
        let runs = painted_runs(&mut code, 600.0);

        assert!(
            runs.iter().all(|(_, family)| *family == FontFamily::Mono),
            "every code run must be mono, got {runs:?}"
        );
        assert!(
            runs.iter().any(|(text, _)| text == source),
            "the body must be drawn verbatim, indentation included, got {runs:?}"
        );
        assert!(
            runs.iter().any(|(text, _)| text == "rust"),
            "the fence's language label must be drawn, got {runs:?}"
        );
        assert_eq!(code.language(), Some("rust"));
    }

    #[test]
    fn code_does_not_wrap_but_can_opt_in() {
        let app = AppContext::default();
        let line = "let some_really_long_identifier = compute_something(1, 2, 3);";

        let mut unwrapped = Code::new(line);
        let wide = unwrapped.layout(
            SizeConstraint::loose(vec2f(1000.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let narrow = unwrapped.layout(
            SizeConstraint::loose(vec2f(40.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(!unwrapped.wraps());
        assert_eq!(
            wide.y, narrow.y,
            "a pre-formatted line must not wrap when the width shrinks"
        );

        let mut wrapped = Code::new(line).with_wrap(true);
        let wide = wrapped.layout(
            SizeConstraint::loose(vec2f(1000.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let narrow = wrapped.layout(
            SizeConstraint::loose(vec2f(40.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(wrapped.wraps());
        assert!(
            narrow.y > wide.y,
            "opted-in wrapping must fold the line and grow the height"
        );
    }

    #[test]
    fn inline_code_has_no_label() {
        let mut code = Code::new("x").with_language("rust").with_inline(true);
        let runs = painted_runs(&mut code, 200.0);
        assert!(
            runs.iter().all(|(text, _)| text != "rust"),
            "inline code must not draw the fence label, got {runs:?}"
        );
    }

    #[test]
    fn highlighted_code_paints_the_runs_in_more_than_one_colour() {
        let source = "fn main() {\n    let x = 1;\n}";
        let lines = crate::syntax::highlight(source, "rust").expect("rust resolves");
        let mut code = Code::new(source)
            .with_language("rust")
            .with_highlighted_lines(lines);
        let app = AppContext::default();
        code.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut paint_ctx = PaintContext::new(Renderer::new());
        code.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default();

        let mut colours = std::collections::HashSet::new();
        let mut joined = String::new();
        for command in &commands {
            if let RenderCommand::DrawText {
                text,
                font_family,
                color,
                ..
            } = command
            {
                if text == "rust" {
                    continue;
                }
                assert_eq!(*font_family, FontFamily::Mono, "run {text:?} is not mono");
                colours.insert(*color);
                joined.push_str(text);
            }
        }
        assert!(
            colours.len() > 1,
            "highlighted code must paint more than one colour, got {colours:?}"
        );
        // The split into runs loses nothing but the line endings.
        let stripped: String = source.chars().filter(|c| *c != '\n').collect();
        assert_eq!(joined, stripped);
    }

    #[test]
    fn an_unresolved_language_keeps_the_plain_body() {
        let unresolved = Code::new("fn main() {}").with_highlight("not-a-language");
        assert!(unresolved.highlighted.is_none());
        assert!(crate::syntax::highlight("fn main() {}", "not-a-language").is_none());

        let resolved = Code::new("fn main() {}").with_highlight("rust");
        assert!(resolved.highlighted.is_some());
    }
}
