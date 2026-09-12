use std::time::Duration;

use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, Flex, LayoutContext,
    MainAxisAlignment, MainAxisSize, PaintContext, Point, RunningIndicator, SizeConstraint, Spacer,
    Text,
};
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

const FONT_SIZE: f32 = 12.0;
const PADDING_Y: f32 = 6.0;
const SPINNER_SIZE: f32 = 12.0;

/// How long one full spinner rotation takes. The frame is derived from the turn's
/// real elapsed time; the element layer owns no clock of its own.
const SPINNER_PERIOD: Duration = Duration::from_millis(480);

/// What the pane's turn is doing right now. The app resolves this from live
/// state (C1's `LiveWork`); the element only draws the phrasing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnActivity {
    /// A tool call is in flight, drawn as its command when it has one and as the
    /// tool's name otherwise.
    Tool {
        name: String,
        command: Option<String>,
    },
    /// The turn is suspended on the user's approval of a command proposal.
    WaitingOnApproval,
    /// The turn is suspended on the user's answer to an `ask_user` question.
    WaitingOnQuestion,
    /// The model is reasoning.
    Thinking,
    /// The model is writing its answer.
    Responding,
}

impl TurnActivity {
    pub fn label(&self) -> String {
        match self {
            Self::Tool {
                name: _,
                command: Some(command),
            } => command.clone(),
            Self::Tool {
                name,
                command: None,
            } => name.clone(),
            Self::WaitingOnApproval => "Waiting on approval…".to_string(),
            Self::WaitingOnQuestion => "Waiting on a question…".to_string(),
            Self::Thinking => "Thinking…".to_string(),
            Self::Responding => "Responding…".to_string(),
        }
    }
}

/// A kind of work that can outlive the pane's turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkKind {
    /// In-flight tool calls belonging to the pane.
    Command,
    /// Worker agent executions the service reports as running.
    Execution,
    /// A spawned sub-agent. No source produces one yet (the sub-agent events
    /// land with S4), so the app never emits this kind today.
    SubAgent,
}

impl WorkKind {
    pub fn noun(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Execution => "execution",
            Self::SubAgent => "sub-agent",
        }
    }
}

/// One kind's count on the still-running line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkKindCount {
    pub kind: WorkKind,
    pub count: usize,
}

/// The footer's three states, and no others.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum TurnStatus {
    /// Nothing is in flight: the row collapses to zero height.
    #[default]
    Idle,
    /// The pane's own turn is busy. `work_count` is what is in flight beside it.
    Busy {
        activity: TurnActivity,
        elapsed: Option<Duration>,
        work_count: usize,
    },
    /// The turn is over but work is still running, named by kind. An empty list
    /// means nothing is in flight, so the row collapses like [`Self::Idle`].
    StillRunning { kinds: Vec<WorkKindCount> },
}

impl TurnStatus {
    pub fn idle() -> Self {
        Self::Idle
    }

    pub fn busy(activity: TurnActivity, elapsed: Option<Duration>, work_count: usize) -> Self {
        Self::Busy {
            activity,
            elapsed,
            work_count,
        }
    }

    pub fn still_running(kinds: Vec<WorkKindCount>) -> Self {
        Self::StillRunning { kinds }
    }

    /// Whether this status costs a row. False is the zero-height state.
    pub fn is_in_flight(&self) -> bool {
        match self {
            Self::Idle => false,
            Self::Busy { .. } => true,
            Self::StillRunning { kinds } => !kinds.is_empty(),
        }
    }
}

/// The live turn-status footer: one row above the composer that says what the
/// pane is doing, or what is still running once it stops.
///
/// It is drawn in the pager idiom — full width, no border, no rounded container,
/// a bullet as the mark — exactly the treatment the transcript rows use. It has
/// three states and no others: busy (a spinner, the activity, the elapsed time
/// and the in-flight count), idle with work in flight (one line naming each kind
/// still running), and nothing in flight (zero height).
///
/// It is information only: no state here is a hit target. The composer keeps the
/// only control on a turn.
///
/// The spinner's frame and the elapsed label both come from the app's real
/// clock: the element layer owns neither a clock nor a frame counter, so the app
/// passes the age of the observed turn start (C1's `turn_started_at`) and the
/// phase advances with it. No token count is drawn — the live events carry none,
/// and one is not invented.
pub struct TurnStatusFooter {
    status: TurnStatus,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TurnStatusFooter {
    pub fn new(status: TurnStatus) -> Self {
        Self {
            status,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn status(&self) -> &TurnStatus {
        &self.status
    }

    fn ensure_root(&mut self, app: &AppContext) {
        if self.root.is_some() {
            return;
        }
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let root: Box<dyn Element> = match &self.status {
            // Nothing in flight: no root, so `paint` draws nothing and the row
            // takes zero height.
            TurnStatus::Idle => return,
            TurnStatus::StillRunning { kinds } if kinds.is_empty() => return,
            TurnStatus::Busy {
                activity,
                elapsed,
                work_count,
            } => {
                let phase = elapsed.map(phase_from_elapsed).unwrap_or(0.0);
                let mut row = Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::Start)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(sm)
                    .with_child(
                        RunningIndicator::new()
                            .with_size(SPINNER_SIZE)
                            .with_phase(phase)
                            .finish(),
                    )
                    .with_child(
                        Text::new(activity.label())
                            .with_theme_color(ColorToken::Text, app)
                            .with_font_size(FONT_SIZE)
                            .with_max_lines(1)
                            .finish(),
                    );
                if let Some(elapsed) = elapsed {
                    row = row.with_child(
                        Text::new(format_elapsed(*elapsed))
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(FONT_SIZE)
                            .with_max_lines(1)
                            .finish(),
                    );
                }
                if *work_count > 0 {
                    row = row.with_child(Spacer::new().finish()).with_child(
                        Text::new(format!("◆ {work_count}"))
                            .with_theme_color(ColorToken::Accent, app)
                            .with_font_size(FONT_SIZE)
                            .with_max_lines(1)
                            .finish(),
                    );
                }
                Container::new(row.finish())
                    .with_padding(EdgeInsets::new(PADDING_Y, sm, PADDING_Y, sm))
                    .finish()
            }
            TurnStatus::StillRunning { kinds } => {
                let row = Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::Start)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(sm)
                    .with_child(
                        Text::new("◆")
                            .with_font_size(FONT_SIZE)
                            .with_theme_color(ColorToken::Accent, app)
                            .finish(),
                    )
                    .with_child(
                        Text::new(still_running_label(kinds))
                            .with_theme_color(ColorToken::Text, app)
                            .with_font_size(FONT_SIZE)
                            .with_max_lines(1)
                            .finish(),
                    )
                    .finish();
                Container::new(row)
                    .with_padding(EdgeInsets::new(PADDING_Y, sm, PADDING_Y, sm))
                    .finish()
            }
        };
        self.root = Some(root);
    }
}

impl Element for TurnStatusFooter {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        if !self.status.is_in_flight() {
            self.size = Some(Vector2F::zero());
            return Vector2F::zero();
        }
        self.ensure_root(app);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
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
}

/// The spinner's phase from the turn's real elapsed time.
fn phase_from_elapsed(elapsed: Duration) -> f32 {
    let period_ms = SPINNER_PERIOD.as_millis() as u64;
    (elapsed.as_millis() as u64 % period_ms) as f32 / period_ms as f32
}

/// `m:ss`, or `h:mm:ss` past an hour.
fn format_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    let (hours, minutes, seconds) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// `{n} {kind} still running`, one phrase per kind, joined.
fn still_running_label(kinds: &[WorkKindCount]) -> String {
    kinds
        .iter()
        .map(|entry| {
            let noun = entry.kind.noun();
            let plural = if entry.count == 1 { "" } else { "s" };
            format!("{} {noun}{plural} still running", entry.count)
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::running_indicator::SPINNER_FRAMES;
    use crate::elements::{AppContext, EventContext, LayoutContext, PaintContext, SizeConstraint};
    use crate::event::DispatchedEvent;
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;
    use crate::test_util::{command_counts, render_element};

    fn texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_footer_draws_the_activity_and_the_elapsed_time_while_busy() {
        let status = TurnStatus::busy(
            TurnActivity::Tool {
                name: "run_command".to_string(),
                command: Some("cargo test -p goble-ui".to_string()),
            },
            Some(Duration::from_secs(65)),
            2,
        );
        let mut element: Box<dyn Element> = TurnStatusFooter::new(status).finish();

        let app = AppContext::default();
        let commands = render_element(&mut element, vec2f(600.0, 40.0), &app);
        let drawn = texts(&commands);

        assert!(
            drawn.iter().any(|t| t == "cargo test -p goble-ui"),
            "the running command is the activity: {drawn:?}"
        );
        assert!(
            drawn.iter().any(|t| t == "1:05"),
            "the elapsed time is drawn: {drawn:?}"
        );
        assert!(
            drawn.iter().any(|t| t == "◆ 2"),
            "the in-flight work count is on the right: {drawn:?}"
        );
        assert!(
            drawn.iter().any(|t| SPINNER_FRAMES.contains(&t.as_str())),
            "a spinner frame is drawn: {drawn:?}"
        );
    }

    #[test]
    fn a_busy_footer_with_no_observed_start_invents_no_elapsed_time() {
        let status = TurnStatus::busy(TurnActivity::Thinking, None, 0);
        let mut element: Box<dyn Element> = TurnStatusFooter::new(status).finish();

        let app = AppContext::default();
        let commands = render_element(&mut element, vec2f(600.0, 40.0), &app);
        let drawn = texts(&commands);

        assert!(
            drawn.iter().any(|t| t == "Thinking…"),
            "the activity is still drawn: {drawn:?}"
        );
        assert!(
            !drawn.iter().any(|t| t.contains(':')),
            "no elapsed time is invented when the start was not observed: {drawn:?}"
        );
    }

    #[test]
    fn the_waiting_states_are_named() {
        fn label(activity: TurnActivity) -> String {
            activity.label()
        }
        assert_eq!(
            label(TurnActivity::WaitingOnApproval),
            "Waiting on approval…"
        );
        assert_eq!(
            label(TurnActivity::WaitingOnQuestion),
            "Waiting on a question…"
        );
        assert_eq!(label(TurnActivity::Thinking), "Thinking…");
        assert_eq!(label(TurnActivity::Responding), "Responding…");
        // A tool without a command falls back to its name.
        assert_eq!(
            label(TurnActivity::Tool {
                name: "read_file".to_string(),
                command: None,
            }),
            "read_file"
        );
    }

    #[test]
    fn the_footer_names_each_kind_that_is_still_running() {
        let status = TurnStatus::still_running(vec![
            WorkKindCount {
                kind: WorkKind::Command,
                count: 2,
            },
            WorkKindCount {
                kind: WorkKind::Execution,
                count: 1,
            },
        ]);
        let mut element: Box<dyn Element> = TurnStatusFooter::new(status).finish();

        let app = AppContext::default();
        let commands = render_element(&mut element, vec2f(600.0, 40.0), &app);
        let line = texts(&commands)
            .into_iter()
            .find(|t| t.contains("still running"))
            .expect("the still-running line is drawn");

        assert!(line.contains("2 commands still running"), "{line}");
        assert!(line.contains("1 execution still running"), "{line}");
        assert_eq!(command_counts(&commands).stroke_rect, 0, "no border");
    }

    #[test]
    fn the_footer_takes_zero_height_when_nothing_is_in_flight() {
        for status in [TurnStatus::idle(), TurnStatus::still_running(Vec::new())] {
            let mut element: Box<dyn Element> = TurnStatusFooter::new(status).finish();
            let app = AppContext::default();
            let commands = render_element(&mut element, vec2f(600.0, 40.0), &app);

            assert_eq!(
                element.size().map(|s| s.y),
                Some(0.0),
                "an empty footer takes zero height"
            );
            assert!(
                commands.is_empty(),
                "an empty footer draws nothing: {commands:?}"
            );
        }
    }

    /// The still-running footer is information only: it draws the line and takes
    /// no pointer event, so nothing can be opened from it.
    #[test]
    fn the_still_running_footer_is_information_not_a_hit_target() {
        let status = TurnStatus::still_running(vec![WorkKindCount {
            kind: WorkKind::Execution,
            count: 1,
        }]);
        let mut footer = TurnStatusFooter::new(status);

        let app = AppContext::default();
        footer.layout(
            SizeConstraint::loose(vec2f(600.0, 40.0)),
            &mut LayoutContext,
            &app,
        );
        footer.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut ctx = EventContext::default();
        for event in [
            DispatchedEvent::MouseDown {
                position: vec2f(10.0, 10.0),
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: vec2f(10.0, 10.0),
                button: 0,
            },
        ] {
            assert!(
                !footer.dispatch_event(&event, &mut ctx, &app),
                "the footer takes no pointer event"
            );
        }
    }

    #[test]
    fn a_busy_footer_is_not_a_hit_target() {
        let status = TurnStatus::busy(TurnActivity::Thinking, Some(Duration::from_secs(3)), 0);
        let mut footer = TurnStatusFooter::new(status);

        let app = AppContext::default();
        footer.layout(
            SizeConstraint::loose(vec2f(600.0, 40.0)),
            &mut LayoutContext,
            &app,
        );
        footer.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
        let mut ctx = EventContext::default();
        for event in [
            DispatchedEvent::MouseDown {
                position: vec2f(10.0, 10.0),
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: vec2f(10.0, 10.0),
                button: 0,
            },
        ] {
            assert!(
                !footer.dispatch_event(&event, &mut ctx, &app),
                "a busy footer takes no pointer event either"
            );
        }
    }
}
