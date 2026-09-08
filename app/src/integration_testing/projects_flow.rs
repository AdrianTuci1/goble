//! Integration tests for the per-project observability panel: rendering the
//! per-project session list with a running/scheduled status, and the tab
//! wired through the real [`RootView`].

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use goble_app::projects::ProjectsState;
use goble_app::root_view::RootView;
use goble_app::ui::projects::build_projects_view;
use goble_app::ui::{
    AppTab, ProjectEntry, ProjectSessionEntry, ProjectsActions, ProjectsSnapshot,
};
use goble_ui::elements::AppContext;
use goble_ui::render::RenderCommand;
use goble_ui::test_util::{command_counts, render_element};
use goble_ui::{vec2f, Element};

/// Collect the human-readable text of every `DrawText` command.
fn draw_texts(commands: &[RenderCommand]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn projects_panel_renders_per_project_list_and_status() {
    let app = AppContext::default();
    let snapshot = ProjectsSnapshot {
        projects: vec![
            ProjectEntry {
                project_id: "default".to_string(),
                directory: "/workspace/goble".to_string(),
                status: "running".to_string(),
                sessions: vec![ProjectSessionEntry {
                    session_id: "s-1".to_string(),
                    medium: "local".to_string(),
                    created_at: "just now".to_string(),
                    status: "running".to_string(),
                }],
            },
            ProjectEntry {
                project_id: "notebooks".to_string(),
                directory: "/workspace/notebooks".to_string(),
                status: "scheduled".to_string(),
                sessions: vec![ProjectSessionEntry {
                    session_id: "s-2".to_string(),
                    medium: "remote".to_string(),
                    created_at: "10 min ago".to_string(),
                    status: "scheduled".to_string(),
                }],
            },
        ],
    };
    let actions = ProjectsActions {
        on_refresh: Rc::new(RefCell::new(|| {})),
    };

    let mut element = build_projects_view(&app, &snapshot, &actions);
    let commands = render_element(&mut element, vec2f(800.0, 600.0), &app);
    let counts = command_counts(&commands);
    assert!(counts.fill_rect > 0, "panel should paint backgrounds");

    let texts = draw_texts(&commands);
    assert!(
        texts.iter().any(|t| t.contains("/workspace/goble")),
        "should list the default project, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("running")),
        "should show a running status, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("scheduled")),
        "should show a scheduled status, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("s-1 @ local")),
        "should list the session under its project, got {texts:?}"
    );
}

#[test]
fn projects_tab_renders_through_root_view() {
    let app = AppContext::default();
    let view = RootView::new(&app, None, None);
    view.state_rc().borrow_mut().current_tab = AppTab::Projects;

    let mut root: Box<dyn Element> = Box::new(view);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    let texts = draw_texts(&commands);
    assert!(
        texts.iter().any(|t| t.contains("Projects")),
        "the projects tab should render its header, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("running")),
        "mock projects panel shows a running project, got {texts:?}"
    );
}

#[test]
fn projects_state_from_empty_desktop_is_empty() {
    let (desktop, _dir) = common::desktop_state();
    let state = ProjectsState::from_desktop(&desktop);
    assert!(state.projects.is_empty());
}
