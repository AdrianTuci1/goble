use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::execution::{ExecutionStatus, ExecutionTrace, LogLevel, Metric, Step, TraceEvent};
use goble_desktop_service::DesktopState;
use goble_ui::elements::{
    ActiveView, AppContext, Axis, Button, ButtonVariant, Container, CrossAxisAlignment, EdgeInsets,
    Element, EventContext, Fill, Flex, LayoutContext, MainAxisAlignment, PaintContext, Point,
    Scrollable, SizeConstraint, Text,
};
use goble_ui::elements::ShellState;
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};

use crate::app::UiState;

fn status_label(status: &ExecutionStatus) -> String {
    match status {
        ExecutionStatus::Pending => "pending".to_string(),
        ExecutionStatus::Running => "running".to_string(),
        ExecutionStatus::Success => "success".to_string(),
        ExecutionStatus::Failure(msg) => format!("failure: {}", msg),
        ExecutionStatus::Cancelled => "cancelled".to_string(),
    }
}

fn level_label(level: &LogLevel) -> &'static str {
    match level {
        LogLevel::Debug => "DEBUG",
        LogLevel::Info => "INFO",
        LogLevel::Warn => "WARN",
        LogLevel::Error => "ERROR",
    }
}

fn render_step(step: &Step, depth: usize, app: &AppContext) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let indent = 16.0 * depth as f32;
    let label = format!(
        "{} — {} — {}",
        step.name,
        status_label(&step.status),
        step.finished_at
            .map(|t| t.to_rfc3339())
            .as_deref()
            .unwrap_or("running")
    );
    let mut col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm)
        .with_child(
            Text::new(label)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        );
    for log in &step.logs {
        let line = format!(
            "[{}] [{}] {}",
            &log.timestamp.to_rfc3339()[..log.timestamp.to_rfc3339().len().min(19)],
            level_label(&log.level),
            log.message
        );
        col = col.with_child(
            Text::new(line)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    Container::new(col.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets {
            left: sm + indent,
            right: sm,
            top: sm,
            bottom: sm,
        })
        .finish()
}

fn render_steps(trace: &ExecutionTrace, app: &AppContext) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let mut col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    let sequential = trace.sequential_view();
    if sequential.is_empty() {
        col = col.with_child(
            Text::new("No steps recorded.")
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    } else {
        for (depth, step) in sequential {
            col = col.with_child(render_step(step, depth, app));
        }
    }
    Container::new(col.finish()).finish()
}

fn render_metrics(metrics: &[Metric], app: &AppContext) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let mut col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    if metrics.is_empty() {
        col = col.with_child(
            Text::new("No metrics.")
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    } else {
        for metric in metrics {
            let line = format!(
                "{} = {:.2} at {}",
                metric.name,
                metric.value,
                &metric.recorded_at.to_rfc3339()[..metric.recorded_at.to_rfc3339().len().min(19)]
            );
            col = col.with_child(
                Text::new(line)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            );
        }
    }
    Container::new(col.finish()).finish()
}

fn render_events(events: &[TraceEvent], app: &AppContext) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let mut col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    if events.is_empty() {
        col = col.with_child(
            Text::new("No events.")
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    } else {
        for event in events {
            let (timestamp, line) = match event {
                TraceEvent::Log {
                    timestamp,
                    level,
                    message,
                } => (
                    timestamp,
                    format!("[{}] {}", level_label(level), message),
                ),
                TraceEvent::AssistantDelta { timestamp, delta } => (
                    timestamp,
                    format!("[assistant delta] {}", delta),
                ),
                TraceEvent::ToolCallStarted {
                    timestamp,
                    id,
                    name,
                    arguments,
                } => (
                    timestamp,
                    format!("[tool start] {} {} {}", id, name, arguments),
                ),
                TraceEvent::ToolCallFinished { timestamp, id, result } => (
                    timestamp,
                    format!("[tool finish] {} {}", id, result),
                ),
                TraceEvent::ToolCallError { timestamp, id, message } => (
                    timestamp,
                    format!("[tool error] {} {}", id, message),
                ),
                TraceEvent::AskUser {
                    timestamp,
                    question,
                    quick_replies,
                } => (
                    timestamp,
                    format!("[ask] {} {:?}", question, quick_replies),
                ),
                TraceEvent::Done { timestamp, status } => (
                    timestamp,
                    format!("[done] {}", status_label(status)),
                ),
            };
            let ts = &timestamp.to_rfc3339()[..timestamp.to_rfc3339().len().min(19)];
            col = col.with_child(
                Text::new(format!("{} {}", ts, line))
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            );
        }
    }
    Container::new(col.finish()).finish()
}

pub struct AgentTraceViewPanel {
    content: Box<dyn Element>,
}

impl AgentTraceViewPanel {
    pub fn new(
        state: Arc<DesktopState>,
        shell_state: Rc<RefCell<ShellState>>,
        ui_state: Rc<RefCell<UiState>>,
        dirty: Rc<RefCell<bool>>,
        app: &AppContext,
    ) -> Self {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let _sm = app.theme.spacing_px(SpacingToken::Sm);

        let trace_id = ui_state.borrow().selected_trace_id.clone();
        let trace = trace_id.as_ref().and_then(|id| state.get_execution_trace(id));

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        let shell_state_for_back = Rc::clone(&shell_state);
        let dirty_for_back = Rc::clone(&dirty);
        let back = Button::new(Text::new("Back to executions").with_theme_color(ColorToken::Text, app).finish())
            .with_variant(ButtonVariant::Ghost)
            .with_on_click(move || {
                shell_state_for_back.borrow_mut().active_view = ActiveView::Executions;
                *dirty_for_back.borrow_mut() = true;
            })
            .finish();

        column = column.with_child(
            Flex::row()
                .with_main_axis_alignment(MainAxisAlignment::Start)
                .with_child(back)
                .finish(),
        );

        match trace {
            Some(trace) => {
                let header = format!(
                    "Trace {} — agent: {} — status: {} — started: {} — finished: {}",
                    trace_id.as_deref().unwrap_or("unknown"),
                    trace.agent_id.0,
                    status_label(&trace.status),
                    &trace.started_at.to_rfc3339()[..trace.started_at.to_rfc3339().len().min(19)],
                    trace
                        .finished_at
                        .map(|t| t.to_rfc3339())
                        .as_deref()
                        .unwrap_or("running")
                );
                column = column.with_child(
                    Text::new(header)
                        .with_font_size(18.0)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                );

                column = column.with_child(
                    Text::new("Steps")
                        .with_font_size(16.0)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                );
                column = column.with_child(render_steps(&trace, app));

                column = column.with_child(
                    Text::new("Metrics")
                        .with_font_size(16.0)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                );
                column = column.with_child(render_metrics(&trace.metrics, app));

                column = column.with_child(
                    Text::new("Events")
                        .with_font_size(16.0)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                );
                column = column.with_child(render_events(&trace.events, app));
            }
            None => {
                column = column.with_child(
                    Text::new("No trace selected or trace not found.")
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                );
            }
        }

        let content = Container::new(Scrollable::new(column.finish(), Axis::Vertical).finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .with_padding(EdgeInsets::uniform(spacing))
            .finish();

        Self { content }
    }
}

impl Element for AgentTraceViewPanel {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.content.layout(constraint, ctx, app)
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.content.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.content.size()
    }

    fn origin(&self) -> Option<Point> {
        self.content.origin()
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.content.dispatch_event(event, ctx, app)
    }
}
