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
use crate::elements::running_indicator::SPINNER_FRAMES;
use crate::elements::{
    AppContext, Chip, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Expanded, Fill,
    Flex, HoverRow, Icon, InlineText, LayoutContext, MainAxisSize, Padding, PaintContext, Point,
    SizeConstraint, Text, TextSpan,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, FontFamily, SpacingToken};

/// The dock's text size: the transcript's message text is 14 px, so the dock
/// reads at the conversation's size instead of as a caption under it.
const FONT_SIZE: f32 = 14.0;

/// The meta column and the `show more` line are a step under the rows.
const META_FONT_SIZE: f32 = 12.0;
const LINE_HEIGHT: f32 = 1.2;

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

/// The dock: every non-empty group, its rows and the controls that reach the
/// app. An empty dock draws nothing at all, so a pane with nothing in flight
/// pays no height for it.
pub struct WorkDock {
    groups: Vec<WorkGroup>,
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
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl WorkDock {
    pub fn new(groups: Vec<WorkGroup>) -> Self {
        Self {
            groups: groups.into_iter().filter(|group| !group.rows.is_empty()).collect(),
            phase: 0.0,
            on_toggle: None,
            on_show_all: None,
            on_open: None,
            on_stop: None,
            root: None,
            size: None,
            origin: None,
        }
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
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0);
        for (index, group) in self.groups.iter().enumerate() {
            if index > 0 {
                column = column.with_child(Divider::horizontal().finish());
            }
            column = column.with_child(self.header(group, app));
            if group.open {
                for row in group.visible_rows() {
                    column = column.with_child(self.row(group.section, row, app));
                }
                let hidden = group.hidden_rows();
                if hidden > 0 {
                    column = column.with_child(self.show_more(group.section, hidden, app));
                }
            }
        }
        self.root = Some(
            Container::new(column.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                .with_padding(EdgeInsets::new(md, 0.0, md, 0.0))
                .finish(),
        );
    }

    fn header(&self, group: &WorkGroup, app: &AppContext) -> Box<dyn Element> {
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(
                Icon::new(if group.open {
                    "chevron-down"
                } else {
                    "chevron-right"
                })
                .with_size(MARK_SIZE)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
            )
            .with_child(
                Text::new(group.section.label())
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(FONT_SIZE)
                    .with_weight(FontWeight::Bold)
                    .with_max_lines(1)
                    .finish(),
            )
            .with_child(
                Text::new(group.rows.len().to_string())
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(FONT_SIZE)
                    .with_max_lines(1)
                    .finish(),
            )
            .finish();
        let toggle = self.on_toggle.clone();
        let section = group.section;
        let mut chip = Chip::flat(
            Padding::new(row, EdgeInsets::new(0.0, ROW_PADDING_Y, 0.0, ROW_PADDING_Y)).finish(),
        );
        if let Some(toggle) = toggle {
            chip = chip.with_on_click(move || (toggle.borrow_mut())(section));
        }
        chip.finish()
    }

    fn row(&self, section: WorkSection, row: &WorkRow, app: &AppContext) -> Box<dyn Element> {
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let run = |text: String, color: ColorU| -> TextSpan {
            TextSpan::plain(text)
                .with_color(color)
                .with_family(FontFamily::Mono)
                .with_weight(FontWeight::Regular)
        };
        let (dot, dot_color) = if row.spinning {
            (self.spinner_frame().to_string(), ColorToken::Warning)
        } else {
            (RESTING_DOT.to_string(), ColorToken::Muted)
        };
        let mut spans = vec![
            run(format!("{dot} "), app.theme.color(dot_color)),
            run(row.kind.clone(), app.theme.color(ColorToken::Warning)),
            run(
                format!(" {}", row.description),
                app.theme.color(ColorToken::Text),
            ),
        ];
        if let Some(activity) = row.activity.as_deref().filter(|text| !text.is_empty()) {
            spans.push(run(
                format!(" — {activity}"),
                app.theme.color(ColorToken::Muted),
            ));
        }
        let line = InlineText::new(spans)
            .with_font_size(FONT_SIZE)
            .with_line_height(LINE_HEIGHT)
            .finish();
        let mut line_row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(Expanded::new(line).with_flex(1.0).finish());
        if !row.meta.is_empty() {
            line_row = line_row.with_child(
                Text::new(row.meta.clone())
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(META_FONT_SIZE)
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

    fn show_more(&self, section: WorkSection, hidden: usize, app: &AppContext) -> Box<dyn Element> {
        let line = InlineText::new(vec![TextSpan::plain(format!(
            "{RESTING_DOT} show {hidden} more"
        ))
        .with_color(app.theme.color(ColorToken::Muted))
        .with_family(FontFamily::Mono)])
        .with_font_size(META_FONT_SIZE)
        .with_line_height(LINE_HEIGHT)
        .finish();
        let mut chip = Chip::flat(
            Padding::new(
                line,
                EdgeInsets::new(MARK_SIZE, ROW_PADDING_Y, 0.0, ROW_PADDING_Y),
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
        let size = match self.root.as_mut() {
            Some(root) => root.layout(constraint, ctx, app),
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
    use crate::elements::EventContext;
    use crate::geometry::vec2f;
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
    /// reads as one string.
    fn row_texts(commands: &[RenderCommand]) -> Vec<String> {
        let mut runs: Vec<(i32, f32, String)> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } => {
                    Some(((origin.y * 10.0).round() as i32, origin.x, text.clone()))
                }
                _ => None,
            })
            .collect();
        runs.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut rows: Vec<(i32, String)> = Vec::new();
        for (y, _, text) in runs {
            match rows.last_mut() {
                Some((last_y, line)) if *last_y == y => line.push_str(&text),
                _ => rows.push((y, text)),
            }
        }
        rows.into_iter().map(|(_, line)| line).collect()
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
            rows.iter().any(|line| line == "6.6s"),
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
}
