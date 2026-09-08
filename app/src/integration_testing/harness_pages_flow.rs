//! Smoke test for the harness observability pages: a topbar tab renders real
//! backend data (a seeded workflow) through the full [`RootView`], and the app
//! navigates back to the chat workspace.

mod common;

use goble_app::root_view::RootView;
use goble_app::ui::AppTab;
use goble_core::agent::Trigger;
use goble_ui::elements::AppContext;
use goble_ui::render::RenderCommand;
use goble_ui::test_util::render_element;
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
fn workflows_tab_renders_real_workflow_and_returns_to_chat() {
    let (desktop, _dir) = common::desktop_state();
    // Seed a real workflow so the page has data to render (not hardcoded).
    desktop
        .create_workflow(
            "Nightly digest",
            "e2e smoke",
            vec![],
            Trigger::Cron {
                expression: "0 12 * * *".to_string(),
            },
        )
        .expect("seed workflow");

    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state_rc = view.state_rc();
    state_rc.borrow_mut().current_tab = AppTab::Workflows;

    let mut root: Box<dyn Element> = Box::new(view);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    let texts = draw_texts(&commands);
    assert!(
        texts.iter().any(|t| t.contains("Workflows")),
        "the workflows tab should render its header, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("Nightly digest")),
        "the workflows tab should list the real seeded workflow, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("cron 0 12 * * *")),
        "the workflow row should show its trigger, got {texts:?}"
    );

    // Navigate back to the chat workspace and re-render without panicking.
    state_rc.borrow_mut().current_tab = AppTab::Chat;
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    let texts = draw_texts(&commands);
    assert!(
        texts.iter().any(|t| t.contains("Space")),
        "the chat workspace should render its space tab, got {texts:?}"
    );
}
