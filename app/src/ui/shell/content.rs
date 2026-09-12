use goble_ui::elements::{
    AppContext, Button, ButtonVariant, Container, CrossAxisAlignment, Element, Flex, Text,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::super::connectors::build_connectors_page;
use super::super::harness;
use super::super::panes;
use super::super::projects::build_projects_view;
use super::super::{
    AiActions, AiSnapshot, AppTab, MediaActions, MediaSnapshot, ProjectsActions, ProjectsSnapshot,
    UiActions, UiSnapshot,
};

/// Main content area: threads placeholder, terminal/chat, settings, or the
/// per-project observability panel. Navigation is driven by the topbar buttons.
pub fn build_main(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    projects: &ProjectsSnapshot,
    projects_actions: &ProjectsActions,
    ai: &AiSnapshot,
    ai_actions: &AiActions,
    _media: &MediaSnapshot,
    _media_actions: &MediaActions,
) -> Box<dyn Element> {
    match state.current_tab {
        // The chat/terminal area is the active space's pane tree; the medium
        // selector lives in the topbar, so the whole main column is panes.
        AppTab::Chat => panes::build_active_space(app, state, actions),
        // The Settings surface is disabled in this build: reachable only if a
        // stale tab flag survives, so we render a dead-end instead of the view.
        AppTab::Settings => build_settings_disabled(app, actions),
        AppTab::Projects => build_projects_view(app, projects, projects_actions),
        // Harness observability pages, driven by real daemon/store data.
        AppTab::Workflows => harness::build_workflows_page(app, state, actions),
        AppTab::Executions => harness::build_executions_page(app, state, actions),
        AppTab::Timeline => harness::build_timeline_page(app, state, actions),
        AppTab::Costs => harness::build_costs_page(app, state, actions),
        AppTab::Mcps => build_connectors_page(app, ai, ai_actions, &actions.on_settings_back),
    }
}

/// A dead-end panel shown if the (disabled) Settings tab is ever reached. The
/// Settings surface is inactive in this build, so this only backs out to Chat.
fn build_settings_disabled(app: &AppContext, actions: &UiActions) -> Box<dyn Element> {
    let on_back = actions.on_settings_back.clone();
    let back = Button::new(Text::new("← Back").with_theme_color(ColorToken::Text, app).finish())
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || (on_back.borrow_mut())())
        .finish();
    let md = app.theme.spacing_px(SpacingToken::Md);
    let body = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(md)
        .with_child(back)
        .with_child(
            Text::new("Settings")
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(16.0)
                .finish(),
        )
        .with_child(
            Text::new("Settings is disabled in this version.")
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(12.0)
                .finish(),
        )
        .finish();
    Container::new(body)
        .with_padding(goble_ui::elements::EdgeInsets::uniform(md))
        .finish()
}
