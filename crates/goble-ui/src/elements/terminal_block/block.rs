use std::cell::RefCell;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::{AppContext, Border, Clipped, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets, Element, Fill, Flex, Icon, InlineText, LayoutContext, MainAxisSize, PaintContext, Point, PopupMenu, PopupMenuItem, PopupMenuPosition, SizeConstraint, Spacer, Text, TextSpan, TopbarButton};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::platform::text_atlas::{measure_text_family, FontWeight};
use crate::theme::{ColorToken, FontFamily};
use super::data::{TerminalData, TerminalMeta, TerminalStatus};
use super::filter::{FILTERS, TerminalCopyHandler, TerminalFilter};
use super::line::{TerminalLine, TerminalLineKind};

/// Blend two colours in sRGB space, weight `t` toward `b`.
fn mix(a: ColorU, b: ColorU, t: f32) -> ColorU {
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    ColorU::new(lerp(a.r, b.r), lerp(a.g, b.g), lerp(a.b, b.b), lerp(a.a, b.a))
}

const FONT_SIZE: f32 = 13.0;

const LINE_HEIGHT: f32 = 1.4;

const HEADER_FONT_SIZE: f32 = 12.0;

/// Font size of the header's context labels (working directory, branch,
/// duration) — smaller than the block's title so the context reads as a note on
/// the command rather than as the command.
const META_FONT_SIZE: f32 = 11.0;

/// Widest a single context label is allowed to draw before it ellipsizes.
const META_MAX_WIDTH: f32 = 240.0;

pub(crate) const PADDING_X: f32 = 12.0;

pub(crate) const PADDING_Y: f32 = 10.0;

pub(crate) const HEADER_SPACING: f32 = 8.0;

pub(crate) const BUTTON_SIZE: f32 = 24.0;

const PROMPT_PREFIX: &str = "❯ ";

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

/// A terminal-style command block matching warp's layout: a single header row
/// with the command identity on the left and copy + filter controls on the
/// right (no icon, no literal status label — state is shown by colour), plus
/// monospaced command/output lines.
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

    /// Attach the app-owned per-block filter state (open flag + selected index).
    pub fn with_filter(mut self, filter: TerminalFilter) -> Self {
        self.filter = filter;
        self
    }

    /// Attach an optional whole-transcript filter. A line is shown only if it
    /// matches this global filter *and* the block's own filter.
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

    fn filter_matches(selected: usize, kind: TerminalLineKind) -> bool {
        FILTERS
            .get(selected)
            .map(|(_, pred)| pred(kind))
            .unwrap_or(true)
    }

    /// Whether a line passes both the block's own filter and, if present, the
    /// whole-transcript filter.
    fn line_shown(
        block_selected: usize,
        global_selected: Option<usize>,
        kind: TerminalLineKind,
    ) -> bool {
        let block_ok = Self::filter_matches(block_selected, kind);
        let global_ok = global_selected.map_or(true, |g| Self::filter_matches(g, kind));
        block_ok && global_ok
    }

    /// Colour for the block identity in the header, conveying state without a
    /// literal label: accent while running, error colour on failure, muted
    /// otherwise.
    const fn title_color(status: Option<TerminalStatus>) -> ColorToken {
        match status {
            Some(TerminalStatus::Running) => ColorToken::Accent,
            Some(TerminalStatus::Error) => ColorToken::Error,
            _ => ColorToken::Muted,
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

    /// The labels the header's left-hand group is made of, in draw order: the
    /// block's title, then the context it carries. An empty title is left out,
    /// so a section whose header is only context draws only that.
    fn header_labels(&self) -> Vec<String> {
        let mut labels = Vec::new();
        if !self.title.is_empty() {
            labels.push(self.title.clone());
        }
        if let Some(meta) = &self.meta {
            labels.extend(meta.labels());
        }
        labels
    }

    /// One header label. The block's own title carries the status colour; the
    /// context labels stay muted, since they describe the command rather than
    /// report on it. A context label is capped so a deep path ellipsizes
    /// instead of pushing the header's controls off the block.
    fn header_label(&self, label: &str, is_title: bool, app: &AppContext) -> Box<dyn Element> {
        let text = Text::new(label.to_string())
            .with_theme_color(
                if is_title {
                    Self::title_color(self.status)
                } else {
                    ColorToken::Muted
                },
                app,
            )
            .with_font_size(if is_title { HEADER_FONT_SIZE } else { META_FONT_SIZE })
            .with_font_family(FontFamily::Mono)
            .with_max_lines(1)
            .finish();
        if is_title {
            return text;
        }
        ConstrainedBox::new(Clipped::new(text).finish())
            .with_max_width(META_MAX_WIDTH)
            .finish()
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
        let radius = app.theme.radius_px();
        let muted = ColorToken::Muted;

        // Header: warp-style command-block header. The label group (the block's
        // title and the context it carries) is content-sized but the row is
        // flex-grown so the Spacer pushes the copy/filter controls to the right
        // edge. There is no leading icon and no literal status label — the block
        // identity carries the state colour and the card is tinted on failure.
        let labels = self.header_labels();
        let title = self.title.clone();
        let mut header = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(HEADER_SPACING);
        for label in &labels {
            let is_title = *label == title;
            let element = self.header_label(label, is_title, app);
            header = header.with_child(element);
        }
        header = header.with_child(Spacer::new().finish());

        // Copy button: passes the block's display text to the app's copy handler.
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

        // Filter button: opens the filter tray selecting which line kinds show.
        let filter = self.filter.clone();
        let selected = *filter.selected.borrow();
        let filter_items = FILTERS
            .iter()
            .enumerate()
            .map(|(i, (label, _))| {
                let mut item = PopupMenuItem::new(*label);
                if i == selected {
                    item = item.selected();
                }
                item
            })
            .collect::<Vec<_>>();
        let filter_trigger = TopbarButton::new(
            Icon::new("sliders")
                .with_size(14.0)
                .with_theme_color(muted, app)
                .finish(),
        )
        .with_size(BUTTON_SIZE)
        .finish();
        let filter_for_select = filter.clone();
        let filter_menu = PopupMenu::new(filter_trigger, filter_items)
            .with_open(filter.open.clone())
            .with_position(PopupMenuPosition::Below)
            .with_on_select(move |idx| {
                *filter_for_select.selected.borrow_mut() = idx;
            })
            .finish();
        header = header.with_child(filter_menu);

        // Body: mono lines filtered by the selected filter. The lines are
        // stretched to the resolved block width so a too-long line wraps at the
        // block edge rather than overflowing the card.
        let global_selected = self.global_filter.as_ref().map(|g| *g.selected.borrow());
        let mut body = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm / 2.0);
        for line in self
            .lines
            .iter()
            .filter(|l| Self::line_shown(selected, global_selected, l.kind))
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

        // The card is tinted toward the error colour on failure (warp paints a
        // red-tinted block background) and stays default otherwise.
        let base_bg = app.theme.color(ColorToken::SurfaceRaised);
        let bg = match self.status {
            Some(TerminalStatus::Error) => mix(base_bg, app.theme.color(ColorToken::Error), 0.10),
            _ => base_bg,
        };
        let card = Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(sm)
                .with_child(header.finish())
                .with_child(body.finish())
                .finish(),
        )
        .with_background(Fill::Solid(bg))
        .with_border(Border::all(1.0).with_border_fill(Fill::Solid(app.theme.color(ColorToken::Border))))
        .with_padding(EdgeInsets::new(PADDING_Y, PADDING_X, PADDING_Y, PADDING_X))
        .with_corner_radius(radius)
        .finish();

        // Width is offered below (in layout) via `self.constraint_width`; the
        // block stays content-adaptive. Store a placeholder width of 0 here and
        // let `layout` rebuild with the resolved width.
        self.root = Some(card);
    }

    fn resolve_width(&self, max_text_width: f32) -> f32 {
        let content_w = self.content_width(max_text_width);
        let title = self.title.clone();
        // Conservative minimum for the header (its labels + copy + filter and
        // their spacing) so the right-aligned buttons never clip. The Spacer is
        // flexible (min 0), so only the labels and the two buttons count, plus
        // one gap per label and two more around the spacer.
        let labels = self.header_labels();
        let labels_w: f32 = labels
            .iter()
            .map(|label| {
                let is_title = *label == title;
                measure_text_family(
                    label,
                    if is_title { HEADER_FONT_SIZE } else { META_FONT_SIZE },
                    1.2,
                    if is_title { max_text_width } else { META_MAX_WIDTH },
                    FontWeight::Regular,
                    FontFamily::Mono,
                    false,
                )
                .x
            })
            .sum();
        let header_min =
            labels_w + (labels.len() as f32 + 2.0) * HEADER_SPACING + 2.0 * BUTTON_SIZE;
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
        let max_text_width = (constraint.max.x - 2.0 * PADDING_X).max(0.0);
        let block_width = (self.resolve_width(max_text_width) + 2.0 * PADDING_X)
            .min(constraint.max.x);
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
        self.root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}
