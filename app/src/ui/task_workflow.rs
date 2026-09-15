//! The workflow-runs overlay: what the daemon has registered, drawn the way
//! grok-build's "Workflow Runs" panel draws a run — the run's name, objective
//! and status, the phases it declares with the active one marked, and the agents
//! that run them.
//!
//! goble's daemon keeps no run ledger: a registered workflow is the whole record,
//! and the only progress beside it is the execution ledger. So a registered
//! workflow stands in for a run — its steps are the phases and a phase's agents
//! are that step's agent's executions, read from the real records behind
//! [`UiSnapshot::workflows`] and [`UiSnapshot::executions`]. What grok-build
//! reports and goble has no source for — elapsed time, phase progress, the agent
//! budget, and the pause / resume / stop / save affordances its rows carry — is
//! left out rather than invented. The full tasks / executions dump is still its
//! own page, so nothing is dropped for being out of place here.
//!
//! It is an overlay rather than a page: the workspace (the pane tree and its
//! transcript) stays mounted underneath, the backdrop closes it, and nothing
//! here replaces `AppTab`. Its rows come from [`super::harness`], so a record is
//! drawn once.

use goble_ui::elements::{
    AppContext, Axis, ConstrainedBox, Container, CrossAxisAlignment, Divider, EdgeInsets, Element,
    Expanded, Fill, Flex, HoverRow, Icon, KeyHandler, MainAxisAlignment, MainAxisSize, Scrollable,
    Spacer, Text, TopbarButton,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::harness::{
    build_execution_row, build_workflow_run_row, workflow_run_agent_count, workflow_run_step_count,
};
use super::{ExecutionEntry, UiActions, UiSnapshot, WorkflowEntry, WorkflowStepEntry};

/// How far the panel stands off every edge of the window. The overlay is a card
/// floating over the workspace, never a strip of the layout.
pub const OVERLAY_MARGIN: f32 = 32.0;

/// The width of the run list beside the detail, and of the phase rail inside it.
const RUN_LIST_WIDTH: f32 = 300.0;
const PHASE_RAIL_WIDTH: f32 = 200.0;

/// A section heading: a name and its count, in the pages' muted caption style.
fn section(app: &AppContext, title: &str, count: usize) -> Box<dyn Element> {
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
        .with_child(muted(app, title, 11.0))
        .with_child(muted(app, &count.to_string(), 11.0))
        .finish()
}

/// A muted line: the caption style, and the honest note a section with no
/// records gets.
fn muted(app: &AppContext, text: &str, size: f32) -> Box<dyn Element> {
    Text::new(text)
        .with_font_size(size)
        .with_theme_color(ColorToken::Muted, app)
        .finish()
}

/// The ✕ in the panel's header.
fn close_control(app: &AppContext, actions: &UiActions) -> Box<dyn Element> {
    let on_close = actions.on_close_task_workflow.clone();
    TopbarButton::new(
        Icon::new("close")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_corner_radius(0.0)
    .with_on_click(move || (on_close.borrow_mut())())
    .finish()
}

/// What a run declares, beside its name in either column: its phases and the
/// agents that run them. Both are counts of the record — goble keeps no run
/// progress to report against them.
fn run_counts(wf: &WorkflowEntry) -> String {
    format!(
        "{} · {}",
        workflow_run_step_count(wf),
        workflow_run_agent_count(wf)
    )
}

/// The run list: one row per registered workflow, each selecting the run whose
/// detail the panel shows beside it.
fn build_run_list(app: &AppContext, state: &UiSnapshot, selected: usize) -> Box<dyn Element> {
    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.0);
    if state.workflows.is_empty() {
        return column
            .with_child(muted(app, "No workflow runs in this session yet.", 11.0))
            .finish();
    }
    for (index, wf) in state.workflows.iter().enumerate() {
        let selected_cell = state.task_workflow_selected.clone();
        let phase_cell = state.task_workflow_phase.clone();
        column = column.with_child(build_workflow_run_row(app, wf, index == selected, move || {
            *selected_cell.borrow_mut() = index;
            // The phases belong to the run that was selected, so the phase
            // selection starts over with it.
            *phase_cell.borrow_mut() = 0;
        }));
    }
    column.finish()
}

/// The detail column: the selected run's objective and status, then its phases
/// beside the roster of the phase it has selected.
fn build_run_detail(
    app: &AppContext,
    state: &UiSnapshot,
    wf: &WorkflowEntry,
    phase: usize,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let header = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new(wf.name.clone())
                .with_font_size(13.0)
                .with_theme_color(ColorToken::Accent, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(muted(app, &run_counts(wf), 11.0))
        .finish();

    // What the record itself says: how the workflow is triggered, whether it is
    // enabled, and when it was created. There is no run status to report beside
    // it, so this line is the run's status.
    let status = format!(
        "{} · {} · created {}",
        wf.trigger,
        if wf.enabled { "enabled" } else { "disabled" },
        wf.created_at
    );

    let selected_phase = phase.min(wf.steps.len().saturating_sub(1));
    let body = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm)
        .with_child(
            ConstrainedBox::new(build_phase_rail(app, state, wf, selected_phase))
                .with_width(PHASE_RAIL_WIDTH)
                .with_min_width(PHASE_RAIL_WIDTH)
                .finish(),
        )
        .with_child(Divider::vertical().finish())
        .with_child(Expanded::new(build_roster(app, state, wf.steps.get(selected_phase))).finish());

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing)
        .with_child(header)
        .with_child(
            Text::new(wf.description.clone())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(muted(app, &status, 11.0));
    if !wf.enabled {
        column = column.with_child(muted(
            app,
            "This workflow is disabled: nothing runs it until it is enabled.",
            11.0,
        ));
    }
    column.with_child(Expanded::new(body.finish()).finish()).finish()
}

/// The phases of `wf`, in the record's own order. `❯` marks the phase the roster
/// is showing; a phase whose agent has a running execution is marked `●` and
/// labelled `running`, the way grok-build marks the phase a run is in.
fn build_phase_rail(
    app: &AppContext,
    state: &UiSnapshot,
    wf: &WorkflowEntry,
    selected: usize,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let steps = &wf.steps;

    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.0)
        .with_child(section(app, "Phases", steps.len()));
    if steps.is_empty() {
        return column
            .with_child(muted(app, "No steps recorded for this workflow.", 11.0))
            .finish();
    }

    for (index, step) in steps.iter().enumerate() {
        let executions = executions_for_agent(state, &step.agent_id);
        let running = executions.iter().any(|ex| ex.status == "running");
        // What the phase is doing, from the phase's own agent's ledger: running,
        // its last recorded status, or nothing recorded at all.
        let phase_state = if step.agent_id.trim().is_empty() {
            "no agent named".to_string()
        } else if running {
            "● running".to_string()
        } else if executions.is_empty() {
            "no execution yet".to_string()
        } else {
            executions[0].status.clone()
        };
        let marker = if index == selected { "❯" } else { " " };
        let phase_cell = state.task_workflow_phase.clone();
        let row = HoverRow::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(
                    Text::new(format!("{marker} {}", step.name))
                        .with_font_size(11.0)
                        .with_theme_color(
                            if index == selected {
                                ColorToken::Text
                            } else {
                                ColorToken::Muted
                            },
                            app,
                        )
                        .with_max_lines(1)
                        .finish(),
                )
                .with_child(Spacer::new().finish())
                .with_child(muted(app, &phase_state, 11.0))
                .finish(),
        )
        .with_selected(index == selected)
        .with_padding(EdgeInsets::uniform(sm))
        .with_on_click(move || *phase_cell.borrow_mut() = index)
        .finish();
        column = column.with_child(row);
    }
    column.finish()
}

/// The roster of the selected phase: the executions the daemon's ledger holds
/// for that phase's agent. A phase with no agent, or an agent with no execution,
/// says so rather than drawing an empty column.
fn build_roster(
    app: &AppContext,
    state: &UiSnapshot,
    step: Option<&WorkflowStepEntry>,
) -> Box<dyn Element> {
    let Some(step) = step else {
        return Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(section(app, "Agents", 0))
            .with_child(muted(app, "No phase selected.", 11.0))
            .finish();
    };

    let executions = executions_for_agent(state, &step.agent_id);
    let agent = if step.agent_id.trim().is_empty() {
        "—".to_string()
    } else {
        step.agent_id.clone()
    };
    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
        .with_child(section(app, "Agents", executions.len()))
        .with_child(muted(app, &format!("agent {agent}"), 11.0));
    if executions.is_empty() {
        return column
            .with_child(muted(app, "No execution recorded for this agent yet.", 11.0))
            .finish();
    }
    for execution in executions {
        column = column.with_child(build_execution_row(app, execution));
    }
    column.finish()
}

/// Every execution the ledger holds for `agent_id`, in the ledger's own order.
/// A phase that names no agent has none.
fn executions_for_agent<'a>(state: &'a UiSnapshot, agent_id: &str) -> Vec<&'a ExecutionEntry> {
    if agent_id.trim().is_empty() {
        return Vec::new();
    }
    state
        .executions
        .iter()
        .filter(|ex| ex.agent_id == agent_id)
        .collect()
}

/// The panel: header (title, count, close), then the run list beside the
/// selected run's detail.
pub fn build_task_workflow_overlay(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let header = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new("Workflow Runs")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(muted(app, &format!("{} registered", state.workflows.len()), 11.0))
        .with_child(Spacer::new().finish())
        .with_child(close_control(app, actions))
        .finish();

    let selected_run =
        (*state.task_workflow_selected.borrow()).min(state.workflows.len().saturating_sub(1));
    let selected_phase = *state.task_workflow_phase.borrow();

    let detail: Box<dyn Element> = match state.workflows.get(selected_run) {
        Some(wf) => build_run_detail(app, state, wf, selected_phase),
        None => Flex::column()
            .with_child(muted(
                app,
                "No run selected. A workflow appears here once the daemon has registered one.",
                11.0,
            ))
            .finish(),
    };

    let body = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::Start)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            ConstrainedBox::new(
                Container::new(
                    Scrollable::new(build_run_list(app, state, selected_run), Axis::Vertical)
                        .finish(),
                )
                .with_padding(EdgeInsets::uniform(sm))
                .finish(),
            )
            .with_width(RUN_LIST_WIDTH)
            .with_min_width(RUN_LIST_WIDTH)
            .finish(),
        )
        .with_child(Divider::vertical().finish())
        .with_child(
            Expanded::new(
                Container::new(Scrollable::new(detail, Axis::Vertical).finish())
                    .with_padding(EdgeInsets::uniform(sm))
                    .finish(),
            )
            .finish(),
        );

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(header);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(Expanded::new(body.finish()).finish());

    // The panel's rows are mouse-driven, so its keys are intercepted here,
    // before any child sees them: Escape closes the overlay, exactly as the
    // Settings panel's does.
    let on_escape = actions.on_close_task_workflow.clone();
    KeyHandler::new(
        Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_padding(EdgeInsets::uniform(spacing))
            .finish(),
        move |key: &str, modifiers| {
            if modifiers.ctrl || modifiers.command || modifiers.alt {
                return false;
            }
            if key == "Escape" {
                (on_escape.borrow_mut())();
                return true;
            }
            false
        },
    )
    .finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root_view::RootView;
    use crate::state::UiState;
    use crate::ui::{ExecutionEntry, WorkflowEntry, WorkflowStepEntry};
    use goble_core::agent::Trigger;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::event::{DispatchedEvent, ModifiersState};
    use goble_ui::geometry::{vec2f, RectF};
    use goble_ui::render::{RenderCommand, Renderer};
    use goble_ui::{Element, EventContext, LayoutContext, PaintContext, SizeConstraint};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    const WINDOW: (f32, f32) = (1024.0, 768.0);

    /// A whole app tree over a real store: one registered workflow whose phase
    /// names an agent, and the execution the ledger holds for that agent. The
    /// ledger's own refresh path is covered by the observability tests; what is
    /// under test here is that the overlay draws the records it is given.
    fn app_with_records() -> (Box<dyn Element>, Rc<RefCell<UiState>>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        desktop
            .create_workflow(
                "Nightly digest",
                "summarise the day",
                vec![],
                Trigger::Cron {
                    expression: "0 12 * * *".to_string(),
                },
            )
            .expect("seed workflow");

        let root = RootView::new(&AppContext::default(), &desktop, None);
        let state = root.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.task_workflow_open = false;
            s.tasks = vec![];
            s.workflows = vec![WorkflowEntry {
                id: "wf-1".to_string(),
                name: "Nightly digest".to_string(),
                description: "summarise the day".to_string(),
                steps: vec![WorkflowStepEntry {
                    name: "Digest".to_string(),
                    agent_id: "agent-1".to_string(),
                }],
                trigger: "cron 0 12 * * *".to_string(),
                enabled: true,
                created_at: "3 min ago".to_string(),
            }];
            s.executions = vec![ExecutionEntry {
                id: "exec-3".to_string(),
                agent_id: "agent-1".to_string(),
                worker_id: None,
                status: "running".to_string(),
                started_at: "1 min ago".to_string(),
                finished_at: None,
                step_count: 4,
            }];
        }
        (Box::new(root), state, dir)
    }

    /// One frame through the whole app, returning what it painted. The rebuild
    /// runs inside `layout`, so this is also what a key press lands on.
    fn frame(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<RenderCommand> {
        let constraint = SizeConstraint::loose(vec2f(WINDOW.0, WINDOW.1));
        let _ = root.layout(constraint, &mut LayoutContext::default(), app);
        let mut ctx = PaintContext::new(Renderer::new());
        root.paint(vec2f(0.0, 0.0), &mut ctx, app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    fn press(root: &mut Box<dyn Element>, app: &AppContext, key: &str) -> bool {
        chord(root, app, key, ModifiersState::none())
    }

    fn chord(
        root: &mut Box<dyn Element>,
        app: &AppContext,
        key: &str,
        modifiers: ModifiersState,
    ) -> bool {
        let mut ctx = EventContext::default();
        root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: key.to_string(),
                modifiers,
            },
            &mut ctx,
            app,
        )
    }

    /// The overlay's own chord: Cmd/Ctrl+Shift+W.
    fn toggle_chord(root: &mut Box<dyn Element>, app: &AppContext) -> bool {
        chord(
            root,
            app,
            "w",
            ModifiersState {
                command: true,
                shift: true,
                ..Default::default()
            },
        )
    }

    fn drawn(commands: &[RenderCommand], text: &str) -> bool {
        commands.iter().any(|command| {
            matches!(command, RenderCommand::DrawText { text: run, .. } if run == text)
        })
    }

    /// The panel's own surface fill — the card over the dimmed backdrop. The
    /// dialog paints its backdrop over the whole window first and the panel's
    /// surface over it, so the first surface fill after the backdrop is the
    /// panel's. This is the rect the "not a sidebar" assertions read.
    fn panel_rect(commands: &[RenderCommand], app: &AppContext) -> RectF {
        let backdrop = goble_ui::color::ColorU::new(0, 0, 0, 110);
        let dimmed = commands
            .iter()
            .position(|command| {
                matches!(command, RenderCommand::FillRect { color, .. } if *color == backdrop)
            })
            .expect("the overlay dims the workspace behind it");
        commands[dimmed..]
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == app.theme.color(ColorToken::Surface) =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the panel paints its own surface")
    }

    /// The overlay floats with a margin on every side — a panel over the
    /// workspace, not a right-anchored full-height strip — and it draws the real
    /// records: the run, its objective, its phase and its agent's execution. And
    /// every route closes it: the toggle key, Escape, the ✕ and the backdrop.
    #[test]
    fn the_overlay_is_an_inset_panel_and_every_route_closes_it() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_with_records();

        // Closed, none of it is drawn.
        let commands = frame(&mut root, &app);
        assert!(!drawn(&commands, "Workflow Runs"));
        assert!(!drawn(&commands, "Nightly digest"));

        // The Cmd+K palette carries the same entry, so the overlay is reachable
        // without the chord.
        state.borrow_mut().command_palette_open = true;
        let commands = frame(&mut root, &app);
        assert!(
            drawn(&commands, "Workflow runs"),
            "the palette lists the overlay"
        );
        state.borrow_mut().command_palette_open = false;

        // The chord opens it.
        assert!(toggle_chord(&mut root, &app), "the chord is consumed");
        assert!(
            state.borrow().task_workflow_open,
            "Cmd+Shift+W opens the overlay"
        );

        let commands = frame(&mut root, &app);
        for run in [
            "Workflow Runs",
            "1 registered",
            "Nightly digest",
            "summarise the day",
            "cron 0 12 * * * · enabled · created 3 min ago",
            "1 phase · 1 agent",
            "Phases",
            "❯ Digest",
            "● running",
            "Agents",
            "exec-3",
        ] {
            assert!(drawn(&commands, run), "the overlay draws {run:?}");
        }
        // The workspace is still mounted underneath: the pane's own composer and
        // the space tab are drawn alongside the panel, so the overlay covers the
        // workspace instead of replacing it. (The tab holds an agent with no
        // conversation subject yet, so it reads "New Agent".)
        assert!(drawn(&commands, "New Agent"), "the workspace stays mounted");
        assert!(
            drawn(&commands, "Ask anything..."),
            "the pane's composer stays mounted under the overlay"
        );

        // The panel has a margin on all four sides: strictly inside the viewport,
        // and no longer the full-height strip flush to the window's right edge.
        let panel = panel_rect(&commands, &app);
        assert_eq!(panel.min_x(), OVERLAY_MARGIN, "a margin on the left");
        assert_eq!(panel.min_y(), OVERLAY_MARGIN, "a margin on the top");
        assert_eq!(
            panel.max_x(),
            WINDOW.0 - OVERLAY_MARGIN,
            "a margin on the right"
        );
        assert_eq!(
            panel.max_y(),
            WINDOW.1 - OVERLAY_MARGIN,
            "a margin on the bottom"
        );
        assert!(
            panel.width() < WINDOW.0 && panel.height() < WINDOW.1,
            "the panel does not fill the window: {panel:?}"
        );

        // Escape closes it.
        assert!(press(&mut root, &app, "Escape"), "Escape is consumed");
        assert!(
            !state.borrow().task_workflow_open,
            "Escape closes the overlay"
        );

        // Re-open, and the ✕ closes it.
        assert!(toggle_chord(&mut root, &app));
        let commands = frame(&mut root, &app);
        let close = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon { origin, name, .. } if name == "close" => Some(*origin),
                _ => None,
            })
            .expect("the panel draws its close control");
        let mut ctx = EventContext::default();
        for event in [
            DispatchedEvent::MouseDown {
                position: close,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: close,
                button: 0,
            },
        ] {
            root.dispatch_event(&event, &mut ctx, &app);
        }
        assert!(
            !state.borrow().task_workflow_open,
            "the ✕ closes the overlay"
        );

        // Re-open, and a click on the backdrop (left of the panel) closes it.
        assert!(toggle_chord(&mut root, &app));
        frame(&mut root, &app);
        let mut ctx = EventContext::default();
        root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(10.0, 400.0),
                button: 0,
            },
            &mut ctx,
            &app,
        );
        assert!(
            !state.borrow().task_workflow_open,
            "the backdrop closes the overlay"
        );
    }

    /// The run list picks the run whose detail is drawn, and a phase row moves
    /// the roster onto that phase's agent.
    #[test]
    fn the_run_list_and_the_phase_rail_carry_the_selection() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_with_records();
        state.borrow_mut().workflows.push(WorkflowEntry {
            id: "wf-2".to_string(),
            name: "Weekly report".to_string(),
            description: "write the week up".to_string(),
            steps: vec![
                WorkflowStepEntry {
                    name: "Collect".to_string(),
                    agent_id: "agent-2".to_string(),
                },
                WorkflowStepEntry {
                    name: "Write".to_string(),
                    agent_id: "agent-3".to_string(),
                },
            ],
            trigger: "manual".to_string(),
            enabled: false,
            created_at: "1 h ago".to_string(),
        });
        state.borrow_mut().task_workflow_open = true;
        let commands = frame(&mut root, &app);
        // The first run is selected by default, so its detail is the one drawn.
        assert!(drawn(&commands, "❯ Digest"), "the first run's phase");
        assert!(!drawn(&commands, "❯ Collect"));

        // Selecting the second run moves the detail onto it.
        *state.borrow_mut().task_workflow_selected.borrow_mut() = 1;
        let commands = frame(&mut root, &app);
        assert!(drawn(&commands, "❯ Collect"), "the second run's phases");
        assert!(
            drawn(
                &commands,
                "This workflow is disabled: nothing runs it until it is enabled."
            ),
            "a disabled run says so"
        );
        assert!(
            !drawn(&commands, "❯ Digest"),
            "the previous run's phases are gone"
        );

        *state.borrow_mut().task_workflow_phase.borrow_mut() = 1;
        let commands = frame(&mut root, &app);
        assert!(drawn(&commands, "❯ Write"), "the selected phase is marked");
        assert!(
            drawn(&commands, "No execution recorded for this agent yet."),
            "a phase whose agent has no execution says so"
        );
    }

    /// A workflow with no phases, and no workflow at all: both say what is
    /// missing instead of drawing a row for a record that does not exist.
    #[test]
    fn empty_records_are_named_not_faked() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_with_records();
        {
            let mut s = state.borrow_mut();
            s.workflows = vec![WorkflowEntry {
                id: "wf-3".to_string(),
                name: "Empty sheet".to_string(),
                description: String::new(),
                steps: Vec::new(),
                trigger: "manual".to_string(),
                enabled: true,
                created_at: "now".to_string(),
            }];
            s.executions = vec![];
        }
        state.borrow_mut().task_workflow_open = true;
        let commands = frame(&mut root, &app);
        assert!(drawn(&commands, "No steps recorded for this workflow."));

        state.borrow_mut().workflows = vec![];
        let commands = frame(&mut root, &app);
        assert!(drawn(&commands, "0 registered"));
        assert!(drawn(&commands, "No workflow runs in this session yet."));
    }
}
