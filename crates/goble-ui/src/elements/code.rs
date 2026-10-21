use std::cell::RefCell;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::text_selection::{RunMetrics, SelectableText, SelectionRun};
use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{vec2f, Vector2F};
use crate::platform::text_atlas::{
    advance_width, measure_text_family, mono_advance, wrapped_lines, FontWeight,
};
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
    /// When set, every visual row this element draws is recorded here so a
    /// surface can select and copy the code. See [`SelectableText`].
    selection: Option<Rc<RefCell<SelectableText>>>,
    /// The width the body was last measured against, when it wraps. Drawing
    /// breaks a line at a different width than the measurement did, so the
    /// drawn block follows this number and not the measured size.
    wrap_width: f32,
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
            selection: None,
            wrap_width: f32::INFINITY,
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

    /// Record each visual row this element draws into `selection`, so a pointer
    /// can select the code and copy it.
    pub fn with_text_selection(mut self, selection: Rc<RefCell<SelectableText>>) -> Self {
        self.selection = Some(selection);
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

    /// Record one drawn run so a surface's selection has the row it landed on.
    /// Rows drawn at the same top join one selectable line.
    fn record_run(&self, x: f32, top: f32, text: &str, weight: FontWeight, italic: bool) {
        let Some(selection) = &self.selection else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let metrics = RunMetrics::new(
            self.font_size,
            self.line_height,
            FontFamily::Mono,
            weight,
            italic,
        );
        selection.borrow_mut().push_run(SelectionRun::new(
            x,
            top,
            self.font_size * self.line_height,
            text,
            metrics,
        ));
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
            Some(lines) => measure_highlighted_lines(lines, self.font_size, self.line_height, max_width),
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
        self.wrap_width = max_width;
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
        // The drawing wrap width is the one the layout measured at, so the drawn
        // lines break where the box was sized to break.
        let max_width = self.wrap_width;
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
            if self.wrap && self.wrap_width.is_finite() {
                // Each logical line is folded into the visual rows that fit the
                // box, then every row's characters are grouped back into
                // same-styled runs so the colours survive the fold.
                let chars = styled_chars(lines);
                let mut row = 0usize;
                for line in &chars {
                    for (start, end) in wrap_styled(line, self.wrap_width, self.font_size) {
                        let line_y = y + row as f32 * line_height;
                        let mut x = origin.x;
                        let mut index = start;
                        while index < end {
                            let first = &line[index];
                            let run_start = index;
                            index += 1;
                            while index < end
                                && line[index].weight == first.weight
                                && line[index].italic == first.italic
                                && line[index].color == first.color
                            {
                                index += 1;
                            }
                            let text: String =
                                line[run_start..index].iter().map(|ch| ch.ch).collect();
                            renderer.draw_text_with_font(
                                vec2f(x, line_y),
                                text.clone(),
                                self.font_size,
                                first.color,
                                f32::INFINITY,
                                self.line_height,
                                first.weight,
                                FontFamily::Mono,
                                first.italic,
                            );
                            self.record_run(x, line_y, &text, first.weight, first.italic);
                            x += advance_width(
                                &text,
                                self.font_size,
                                first.weight,
                                FontFamily::Mono,
                                first.italic,
                            );
                        }
                        row += 1;
                    }
                }
                return;
            }
            for (index, runs) in lines.iter().enumerate() {
                let line_y = y + index as f32 * line_height;
                let mut x = origin.x;
                for run in runs {
                    // The runs are placed by their advances, not by their ink:
                    // the renderer advances the glyphs inside a run by the same
                    // amount, so a run placed a bearing narrower than its own
                    // advance would overlap the run before it.
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
                    self.record_run(x, line_y, &run.text, run.weight, run.italic);
                    x += advance_width(
                        &run.text,
                        self.font_size,
                        run.weight,
                        FontFamily::Mono,
                        run.italic,
                    );
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
        if self.selection.is_some() {
            let rows = wrapped_lines(
                &self.text,
                self.font_size,
                self.line_height,
                max_width,
                FontWeight::Regular,
                FontFamily::Mono,
                false,
            );
            for row in rows {
                self.record_run(
                    origin.x,
                    y + row.top,
                    &row.text,
                    FontWeight::Regular,
                    false,
                );
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

/// One character of a highlighted line, carrying the style of the run it came
/// from. Wrapping a highlighted body has to move whole characters between
/// visual rows, so the runs are flattened to this for the wrap pass and grouped
/// back into same-styled runs when each row is painted.
struct StyledChar {
    ch: char,
    color: ColorU,
    weight: FontWeight,
    italic: bool,
}

fn styled_chars(lines: &[crate::syntax::HighlightedLine]) -> Vec<Vec<StyledChar>> {
    lines
        .iter()
        .map(|runs| {
            let mut out = Vec::new();
            for run in runs {
                for ch in run.text.chars() {
                    // The highlighter keeps the line endings; they are not
                    // glyphs and a row break is drawn by the wrap pass itself.
                    if ch == '\n' || ch == '\r' {
                        continue;
                    }
                    out.push(StyledChar {
                        ch,
                        color: run.color,
                        weight: run.weight,
                        italic: run.italic,
                    });
                }
            }
            out
        })
        .collect()
}

/// The advance one highlighted character is drawn with. Highlighted code is
/// mono, and every glyph is rounded up to a whole pixel when it is drawn, so a
/// cell is the rounded advance.
fn styled_char_advance(character: &StyledChar, font_size: f32) -> f32 {
    if character.ch.is_control() {
        0.0
    } else {
        mono_advance(font_size, character.weight).ceil().max(0.0)
    }
}

/// The `(start, end)` character ranges one logical line breaks into at
/// `max_width`: at the last space of the row when one can take the break, and
/// mid-word only when a single word cannot fit.
fn wrap_styled(chars: &[StyledChar], max_width: f32, font_size: f32) -> Vec<(usize, usize)> {
    if chars.is_empty() {
        return vec![(0, 0)];
    }
    if !max_width.is_finite() || max_width <= 0.0 {
        return vec![(0, chars.len())];
    }
    let mut rows = Vec::new();
    let mut start = 0usize;
    let mut last_space: Option<usize> = None;
    let mut width = 0.0f32;
    let mut index = 0usize;
    while index < chars.len() {
        let advance = styled_char_advance(&chars[index], font_size);
        if width + advance > max_width && index > start {
            let break_at = last_space.filter(|space| *space > start).unwrap_or(index);
            rows.push((start, break_at));
            start = break_at;
            if start < chars.len() && chars[start].ch == ' ' {
                start += 1;
            }
            // The tail after the break moves to the next row, so it is
            // measured again from the new start rather than from the overflow.
            index = start;
            width = 0.0;
            last_space = None;
            continue;
        }
        width += advance;
        if chars[index].ch == ' ' {
            last_space = Some(index);
        }
        index += 1;
    }
    if start < chars.len() || rows.is_empty() {
        rows.push((start, chars.len()));
    }
    rows
}

/// Measure highlighted lines: the widest visual row's runs summed, one line
/// height per visual row. With wrapping on, a row is what fits in `max_width`,
/// so the measured width never exceeds the box the code was given.
///
/// The runs are summed by their advances: that is how far apart the painting
/// places them, and the ink is a bearing short of it.
fn measure_highlighted_lines(
    lines: &[crate::syntax::HighlightedLine],
    font_size: f32,
    line_height: f32,
    max_width: f32,
) -> Vector2F {
    let chars = styled_chars(lines);
    if chars.is_empty() {
        return vec2f(0.0, (font_size * line_height).ceil());
    }
    let mut width: f32 = 0.0;
    let mut rows = 0usize;
    for line in &chars {
        for (start, end) in wrap_styled(line, max_width, font_size) {
            rows += 1;
            let row_width: f32 = line[start..end]
                .iter()
                .map(|character| styled_char_advance(character, font_size))
                .sum();
            width = width.max(row_width);
        }
    }
    vec2f(width.ceil(), (rows.max(1) as f32 * font_size * line_height).ceil())
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
    fn code_records_the_rows_it_paints() {
        use crate::elements::SelectableText;

        let app = AppContext::default();
        let recording = Rc::new(RefCell::new(SelectableText::default()));
        let mut code = Code::new("let alpha = 1;\nlet beta = 2;")
            .with_text_selection(recording.clone());
        code.layout(
            SizeConstraint::loose(vec2f(400.0, 200.0)),
            &mut LayoutContext,
            &app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        code.paint(vec2f(0.0, 0.0), &mut ctx, &app);

        let text = recording.borrow();
        assert_eq!(text.lines.len(), 2, "one row per logical line");
        assert_eq!(text.lines[0].text(), "let alpha = 1;");
        assert_eq!(text.lines[1].text(), "let beta = 2;");
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
    fn wrapped_highlighted_code_folds_inside_its_width() {
        let source = "let some_really_long_identifier = compute_something(one, two, three);";
        let lines = crate::syntax::highlight(source, "rust").expect("rust resolves");
        let app = AppContext::default();

        let mut wide = Code::new(source)
            .with_highlighted_lines(lines.clone())
            .with_wrap(true);
        let wide_size = wide.layout(
            SizeConstraint::loose(vec2f(1000.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut narrow = Code::new(source).with_highlighted_lines(lines).with_wrap(true);
        let narrow_size = narrow.layout(
            SizeConstraint::loose(vec2f(120.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(
            narrow_size.x <= 120.0 + 0.01,
            "a wrapped fence measures within the width it was given: {narrow_size:?}"
        );
        assert!(
            narrow_size.y > wide_size.y,
            "folding the line grows the block: {narrow_size:?} vs {wide_size:?}"
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
