//! The work dock under the agent pane's header: one collapsible group per kind
//! of work the pane or the workspace has in flight — workflows, sub-agents,
//! tasks and watchers — with the controls that enter and stop each row.
//!
//! The dock is grok-build's dock: a non-empty group always keeps its header
//! (chevron, title and count), a group folds to its header alone, and an open
//! group lists its rows behind one leading dot. A live row's dot is the spinner
//! shared with the turn footer; a resting row's is a static dot. A group longer
//! than [`SECTION_ROW_CAP`] rows closes with `show N more`, which reveals the
//! rest. Each row states its kind and description, the activity it is doing,
//! and a right-aligned meta column (model, elapsed, schedule) with the open and
//! stop controls at the row's far edge.

use std::cell::RefCell;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::chat_typography;
use crate::elements::content_lane::lane_column;
use crate::elements::running_indicator::SPINNER_FRAMES;
use crate::elements::{
    AppContext, Chip, Container, CrossAxisAlignment, EdgeInsets, Element, Expanded, Flex, HoverRow,
    Icon, InlineText, LayoutContext, MainAxisSize, Padding, PaintContext, Point, SizeConstraint, Text,
    TextSpan,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

/// A group header's title size. DSH's announcement card draws its title at the
/// content size (14 px) over the row's 24 px line.
const FONT_SIZE: f32 = chat_typography::BODY_SIZE;
const TITLE_LINE_HEIGHT: f32 = chat_typography::BODY_LH;

/// The count is the card header's trailing "time" column: DSH's 12 / 18 small
/// tier, right-aligned at the card's other edge.
const META_FONT_SIZE: f32 = chat_typography::SMALL_SIZE;
const META_LINE_HEIGHT: f32 = chat_typography::SMALL_LH;

/// A body row's line: DSH's 13 / 24 process-row type.
const ROW_FONT_SIZE: f32 = chat_typography::ROW_SIZE;
const LINE_HEIGHT: f32 = chat_typography::ROW_LH;

/// The vertical room a row keeps above and below its line.
const ROW_PADDING_Y: f32 = 4.0;

/// The chevron, the leading dot and the row's two controls.
const MARK_SIZE: f32 = 12.0;

/// Rows a group lists before it closes with `show N more`.
const SECTION_ROW_CAP: usize = 4;

/// The leading dot a resting row draws, where a live row draws the spinner.
const RESTING_DOT: &str = "·";

/// The dock's groups, in the order they are drawn. The wire value of each is
/// the key the app stores its fold under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WorkSection {
    Workflows,
    Subagents,
    Tasks,
    Watchers,
}

impl WorkSection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Workflows => "Workflows",
            Self::Subagents => "Subagents",
            Self::Tasks => "Tasks",
            Self::Watchers => "Watchers",
        }
    }

    /// The stable key the app stores the group's fold under.
    pub fn key(self) -> &'static str {
        match self {
            Self::Workflows => "workflows",
            Self::Subagents => "subagents",
            Self::Tasks => "tasks",
            Self::Watchers => "watchers",
        }
    }

    /// The group a stored key names, if it names one.
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "workflows" => Some(Self::Workflows),
            "subagents" => Some(Self::Subagents),
            "tasks" => Some(Self::Tasks),
            "watchers" => Some(Self::Watchers),
            _ => None,
        }
    }
}

/// One row of the dock: a live piece of work and the controls it offers.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkRow {
    /// What the row's controls carry back to the app.
    pub id: String,
    /// The row's kind, in the dock's own word: `Workflow`, `Subagent`, `Task`,
    /// `Loop`.
    pub kind: String,
    pub description: String,
    /// What the row is doing, drawn after an em dash. `None` draws none.
    pub activity: Option<String>,
    /// The right-aligned meta column: a model, an elapsed time or a schedule.
    pub meta: String,
    /// Whether the row's leading dot spins. A live row spins; a scheduled loop
    /// keeps the static dot.
    pub spinning: bool,
    /// Whether the row offers the control that opens it.
    pub openable: bool,
    /// Whether the row offers the control that stops it.
    pub killable: bool,
}

impl WorkRow {
    /// A row with nothing live about it: a static dot, no activity and no
    /// controls until the caller asks for them.
    pub fn new(id: impl Into<String>, kind: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            description: description.into(),
            activity: None,
            meta: String::new(),
            spinning: false,
            openable: false,
            killable: false,
        }
    }

    pub fn with_activity(mut self, activity: impl Into<String>) -> Self {
        self.activity = Some(activity.into());
        self
    }

    pub fn with_meta(mut self, meta: impl Into<String>) -> Self {
        self.meta = meta.into();
        self
    }

    pub fn spinning(mut self, spinning: bool) -> Self {
        self.spinning = spinning;
        self
    }

    pub fn openable(mut self, openable: bool) -> Self {
        self.openable = openable;
        self
    }

    pub fn killable(mut self, killable: bool) -> Self {
        self.killable = killable;
        self
    }
}

/// One group of the dock, under the header that folds it.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkGroup {
    pub section: WorkSection,
    pub rows: Vec<WorkRow>,
    /// Whether the group's rows are drawn. A folded group keeps its header.
    pub open: bool,
    /// Whether `show N more` has been pressed: the rest of the rows are drawn.
    pub show_all: bool,
}

impl WorkGroup {
    pub fn new(section: WorkSection, rows: Vec<WorkRow>, open: bool) -> Self {
        Self {
            section,
            rows,
            open,
            show_all: false,
        }
    }

    pub fn with_show_all(mut self, show_all: bool) -> Self {
        self.show_all = show_all;
        self
    }

    /// The rows this group draws now: the first [`SECTION_ROW_CAP`] unless the
    /// user asked for the rest.
    fn visible_rows(&self) -> &[WorkRow] {
        if self.show_all {
            &self.rows
        } else {
            &self.rows[..self.rows.len().min(SECTION_ROW_CAP)]
        }
    }

    /// The rows this group still holds behind `show N more`.
    fn hidden_rows(&self) -> usize {
        self.rows.len().saturating_sub(self.visible_rows().len())
    }
}

/// How the dock's cards are bounded inside its full-width band.
///
/// The band itself always spans the pane: it is the pane's chrome, and the
/// owner asked for a full-width square band with only the text inside it
/// bounded to the tab's own content width (the width the conversation column
/// is centred at). The band's inset is therefore a layout decision for the
/// cards alone, not for the band's paint.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum ContentWidth {
    /// The cards span the band: a pane whose content is not metered by a
    /// content lane (every pane today unless the host wires one).
    #[default]
    Full,
    /// The cards are bounded to the content lane this dock is laid out in:
    /// the same column `ContentLane` resolves for the conversation under it,
    /// from the band's own width and the preference (`None` = the ratio
    /// model, `Some` = the pane's committed width).
    Lane(Option<f32>),
    /// The cards are bounded to an explicit width, centred in the band.
    Exact(f32),
}

impl ContentWidth {
    /// The cards' `(left offset, width)` inside a band `available` wide.
    fn column(self, available: f32) -> (f32, f32) {
        if !available.is_finite() {
            // An unbounded band (a dock measured inside a scroll region):
            // there is no column to resolve, so pass the constraint through.
            return (0.0, available);
        }
        match self {
            Self::Full => (0.0, available.max(0.0)),
            // The lane's own width axis, so the band's cards can never land at
            // a width the conversation under them does not share.
            Self::Lane(preference) => lane_column(available, preference),
            Self::Exact(width) => {
                let width = width.min(available).max(0.0);
                (((available - width) / 2.0).max(0.0), width)
            }
        }
    }
}

/// The dock: every non-empty group, its rows and the controls that reach the
/// app. An empty dock draws nothing at all, so a pane with nothing in flight
/// pays no height for it.
pub struct WorkDock {
    groups: Vec<WorkGroup>,
    /// Whether the cards span the band or are bounded to the pane's content
    /// lane width (centred), and the width that column resolves to.
    content: ContentWidth,
    /// How far the band paints past its own left and right layout edges, so
    /// the pane's chrome band runs flush with the pane's slot while the pane
    /// body below keeps its own inset frame. `0` paints the band exactly as
    /// wide as the dock is laid out.
    horizontal_bleed: f32,
    /// The spinner's phase in `0..1`, supplied by the app so every row in a
    /// frame turns together.
    phase: f32,
    /// Folds or opens one group: the header row's own click.
    on_toggle: Option<Rc<RefCell<dyn FnMut(WorkSection)>>>,
    /// Reveals the rest of one group: the `show N more` row's click.
    on_show_all: Option<Rc<RefCell<dyn FnMut(WorkSection)>>>,
    /// Enters the row's work, carrying the group it belongs to and its id.
    on_open: Option<Rc<RefCell<dyn FnMut(WorkSection, String)>>>,
    /// Stops the row's work, carrying the group it belongs to and its id.
    on_stop: Option<Rc<RefCell<dyn FnMut(WorkSection, String)>>>,
    root: Option<Box<dyn Element>>,
    /// The cards' left offset inside the band, from the last layout.
    content_offset: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl WorkDock {
    pub fn new(groups: Vec<WorkGroup>) -> Self {
        Self {
            groups: groups.into_iter().filter(|group| !group.rows.is_empty()).collect(),
            content: ContentWidth::Full,
            horizontal_bleed: 0.0,
            phase: 0.0,
            on_toggle: None,
            on_show_all: None,
            on_open: None,
            on_stop: None,
            root: None,
            content_offset: 0.0,
            size: None,
            origin: None,
        }
    }

    /// Bound the band's cards to the pane's content lane: the same column
    /// `ContentLane` resolves for the conversation under the band, from the
    /// band's own width and the pane's committed width preference. The band
    /// itself keeps spanning the whole pane.
    pub fn with_content_lane(mut self, preference: Option<f32>) -> Self {
        self.content = ContentWidth::Lane(preference);
        self
    }

    /// Bound the band's cards to an explicit width, centred in the band: the
    /// host that already resolved the pane's lane width hands it in.
    pub fn with_content_width(mut self, width: f32) -> Self {
        self.content = ContentWidth::Exact(width);
        self
    }

    /// Paint the band `bleed` points past its left and right edges, so the
    /// pane's chrome band is flush with the pane's slot while the pane body
    /// below keeps its inset frame (the app passes `PANE_INSET`).
    pub fn with_band_bleed(mut self, bleed: f32) -> Self {
        self.horizontal_bleed = bleed.max(0.0);
        self
    }

    /// Set the spinner's phase in `0..1`. The frame shown advances through the
    /// shared spinner frames, so the dock's dot turns with the turn footer's.
    pub fn with_phase(mut self, phase: f32) -> Self {
        self.phase = phase;
        self
    }

    /// Bind the header's fold.
    pub fn with_on_toggle<F: FnMut(WorkSection) + 'static>(mut self, callback: F) -> Self {
        self.on_toggle = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Bind `show N more`.
    pub fn with_on_show_all<F: FnMut(WorkSection) + 'static>(mut self, callback: F) -> Self {
        self.on_show_all = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Bind the control that enters a row's work.
    pub fn with_on_open(mut self, callback: Rc<RefCell<dyn FnMut(WorkSection, String)>>) -> Self {
        self.on_open = Some(callback);
        self
    }

    /// Bind the control that stops a row's work.
    pub fn with_on_stop(mut self, callback: Rc<RefCell<dyn FnMut(WorkSection, String)>>) -> Self {
        self.on_stop = Some(callback);
        self
    }

    /// Whether any row is live, which keeps the pane animating so the dock's
    /// spinner turns while the work does.
    pub fn is_animating(&self) -> bool {
        self.groups
            .iter()
            .any(|group| group.rows.iter().any(|row| row.spinning))
    }

    /// The spinner glyph this dock draws for its current phase: the same frames
    /// the footer's running indicator uses.
    fn spinner_frame(&self) -> &'static str {
        let count = SPINNER_FRAMES.len();
        let index = (self.phase.rem_euclid(1.0) * count as f32) as usize;
        SPINNER_FRAMES[index.min(count - 1)]
    }

    fn ensure_root(&mut self, app: &AppContext) {
        if self.root.is_some() || self.groups.is_empty() {
            return;
        }
        let md = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        // Each group is one of DSH's quiet inset announcement cards: a rounded
        // hairline card its header folds, its opened body the work the group
        // holds. The cards are the band's *inner* content: the band's own
        // `Surface` and its square, full-pane paint live on the dock element
        // itself (see `WorkDock::paint`), so the band never picks up a radius
        // or a gutter from the cards inside it.
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm);
        for group in &self.groups {
            column = column.with_child(self.card(group, app));
        }
        // The band's own horizontal padding: the cards read as insets inside
        // the band, exactly as they did when the band itself was this wide.
        self.root = Some(
            Padding::new(column.finish(), EdgeInsets::new(md, 0.0, md, 0.0)).finish(),
        );
    }

    /// One group as DSH's announcement card: a quiet inset surface, a hairline,
    /// the announcement radius, a header that folds it and — while open — the
    /// body of work it holds, indented under the card's header.
    fn card(&self, group: &WorkGroup, app: &AppContext) -> Box<dyn Element> {
        let mut body = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.header(group, app));
        if group.open {
            let mut rows = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for row in group.visible_rows() {
                rows = rows.with_child(self.row(group.section, row, app));
            }
            let hidden = group.hidden_rows();
            if hidden > 0 {
                rows = rows.with_child(self.show_more(group.section, hidden, app));
            }
            // DSH's opened body: `padding: 0 16 12 40`, the 40 px left inset
            // hanging the work under the header's title.
            body = body.with_child(
                Padding::new(
                    rows.finish(),
                    EdgeInsets::new(
                        chat_typography::ANNOUNCEMENT_BODY_INDENT,
                        0.0,
                        chat_typography::ANNOUNCEMENT_PAD_X,
                        chat_typography::ANNOUNCEMENT_PAD_Y,
                    ),
                )
                .finish(),
            );
        }
        Container::new(body.finish())
            .with_background(chat_typography::announcement_surface(app))
            .with_border(chat_typography::card_border(app))
            .with_corner_radius(chat_typography::announcement_radius(app))
            .finish()
    }

    /// The card's header: DSH's one-line trigger — a 12 px chevron in its 16 px
    /// box, a 14 / 24 title in the quiet ink, and the count at the card's
    /// trailing edge as the header's 12 / 18 time column. The whole line folds
    /// the group.
    fn header(&self, group: &WorkGroup, app: &AppContext) -> Box<dyn Element> {
        let family = chat_typography::chat_font(app);
        // DSH's card header: tertiary ink that brightens to secondary (Goble's
        // `Text`) with the pointer over the whole header. The header row is
        // built twice — resting and hovered — because a run's ink is fixed when
        // the tree is built, and the chip paints the variant paint chooses.
        let row = |ink: ColorU| {
            let leading = chat_typography::leading_icon(
                if group.open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                MARK_SIZE,
                ink,
            );
            let title = Text::new(group.section.label())
                .with_color(ink)
                .with_font_family(family)
                .with_font_size(FONT_SIZE)
                .with_line_height(TITLE_LINE_HEIGHT)
                .with_max_lines(1)
                .finish();
            let count = Text::new(group.rows.len().to_string())
                .with_color(ink)
                .with_font_family(family)
                .with_font_size(META_FONT_SIZE)
                .with_line_height(META_LINE_HEIGHT)
                .with_max_lines(1)
                .finish();
            let content = Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(chat_typography::ANNOUNCEMENT_GAP)
                .with_child(leading)
                .with_child(Expanded::new(title).with_flex(1.0).finish())
                .with_child(count)
                .finish();
            // DSH's card header padding: 12 px vertical, 16 px horizontal.
            Padding::new(
                content,
                EdgeInsets::new(
                    chat_typography::ANNOUNCEMENT_PAD_X,
                    chat_typography::ANNOUNCEMENT_PAD_Y,
                    chat_typography::ANNOUNCEMENT_PAD_X,
                    chat_typography::ANNOUNCEMENT_PAD_Y,
                ),
            )
            .finish()
        };
        let toggle = self.on_toggle.clone();
        let section = group.section;
        let mut chip = Chip::flat(row(chat_typography::chat_ink_quiet(app)))
            .with_hover_variant(row(chat_typography::chat_ink(app)));
        if let Some(toggle) = toggle {
            chip = chip.with_on_click(move || (toggle.borrow_mut())(section));
        }
        chip.finish()
    }

    /// One body row: DSH's process row — a 16 px leading box holding Goble's
    /// own mark (`·` or the shared spinner frame) with its 6 px gap, the row's
    /// words at 13 / 24 in the quiet ink, and the 12 / 18 meta column at the
    /// row's far edge beside the open and stop controls.
    fn row(&self, section: WorkSection, row: &WorkRow, app: &AppContext) -> Box<dyn Element> {
        let family = chat_typography::chat_font(app);
        let quiet = chat_typography::chat_ink_quiet(app);
        let run = |text: String, color: ColorU| -> TextSpan {
            TextSpan::plain(text).with_color(color).with_family(family)
        };
        let (dot, dot_color) = if row.spinning {
            (self.spinner_frame().to_string(), ColorToken::Warning)
        } else {
            (RESTING_DOT.to_string(), ColorToken::Muted)
        };
        let leading = chat_typography::leading_text(dot, app.theme.color(dot_color), app);
        let mut spans = vec![
            run(row.kind.clone(), quiet),
            run(format!(" {}", row.description), quiet),
        ];
        if let Some(activity) = row.activity.as_deref().filter(|text| !text.is_empty()) {
            spans.push(run(format!(" — {activity}"), quiet));
        }
        let line = InlineText::new(spans)
            .with_font_size(ROW_FONT_SIZE)
            .with_line_height(LINE_HEIGHT)
            .finish();
        let mut line_row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(chat_typography::GLYPH_GAP)
            .with_child(leading)
            .with_child(Expanded::new(line).with_flex(1.0).finish());
        if !row.meta.is_empty() {
            line_row = line_row.with_child(
                Text::new(row.meta.clone())
                    .with_color(quiet)
                    .with_font_family(family)
                    .with_font_size(META_FONT_SIZE)
                    .with_line_height(META_LINE_HEIGHT)
                    .with_max_lines(1)
                    .finish(),
            );
        }
        if row.openable {
            line_row = line_row.with_child(control(
                "arrow-up",
                ColorToken::Muted,
                WorkControl::Open,
                section,
                row,
                self,
                app,
            ));
        }
        if row.killable {
            line_row = line_row.with_child(control(
                "x-close",
                ColorToken::Error,
                WorkControl::Stop,
                section,
                row,
                self,
                app,
            ));
        }
        let padded = Padding::new(
            line_row.finish(),
            EdgeInsets::new(0.0, ROW_PADDING_Y, 0.0, ROW_PADDING_Y),
        )
        .finish();
        // The whole line opens the row's work, the way grok's dock rows open
        // theirs. The controls stay their own targets: the hover row dispatches
        // to its content first.
        let Some(open) = self.on_open.clone().filter(|_| row.openable) else {
            return padded;
        };
        let id = row.id.clone();
        HoverRow::new(padded)
            .with_on_click(move || (open.borrow_mut())(section, id.clone()))
            .finish()
    }

    /// `show N more` closes a long group's body: one 13 / 24 quiet row, aligned
    /// under the body rows' words (past their leading box and gap).
    fn show_more(&self, section: WorkSection, hidden: usize, app: &AppContext) -> Box<dyn Element> {
        let line = InlineText::new(vec![TextSpan::plain(format!(
            "{RESTING_DOT} show {hidden} more"
        ))
        .with_color(chat_typography::chat_ink_quiet(app))
        .with_family(chat_typography::chat_font(app))])
        .with_font_size(ROW_FONT_SIZE)
        .with_line_height(LINE_HEIGHT)
        .finish();
        let mut chip = Chip::flat(
            Padding::new(
                line,
                EdgeInsets::new(
                    chat_typography::LEADING_BOX + chat_typography::GLYPH_GAP,
                    ROW_PADDING_Y,
                    0.0,
                    ROW_PADDING_Y,
                ),
            )
            .finish(),
        );
        if let Some(show_all) = self.on_show_all.clone() {
            chip = chip.with_on_click(move || (show_all.borrow_mut())(section));
        }
        chip.finish()
    }
}

/// Which action a row's icon control fires.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WorkControl {
    Open,
    Stop,
}

/// One of a row's icon controls: a bare hit target around the glyph, firing the
/// dock's own callback with the group and id it belongs to. A dock with no
/// callback bound still draws the glyph; the click simply goes nowhere.
fn control(
    icon: &'static str,
    token: ColorToken,
    which: WorkControl,
    section: WorkSection,
    row: &WorkRow,
    dock: &WorkDock,
    app: &AppContext,
) -> Box<dyn Element> {
    let mut chip = Chip::flat(
        Icon::new(icon)
            .with_size(MARK_SIZE)
            .with_theme_color(token, app)
            .finish(),
    );
    let callback = match which {
        WorkControl::Open => dock.on_open.clone(),
        WorkControl::Stop => dock.on_stop.clone(),
    };
    if let Some(callback) = callback {
        let id = row.id.clone();
        chip = chip.with_on_click(move || (callback.borrow_mut())(section, id.clone()));
    }
    chip.finish()
}

impl Element for WorkDock {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.ensure_root(app);
        let available = constraint.max.x;
        let (offset, width) = self.content.column(available);
        self.content_offset = offset;
        let size = match self.root.as_mut() {
            Some(root) => {
                let child_constraint = if available.is_finite() {
                    SizeConstraint::new(
                        vec2f(0.0, constraint.min.y),
                        vec2f(width.max(0.0), constraint.max.y),
                    )
                } else {
                    constraint
                };
                let child_size = root.layout(child_constraint, ctx, app);
                vec2f(
                    if available.is_finite() {
                        available.max(0.0)
                    } else {
                        child_size.x
                    },
                    child_size.y,
                )
            }
            None => Vector2F::zero(),
        };
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        if size.x <= 0.0 || size.y <= 0.0 {
            return;
        }
        // The band: the pane's full width, square, painted past the dock's own
        // layout edges so it runs flush with the pane's slot. Only the cards
        // inside it are bounded to the content lane.
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rounded_rect(
                rectf(
                    origin.x - self.horizontal_bleed,
                    origin.y,
                    size.x + self.horizontal_bleed * 2.0,
                    size.y,
                ),
                app.theme.color(ColorToken::Surface),
                0.0,
            );
        }
        if let Some(root) = self.root.as_mut() {
            root.paint(origin + vec2f(self.content_offset, 0.0), ctx, app);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn wants_animation(&self) -> bool {
        self.is_animating()
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        match self.root.as_mut() {
            Some(root) => root.dispatch_event(event, ctx, app),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::content_lane::resolve_content_width;
    use crate::elements::EventContext;
    use crate::geometry::{vec2f, RectF};
    use crate::render::{RenderCommand, Renderer};
    use crate::theme::SpacingToken;

    fn row(id: &str, kind: &str, description: &str) -> WorkRow {
        WorkRow::new(id, kind, description)
    }

    fn layout(dock: &mut WorkDock, width: f32) -> Vector2F {
        dock.layout(
            SizeConstraint::loose(vec2f(width, 400.0)),
            &mut LayoutContext::default(),
            &AppContext::default(),
        )
    }

    fn paint(dock: &mut WorkDock, width: f32) -> Vec<RenderCommand> {
        let app = AppContext::default();
        layout(dock, width);
        let mut ctx = PaintContext::new(Renderer::new());
        dock.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    /// Each visual row's runs concatenated, so a line drawn as several runs
    /// reads as one string. Runs are clustered by baseline: a row's leading
    /// glyph sits a few points off its words' baseline.
    fn row_texts(commands: &[RenderCommand]) -> Vec<String> {
        let mut runs: Vec<(f32, f32, String)> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } => {
                    Some((origin.y, origin.x, text.clone()))
                }
                _ => None,
            })
            .collect();
        runs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut clusters: Vec<(f32, Vec<(f32, String)>)> = Vec::new();
        for (y, x, text) in runs {
            match clusters.last_mut() {
                Some((last_y, row)) if (y - *last_y).abs() <= 6.0 => row.push((x, text)),
                _ => clusters.push((y, vec![(x, text)])),
            }
        }
        clusters
            .into_iter()
            .map(|(_, mut row)| {
                row.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut line = String::new();
                let mut previous: Option<f32> = None;
                for (x, text) in row {
                    if let Some(last_x) = previous {
                        if x - last_x >= 8.0 && !line.ends_with(' ') {
                            line.push(' ');
                        }
                    }
                    previous = Some(x);
                    line.push_str(&text);
                }
                line
            })
            .collect()
    }

    fn icon_centre(commands: &[RenderCommand], name: &str, nth: usize) -> Option<Vector2F> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon {
                    name: drawn,
                    origin,
                    size,
                    ..
                } if drawn == name => Some(*origin + vec2f(size / 2.0, size / 2.0)),
                _ => None,
            })
            .nth(nth)
    }

    fn click(dock: &mut WorkDock, position: Vector2F) {
        let app = AppContext::default();
        let mut ctx = EventContext::default();
        for event in [
            DispatchedEvent::MouseDown {
                position,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position,
                button: 0,
            },
        ] {
            dock.dispatch_event(&event, &mut ctx, &app);
        }
    }

    fn groups() -> Vec<WorkGroup> {
        vec![
            WorkGroup::new(
                WorkSection::Tasks,
                vec![
                    row("t1", "Task", "Watch QR rotation over 80 seconds")
                        .with_meta("6.6s")
                        .spinning(true)
                        .openable(true)
                        .killable(true),
                    row("t2", "Task", "Wait for the phone to pair")
                        .with_meta("3m53s")
                        .spinning(true),
                ],
                true,
            ),
            WorkGroup::new(
                WorkSection::Watchers,
                vec![row("w1", "Monitor", "Watch the dev WhatsApp instance")
                    .with_meta("every 5m")
                    .openable(true)],
                true,
            ),
        ]
    }

    /// Every non-empty group keeps a header: the chevron, the word and the
    /// count, with the rows under it while it is open.
    #[test]
    fn each_group_header_names_its_kind_and_count() {
        let commands = paint(&mut WorkDock::new(groups()), 900.0);
        let rows = row_texts(&commands);
        assert!(
            rows.iter().any(|line| line.starts_with("Tasks") && line.ends_with('2')),
            "the tasks header names its count, got {rows:?}"
        );
        assert!(
            rows.iter()
                .any(|line| line.starts_with("Watchers") && line.ends_with('1')),
            "the watchers header names its count, got {rows:?}"
        );
        assert!(
            rows.iter().any(|line| line.contains("Task Watch QR")),
            "the open group lists its rows, got {rows:?}"
        );
        assert_eq!(
            icon_centre(&commands, "chevron-down", 1).is_some(),
            true,
            "an open group shows the down chevron"
        );
    }

    /// An open group lists each row as dot, kind, description, activity and
    /// meta; a live row's dot is a spinner frame and a resting row's is the
    /// static dot.
    #[test]
    fn a_row_states_its_kind_description_and_meta() {
        let mut groups = groups();
        groups[0].rows[0].activity = Some("reading".to_string());
        let commands = paint(&mut WorkDock::new(groups).with_phase(0.0), 900.0);
        let rows = row_texts(&commands);
        let row = rows
            .iter()
            .find(|line| line.contains("Task Watch QR rotation"))
            .unwrap_or_else(|| panic!("the task row is drawn, got {rows:?}"));
        assert!(
            row.starts_with(&format!("{} Task", SPINNER_FRAMES[0])),
            "a live task leads with the spinner, got {row:?}"
        );
        assert!(row.contains("— reading"), "the activity follows, got {row:?}");
        assert!(
            rows.iter().any(|line| line.ends_with("6.6s")),
            "the meta is drawn at the row's edge, got {rows:?}"
        );
        let watcher = rows
            .iter()
            .find(|line| line.contains("Monitor Watch"))
            .unwrap_or_else(|| panic!("the watcher row is drawn, got {rows:?}"));
        assert!(
            watcher.starts_with("· Monitor"),
            "a resting watcher keeps the static dot, got {watcher:?}"
        );
    }

    /// A folded group keeps its header and draws no rows.
    #[test]
    fn a_folded_group_draws_only_its_header() {
        let mut groups = groups();
        groups[0].open = false;
        let rows = row_texts(&paint(&mut WorkDock::new(groups), 900.0));
        assert!(
            rows.iter().any(|line| line.starts_with("Tasks") && line.ends_with('2')),
            "the folded group keeps its header, got {rows:?}"
        );
        assert!(
            !rows.iter().any(|line| line.contains("Watch QR rotation")),
            "a folded group hides its rows, got {rows:?}"
        );
    }

    /// A group longer than the cap closes with `show N more`; pressing it
    /// reveals the rest.
    #[test]
    fn a_long_group_closes_with_show_more() {
        let rows = (0..6)
            .map(|i| row(&format!("t{i}"), "Task", &format!("Task {i}")).spinning(true))
            .collect();
        let mut dock = WorkDock::new(vec![WorkGroup::new(WorkSection::Tasks, rows, true)]);
        let shown = row_texts(&paint(&mut dock, 900.0));
        assert!(
            shown.iter().any(|line| line.contains("show 2 more")),
            "the group states how many rows it hides, got {shown:?}"
        );
        assert!(
            !shown.iter().any(|line| line.contains("Task 5")),
            "the hidden rows are not drawn, got {shown:?}"
        );

        let revealed = Rc::new(RefCell::new(Vec::new()));
        let sink = revealed.clone();
        let mut dock = WorkDock::new(vec![WorkGroup::new(
            WorkSection::Tasks,
            (0..6)
                .map(|i| row(&format!("t{i}"), "Task", &format!("Task {i}")))
                .collect(),
            true,
        )])
        .with_on_show_all(move |section| sink.borrow_mut().push(section));
        let commands = paint(&mut dock, 900.0);
        let more = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text.contains("show 2 more") =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the show-more line is drawn");
        click(&mut dock, more + vec2f(2.0, 2.0));
        assert_eq!(
            *revealed.borrow(),
            vec![WorkSection::Tasks],
            "the show-more line reveals its group"
        );
    }

    /// Clicking a header folds its group through the app-owned callback.
    #[test]
    fn a_header_click_folds_its_group() {
        let toggled = Rc::new(RefCell::new(Vec::new()));
        let sink = toggled.clone();
        let mut dock = WorkDock::new(groups()).with_on_toggle(move |section| {
            sink.borrow_mut().push(section);
        });
        let commands = paint(&mut dock, 900.0);
        let header = icon_centre(&commands, "chevron-down", 0).expect("the first header's chevron");
        click(&mut dock, header);
        assert_eq!(
            *toggled.borrow(),
            vec![WorkSection::Tasks],
            "the header folds the group it belongs to"
        );
    }

    /// The row's controls carry the group and the id they belong to; a row
    /// that is not openable draws no open control.
    #[test]
    fn the_controls_carry_the_row_they_belong_to() {
        let opened = Rc::new(RefCell::new(Vec::new()));
        let stopped = Rc::new(RefCell::new(Vec::new()));
        let open_sink = opened.clone();
        let stop_sink = stopped.clone();
        let mut dock = WorkDock::new(groups())
            .with_on_open(Rc::new(RefCell::new(
                move |section: WorkSection, id: String| {
                    open_sink.borrow_mut().push((section, id))
                },
            )))
            .with_on_stop(Rc::new(RefCell::new(
                move |section: WorkSection, id: String| {
                    stop_sink.borrow_mut().push((section, id))
                },
            )));
        let commands = paint(&mut dock, 900.0);
        let open = icon_centre(&commands, "arrow-up", 0).expect("the first row's open control");
        click(&mut dock, open);
        let commands = paint(&mut dock, 900.0);
        let stop = icon_centre(&commands, "x-close", 0).expect("the first row's stop control");
        click(&mut dock, stop);
        assert_eq!(
            *opened.borrow(),
            vec![(WorkSection::Tasks, "t1".to_string())],
            "the open control enters the row it belongs to"
        );
        assert_eq!(
            *stopped.borrow(),
            vec![(WorkSection::Tasks, "t1".to_string())],
            "the stop control stops the row it belongs to"
        );
        assert!(
            icon_centre(&commands, "arrow-up", 1).is_some(),
            "the watcher keeps its open control"
        );
        assert!(
            icon_centre(&commands, "x-close", 1).is_none(),
            "a row that cannot stop draws no stop control"
        );
    }

    /// The dock reads at the transcript's size and keeps room above and below
    /// each line.
    #[test]
    fn the_rows_are_chat_sized_with_room_around_them() {
        let app = AppContext::default();
        let commands = paint(
            &mut WorkDock::new(vec![WorkGroup::new(
                WorkSection::Subagents,
                vec![row("c1", "Subagent", "Audit the migration").spinning(true)],
                true,
            )]),
            900.0,
        );
        let draws: Vec<f32> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { font_size, .. } => Some(*font_size),
                _ => None,
            })
            .collect();
        assert!(draws.contains(&FONT_SIZE), "the row reads at chat size");
        assert!(
            commands.iter().any(|command| match command {
                RenderCommand::DrawText { origin, .. } => {
                    origin.y >= app.theme.spacing_px(SpacingToken::Xs)
                }
                _ => false,
            }),
            "the row is not pressed against the bar"
        );
    }

    /// An empty dock draws nothing and takes no height.
    #[test]
    fn an_empty_dock_takes_no_room() {
        let mut dock = WorkDock::new(Vec::new());
        assert_eq!(layout(&mut dock, 900.0), Vector2F::zero());
    }

    /// A live row keeps the pane animating; a dock of resting rows does not.
    #[test]
    fn a_live_row_keeps_the_dock_animating() {
        let live = WorkDock::new(groups());
        assert!(live.wants_animation());
        let resting = WorkDock::new(vec![WorkGroup::new(
            WorkSection::Watchers,
            vec![row("w1", "Loop", "every hour")],
            true,
        )]);
        assert!(!resting.wants_animation());
    }
    /// An announcement card is DSH's quiet inset: the announcement radius, a
    /// 0.5 px hairline in the theme's border ink, a header title at 14 / 24 and
    /// the count at 12 / 18 on the card's trailing edge, over 13 / 24 body
    /// rows in the quiet ink.
    #[test]
    fn a_group_is_one_dsh_announcement_card() {
        use crate::elements::chat_typography;

        let app = AppContext::default();
        let commands = paint(
            &mut WorkDock::new(vec![WorkGroup::new(
                WorkSection::Subagents,
                vec![row("c1", "Subagent", "Audit the migration").spinning(true)],
                true,
            )]),
            900.0,
        );

        let radius = chat_typography::announcement_radius(&app);
        let card = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::SurfaceRaised)
                    && (*corner_radius - radius).abs() < 0.01 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the group draws an announcement card");
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::StrokeRect { width, corner_radius, .. }
                    if (*width - chat_typography::HAIRLINE).abs() < 1e-5
                        && (*corner_radius - radius).abs() < 0.01
            )),
            "the card carries DSH's 0.5 px hairline"
        );

        // The header's title is 14 / 24; the count is 12 / 18, right-aligned at
        // the card's other edge.
        let title = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    text,
                    font_size,
                    line_height,
                    color,
                    ..
                } if text == "Subagents" => Some((*font_size, *line_height, *color)),
                _ => None,
            })
            .expect("the card header draws its title");
        assert_eq!(title.0, 14.0, "the announcement title is 14 px");
        assert!((title.1 - chat_typography::BODY_LH).abs() < 1e-5);
        assert_eq!(title.2, app.theme.color(ColorToken::Muted));

        let (count, count_x) = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    text,
                    origin,
                    font_size,
                    line_height,
                    ..
                } if text == "1" => Some(((*font_size, *line_height), origin.x)),
                _ => None,
            })
            .expect("the header draws its count");
        assert_eq!(count.0, 12.0, "the header's count is the 12 px tier");
        assert!((count.1 - chat_typography::SMALL_LH).abs() < 1e-5);
        assert!(
            count_x > card.max_x() * 0.6,
            "the count sits at the card's trailing edge, got {count_x} in {card:?}"
        );

        // The body row reads at 13 / 24 in the quiet ink.
        let row_run = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    text,
                    font_size,
                    line_height,
                    color,
                    ..
                } if text.contains("Audit the migration") => Some((*font_size, *line_height, *color)),
                _ => None,
            })
            .expect("the card body draws its row");
        assert_eq!(row_run.0, 13.0);
        assert!((row_run.1 - chat_typography::ROW_LH).abs() < 1e-5);
        assert_eq!(row_run.2, app.theme.color(ColorToken::Muted));
    }

    /// Paint the dock a second time with the pointer at `cursor`, the way the
    /// window's frame loop does after a move.
    fn paint_with_cursor(dock: &mut WorkDock, width: f32, cursor: Vector2F) -> Vec<RenderCommand> {
        let app = AppContext::default();
        layout(dock, width);
        let mut ctx = PaintContext::new(Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = cursor;
        dock.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    /// The dock's body rows are DSH's 24 px rows plus their 4 px vertical
    /// padding — a 32 px pitch — and each row's mark shares its words'
    /// baseline. The header's chevron sits in a real 16 × 16 cell with the
    /// 12 px glyph centred inside it.
    #[test]
    fn dock_rows_are_thirty_two_high_and_baseline_aligned() {
        use crate::elements::chat_typography;

        let app = AppContext::default();
        let commands = paint(
            &mut WorkDock::new(vec![WorkGroup::new(
                WorkSection::Tasks,
                vec![
                    row("t1", "Task", "Watch QR rotation over 80 seconds").spinning(true),
                    row("t2", "Task", "Wait for the phone to pair").spinning(true),
                ],
                true,
            )]),
            900.0,
        );

        let row_run = |needle: &str| {
            commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::DrawText {
                        origin,
                        text,
                        font_size,
                        line_height,
                        ..
                    } if text.contains(needle) => Some((*origin, *font_size, *line_height)),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("the row {needle:?} is drawn"))
        };
        let (first, size, line) = row_run("Watch QR");
        let (second, ..) = row_run("Wait for");
        assert_eq!(size, 13.0, "a dock row reads at DSH's 13 px");
        assert!(
            (size * line - 24.0).abs() < 1e-5,
            "a dock row's line box is 24 px, got {}",
            size * line
        );
        assert!(
            ((second.y - first.y) - 32.0).abs() < 0.01,
            "dock rows pitch at 24 + 2×4 = 32 px, got {}",
            second.y - first.y
        );

        // Each row's mark is a text run in the 16 px cell; it shares the
        // words' baseline instead of sitting four points low.
        let mut marks = commands.iter().filter_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. }
                if text.chars().count() <= 3
                    && text.chars().any(|ch| !ch.is_ascii())
                    && !text.chars().any(|ch| ch.is_ascii_alphanumeric()) =>
            {
                Some(*origin)
            }
            _ => None,
        });
        let first_mark = marks.next().expect("the first row's mark is drawn");
        let second_mark = marks.next().expect("the second row's mark is drawn");
        assert!(
            (first_mark.y - first.y).abs() < 0.01,
            "the mark and the words share a baseline, got {} against {}",
            first_mark.y,
            first.y
        );
        assert!(
            (second_mark.y - second.y).abs() < 0.01,
            "the second row's mark shares its words' baseline"
        );

        // The header chevron: a fixed 16 × 16 cell, the 12 px glyph centred.
        let card_radius = chat_typography::announcement_radius(&app);
        let card = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, corner_radius, .. }
                    if (*corner_radius - card_radius).abs() < 0.01 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the group draws its card");
        let (title_origin, ..) = row_run("Watch QR");
        let _ = title_origin;
        let title = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { origin, text, .. } if text == "Tasks" => Some(*origin),
                _ => None,
            })
            .expect("the header draws its title");
        let (chevron_origin, chevron_size) = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    name, origin, size, ..
                } if name == "chevron-down" => Some((*origin, *size)),
                _ => None,
            })
            .expect("the header draws its chevron");
        assert_eq!(chevron_size, 12.0, "the chevron glyph is 12 px");
        assert!(
            (chevron_origin.x - (card.min_x() + chat_typography::ANNOUNCEMENT_PAD_X + 2.0)).abs()
                < 0.01,
            "the chevron is centred in its 16 px cell, got x {}",
            chevron_origin.x
        );
        assert!(
            (chevron_origin.y + 6.0 - (title.y + 12.0)).abs() < 0.01,
            "the chevron's centre is the title line's centre, got {} against {}",
            chevron_origin.y + 6.0,
            title.y + 12.0
        );
    }

    /// The announcement header's ink brightens from `Muted` to `Text` while the
    /// pointer is over it — title, count and chevron together.
    #[test]
    fn the_announcement_header_has_hover_ink() {
        let app = AppContext::default();
        let mut dock = WorkDock::new(vec![WorkGroup::new(
            WorkSection::Subagents,
            vec![row("c1", "Subagent", "Audit the migration").spinning(true)],
            true,
        )]);
        let resting = paint(&mut dock, 900.0);
        let title_origin = resting
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    origin,
                    text,
                    color,
                    ..
                } if text == "Subagents" => Some((*origin, *color)),
                _ => None,
            })
            .expect("the header title is drawn");
        assert_eq!(
            title_origin.1,
            app.theme.color(ColorToken::Muted),
            "a resting header reads quiet"
        );
        let hovered = paint_with_cursor(&mut dock, 900.0, title_origin.0 + vec2f(2.0, 2.0));
        let hovered_title = hovered
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    text, color, ..
                } if text == "Subagents" => Some(*color),
                _ => None,
            })
            .expect("the hovered header title is drawn");
        assert_eq!(
            hovered_title,
            app.theme.color(ColorToken::Text),
            "the header title brightens under the pointer"
        );
        let hovered_count = hovered
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    text, color, ..
                } if text == "1" => Some(*color),
                _ => None,
            })
            .expect("the hovered header count is drawn");
        assert_eq!(
            hovered_count,
            app.theme.color(ColorToken::Text),
            "the header count brightens with its title"
        );
    }

    // ---- The chrome band: full pane width, square, lane-bounded cards -------

    /// The Surface fills that are the dock's own band: square (`r == 0`), as
    /// opposed to the cards' rounded fills.
    fn band_rects(commands: &[RenderCommand], app: &AppContext) -> Vec<RectF> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::Surface)
                    && *corner_radius == 0.0 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .collect()
    }

    /// The announcement cards' rects, found by the announcement radius on their
    /// hairline strokes.
    fn card_rects(commands: &[RenderCommand], app: &AppContext) -> Vec<RectF> {
        let radius = chat_typography::announcement_radius(app);
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::StrokeRect {
                    rect,
                    corner_radius,
                    ..
                } if (*corner_radius - radius).abs() < 0.01 => Some(*rect),
                _ => None,
            })
            .collect()
    }

    /// The band is the pane's chrome: it spans the whole width it is laid out
    /// at, as a square fill. The cards keep their own radius inside it — the
    /// band never does, and it has no side margin of its own.
    #[test]
    fn the_band_spans_the_full_width_and_has_square_corners() {
        let app = AppContext::default();
        let mut dock = WorkDock::new(groups());
        let size = layout(&mut dock, 900.0);
        let commands = paint(&mut dock, 900.0);
        let bands = band_rects(&commands, &app);
        assert_eq!(bands.len(), 1, "one band: {commands:?}");
        let band = bands[0];
        assert_eq!(band.min_x(), 0.0, "the band starts at the pane's edge");
        assert_eq!(band.max_x(), 900.0, "and ends at the pane's other edge");
        assert_eq!(band.width(), 900.0);
        assert_eq!(band.height(), size.y, "the band is the dock's own height");
        assert!(
            card_rects(&commands, &app)
                .iter()
                .all(|card| card.width() < band.width()),
            "the cards are insets inside the band, never the band itself"
        );
    }

    /// The cards are bounded to the pane's content lane: the same column the
    /// conversation under the band is centred in, resolved from the band's own
    /// width, and centred in the full-width band.
    #[test]
    fn the_cards_are_bounded_to_the_content_lane_and_centred() {
        let app = AppContext::default();
        let width = 1600.0;
        let (offset, lane) = lane_column(width, None);
        let mut dock = WorkDock::new(groups()).with_content_lane(None);
        let size = layout(&mut dock, width);
        let commands = paint(&mut dock, width);
        let md = app.theme.spacing_px(SpacingToken::Md);

        let bands = band_rects(&commands, &app);
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0].width(), width, "the band is still the whole pane");
        assert_eq!(size.x, width);

        let cards = card_rects(&commands, &app);
        assert!(!cards.is_empty(), "the groups draw their cards");
        for card in &cards {
            assert!(
                (card.min_x() - (offset + md)).abs() < 0.01,
                "the card starts at the lane's inset edge: {:?} against {offset}+{md}",
                card
            );
            assert!(
                (card.max_x() - (offset + lane - md)).abs() < 0.01,
                "and ends at the lane's other inset edge: {:?}",
                card
            );
        }
        let band_centre = (bands[0].min_x() + bands[0].max_x()) / 2.0;
        let card_centre = (cards[0].min_x() + cards[0].max_x()) / 2.0;
        assert!(
            (band_centre - card_centre).abs() < 0.01,
            "the bounded content is centred in the band: {card_centre} vs {band_centre}"
        );
        let expected = resolve_content_width(width, None);
        assert!(
            (cards[0].width() - (expected - md * 2.0)).abs() < 0.01,
            "the card content is the lane column minus the band's own padding: {} against {expected}",
            cards[0].width()
        );
    }

    /// A committed per-pane preference (the width the user dragged the lane to)
    /// bounds the cards too: the band stays full width, the content follows the
    /// pane's own column.
    #[test]
    fn a_committed_lane_preference_bounds_the_cards() {
        let app = AppContext::default();
        let width = 1600.0;
        let (offset, lane) = lane_column(width, Some(1000.0));
        assert_eq!(lane, 1000.0);
        let mut dock = WorkDock::new(groups()).with_content_lane(Some(1000.0));
        let commands = paint(&mut dock, width);
        let md = app.theme.spacing_px(SpacingToken::Md);
        let cards = card_rects(&commands, &app);
        assert!(!cards.is_empty());
        for card in &cards {
            assert!((card.min_x() - (offset + md)).abs() < 0.01);
            assert!((card.max_x() - (offset + lane - md)).abs() < 0.01);
        }
    }

    /// A host that already resolved the lane width hands it in: the cards take
    /// exactly that width, centred, whatever the band's width.
    #[test]
    fn an_explicit_content_width_is_centred() {
        let app = AppContext::default();
        let mut dock = WorkDock::new(groups()).with_content_width(800.0);
        let commands = paint(&mut dock, 1200.0);
        let md = app.theme.spacing_px(SpacingToken::Md);
        let cards = card_rects(&commands, &app);
        assert!(!cards.is_empty());
        for card in &cards {
            assert!((card.min_x() - (200.0 + md)).abs() < 0.01);
            assert!((card.max_x() - (1000.0 - md)).abs() < 0.01);
        }
    }

    /// A narrow pane has no lane: the cards take the full band, as they always
    /// did, and the band still spans the pane.
    #[test]
    fn a_narrow_pane_keeps_the_cards_full_width() {
        let app = AppContext::default();
        let mut dock = WorkDock::new(groups()).with_content_lane(None);
        let commands = paint(&mut dock, 800.0);
        let md = app.theme.spacing_px(SpacingToken::Md);
        let cards = card_rects(&commands, &app);
        assert!(!cards.is_empty());
        for card in &cards {
            assert!((card.min_x() - md).abs() < 0.01);
            assert!((card.max_x() - (800.0 - md)).abs() < 0.01);
        }
    }

    /// The band bleeds past the dock's own layout edges — the app's
    /// `PANE_INSET` — so the pane's chrome paints flush with the pane's slot
    /// while the body below keeps its inset frame. The dock's layout size is
    /// unchanged; only the paint grows.
    #[test]
    fn the_band_bleeds_past_its_layout_edges() {
        let app = AppContext::default();
        let mut dock = WorkDock::new(groups()).with_band_bleed(4.0);
        let size = layout(&mut dock, 1292.0);
        assert_eq!(size.x, 1292.0, "the bleed is paint-only");
        let commands = paint(&mut dock, 1292.0);
        let bands = band_rects(&commands, &app);
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0].min_x(), -4.0);
        assert_eq!(bands[0].max_x(), 1296.0);
        assert_eq!(bands[0].width(), 1300.0);
    }

    /// An empty dock draws no band at all: no work, no chrome.
    #[test]
    fn an_empty_dock_paints_no_band() {
        let app = AppContext::default();
        let mut dock = WorkDock::new(Vec::new());
        let commands = paint(&mut dock, 900.0);
        assert!(band_rects(&commands, &app).is_empty());
    }
}
