use goble_ui::elements::Element;

use super::super::connectors::build_connectors_page;
use super::super::harness;
use super::super::panes;
use super::super::projects::build_projects_view;
use super::super::{AppTab, BuildContext};

/// Main content area: threads placeholder, terminal/chat, or the
/// per-project observability panel. Navigation is driven by the topbar buttons.
///
/// The settings tab is not a mode of this match: it is a space in the pane
/// tree, drawn by [`AppTab::Chat`] like every other tab.
pub fn build_main(context: &BuildContext<'_>) -> Box<dyn Element> {
    let BuildContext {
        app,
        state,
        actions,
        ai,
        ai_actions,
        projects,
        projects_actions,
        ..
    } = *context;
    match state.current_tab {
        // The chat/terminal area is the active space's pane tree; the medium
        // selector lives in the topbar, so the whole main column is panes.
        AppTab::Chat => panes::build_active_space(app, state, actions),
        AppTab::Projects => build_projects_view(app, projects, projects_actions),
        // Harness observability pages, driven by real daemon/store data.
        AppTab::Workflows => harness::build_workflows_page(app, state, actions),
        AppTab::Executions => harness::build_executions_page(app, state, actions),
        AppTab::Timeline => harness::build_timeline_page(app, state, actions),
        AppTab::Costs => harness::build_costs_page(app, state, actions),
        AppTab::Mcps => build_connectors_page(app, ai, ai_actions, &actions.on_settings_back),
    }
}
