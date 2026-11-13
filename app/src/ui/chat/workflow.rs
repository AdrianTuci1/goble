//! The pane's workflow-runs view: the run ledger in the pane itself, drawn like
//! grok-build's "Workflow Runs" panel.
//!
//! It opens from the work dock's workflow row on that run's detail, or from the
//! workflow toggle on the run list, and it is a pane surface rather than an
//! overlay: the same title band and body shape as the sub-agent child view
//! ([`super::sub_agent`]), with Esc returning to the pane's own transcript.
//!
//! The list is grok-build's single line per run — name, `done/total phases ·
//! elapsed` and a status badge — and the detail leads with the run's status,
//! clock and objective, then its phases in order, the failure reason, the value
//! a script passed to `complete()` and the end of its log. The definition's
//! catalog stays on the Workflows page; this view is about runs.

use goble_ui::elements::chat_content::format_elapsed;
use goble_ui::elements::{
    AppContext, Axis, Chip, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Fill,
    Flex, HoverRow, Icon, MainAxisSize, Scrollable, Spacer, Text,
};
use goble_ui::theme::{ColorToken, FontFamily, SpacingToken};

use crate::ui::WorkflowRunEntry;

use super::super::{UiActions, UiSnapshot, WorkflowViewSnapshot};

/// The pane's workflow-runs surface. `view.selected` names the run whose detail
/// is open; `None` draws the run list.
pub(crate) fn build_workflow_view(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    _active: bool,
    view: &WorkflowViewSnapshot,
) -> Box<dyn Element> {
    let run = view
        .selected
        .as_ref()
        .and_then(|id| state.workflow_runs.iter().find(|run| &run.id == id));
    let definition = run.and_then(|run| {
        state
            .workflows
            .iter()
            .find(|workflow| workflow.id == run.workflow_id)
    });

    let title = match run {
        Some(run) => definition
            .map(|definition| definition.name.clone())
            .unwrap_or_else(|| run.workflow_id.clone()),
        None => "Workflows".to_string(),
    };
    let status = match run {
        Some(run) => status_word(&run.status).to_string(),
        None => format!(
            "{} run{}",
            state.workflow_runs.len(),
            if state.workflow_runs.len() == 1 {
                ""
            } else {
                "s"
            }
        ),
    };
    let (bullet, bullet_color) = match run {
        Some(run) if run.status == "running" => {
            (spinner_frame(state.work_dock_phase), ColorToken::Warning)
        }
        Some(run) => (status_glyph(&run.status), status_token(&run.status)),
        None => ("◆", ColorToken::Muted),
    };

    let on_close = actions.on_close_workflow_view.clone();
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);
    let close = {
        let on_close = on_close.clone();
        Chip::flat(
            Icon::new("x-close")
                .with_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_on_click(move || (on_close.borrow_mut())(pane_id))
        .finish()
    };

    let mut title_row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new(bullet)
                .with_theme_color(bullet_color, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_child(
            Text::new(title)
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_child(
            Text::new(status)
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(11.0)
                .finish(),
        )
        .with_child(Spacer::new().finish());
    if let Some(elapsed) = run.and_then(|run| run.live_elapsed()) {
        title_row = title_row.with_child(
            Text::new(format_elapsed(elapsed))
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(11.0)
                .finish(),
        );
    }
    if run.is_some() {
        let on_back = actions.on_select_workflow_run.clone();
        title_row = title_row.with_child(
            Chip::flat(
                Text::new("Runs")
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(11.0)
                    .finish(),
            )
            .with_on_click(move || (on_back.borrow_mut())(pane_id, None))
            .finish(),
        );
    }
    let title = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            Container::new(
                title_row
                    .with_child(
                        Text::new("Esc to return")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(11.0)
                            .finish(),
                    )
                    .with_child(close)
                    .finish(),
            )
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_padding(EdgeInsets::new(md, sm, md, sm))
            .finish(),
        )
        .with_child(Divider::horizontal().finish())
        .finish();

    let body: Box<dyn Element> = match run {
        Some(run) => build_detail(app, run, definition),
        None => build_list(app, state, actions, pane_id),
    };
    let body = Scrollable::new(body, Axis::Vertical).finish();

    let column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(title)
        .with_child(
            goble_ui::elements::Expanded::new(
                Container::new(body)
                    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                    .finish(),
            )
            .finish(),
        )
        .finish();

    // Esc returns to the pane's own surface, the way the child view's Esc
    // does, wherever the pointer or the keyboard focus sits in this view.
    let on_escape = actions.on_close_workflow_view.clone();
    goble_ui::elements::KeyHandler::new(
        Container::new(column)
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .finish(),
        move |key: &str, modifiers| {
            if modifiers.ctrl || modifiers.command || modifiers.alt {
                return false;
            }
            if key == "Escape" {
                (on_escape.borrow_mut())(pane_id);
                return true;
            }
            false
        },
    )
    .finish()
}

/// The run list: one row per recorded run, newest first, with the run's status
/// glyph, name, phase progress and elapsed time. A row opens that run's detail
/// in this pane.
fn build_list(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
) -> Box<dyn Element> {
    let md = app.theme.spacing_px(SpacingToken::Md);
    if state.workflow_runs.is_empty() {
        return Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
                .with_child(muted(app, "No workflow runs in this session yet.", 12.0))
                .with_child(muted(
                    app,
                    "A run appears here while it works and after it settles.",
                    11.0,
                ))
                .finish(),
        )
        .with_padding(EdgeInsets::uniform(md))
        .finish();
    }

    let mut rows = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.0);
    for run in &state.workflow_runs {
        let name = state
            .workflows
            .iter()
            .find(|workflow| workflow.id == run.workflow_id)
            .map(|workflow| workflow.name.clone())
            .unwrap_or_else(|| run.workflow_id.clone());
        let on_select = actions.on_select_workflow_run.clone();
        let run_id = run.id.clone();
        rows = rows.with_child(build_row(
            app,
            state.work_dock_phase,
            run,
            name,
            false,
            move || (on_select.borrow_mut())(pane_id, Some(run_id.clone())),
        ));
    }
    Container::new(rows.finish())
        .with_padding(EdgeInsets::uniform(md))
        .finish()
}

/// One run row: the glyph and name on the first line, the status word and the
/// phase clock under it — grok-build's `n/m phases · elapsed` reading.
fn build_row(
    app: &AppContext,
    phase: f32,
    run: &WorkflowRunEntry,
    name: String,
    selected: bool,
    on_click: impl FnMut() + 'static,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let token = status_token(&run.status);
    let bullet = if run.status == "running" {
        spinner_frame(phase)
    } else {
        status_glyph(&run.status)
    };
    let progress = if run.steps.is_empty() {
        // A run recorded at start has no phase yet: the clock and the word are
        // all there is to say.
        let mut parts = vec![status_word(&run.status).to_string()];
        if let Some(elapsed) = run.live_elapsed() {
            parts.push(format_elapsed(elapsed));
        }
        parts.join(" · ")
    } else {
        let mut parts = vec![format!(
            "{}/{} phases",
            completed_phases(run),
            run.steps.len()
        )];
        if let Some(elapsed) = run.live_elapsed() {
            parts.push(format_elapsed(elapsed));
        }
        if run.status == "running" {
            parts.push("running".to_string());
        }
        parts.join(" · ")
    };
    let name_row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new(bullet)
                .with_font_size(10.0)
                .with_font_family(FontFamily::Mono)
                .with_theme_color(token, app)
                .finish(),
        )
        .with_child(
            Text::new(name)
                .with_font_size(13.0)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(muted(app, &progress, 11.0))
        .finish();
    let detail_row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(
            Text::new(status_word(&run.status))
                .with_font_size(11.0)
                .with_theme_color(token, app)
                .finish(),
        )
        .with_child(muted(app, &format!("· started {}", run.started_at), 11.0))
        .finish();
    HoverRow::new(
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(3.0)
            .with_child(name_row)
            .with_child(detail_row)
            .finish(),
    )
    .with_selected(selected)
    .with_padding(EdgeInsets::new(sm, 8.0, sm, 8.0))
    .with_on_click(on_click)
    .finish()
}

/// One run's detail: what the run is for, how far it got, its phases in order,
/// and — once they exist — the failure reason, the `complete()` value and the
/// end of the log.
fn build_detail(
    app: &AppContext,
    run: &WorkflowRunEntry,
    definition: Option<&crate::ui::WorkflowEntry>,
) -> Box<dyn Element> {
    let md = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);

    if let Some(description) = definition
        .map(|definition| definition.description.trim())
        .filter(|description| !description.is_empty())
    {
        column = column.with_child(
            Text::new(description)
                .with_font_size(13.0)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(3)
                .finish(),
        );
    }
    let clock = match (&run.finished_at, run.live_elapsed()) {
        (Some(finished), Some(elapsed)) => format!(
            "started {} · finished {finished} · took {}",
            run.started_at,
            format_elapsed(elapsed)
        ),
        (None, _) => format!("started {} · running", run.started_at),
        (Some(finished), None) => format!("started {} · finished {finished}", run.started_at),
    };
    column = column.with_child(muted(app, &clock, 11.0));

    if let Some(error) = run
        .error
        .as_deref()
        .filter(|error| !error.trim().is_empty())
    {
        column = column.with_child(
            Text::new(error.to_string())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Error, app)
                .finish(),
        );
    }

    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(section(app, "Phases", run.steps.len()));
    if run.steps.is_empty() {
        column = column.with_child(muted(app, "No phase recorded yet.", 11.0));
    }
    for (index, step) in run.steps.iter().enumerate() {
        let token = status_token(&step.status);
        let label = match step.detail.as_deref().map(str::trim) {
            Some(detail) if !detail.is_empty() => {
                format!("{} · {}", status_word(&step.status), short_detail(detail))
            }
            _ => status_word(&step.status).to_string(),
        };
        column = column.with_child(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(
                    Text::new(format!("{}. {}", index + 1, step.name))
                        .with_font_size(12.0)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .with_child(Spacer::new().finish())
                .with_child(
                    Text::new(label)
                        .with_font_size(11.0)
                        .with_theme_color(token, app)
                        .finish(),
                )
                .finish(),
        );
    }

    if let Some(output) = &run.output {
        column = column.with_child(Divider::horizontal().finish());
        column = column.with_child(section(app, "Result", 1));
        column = column.with_child(
            Text::new(compact_output(output))
                .with_font_size(11.0)
                .with_font_family(FontFamily::Mono)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        );
    }
    if !run.log.is_empty() {
        column = column.with_child(Divider::horizontal().finish());
        column = column.with_child(section(app, "Log", run.log.len()));
        let shown = run.log.iter().rev().take(6).collect::<Vec<_>>();
        for line in shown.into_iter().rev() {
            column = column.with_child(muted(app, line, 11.0));
        }
        if run.log.len() > 6 {
            column = column.with_child(muted(
                app,
                &format!("{} earlier lines", run.log.len() - 6),
                11.0,
            ));
        }
    }
    Container::new(column.finish())
        .with_padding(EdgeInsets::uniform(md))
        .finish()
}

fn section(app: &AppContext, title: &str, count: usize) -> Box<dyn Element> {
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
        .with_child(muted(app, title, 11.0))
        .with_child(muted(app, &count.to_string(), 11.0))
        .finish()
}

fn muted(app: &AppContext, text: &str, size: f32) -> Box<dyn Element> {
    Text::new(text)
        .with_font_size(size)
        .with_theme_color(ColorToken::Muted, app)
        .finish()
}

/// How many of a run's phases have settled.
fn completed_phases(run: &WorkflowRunEntry) -> usize {
    run.steps
        .iter()
        .filter(|step| matches!(step.status.as_str(), "succeeded" | "failed" | "skipped"))
        .count()
        .min(run.steps.len())
}

/// The colour a run or phase status reads in.
fn status_token(status: &str) -> ColorToken {
    match status {
        "running" => ColorToken::Warning,
        "succeeded" | "success" | "completed" | "done" => ColorToken::Success,
        "failed" | "failure" | "error" => ColorToken::Error,
        _ => ColorToken::Muted,
    }
}

/// The word a status reads as, in the view's own voice.
fn status_word(status: &str) -> &str {
    match status {
        "" => "never run",
        "succeeded" | "success" | "completed" | "done" => "succeeded",
        "failed" | "failure" | "error" => "failed",
        other => other,
    }
}

/// The glyph a status leads with: a filled circle once the run settles.
fn status_glyph(status: &str) -> &'static str {
    match status {
        "succeeded" | "success" | "completed" | "done" => "●",
        "failed" | "failure" | "error" => "●",
        "running" => "●",
        _ => "○",
    }
}

/// The spinner glyph for `phase`, the same frames the dock and the footer use.
fn spinner_frame(phase: f32) -> &'static str {
    let count = goble_ui::elements::SPINNER_FRAMES.len();
    let index = (phase.rem_euclid(1.0) * count as f32) as usize;
    goble_ui::elements::SPINNER_FRAMES[index.min(count - 1)]
}

/// The first line of a phase's detail, capped to the row it shares.
fn short_detail(detail: &str) -> String {
    let line = detail.lines().next().unwrap_or_default().trim();
    let mut short: String = line.chars().take(40).collect();
    if line.chars().count() > 40 {
        short.push('…');
    }
    short
}

/// One JSON value on one line, capped so a large `complete()` value cannot
/// push the detail out of its column.
fn compact_output(value: &serde_json::Value) -> String {
    let text = value.to_string();
    let mut out: String = text.chars().take(240).collect();
    if text.chars().count() > 240 {
        out.push('…');
    }
    out
}
