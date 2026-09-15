use std::cell::RefCell;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::platform::text_atlas::{advance_width, measure_text_family, FontWeight};
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
    pub italic: bool,
    /// Draw a rule under the run. Terminal output carries underline as an SGR
    /// attribute; prose never sets it.
    pub underline: bool,
    pub color: ColorU,
    pub background: Option<ColorU>,
    /// When set, this run is a live hit target: a click on it invokes the
    /// callback. A link carries one so the span the app painted is the thing
    /// the click lands on, not just text styled as a link.
    pub on_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
}

impl TextSpan {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            weight: FontWeight::Regular,
            family: FontFamily::System,
            italic: false,
            underline: false,
            color: ColorU::default(),
            background: None,
            on_click: None,
        }
    }

    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    pub fn with_italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    pub fn with_underline(mut self, underline: bool) -> Self {
        self.underline = underline;
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

    /// Make this run a live hit target: a click completed inside its placed
    /// bounds invokes `callback`.
    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }
}

struct PlacedSpan {
    x: f32,
    y: f32,
    max_width: f32,
    width: f32,
    span: TextSpan,
    /// Pointer-interaction state for this run, tracked only when it is a hit
    /// target. It is reset by the per-frame rebuild, and `handle_mouse_event`
    /// fires a click on any release inside the bounds regardless.
    state: InteractiveState,
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

    /// The paragraph broken into the units a line may break between: each run of
    /// non-whitespace, plus the whitespace that follows it. The run carries the
    /// index of the span it came from, so the style is read once and each unit
    /// stays cheap to hold.
    ///
    /// Every whitespace character becomes a space: the rasterizer breaks a run
    /// on a hard line break it finds in it, at a place the flow that placed the
    /// run never accounted for.
    fn atoms(&self) -> Vec<(usize, String, bool)> {
        let mut atoms = Vec::new();
        for (index, span) in self.spans.iter().enumerate() {
            let mut run = String::new();
            let mut run_is_space = false;
            for character in span.text.chars() {
                let is_space = character.is_whitespace();
                if !run.is_empty() && is_space != run_is_space {
                    atoms.push((index, std::mem::take(&mut run), run_is_space));
                }
                run_is_space = is_space;
                run.push(if is_space { ' ' } else { character });
            }
            if !run.is_empty() {
                atoms.push((index, run, run_is_space));
            }
        }
        atoms
    }
}

/// Both runs are drawn the same way, so a line of them is one drawn run.
fn same_style(a: &TextSpan, b: &TextSpan) -> bool {
    a.weight == b.weight
        && a.family == b.family
        && a.italic == b.italic
        && a.underline == b.underline
        && a.color == b.color
        && a.background == b.background
        && match (&a.on_click, &b.on_click) {
            (None, None) => true,
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
}

/// The wrap width a drawn run is given: the space left on its line.
fn drawn_width(max_width: f32, cursor_x: f32) -> f32 {
    max_width - cursor_x
}

/// The wrap width of a run the flow has already decided fits on its line: as
/// [`drawn_width`], but never less than the run's own advance. fontdue breaks a
/// run whose advances overrun the width it was given, and it breaks on the sum
/// of the rounded advances, so a width a fraction of a pixel short of that sum
/// drops the run's last word onto a line the flow never reserved for it.
fn drawn_run_width(max_width: f32, cursor_x: f32, advance: f32) -> f32 {
    drawn_width(max_width, cursor_x).max(advance)
}

/// Add a run to the flow, appending it to the previous run when the two are
/// adjacent on the same line and styled alike. A line of one style then draws
/// as one run — one background box for a code span, one underline — rather than
/// one per word.
fn push_chunk(placed: &mut Vec<PlacedSpan>, chunk: PlacedSpan, max_width: f32) {
    if let Some(last) = placed.last_mut() {
        let adjacent = (last.y - chunk.y).abs() < 0.5
            && (last.x + last.width - chunk.x).abs() < 0.5;
        if adjacent && same_style(&last.span, &chunk.span) {
            last.span.text.push_str(&chunk.span.text);
            last.width += chunk.width;
            // The merged run is as wide as the advances it now holds, which is
            // what its own wrap width has to cover.
            last.max_width = drawn_run_width(max_width, last.x, last.width);
            return;
        }
    }
    placed.push(chunk);
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
        // Whether the line under the cursor already holds a word. An empty line
        // never breaks, so the first word of a line is placed even when it is
        // wider than the paragraph.
        let mut line_has_word = false;
        // The whitespace before the word being placed, held back until that word
        // is known to land on the current line: a break drops the space instead
        // of leaving a gap at the end of the line.
        let mut pending_space: Option<(usize, String, f32)> = None;
        let mut placed: Vec<PlacedSpan> = Vec::with_capacity(self.spans.len());
        let mut bottom = 0.0;

        // Words, not whole spans, are what a line breaks between: filling a line
        // to its edge is what makes a paragraph read as one column of text. A
        // span placed whole into the space left on a line would make the rest of
        // the paragraph a second, narrow column beside it.
        for (index, text, is_space) in self.atoms() {
            let span = self.spans[index].clone();
            // The advance, not the measurement: the measurement is the extent
            // of the ink, which is a side bearing narrower than the room the
            // run takes up, and the renderer lays the run out by its advances.
            // Whitespace above all measures as nothing at all.
            let width = advance_width(
                &text,
                self.font_size,
                span.weight,
                span.family,
                span.italic,
            );

            if is_space {
                pending_space = Some((index, text, width));
                continue;
            }

            let gap = pending_space.as_ref().map(|(_, _, w)| *w).unwrap_or(0.0);
            // A single word wider than the paragraph is drawn wrapped inside it,
            // and the flow restarts under the lines that word took. What it then
            // occupies is those lines, not the one width it asked for.
            let over_wide = width > max_width;
            if line_has_word && cursor_x + gap + width > max_width {
                // Break to a new line and drop the space that was held back.
                cursor_y += line_h;
                cursor_x = 0.0;
                pending_space = None;
            }

            if let Some((space_index, space_text, space_width)) = pending_space.take() {
                // The space in front of an over-wide word is dropped: that word
                // is drawn wrapped inside the full width and starts at the left
                // edge, so a space before it would only shift its wrap.
                if !over_wide && cursor_x + space_width <= max_width {
                    let mut space_span = self.spans[space_index].clone();
                    space_span.text = space_text;
                    push_chunk(
                        &mut placed,
                        PlacedSpan {
                            x: cursor_x,
                            y: cursor_y,
                            max_width: drawn_run_width(max_width, cursor_x, space_width),
                            width: space_width,
                            span: space_span,
                            state: InteractiveState::default(),
                        },
                        max_width,
                    );
                    cursor_x += space_width;
                }
            }

            let advance = width;
            let (width, height) = if over_wide {
                let wrapped = measure_text_family(
                    &text,
                    self.font_size,
                    self.line_height,
                    max_width,
                    span.weight,
                    span.family,
                    span.italic,
                );
                (wrapped.x, wrapped.y)
            } else {
                (advance, line_h)
            };
            // A run the flow placed whole on its line is given at least its own
            // advance; the over-wide one is given the space left on the line,
            // which is narrower than its advance because it is to break there.
            let run_width = if over_wide {
                drawn_width(max_width, cursor_x)
            } else {
                drawn_run_width(max_width, cursor_x, advance)
            };
            let mut word_span = span;
            word_span.text = text;
            push_chunk(
                &mut placed,
                PlacedSpan {
                    x: cursor_x,
                    y: cursor_y,
                    max_width: run_width,
                    width,
                    span: word_span,
                    state: InteractiveState::default(),
                },
                max_width,
            );
            bottom = cursor_y + height;

            if over_wide {
                cursor_y += height;
                cursor_x = 0.0;
                line_has_word = false;
            } else {
                cursor_x += width;
                line_has_word = true;
            }
        }

        let height = if placed.is_empty() { line_h } else { bottom };
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
                // Whitespace only pads a run out to the next word; it carries no
                // box of its own.
                if !placed.span.text.trim().is_empty() {
                    let height = self.font_size * self.line_height;
                    renderer.fill_rounded_rect(
                        rectf(base.x - CODE_BG_PAD, base.y - CODE_BG_PAD, placed.width + CODE_BG_PAD * 2.0, height + CODE_BG_PAD * 2.0),
                        bg,
                        3.0,
                    );
                }
            }
            renderer.draw_text_with_font(
                base,
                placed.span.text.clone(),
                self.font_size,
                placed.span.color,
                placed.max_width,
                self.line_height,
                placed.span.weight,
                placed.span.family,
                placed.span.italic,
            );
            if placed.span.underline {
                let thickness = 1.0;
                let baseline = base.y + self.font_size * self.line_height - thickness;
                renderer.fill_rect(
                    rectf(base.x, baseline, placed.width, thickness),
                    placed.span.color,
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

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        let origin = match self.origin {
            Some(origin) => origin.xy(),
            None => return false,
        };
        let line_h = self.font_size * self.line_height;
        for placed in &mut self.placed {
            let callback = match placed.span.on_click.clone() {
                Some(callback) => callback,
                None => continue,
            };
            let bounds = rectf(
                origin.x + placed.x,
                origin.y + placed.y,
                placed.width.max(1.0),
                line_h,
            );
            let mut on_click = move || (callback.borrow_mut())();
            if handle_mouse_event(&mut placed.state, event, bounds, ctx, &mut on_click) {
                return true;
            }
        }
        false
    }
}

/// Resolve an inline span's style to a renderable [`TextSpan`] using theme colors.
/// `italic` selects the oblique face; `is_link` is true when the span is a
/// hyperlink (rendered in the accent color).
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
    TextSpan {
        text,
        weight,
        family,
        italic,
        underline: false,
        color,
        background,
        on_click: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::RenderCommand;

    /// Lay the spans out at `width` and paint them, returning the box and the
    /// commands: what the paragraph reserves and what it actually draws.
    fn painted(width: f32, spans: Vec<TextSpan>) -> (Vector2F, Vec<RenderCommand>) {
        let app = AppContext::default();
        let mut element: Box<dyn Element> = Box::new(InlineText::new(spans).with_font_size(12.0));
        let commands = crate::test_util::render_element(&mut element, vec2f(width, 4000.0), &app);
        let size = element.size().expect("the paragraph lays out");
        (size, commands)
    }

    /// Every drawn run of `commands`, as (origin, text, size, line height, wrap
    /// width).
    fn drawn_runs(commands: &[RenderCommand]) -> Vec<(Vector2F, String, f32, f32, f32)> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText {
                    origin,
                    text,
                    font_size,
                    line_height,
                    max_width,
                    ..
                } => Some((*origin, text.clone(), *font_size, *line_height, *max_width)),
                _ => None,
            })
            .collect()
    }

    /// The space between two runs of different style is drawn, and the run after
    /// it starts one advance past the run before: budgeting by the measured ink
    /// instead leaves every following word a bearing too close.
    #[test]
    fn inline_text_keeps_the_spaces_between_runs() {
        let (_, commands) = painted(
            400.0,
            vec![
                TextSpan::plain("see "),
                TextSpan::plain("Goble").with_weight(FontWeight::Bold),
                TextSpan::plain(" for details"),
            ],
        );
        let runs = drawn_runs(&commands);
        let drawn: String = runs.iter().map(|(_, text, ..)| text.as_str()).collect();
        assert_eq!(
            drawn, "see Goble for details",
            "the spaces between the runs are part of the paragraph"
        );
        let (origin, ..) = runs
            .iter()
            .find(|(_, text, ..)| text == "Goble")
            .expect("the bold run is drawn");
        assert_eq!(
            origin.x,
            advance_width("see ", 12.0, FontWeight::Regular, FontFamily::System, false),
            "the bold run starts where the plain run's advance ends"
        );
    }

    /// A run of several spaces is a run of several spaces: nothing collapses it
    /// to one, and nothing drops it.
    #[test]
    fn inline_text_keeps_a_run_of_spaces() {
        let (_, commands) = painted(400.0, vec![TextSpan::plain("a  b   c")]);
        let runs = drawn_runs(&commands);
        let drawn: String = runs.iter().map(|(_, text, ..)| text.as_str()).collect();
        assert_eq!(drawn, "a  b   c");
        assert_eq!(
            runs.len(),
            1,
            "one style draws as one run, got {runs:?}"
        );
        let (origin, _, _, _, max_width) = runs[0];
        assert_eq!(origin.x, 0.0);
        assert!(
            max_width >= advance_width("a  b   c", 12.0, FontWeight::Regular, FontFamily::System, false),
            "the run is given room for every space it holds, got {max_width}"
        );
    }

    /// No drawn run may leave the paragraph's box: each is measured at the width
    /// it is drawn at, so a run the flow placed on one line stays on that line
    /// instead of wrapping inside its own quad and spilling over the line below.
    #[test]
    fn inline_text_draws_every_run_inside_the_paragraph() {
        let paragraph = "The agent wrote a long paragraph of prose that has to reflow on every resize and it keeps going";
        for width in [120.0_f32, 200.0, 240.0, 300.0, 360.0] {
            let (size, commands) = painted(width, vec![TextSpan::plain(paragraph)]);
            let runs = drawn_runs(&commands);
            assert!(!runs.is_empty(), "the paragraph draws at {width}");
            for (origin, text, font_size, line_height, max_width) in &runs {
                let drawn = measure_text_family(
                    text,
                    *font_size,
                    *line_height,
                    *max_width,
                    FontWeight::Regular,
                    FontFamily::System,
                    false,
                );
                assert!(
                    origin.x + drawn.x <= width + 0.5,
                    "the run {text:?} at x={} is drawn past the {width} wide paragraph, ends at {}",
                    origin.x,
                    origin.x + drawn.x
                );
                assert!(
                    origin.y + font_size * line_height <= size.y + 0.5,
                    "the run {text:?} at y={} is placed past the paragraph's {} of lines",
                    origin.y,
                    size.y
                );
                assert!(
                    drawn.y <= (font_size * line_height).ceil() + 0.5,
                    "the run {text:?} is drawn over more than its one line box: {}",
                    drawn.y
                );
            }
        }
    }

    /// A narrower constraint re-wraps the paragraph, and the run widths it draws
    /// with come down with it.
    #[test]
    fn inline_text_rewraps_and_narrows_when_the_constraint_shrinks() {
        let spans = || vec![TextSpan::plain("one two three four five six seven eight nine")];
        let (wide_size, wide_commands) = painted(400.0, spans());
        let (narrow_size, narrow_commands) = painted(150.0, spans());
        let widest = |commands: &[RenderCommand]| {
            drawn_runs(commands)
                .iter()
                .map(|(_, _, _, _, max_width)| *max_width)
                .fold(0.0_f32, f32::max)
        };
        assert!(
            narrow_size.y > wide_size.y,
            "the paragraph takes more lines at 150 than at 400"
        );
        assert!(
            widest(&narrow_commands) <= 150.0 + 0.5,
            "no run is drawn with room past the narrower constraint"
        );
        assert!(
            widest(&wide_commands) > widest(&narrow_commands),
            "the drawn runs come down with the constraint"
        );
    }

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

    /// The right edge of each flow line, in order.
    fn line_right_edges(inline: &InlineText) -> Vec<f32> {
        let mut edges: Vec<(f32, f32)> = Vec::new();
        for placed in &inline.placed {
            match edges.last_mut() {
                Some((y, right)) if (*y - placed.y).abs() < 0.5 => {
                    *right = right.max(placed.x + placed.width);
                }
                _ => edges.push((placed.y, placed.x + placed.width)),
            }
        }
        edges.into_iter().map(|(_, right)| right).collect()
    }

    /// A paragraph whose second run does not fit on the first line fills that
    /// line with the words that do fit, instead of dropping the whole run into
    /// the space left over — which drew a second, narrow column of text beside
    /// the first line and then restarted underneath it.
    #[test]
    fn inline_text_does_not_draw_a_second_narrow_column() {
        let app = AppContext::default();
        let width = 320.0;
        let mut inline = InlineText::new(vec![
            TextSpan::plain("Nested "),
            TextSpan::plain("code causes a bug where the text does not continue on the line and instead forms a narrow")
                .with_weight(FontWeight::Bold),
        ]);
        inline.layout(
            SizeConstraint::loose(vec2f(width, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );

        assert!(
            !inline.placed.is_empty(),
            "the paragraph places at least one run"
        );
        for placed in &inline.placed {
            assert!(
                placed.x + placed.width <= width + 0.5,
                "a run is drawn past the paragraph width: {placed_x} + {placed_w}",
                placed_x = placed.x,
                placed_w = placed.width
            );
            // A run that does not start at the left edge sits on a line that
            // already holds text, so it can only be one line tall. A run placed
            // whole into the space left on a line is drawn wrapped inside that
            // space — the narrow column.
            let line_h = inline.font_size * inline.line_height;
            let height = measure_text_family(
                &placed.span.text,
                inline.font_size,
                inline.line_height,
                placed.max_width,
                placed.span.weight,
                placed.span.family,
                placed.span.italic,
            )
            .y;
            assert!(
                placed.x == 0.0 || height <= line_h.ceil() + 0.5,
                "the run at x={} is drawn wrapped in the space left on its line: {:?}",
                placed.x,
                placed.span.text
            );
        }
        let edges = line_right_edges(&inline);
        assert!(edges.len() >= 2, "the paragraph takes several lines");
        for (line, right) in edges.iter().enumerate().take(edges.len() - 1) {
            assert!(
                *right > width * 0.6,
                "line {line} stops at {right} of {width}: the words that fit are not packed onto it"
            );
        }
    }

    /// A single run with no break in it — a URL, a path — is drawn in the full
    /// width, wrapped inside it, and the flow continues below the lines it took.
    #[test]
    fn inline_text_keeps_a_word_wider_than_the_line_in_the_flow() {
        let app = AppContext::default();
        let width = 120.0;
        let mut inline = InlineText::new(vec![
            TextSpan::plain("see "),
            TextSpan::plain("a-very-long-unbreakable-path-that-is-wider-than-the-paragraph"),
            TextSpan::plain(" above"),
        ]);
        let size = inline.layout(
            SizeConstraint::loose(vec2f(width, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );

        for placed in &inline.placed {
            assert!(
                placed.x + placed.width <= width + 0.5,
                "a run is drawn past the paragraph width: {placed_x} + {placed_w}",
                placed_x = placed.x,
                placed_w = placed.width
            );
        }
        let edges = line_right_edges(&inline);
        assert!(
            edges.len() >= 3,
            "the wide run takes several lines of its own and the flow continues after it"
        );
        // The block is as tall as the lines its runs occupy: the flow's height
        // covers the wrapped wide run rather than the single line it started on.
        let line_h = inline.font_size * inline.line_height;
        assert!(
            size.y > line_h * 2.0,
            "the wrapped run's lines are counted in the paragraph height"
        );
    }
}
