use crate::color::ColorU;
use crate::elements::{
    AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::platform::text_atlas::{measure_text_family, FontWeight};
use crate::theme::{ColorToken, FontFamily};

const DEFAULT_FONT_SIZE: f32 = 12.0;
const DEFAULT_LINE_HEIGHT: f32 = 1.2;
const CODE_BG_PAD: f32 = 2.0;

/// A single run of text with a resolved appearance, used by [`InlineText`].
#[derive(Clone)]
pub struct TextSpan {
    pub text: String,
    pub weight: FontWeight,
    pub family: FontFamily,
    pub color: ColorU,
    pub background: Option<ColorU>,
}

impl TextSpan {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            weight: FontWeight::Regular,
            family: FontFamily::System,
            color: ColorU::default(),
            background: None,
        }
    }

    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    pub fn with_family(mut self, family: FontFamily) -> Self {
        self.family = family;
        self
    }

    pub fn with_color(mut self, color: impl Into<ColorU>) -> Self {
        self.color = color.into();
        self
    }

    pub fn with_background(mut self, color: impl Into<ColorU>) -> Self {
        self.background = Some(color.into());
        self
    }
}

struct PlacedSpan {
    x: f32,
    y: f32,
    max_width: f32,
    width: f32,
    span: TextSpan,
}

/// Renders a sequence of [`TextSpan`]s as a single wrapping text flow.
///
/// This is what lets a paragraph mix bold/italic/inline-code/link spans without
/// breaking each span onto its own line the way the old per-fragment bubble did.
pub struct InlineText {
    spans: Vec<TextSpan>,
    font_size: f32,
    line_height: f32,
    placed: Vec<PlacedSpan>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl InlineText {
    pub fn new(spans: Vec<TextSpan>) -> Self {
        Self {
            spans,
            font_size: DEFAULT_FONT_SIZE,
            line_height: DEFAULT_LINE_HEIGHT,
            placed: Vec::new(),
            size: None,
            origin: None,
        }
    }

    pub fn with_font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    pub fn with_line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }
}

impl Default for InlineText {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl Element for InlineText {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let _ = ctx;
        let _ = app;
        let max_width = if constraint.max.x.is_finite() && constraint.max.x > 0.0 {
            constraint.max.x
        } else {
            f32::INFINITY
        };
        let line_h = self.font_size * self.line_height;

        let mut cursor_x = 0.0;
        let mut cursor_y = 0.0;
        let mut placed: Vec<PlacedSpan> = Vec::with_capacity(self.spans.len());

        for span in &self.spans {
            let single = measure_text_family(
                &span.text,
                self.font_size,
                self.line_height,
                f32::INFINITY,
                span.weight,
                span.family,
            );
            let single_w = single.x;

            if cursor_x + single_w <= max_width {
                placed.push(PlacedSpan {
                    x: cursor_x,
                    y: cursor_y,
                    max_width: max_width - cursor_x,
                    width: single_w,
                    span: span.clone(),
                });
                cursor_x += single_w;
                continue;
            }

            // The span does not fit on the current line. If it can wrap within
            // the remaining space, place it there (possibly spanning lines);
            // otherwise move to the next line and place it at full width.
            let remaining = (max_width - cursor_x).max(1.0);
            let wrapped = measure_text_family(
                &span.text,
                self.font_size,
                self.line_height,
                remaining,
                span.weight,
                span.family,
            );
            if wrapped.y > line_h + 0.5 {
                placed.push(PlacedSpan {
                    x: cursor_x,
                    y: cursor_y,
                    max_width: remaining,
                    width: wrapped.x,
                    span: span.clone(),
                });
                cursor_y += wrapped.y;
                cursor_x = 0.0;
            } else {
                cursor_y += line_h;
                let full = measure_text_family(
                    &span.text,
                    self.font_size,
                    self.line_height,
                    max_width.max(1.0),
                    span.weight,
                    span.family,
                );
                placed.push(PlacedSpan {
                    x: 0.0,
                    y: cursor_y,
                    max_width: max_width,
                    width: full.x,
                    span: span.clone(),
                });
                cursor_x = full.x;
            }
        }

        let height = if placed.is_empty() {
            line_h
        } else {
            cursor_y + line_h
        };
        self.placed = placed;
        let size = vec2f(max_width.min(f32::INFINITY).max(0.0), height);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let renderer = match ctx.renderer.as_mut() {
            Some(r) => r,
            None => return,
        };
        for placed in &self.placed {
            let base = vec2f(origin.x + placed.x, origin.y + placed.y);
            if let Some(bg) = placed.span.background {
                let height = self.font_size * self.line_height;
                renderer.fill_rounded_rect(
                    rectf(base.x - CODE_BG_PAD, base.y - CODE_BG_PAD, placed.width + CODE_BG_PAD * 2.0, height + CODE_BG_PAD * 2.0),
                    bg,
                    3.0,
                );
            }
            renderer.draw_text_with_font(
                base,
                placed.span.text.clone(),
                self.font_size,
                placed.span.color,
                placed.max_width,
                placed.span.weight,
                placed.span.family,
            );
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

/// Resolve an inline span's style to a renderable [`TextSpan`] using theme colors.
/// `is_link` is true when the span is a hyperlink (rendered in the accent color).
pub fn resolve_span(
    text: String,
    bold: bool,
    italic: bool,
    is_code: bool,
    is_link: bool,
    app: &AppContext,
) -> TextSpan {
    let weight = if bold {
        FontWeight::Bold
    } else {
        FontWeight::Regular
    };
    let (family, color, background) = if is_code {
        (
            FontFamily::Mono,
            app.theme.color(ColorToken::Text),
            Some(app.theme.color(ColorToken::SurfaceRaised)),
        )
    } else if is_link {
        (
            FontFamily::System,
            app.theme.color(ColorToken::Accent),
            None,
        )
    } else {
        (
            FontFamily::System,
            app.theme.color(ColorToken::Text),
            None,
        )
    };
    let _ = italic;
    TextSpan {
        text,
        weight,
        family,
        color,
        background,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_text_layouts_non_zero() {
        let app = AppContext::default();
        let mut inline = InlineText::new(vec![TextSpan::plain("hello world")]);
        let size = inline.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn inline_text_wraps_at_max_width() {
        let app = AppContext::default();
        let mut inline = InlineText::new(vec![TextSpan::plain("one two three four five")]);
        let single = inline.layout(
            SizeConstraint::loose(vec2f(100000.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let wide = inline.layout(
            SizeConstraint::loose(vec2f(20.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        // Wrapping to a narrow column makes the text taller.
        assert!(wide.y > single.y);
    }
}
