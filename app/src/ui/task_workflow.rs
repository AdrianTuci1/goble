//! The tasks & workflows overlay: one right-anchored panel listing the durable
//! tasks, the daemon's execution ledger and the registered workflows, from the
//! real records the observability pages read.
//!
//! It is an overlay rather than a page: the workspace (the pane tree and its
//! transcript) stays mounted underneath, the backdrop closes it, and nothing
//! here replaces `AppTab`. The rows are the pages' own row builders
//! ([`super::harness`]), so a workflow or a task is drawn once.

use goble_ui::elements::{
    AppContext, Axis, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Fill, Flex,
    Icon, KeyHandler, MainAxisSize, Scrollable, Spacer, Text, TopbarButton,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::harness::{build_execution_row, build_task_row, build_workflow_row};
use super::{UiActions, UiSnapshot};

/// A section heading: a name and its count, in the pages' muted caption style.
fn section(app: &AppContext, title: &str, count: usize) -> Box<dyn Element> {
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
        .with_child(
            Text::new(title)
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Text::new(count.to_string())
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish()
}

/// An honest empty line: named for the section that has no records, never a
/// fabricated row.
fn empty(app: &AppContext, note: &str) -> Box<dyn Element> {
    Text::new(note)
        .with_font_size(11.0)
        .with_theme_color(ColorToken::Muted, app)
        .finish()
}

/// The panel: header (title, counts, close), then the three real sections.
pub fn build_task_workflow_overlay(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let on_close = actions.on_close_task_workflow.clone();
    let close_button = TopbarButton::new(
        Icon::new("close")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_corner_radius(0.0)
    .with_on_click(move || (on_close.borrow_mut())())
    .finish();

    let header = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new("Tasks & workflows")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(
            Text::new(format!(
                "{} tasks · {} executions · {} workflows",
                state.tasks.len(),
                state.executions.len(),
                state.workflows.len()
            ))
            .with_font_size(11.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(close_button)
        .finish();

    let mut body = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);

    body = body.with_child(section(app, "Tasks", state.tasks.len()));
    if state.tasks.is_empty() {
        body = body.with_child(empty(app, "No durable tasks recorded yet."));
    }
    for task in &state.tasks {
        body = body.with_child(build_task_row(app, task));
    }

    body = body.with_child(section(app, "Executions", state.executions.len()));
    if state.executions.is_empty() {
        body = body.with_child(empty(app, "No executions recorded yet."));
    }
    for execution in &state.executions {
        body = body.with_child(build_execution_row(app, execution));
    }

    body = body.with_child(section(app, "Workflows", state.workflows.len()));
    if state.workflows.is_empty() {
        body = body.with_child(empty(app, "No workflows registered yet."));
    }
    for workflow in &state.workflows {
        body = body.with_child(build_workflow_row(app, workflow));
    }

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(header);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(Scrollable::new(body.finish(), Axis::Vertical).finish());

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
    use crate::ui::{ExecutionEntry, TaskEntry};
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

    /// A whole app tree over a real store: one workflow seeded through the
    /// desktop API, plus a durable task and an execution in app state. The
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
            s.settings_overlay_open = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.task_workflow_open = false;
            s.tasks = vec![TaskEntry {
                id: "task-7".to_string(),
                session_id: "session-2".to_string(),
                trigger: "manual".to_string(),
                status: "queued".to_string(),
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
        let constraint = SizeConstraint::loose(vec2f(1024.0, 768.0));
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

    /// The band the panel occupies: it is right-anchored, `SHEET_DEFAULT_WIDTH`
    /// wide, so a rect touching the right edge is the panel's.
    fn in_panel(rect: RectF, window_width: f32) -> bool {
        rect.max_x() >= window_width - goble_ui::SHEET_DEFAULT_WIDTH
    }

    /// The overlay draws the real records — a durable task, an execution and a
    /// registered workflow — over a workspace that stays mounted, with square
    /// rows; and every route closes it: the toggle key, Escape, the ✕ and the
    /// backdrop.
    #[test]
    fn the_overlay_lists_the_real_records_and_every_route_closes_it() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_with_records();
        let window = 1024.0;

        // Closed, none of it is drawn.
        let commands = frame(&mut root, &app);
        assert!(!drawn(&commands, "Tasks & workflows"));
        assert!(!drawn(&commands, "Nightly digest"));

        // The Cmd+K palette carries the same entry, so the overlay is reachable
        // without the chord.
        state.borrow_mut().command_palette_open = true;
        let commands = frame(&mut root, &app);
        assert!(
            drawn(&commands, "Tasks & workflows"),
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
            "Tasks & workflows",
            "1 tasks · 1 executions · 1 workflows",
            "Tasks",
            "task-7",
            "Executions",
            "exec-3",
            "Workflows",
            "Nightly digest",
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

        let rounded: Vec<String> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    corner_radius,
                    ..
                }
                | RenderCommand::FillRectFadeRight {
                    rect,
                    corner_radius,
                    ..
                } if in_panel(*rect, window)
                    && *corner_radius > 0.0
                    // An 8 px status dot is a dot, not a row: the panel's rows
                    // are bands, and a band is never that small.
                    && rect.width() > 12.0 =>
                {
                    Some(format!("fill {rect:?} radius {corner_radius}"))
                }
                _ => None,
            })
            .collect();
        assert!(
            rounded.is_empty(),
            "the overlay's rows are square, got {}",
            rounded.join(", ")
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
}
