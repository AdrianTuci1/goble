//! The sub-agent strip under the agent pane's header: one collapsible row per
//! child the pane spawned, with the controls that enter and stop it.
//!
//! The strip owns the delegation chrome. The chat says what a child is doing
//! and nothing more — a transcript row that carried its own open and stop
//! targets put two controls in the middle of the reply — while the strip sits
//! directly under the pane's bar, where grok-new keeps the same `Subagents`
//! disclosure above the conversation.

use std::cell::RefCell;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::{
    AppContext, Chip, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Expanded, Fill,
    Flex, Icon, InlineText, LayoutContext, MainAxisSize, PaintContext, Padding, Point,
    SizeConstraint, Text, TextSpan,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, FontFamily, SpacingToken};

use super::{format_subagent_elapsed, SubAgentRow};

/// The strip's own text size: a step under the transcript's, so the rows read
/// as chrome around the conversation rather than as more of it.
const FONT_SIZE: f32 = 11.0;
const LINE_HEIGHT: f32 = 1.2;

/// The vertical room a strip row keeps above and below its line. The strip is
/// deliberately tighter than a transcript row: it is chrome, not content.
const ROW_PADDING_Y: f32 = 3.0;

/// The chevron and the row's two controls.
const MARK_SIZE: f32 = 12.0;

/// The strip's header: the chevron that folds it, the word `Subagents` and the
/// count of the children it lists.
const TITLE: &str = "Subagents";

/// The strip's rows, under a header that folds them, drawn directly under the
/// agent pane's header bar.
///
/// The header row is the fold: a click anywhere on it toggles the panel through
/// [`Self::with_on_toggle`]. One row per child carries the status mark the
/// transcript row draws, the child's description, what it is doing, and — at
/// the row's far edge — the pane's model, the child's elapsed time, the control
/// that enters its conversation and, while it runs, the one that stops it.
///
/// An empty list draws nothing at all: a strip with no children is not a band
/// the pane has to pay for.
pub struct SubAgentPanel {
    rows: Vec<SubAgentRow>,
    /// The model the pane's turns run on, drawn at each row's far edge. Empty
    /// draws no model segment rather than an empty column.
    model: String,
    open: bool,
    /// Enters a child's own conversation, carrying the child's id.
    on_open: Option<Rc<RefCell<dyn FnMut(String)>>>,
    /// Cancels a running child's turn, carrying the child's id.
    on_stop: Option<Rc<RefCell<dyn FnMut(String)>>>,
    /// Folds or opens the strip: the header row's own click.
    on_toggle: Option<Rc<RefCell<dyn FnMut()>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SubAgentPanel {
    pub fn new(rows: Vec<SubAgentRow>, model: impl Into<String>, open: bool) -> Self {
        Self {
            rows,
            model: model.into(),
            open,
            on_open: None,
            on_stop: None,
            on_toggle: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    /// Bind the control that enters a child's conversation.
    pub fn with_on_open(mut self, callback: Rc<RefCell<dyn FnMut(String)>>) -> Self {
        self.on_open = Some(callback);
        self
    }

    /// Bind the control that stops a running child.
    pub fn with_on_stop(mut self, callback: Rc<RefCell<dyn FnMut(String)>>) -> Self {
        self.on_stop = Some(callback);
        self
    }

    /// Bind the header row's fold.
    pub fn with_on_toggle<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_toggle = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Build the strip once, on the first layout that has the theme to draw it
    /// with. An empty list stays rootless, so nothing is painted and the strip
    /// takes no height.
    fn ensure_root(&mut self, app: &AppContext) {
        if self.root.is_some() || self.rows.is_empty() {
            return;
        }
        let md = app.theme.spacing_px(SpacingToken::Md);
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.header(app))
            .with_child(Divider::horizontal().finish());
        if self.open {
            for row in &self.rows {
                column = column.with_child(self.row(row, app));
            }
        }
        self.root = Some(
            Container::new(column.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                .with_padding(EdgeInsets::new(md, 0.0, md, 0.0))
                .finish(),
        );
    }

    fn header(&self, app: &AppContext) -> Box<dyn Element> {
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(
                Icon::new(if self.open {
                    "chevron-down"
                } else {
                    "chevron-right"
                })
                .with_size(MARK_SIZE)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
            )
            .with_child(
                Text::new(TITLE)
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(FONT_SIZE)
                    .with_max_lines(1)
                    .finish(),
            )
            .with_child(
                Text::new(self.rows.len().to_string())
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(FONT_SIZE)
                    .with_max_lines(1)
                    .finish(),
            )
            .finish();
        // The chip's bounds are the row's: the whole header folds the strip,
        // not only the words in it.
        let toggle = self.on_toggle.clone();
        let mut chip = Chip::flat(
            Padding::new(row, EdgeInsets::new(0.0, ROW_PADDING_Y, 0.0, ROW_PADDING_Y)).finish(),
        );
        if let Some(toggle) = toggle {
            chip = chip.with_on_click(move || (toggle.borrow_mut())());
        }
        chip.finish()
    }

    fn row(&self, record: &SubAgentRow, app: &AppContext) -> Box<dyn Element> {
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let (mark, token) = record.status_affordance();
        let run = |text: String, color: ColorU| -> TextSpan {
            TextSpan::plain(text)
                .with_color(color)
                .with_family(FontFamily::Mono)
                .with_weight(FontWeight::Regular)
        };
        // One wrapping flow: a description longer than the pane folds inside
        // it rather than running under the row's controls.
        let line = InlineText::new(vec![
            run(format!("{mark} "), app.theme.color(token)),
            run("Subagent".to_string(), app.theme.color(ColorToken::Text)),
            run(
                format!(" {}", record.description),
                app.theme.color(ColorToken::Text),
            ),
            run(
                format!(" · {}", record.panel_status()),
                app.theme.color(ColorToken::Muted),
            ),
        ])
        .with_font_size(FONT_SIZE)
        .with_line_height(LINE_HEIGHT)
        .finish();
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(Expanded::new(line).with_flex(1.0).finish());
        let model = self.model.trim();
        if !model.is_empty() {
            row = row.with_child(
                Text::new(model)
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(FONT_SIZE)
                    .with_max_lines(1)
                    .finish(),
            );
        }
        row = row.with_child(
            Text::new(format_subagent_elapsed(record.elapsed))
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(FONT_SIZE)
                .with_max_lines(1)
                .finish(),
        );
        if !record.child_id.is_empty() {
            row = row.with_child(control(
                "arrow-up",
                ColorToken::Muted,
                self.on_open.as_ref(),
                &record.child_id,
                app,
            ));
            // A child that already ended has nothing left to stop, and a dead
            // affordance is not drawn.
            if record.is_running() {
                row = row.with_child(control(
                    "x",
                    ColorToken::Error,
                    self.on_stop.as_ref(),
                    &record.child_id,
                    app,
                ));
            }
        }
        Padding::new(
            row.finish(),
            EdgeInsets::new(0.0, ROW_PADDING_Y, 0.0, ROW_PADDING_Y),
        )
        .finish()
    }
}

/// One of a row's two icon controls: a bare hit target around the glyph, firing
/// `callback` with the child it belongs to. A strip with no callback bound
/// still draws the glyph; the click simply goes nowhere.
fn control(
    icon: &'static str,
    token: ColorToken,
    callback: Option<&Rc<RefCell<dyn FnMut(String)>>>,
    child_id: &str,
    app: &AppContext,
) -> Box<dyn Element> {
    let mut chip = Chip::flat(
        Icon::new(icon)
            .with_size(MARK_SIZE)
            .with_theme_color(token, app)
            .finish(),
    );
    if let Some(callback) = callback {
        let callback = callback.clone();
        let child_id = child_id.to_string();
        chip = chip.with_on_click(move || (callback.borrow_mut())(child_id.clone()));
    }
    chip.finish()
}

impl Element for SubAgentPanel {
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
    use std::time::Duration;

    use super::*;
    use crate::elements::chat_content::SubAgentRowStatus;
    use crate::elements::EventContext;
    use crate::geometry::vec2f;
    use crate::render::{RenderCommand, Renderer};

    fn row(
        child_id: &str,
        status: SubAgentRowStatus,
        activity: &str,
        elapsed: Duration,
    ) -> SubAgentRow {
        SubAgentRow {
            child_id: child_id.to_string(),
            subagent_type: "worker".to_string(),
            description: "Write simple slugify function".to_string(),
            status,
            activity: activity.to_string(),
            elapsed,
            turns: 1,
            tool_calls: 2,
            tokens: 300,
            outcome: None,
            background: false,
        }
    }

    fn layout(panel: &mut SubAgentPanel, width: f32) -> Vector2F {
        panel.layout(
            SizeConstraint::loose(vec2f(width, 400.0)),
            &mut LayoutContext::default(),
            &AppContext::default(),
        )
    }

    fn paint(panel: &mut SubAgentPanel, width: f32) -> Vec<RenderCommand> {
        let app = AppContext::default();
        layout(panel, width);
        let mut ctx = PaintContext::new(Renderer::new());
        panel.paint(vec2f(0.0, 0.0), &mut ctx, &app);
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

    /// Where a drawn icon's centre is, which is what a click lands on.
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

    fn click(panel: &mut SubAgentPanel, position: Vector2F) {
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
            panel.dispatch_event(&event, &mut ctx, &app);
        }
    }

    #[test]
    fn the_header_states_the_word_and_the_row_count() {
        let rows = vec![
            row(
                "child-1",
                SubAgentRowStatus::Running,
                "reading",
                Duration::from_millis(8700),
            ),
            row(
                "child-2",
                SubAgentRowStatus::Completed,
                "",
                Duration::from_secs(43),
            ),
        ];
        let mut panel = SubAgentPanel::new(rows, "deepseek", true);
        let commands = paint(&mut panel, 900.0);
        let rows = row_texts(&commands);

        assert!(
            rows[0].starts_with("Subagents"),
            "the strip leads with its word, got {rows:?}"
        );
        assert!(
            rows[0].ends_with('2'),
            "the header states how many children it lists, got {rows:?}"
        );
        assert!(
            rows.iter().any(|line| line.contains("deepseek") && line.contains("8.7s")),
            "a row carries the pane's model and the child's elapsed time, got {rows:?}"
        );
        assert!(
            icon_centre(&commands, "chevron-down", 0).is_some(),
            "an open strip shows the down chevron"
        );
    }

    #[test]
    fn a_running_row_states_its_activity_or_the_wait_for_one() {
        let rows = vec![
            row(
                "child-1",
                SubAgentRowStatus::Running,
                "reading the schema",
                Duration::from_millis(1500),
            ),
            row(
                "child-2",
                SubAgentRowStatus::Running,
                "",
                Duration::from_millis(500),
            ),
        ];
        let mut panel = SubAgentPanel::new(rows, "deepseek", true);
        let rows = row_texts(&paint(&mut panel, 900.0));

        let first = rows
            .iter()
            .find(|line| line.starts_with("◆ ") && line.contains("Subagent"))
            .unwrap_or_else(|| panic!("the strip draws its rows, got {rows:?}"));
        assert!(
            first.contains("◆ Subagent Write simple slugify function · reading the schema"),
            "a running child states what it is doing, got {first:?}"
        );
        assert!(
            rows.iter()
                .any(|line| line.contains("· Waiting for response…")),
            "a child with no activity yet waits for a response, got {rows:?}"
        );
    }

    #[test]
    fn an_ended_row_states_its_terminal_phrase() {
        let rows = vec![
            row(
                "child-1",
                SubAgentRowStatus::Completed,
                "",
                Duration::from_secs(43),
            ),
            row(
                "child-2",
                SubAgentRowStatus::Failed,
                "",
                Duration::from_secs(12),
            ),
            row(
                "child-3",
                SubAgentRowStatus::Cancelled,
                "",
                Duration::from_secs(5),
            ),
        ];
        let mut panel = SubAgentPanel::new(rows, "", true);
        let rows = row_texts(&paint(&mut panel, 900.0));

        for phrase in ["completed in 43s", "failed in 12s", "cancelled in 5.0s"] {
            assert!(
                rows.iter().any(|line| line.contains(phrase)),
                "an ended row states {phrase:?}, got {rows:?}"
            );
        }
        assert!(
            !rows.iter().any(|line| line.contains("initializing")),
            "an ended row states no live activity, got {rows:?}"
        );
    }

    #[test]
    fn only_a_running_child_draws_a_stop() {
        let running = vec![row(
            "child-1",
            SubAgentRowStatus::Running,
            "reading",
            Duration::from_secs(4),
        )];
        let mut panel = SubAgentPanel::new(running, "deepseek", true);
        let commands = paint(&mut panel, 900.0);
        assert!(
            icon_centre(&commands, "arrow-up", 0).is_some(),
            "every row keeps its open control"
        );
        assert!(
            icon_centre(&commands, "x-close", 0).is_some(),
            "a running child draws its stop"
        );

        let ended = vec![
            row(
                "child-1",
                SubAgentRowStatus::Completed,
                "",
                Duration::from_secs(43),
            ),
            row(
                "child-2",
                SubAgentRowStatus::Cancelled,
                "",
                Duration::from_secs(5),
            ),
        ];
        let mut panel = SubAgentPanel::new(ended, "deepseek", true);
        let commands = paint(&mut panel, 900.0);
        assert!(
            icon_centre(&commands, "x-close", 0).is_none(),
            "an ended child draws no stop"
        );
        assert!(
            icon_centre(&commands, "arrow-up", 1).is_some(),
            "an ended child is still enterable"
        );
    }

    #[test]
    fn the_controls_carry_the_child_they_belong_to() {
        let rows = vec![
            row(
                "child-1",
                SubAgentRowStatus::Running,
                "reading",
                Duration::from_secs(4),
            ),
            row(
                "child-2",
                SubAgentRowStatus::Running,
                "writing",
                Duration::from_secs(9),
            ),
        ];
        let opened = Rc::new(RefCell::new(Vec::new()));
        let stopped = Rc::new(RefCell::new(Vec::new()));
        let open_sink = opened.clone();
        let stop_sink = stopped.clone();
        let mut panel = SubAgentPanel::new(rows, "deepseek", true)
            .with_on_open(Rc::new(RefCell::new(move |child_id: String| {
                open_sink.borrow_mut().push(child_id)
            })))
            .with_on_stop(Rc::new(RefCell::new(move |child_id: String| {
                stop_sink.borrow_mut().push(child_id)
            })));

        // The second row's controls, so the id a control carries is the row's
        // own child and not the one above it.
        let commands = paint(&mut panel, 900.0);
        let open = icon_centre(&commands, "arrow-up", 1).expect("the second row's open control");
        click(&mut panel, open);
        let commands = paint(&mut panel, 900.0);
        let stop = icon_centre(&commands, "x-close", 1).expect("the second row's stop");
        click(&mut panel, stop);

        assert_eq!(
            *opened.borrow(),
            vec!["child-2".to_string()],
            "the open control enters the child it belongs to"
        );
        assert_eq!(
            *stopped.borrow(),
            vec!["child-2".to_string()],
            "the stop stops the child it belongs to"
        );
    }

    #[test]
    fn the_header_row_folds_the_strip() {
        let toggles = Rc::new(RefCell::new(0usize));
        let sink = toggles.clone();
        let rows = vec![row(
            "child-1",
            SubAgentRowStatus::Running,
            "reading",
            Duration::from_secs(4),
        )];
        let mut panel = SubAgentPanel::new(rows, "deepseek", true)
            .with_on_toggle(move || *sink.borrow_mut() += 1);
        let commands = paint(&mut panel, 900.0);
        let origin = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == TITLE => Some(*origin),
                _ => None,
            })
            .expect("the header draws its word");
        click(&mut panel, origin + vec2f(1.0, 1.0));
        assert_eq!(
            *toggles.borrow(),
            1,
            "a click on the header row asks the app to fold the strip"
        );
    }

    #[test]
    fn a_collapsed_strip_hides_its_rows_and_keeps_its_header() {
        let rows = vec![row(
            "child-1",
            SubAgentRowStatus::Running,
            "reading",
            Duration::from_secs(4),
        )];
        let mut panel = SubAgentPanel::new(rows, "deepseek", false);
        let commands = paint(&mut panel, 900.0);
        let texts = row_texts(&commands);

        assert!(
            texts.iter().any(|line| line.starts_with("Subagents")),
            "a folded strip keeps its header, got {texts:?}"
        );
        assert!(
            !texts.iter().any(|line| line.contains("Subagent Write")),
            "a folded strip draws none of its rows, got {texts:?}"
        );
        assert!(
            icon_centre(&commands, "chevron-right", 0).is_some(),
            "a folded strip turns its chevron to the right"
        );
        assert!(
            icon_centre(&commands, "arrow-up", 0).is_none(),
            "a folded strip draws none of its row controls"
        );
    }

    #[test]
    fn an_empty_strip_draws_nothing() {
        let mut panel = SubAgentPanel::new(Vec::new(), "deepseek", true);
        let size = layout(&mut panel, 900.0);
        assert_eq!(
            size,
            Vector2F::zero(),
            "a strip with no children takes no room"
        );
        assert!(
            paint(&mut panel, 900.0).is_empty(),
            "a strip with no children paints nothing"
        );
    }
}
