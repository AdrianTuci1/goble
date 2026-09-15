use std::cell::RefCell;
use std::rc::Rc;

use super::data::{TerminalData, TerminalMeta, TerminalStatus};
use super::filter::{terminal_filter_bar, TerminalCopyHandler, TerminalFilter, FILTER_BUTTON_SIZE};
use super::line::{TerminalLine, TerminalLineKind};
use crate::color::ColorU;
use crate::elements::{
    AppContext, Clipped, ConstrainedBox, CrossAxisAlignment, Divider, Element, Flex, Icon,
    InlineText, LayoutContext, MainAxisAlignment, MainAxisSize, PaintContext, Point,
    SizeConstraint, Spacer, Text, TextSpan, TopbarButton,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, Vector2F};
use crate::platform::text_atlas::{measure_text_family, FontWeight};
use crate::theme::{ColorToken, FontFamily};

const FONT_SIZE: f32 = 13.0;

const LINE_HEIGHT: f32 = 1.4;

/// Widest the header's prompt row is allowed to draw before it ellipsizes, so a
/// deep working directory never pushes the block's controls off its edge.
const PROMPT_MAX_WIDTH: f32 = 420.0;

/// The widest a block's filter bar draws; narrow blocks give it what they have.
const FILTER_BAR_MAX_WIDTH: f32 = 380.0;

/// How far a failure tints the whole block, over the pane's own background.
const FAILURE_WASH_ALPHA: u8 = 22;

/// The pole a running or failed block stands on its left edge (warp-new's flag
/// pole), in px.
const FLAG_POLE_WIDTH: f32 = 3.0;

pub(crate) const HEADER_SPACING: f32 = 8.0;

pub(crate) const BUTTON_SIZE: f32 = 24.0;

const PROMPT_PREFIX: &str = "❯ ";

/// The block filter bar's field, named the way warp names it.
const FILTER_PLACEHOLDER: &str = "Filter block output";

/// Build the one terminal block element.
///
/// Every place a block appears draws it through here — a terminal fragment in
/// the transcript, the segment a command the agent ran is drawn as, and the
/// pane's own executed-command block — so one block has one renderer.
pub fn terminal_block(
    data: &TerminalData,
    filter: TerminalFilter,
    global_filter: Option<TerminalFilter>,
    on_copy: Option<TerminalCopyHandler>,
) -> Box<dyn Element> {
    TerminalBlock::new()
        .with_title(data.title.clone())
        .with_status(data.status.unwrap_or_default())
        .with_lines(data.lines.clone())
        .with_meta(data.meta.clone())
        .with_filter(filter)
        .with_global_filter(global_filter)
        .with_on_copy(move |text| {
            if let Some(cb) = on_copy.as_ref() {
                (cb.borrow_mut())(text);
            }
        })
        .finish()
}

/// A terminal-style command block matching warp's layout: one mono prompt row
/// (the block's identity and the context it ran in) with the copy and filter
/// controls shown while the pointer is over the block, a filter bar over the
/// block's own output, and monospaced command/output lines. A running block
/// stands an accent pole on its left edge and a failed one washes the block and
/// stands the pole in the error colour.
pub struct TerminalBlock {
    title: String,
    lines: Vec<TerminalLine>,
    status: Option<TerminalStatus>,
    /// The block's context line (where it ran, how long it took), drawn beside
    /// the title at the top of the section.
    meta: Option<TerminalMeta>,
    filter: TerminalFilter,
    global_filter: Option<TerminalFilter>,
    on_copy: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    root: Option<Box<dyn Element>>,
    pub(super) size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Default for TerminalBlock {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalBlock {
    pub fn new() -> Self {
        Self {
            title: "Terminal".to_string(),
            lines: Vec::new(),
            status: None,
            meta: None,
            filter: TerminalFilter::default(),
            global_filter: None,
            on_copy: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn with_lines(mut self, lines: impl IntoIterator<Item = TerminalLine>) -> Self {
        self.lines.extend(lines);
        self
    }

    pub fn with_line(mut self, line: TerminalLine) -> Self {
        self.lines.push(line);
        self
    }

    pub fn with_status(mut self, status: TerminalStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// Attach the block's context (working directory, branch, duration), drawn
    /// in the header beside the title.
    pub fn with_meta(mut self, meta: Option<TerminalMeta>) -> Self {
        self.meta = meta;
        self
    }

    /// Attach the app-owned per-block filter state: the bar's open flag, the
    /// query typed into it and the line kinds it keeps.
    pub fn with_filter(mut self, filter: TerminalFilter) -> Self {
        self.filter = filter;
        self
    }

    /// Attach an optional whole-surface filter (a pane's own terminal, or the
    /// transcript's). A line is shown only if it matches this filter *and* the
    /// block's own.
    pub fn with_global_filter(mut self, filter: Option<TerminalFilter>) -> Self {
        self.global_filter = filter;
        self
    }

    /// Set the copy callback; it receives the block's full display text.
    pub fn with_on_copy<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_copy = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub(super) fn line_text(line: &TerminalLine) -> String {
        match line.kind {
            TerminalLineKind::Command => format!("{PROMPT_PREFIX}{}", line.text),
            _ => line.text.clone(),
        }
    }

    fn color_token(line: &TerminalLine) -> ColorToken {
        match line.kind {
            TerminalLineKind::Command | TerminalLineKind::Output => ColorToken::Text,
            TerminalLineKind::Info => ColorToken::Muted,
            TerminalLineKind::Success => ColorToken::Success,
            TerminalLineKind::Error => ColorToken::Error,
        }
    }

    /// One line as it is drawn: its styled runs when it carries colour or
    /// emphasis, otherwise its own text in the line kind's colour.
    fn line_element(line: &TerminalLine, color: ColorToken, app: &AppContext) -> Box<dyn Element> {
        if line.runs.is_empty() {
            return Text::new(line.text.clone())
                .with_theme_color(color, app)
                .with_font_size(FONT_SIZE)
                .with_font_family(FontFamily::Mono)
                .finish();
        }
        let spans = line
            .runs
            .iter()
            .map(|run| {
                let mut span = TextSpan::plain(run.text.clone())
                    .with_family(FontFamily::Mono)
                    .with_color(run.color.unwrap_or_else(|| app.theme.color(color)));
                if run.bold {
                    span = span.with_weight(FontWeight::Bold);
                }
                if run.italic {
                    span = span.with_italic(true);
                }
                if run.underline {
                    span = span.with_underline(true);
                }
                span
            })
            .collect();
        InlineText::new(spans)
            .with_font_size(FONT_SIZE)
            .with_line_height(LINE_HEIGHT)
            .finish()
    }

    /// Whether a line passes the block's own filter and, if one is wired, the
    /// whole-surface filter: only a line both keep is drawn.
    fn line_shown(
        block: &TerminalFilter,
        global: Option<&TerminalFilter>,
        line: &TerminalLine,
    ) -> bool {
        block.matches(line.kind, &line.text)
            && global.map_or(true, |g| g.matches(line.kind, &line.text))
    }

    /// The pole a block stands on its left edge: the accent one while its
    /// command runs, the error one once it failed. `None` for a block that is
    /// neither, which is drawn with no mark of its own.
    const fn status_pole(status: Option<TerminalStatus>) -> Option<ColorToken> {
        match status {
            Some(TerminalStatus::Running) => Some(ColorToken::Accent),
            Some(TerminalStatus::Error) => Some(ColorToken::Error),
            _ => None,
        }
    }

    fn content_width(&self, max_text_width: f32) -> f32 {
        self.lines
            .iter()
            .map(|line| {
                measure_text_family(
                    &Self::line_text(line),
                    FONT_SIZE,
                    LINE_HEIGHT,
                    max_text_width,
                    FontWeight::Regular,
                    FontFamily::Mono,
                    false,
                )
                .x
            })
            .fold(0.0, f32::max)
    }

    /// The header's prompt row: the block's identity and the context it carries
    /// (working directory, branch, duration), one string the way a shell prompt
    /// writes them. An empty title and no context leaves the row empty, which a
    /// block with nothing to say above its command draws.
    fn prompt_label(&self) -> String {
        let mut parts = Vec::new();
        if !self.title.is_empty() {
            parts.push(self.title.clone());
        }
        if let Some(meta) = &self.meta {
            parts.extend(meta.labels());
        }
        parts.join(" ")
    }

    /// The prompt row drawn: one mono label in the muted colour, clipped at
    /// [`PROMPT_MAX_WIDTH`] so a deep path ellipsizes instead of pushing the
    /// block's controls off the edge. The block's state is not in this label:
    /// a failure is the wash and the pole under it.
    fn prompt_element(&self, app: &AppContext) -> Box<dyn Element> {
        let text = Text::new(self.prompt_label())
            .with_theme_color(ColorToken::Muted, app)
            .with_font_size(FONT_SIZE)
            .with_line_height(LINE_HEIGHT)
            .with_font_family(FontFamily::Mono)
            .with_max_lines(1)
            .finish();
        ConstrainedBox::new(Clipped::new(text).finish())
            .with_max_width(PROMPT_MAX_WIDTH)
            .finish()
    }

    /// How many of the block's lines the block's own filter keeps, and how many
    /// it has — the count beside its filter bar's field.
    fn filter_counts(&self) -> (usize, usize) {
        let matched = self
            .lines
            .iter()
            .filter(|line| self.filter.matches(line.kind, &line.text))
            .count();
        (matched, self.lines.len())
    }

    fn full_text(&self) -> String {
        self.lines
            .iter()
            .map(Self::line_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn rebuild(&mut self, app: &AppContext) {
        let sm = app.theme.spacing_px(crate::theme::SpacingToken::Sm);
        let muted = ColorToken::Muted;

        // Header: warp's block header. One mono prompt row carries the block's
        // identity and the context it ran in, and the row is flex-grown so the
        // Spacer pushes the controls to the trailing edge. The controls are the
        // block's own, so they show only while the pointer is over the block (or
        // while the filter bar they open is up); state is not in the label — a
        // failure is drawn under the block by `paint`.
        let mut header = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(HEADER_SPACING)
            .with_child(self.prompt_element(app))
            .with_child(Spacer::new().finish());

        let controls_shown = self.filter.is_hovered() || self.filter.is_open();
        if controls_shown {
            // Copy button: passes the block's display text to the app's copy
            // handler.
            let on_copy = self.on_copy.clone();
            let copy_text = self.full_text();
            let copy_button = TopbarButton::new(
                Icon::new("copy")
                    .with_size(14.0)
                    .with_theme_color(muted, app)
                    .finish(),
            )
            .with_size(BUTTON_SIZE)
            .with_on_click(move || {
                if let Some(cb) = on_copy.as_ref() {
                    (cb.borrow_mut())(copy_text.clone());
                }
            })
            .finish();
            header = header.with_child(copy_button);

            // Filter button: shows the block's filter bar under the header.
            let filter = self.filter.clone();
            let filter_open = filter.is_open();
            let filter_for_toggle = filter.clone();
            let filter_button = TopbarButton::new(
                Icon::new("sliders")
                    .with_size(14.0)
                    .with_theme_color(muted, app)
                    .finish(),
            )
            .with_size(FILTER_BUTTON_SIZE)
            .with_active(filter_open)
            .with_on_click(move || filter_for_toggle.toggle_bar())
            .finish();
            header = header.with_child(filter_button);
        }
        let header = header.finish();

        // Filter bar: the query field over this block's own output, hung under
        // the header's trailing edge — where the control that opened it sits.
        let bar: Option<Box<dyn Element>> = self.filter.is_open().then(|| {
            let (matched, total) = self.filter_counts();
            let content =
                terminal_filter_bar(&self.filter, FILTER_PLACEHOLDER, matched, total, app);
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::End)
                .with_child(
                    ConstrainedBox::new(content)
                        .with_max_width(FILTER_BAR_MAX_WIDTH)
                        .finish(),
                )
                .finish()
        });

        // Body: mono lines the block's own filter keeps and, if one is wired,
        // the whole-surface filter too. The lines are stretched to the resolved
        // block width so a too-long line wraps at the block edge rather than
        // overflowing the card.
        let global = self.global_filter.clone();
        let mut body = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm / 2.0);
        for line in self
            .lines
            .iter()
            .filter(|line| Self::line_shown(&self.filter, global.as_ref(), line))
        {
            let color = Self::color_token(line);
            if line.kind == TerminalLineKind::Command {
                let row = Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Text::new(PROMPT_PREFIX.to_string())
                            .with_theme_color(ColorToken::Accent, app)
                            .with_font_size(FONT_SIZE)
                            .with_font_family(FontFamily::Mono)
                            .finish(),
                    )
                    .with_child(Self::line_element(line, color, app))
                    .finish();
                body = body.with_child(row);
            } else {
                body = body.with_child(Self::line_element(line, color, app));
            }
        }

        // A section of the page, not a card: a hairline over the header
        // separates it from what is above, and the rows below it carry no fill,
        // no border and no corner. The block is drawn at the full width it is
        // given (see `layout`), the way a command reads in a terminal rather
        // than in a panel floating over one.
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm)
            .with_child(Divider::horizontal().finish())
            .with_child(header);
        if let Some(bar) = bar {
            column = column.with_child(bar);
        }
        self.root = Some(column.with_child(body.finish()).finish());
    }

    /// The block's own state, drawn under its content (warp-new's failing
    /// block): a failure washes the whole block in the error colour and stands
    /// a pole on its left edge, and a running command keeps the pole in the
    /// accent colour. A finished, successful block draws neither.
    fn paint_status(&self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        let Some(token) = Self::status_pole(self.status) else {
            return;
        };
        let Some(size) = self.size else {
            return;
        };
        let color = app.theme.color(token);
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        if self.status == Some(TerminalStatus::Error) {
            renderer.fill_rect(
                rectf(origin.x, origin.y, size.x, size.y),
                ColorU::new(color.r, color.g, color.b, FAILURE_WASH_ALPHA),
            );
        }
        renderer.fill_rect(rectf(origin.x, origin.y, FLAG_POLE_WIDTH, size.y), color);
    }

    fn resolve_width(&self, max_text_width: f32) -> f32 {
        let content_w = self.content_width(max_text_width);
        // Conservative minimum for the header (its prompt row, the copy and
        // filter controls and their spacing) so the right-aligned controls never
        // clip. The Spacer is flexible (min 0), so only the row and the two
        // buttons count, plus a gap per item and two more around the spacer.
        let prompt_w = measure_text_family(
            &self.prompt_label(),
            FONT_SIZE,
            LINE_HEIGHT,
            PROMPT_MAX_WIDTH.min(max_text_width),
            FontWeight::Regular,
            FontFamily::Mono,
            false,
        )
        .x;
        let header_min = prompt_w + 4.0 * HEADER_SPACING + 2.0 * BUTTON_SIZE;
        content_w.max(header_min).max(0.0)
    }
}

impl Element for TerminalBlock {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // Full width: the block is the pane's own row, so it spans what the
        // pane offers instead of sizing itself to its content. A caller that
        // measures it with no width at all (an unbounded constraint) still gets
        // the width its own lines need.
        let block_width = if constraint.max.x.is_finite() && constraint.max.x > 0.0 {
            constraint.max.x
        } else {
            self.resolve_width(f32::INFINITY)
        };
        self.rebuild(app);
        let mut root = ConstrainedBox::new(self.root.take().expect("rebuild set root"))
            .with_width(block_width);
        let size = root.layout(constraint, ctx, app);
        self.root = Some(Box::new(root));
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.paint_status(origin, ctx, app);
        if let Some(root) = self.root.as_mut() {
            root.paint(origin, ctx, app);
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
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        match event {
            // Whether the pointer is over the block decides whether its controls
            // are drawn. The flag is app-owned, so it is written here and read by
            // the next frame's build.
            DispatchedEvent::MouseMove { position } => {
                let inside = self
                    .bounds()
                    .map(|bounds| crate::elements::interactive::contains(bounds, *position))
                    .unwrap_or(false);
                self.filter.set_hovered(inside);
            }
            // Escape puts the block's output back and takes the bar away with it.
            DispatchedEvent::KeyDown { key, .. } => {
                if key == "Escape" && self.filter.is_open() {
                    self.filter.close_bar();
                    return true;
                }
            }
            _ => {}
        }
        self.root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}
