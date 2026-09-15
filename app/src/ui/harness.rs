//! Harness observability pages: workflows, tasks/executions, timeline and costs.
//!
//! Each page is a full main-area view (like the per-project panel) driven by
//! real data pulled from the embedded daemon / store via
//! [`crate::state::UiState::refresh_observability`]. No hardcoded arrays: a page
//! renders an honest empty/derived state when its backend has no records yet.

use goble_ui::elements::{
    AppContext, Axis, Button, ButtonVariant, Container, CrossAxisAlignment, Divider, EdgeInsets,
    Element, Expanded, Fill, Flex, HoverRow, Icon, MainAxisSize, Rect, Scrollable, Spacer, Text,
};
use goble_ui::geometry::vec2f;
use goble_ui::theme::{ColorToken, SpacingToken};

use super::{CostEntry, ExecutionEntry, TaskEntry, TimelineEntry, UiActions, UiSnapshot, WorkflowEntry};

/// Build an observability page scaffold: a back-to-chat header plus a
/// scrollable content column. `header_title` is the page name, `content` is the
/// body (already laid out as a column), `empty_note` is shown when `is_empty`.
fn build_page(
    app: &AppContext,
    actions: &UiActions,
    header_title: &str,
    count_label: String,
    content: Vec<Box<dyn Element>>,
    is_empty: bool,
    empty_note: &str,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let on_back = actions.on_settings_back.clone();
    let back_button = Button::new(
        Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(
            Icon::new("arrow-left")
                .with_size(13.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(Text::new("Back").with_theme_color(ColorToken::Text, app).finish())
        .finish(),
    )
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || (on_back.borrow_mut())())
        .finish();

    let header = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(back_button)
        .with_child(
            Text::new(header_title)
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(
            Text::new(count_label)
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish();

    let mut body = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    if is_empty {
        body = body.with_child(
            Text::new(empty_note)
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    for child in content {
        body = body.with_child(child);
    }

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(header);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(Scrollable::new(body.finish(), Axis::Vertical).finish());

    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
}

/// A small status dot colored by `token`.
fn status_dot(app: &AppContext, token: ColorToken) -> Box<dyn Element> {
    Container::new(Rect::new().with_size(vec2f(8.0, 8.0)).finish())
        .with_background(Fill::Solid(app.theme.color(token)))
        .with_corner_radius(4.0)
        .finish()
}

/// Resolve a status string to a dot color + short label.
fn status(app: &AppContext, status: &str) -> (ColorToken, Box<dyn Element>) {
    let (color, label): (ColorToken, &str) = match status {
        "running" => (ColorToken::Success, "running"),
        "suspended" | "pending" | "scheduled" | "queued" => (ColorToken::Warning, status),
        "success" | "succeeded" | "completed" | "done" => (ColorToken::Success, status),
        "failure" | "failed" | "error" => (ColorToken::Error, "failed"),
        "cancelled" => (ColorToken::Muted, "cancelled"),
        "idle" => (ColorToken::Muted, "idle"),
        _ => (ColorToken::Muted, if status.is_empty() { "—" } else { status }),
    };
    (
        color,
        Text::new(label)
            .with_font_size(11.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
}

/// Shell for a single list row: a square `SurfaceRaised` band, the shape the
/// transcript's rows and the sidebar's cards use. Shared by the observability
/// pages and the workflow-runs overlay, so a record is drawn once.
fn card(app: &AppContext, content: Box<dyn Element>) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    Container::new(content)
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_padding(EdgeInsets::uniform(sm))
        .finish()
}

/// Workflows page: every workflow registered with the daemon, with its trigger.
pub fn build_workflows_page(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let rows: Vec<Box<dyn Element>> = state.workflows.iter().map(|wf| build_workflow_row(app, wf)).collect();
    let count = if state.workflows.is_empty() {
        String::new()
    } else {
        format!("{} workflows", state.workflows.len())
    };
    build_page(
        app,
        actions,
        "Workflows",
        count,
        rows,
        state.workflows.is_empty(),
        "No workflows registered yet. Create one from the scheduled-tasks drawer or a workflow step.",
    )
}

pub(crate) fn build_workflow_row(app: &AppContext, wf: &WorkflowEntry) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let (color, status_el) = status(app, if wf.enabled { "enabled" } else { "disabled" });
    let info = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(2.0)
        .with_child(
            Text::new(wf.name.clone())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(
            Text::new(format!("{} · {} · created {}", wf.trigger, if wf.enabled { "enabled" } else { "disabled" }, wf.created_at))
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish();
    card(
        app,
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(status_dot(app, color))
            .with_child(info)
            .with_child(Spacer::new().finish())
            .with_child(status_el)
            .finish(),
    )
}

/// One row of the workflow-runs overlay's run list: the run's name and
/// objective, what it declares, and its status. `selected` draws the row whose
/// detail the panel shows beside it; the callback is what walks that selection
/// onto this run.
pub(crate) fn build_workflow_run_row(
    app: &AppContext,
    wf: &WorkflowEntry,
    selected: bool,
    on_click: impl FnMut() + 'static,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let (color, status_el) = status(app, if wf.enabled { "enabled" } else { "disabled" });
    let info = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(2.0)
        .with_child(
            Text::new(wf.name.clone())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(
            // The objective the record carries, under the name it belongs to.
            Text::new(wf.description.clone())
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(1)
                .finish(),
        )
        .finish();
    HoverRow::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(Expanded::new(info).finish())
            .with_child(
                Text::new(format!(
                    "{} · {}",
                    workflow_run_step_count(wf),
                    workflow_run_agent_count(wf)
                ))
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
            )
            .with_child(status_dot(app, color))
            .with_child(status_el)
            .finish(),
    )
    .with_selected(selected)
    .with_padding(EdgeInsets::uniform(sm))
    .with_on_click(on_click)
    .finish()
}

/// How many phases a workflow declares.
pub(crate) fn workflow_run_step_count(wf: &WorkflowEntry) -> String {
    let count = wf.steps.len();
    format!("{count} phase{}", if count == 1 { "" } else { "s" })
}

/// How many distinct agents the workflow's phases name. A step the record left
/// without an agent counts for nobody, so the number never claims an agent that
/// no execution could ever be matched to.
pub(crate) fn workflow_run_agent_count(wf: &WorkflowEntry) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for step in &wf.steps {
        let agent = step.agent_id.trim();
        if !agent.is_empty() && !seen.contains(&agent) {
            seen.push(agent);
        }
    }
    let count = seen.len();
    format!("{count} agent{}", if count == 1 { "" } else { "s" })
}

/// Tasks / executions page: the daemon execution ledger plus durable tasks.
pub fn build_executions_page(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let mut rows: Vec<Box<dyn Element>> = Vec::new();
    if state.executions.is_empty() && state.tasks.is_empty() {
        rows.push(
            Text::new("No executions or tasks recorded yet. Run a turn or schedule a task to see it here.")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    for ex in &state.executions {
        rows.push(build_execution_row(app, ex));
    }
    if !state.tasks.is_empty() {
        rows.push(
            Text::new("Tasks")
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    for task in &state.tasks {
        rows.push(build_task_row(app, task));
    }
    let count = format!(
        "{} executions · {} tasks",
        state.executions.len(),
        state.tasks.len()
    );
    build_page(
        app,
        actions,
        "Tasks / Executions",
        count,
        rows,
        state.executions.is_empty() && state.tasks.is_empty(),
        "No executions or tasks recorded yet.",
    )
}

pub(crate) fn build_execution_row(app: &AppContext, ex: &ExecutionEntry) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let (color, status_el) = status(app, &ex.status);
    let info = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(2.0)
        .with_child(
            Text::new(ex.id.clone())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(
            Text::new(format!(
                "agent {} · {} steps · started {}{}",
                ex.agent_id,
                ex.step_count,
                ex.started_at,
                ex.finished_at
                    .as_deref()
                    .map(|f| format!(" · finished {f}"))
                    .unwrap_or_default(),
            ))
            .with_font_size(11.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        )
        .finish();
    card(
        app,
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(status_dot(app, color))
            .with_child(info)
            .with_child(Spacer::new().finish())
            .with_child(status_el)
            .finish(),
    )
}

pub(crate) fn build_task_row(app: &AppContext, task: &TaskEntry) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let (color, status_el) = status(app, &task.status);
    let info = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(2.0)
        .with_child(
            Text::new(task.id.clone())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(
            Text::new(format!(
                "{} · session {} · created {}",
                task.trigger, task.session_id, task.created_at
            ))
            .with_font_size(11.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        )
        .finish();
    card(
        app,
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(status_dot(app, color))
            .with_child(info)
            .with_child(Spacer::new().finish())
            .with_child(status_el)
            .finish(),
    )
}

/// Timeline page: real execution/session/task records merged by timestamp.
pub fn build_timeline_page(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let rows: Vec<Box<dyn Element>> = state
        .timeline
        .iter()
        .map(|entry| build_timeline_row(app, entry))
        .collect();
    let count = if state.timeline.is_empty() {
        String::new()
    } else {
        format!("{} events", state.timeline.len())
    };
    build_page(
        app,
        actions,
        "Timeline",
        count,
        rows,
        state.timeline.is_empty(),
        "No execution or task activity recorded yet. Start a turn to populate the timeline.",
    )
}

fn build_timeline_row(app: &AppContext, entry: &TimelineEntry) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let (color, status_el) = match &entry.status {
        Some(s) => status(app, s),
        None => (ColorToken::Muted, Text::new("—").with_font_size(11.0).with_theme_color(ColorToken::Muted, app).finish()),
    };
    let info = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(2.0)
        .with_child(
            Text::new(entry.label.clone())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(
            Text::new(format!("{} · {}", entry.kind, entry.at))
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish();
    card(
        app,
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(status_dot(app, color))
            .with_child(info)
            .with_child(Spacer::new().finish())
            .with_child(status_el)
            .finish(),
    )
}

/// Costs page: derived from real execution/usage records. No cost backend
/// exists, so this shows a real derived summary (executions counted, any cost
/// metric summed) rather than fabricated billing numbers.
pub fn build_costs_page(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let rows: Vec<Box<dyn Element>> = state.costs.iter().map(|cost| build_cost_row(app, cost)).collect();
    let count = if state.costs.is_empty() {
        String::new()
    } else {
        format!("{} rows", state.costs.len())
    };
    build_page(
        app,
        actions,
        "Costs",
        count,
        rows,
        state.costs.is_empty(),
        "No usage data recorded yet. Costs are derived from real execution records; no cost backend is wired yet.",
    )
}

fn build_cost_row(app: &AppContext, cost: &CostEntry) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let info = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(2.0)
        .with_child(
            Text::new(cost.label.clone())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(
            Text::new(cost.note.clone())
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish();
    card(
        app,
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(info)
            .with_child(Spacer::new().finish())
            .with_child(
                Text::new(cost.amount.clone())
                    .with_font_size(12.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .finish(),
    )
}
