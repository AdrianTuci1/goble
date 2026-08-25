//! Agent chat tab: identity header + message transcript + composer.

use goble_ui::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, Fill, Flex, Icon, MainAxisSize,
    PopupMenuItem, Spacer, Text, TopbarButton,
};
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::ChatView;

use crate::{UiActions, UiSnapshot};

/// Agent chat tab: a header row with the agent identity/status/copy/restart,
/// then the message transcript + composer (which fills the remaining space).
pub fn build_agent_chat(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let header = build_agent_header(app, state, actions);

    let on_composer_change = actions.on_composer_change.clone();
    let on_composer_focus = actions.on_composer_focus_change.clone();
    let on_send_message = actions.on_send_message.clone();
    let on_attach = actions.on_attach.clone();
    let on_voice = actions.on_voice.clone();
    let on_select_model = actions.on_select_model.clone();
    let on_stop = actions.on_stop.clone();
    let on_profile = actions.on_settings.clone();

    // Model dropdown: one item per available model; the current one is marked
    // selected. Selecting an item maps the index back to a model name.
    let model_items = state
        .models
        .iter()
        .map(|name| {
            let mut item = PopupMenuItem::new(name.clone()).with_icon("cpu");
            if name == &state.selected_model {
                item = item.selected();
            }
            item
        })
        .collect::<Vec<_>>();
    let on_model_select = actions.on_model_select.clone();
    let models_for_select = state.models.clone();
    let model_menu_open = state.model_menu_open.clone();

    // Account dropdown: settings + sign out. Selecting index 0 opens settings.
    let profile_items = vec![
        PopupMenuItem::new("Settings").with_icon("settings"),
        PopupMenuItem::new("Log out").with_icon("user"),
    ];
    let on_profile_select = actions.on_settings.clone();
    let profile_menu_open = state.profile_menu_open.clone();

    ChatView::new()
        .with_header(header)
        .with_messages(state.chat_messages.clone())
        .with_composer_value(state.composer_draft.clone())
        .with_composer_focused(state.composer_focused)
        .with_composer_model_label(state.selected_model.clone())
        .with_composer_stop_visible(state.agent_busy)
        .with_composer_on_change(move |text| (on_composer_change.borrow_mut())(text))
        .with_composer_on_focus_change(move |focused| (on_composer_focus.borrow_mut())(focused))
        .with_composer_on_attach(move || (on_attach.borrow_mut())())
        .with_composer_on_voice(move || (on_voice.borrow_mut())())
        .with_composer_on_select_model(move || (on_select_model.borrow_mut())())
        .with_composer_on_stop(move || (on_stop.borrow_mut())())
        .with_composer_on_profile(move || (on_profile.borrow_mut())())
        .with_composer_model_menu(model_items, model_menu_open, move |index| {
            if let Some(name) = models_for_select.get(index) {
                (on_model_select.borrow_mut())(name.clone());
            }
        })
        .with_composer_profile_menu(profile_items, profile_menu_open, move |index| {
            if index == 0 {
                (on_profile_select.borrow_mut())();
            }
        })
        .with_on_send(move |text| (on_send_message.borrow_mut())(text))
        .finish()
}

/// Agent identity row: name + status dot, then copy/restart/cron actions.
/// No avatar image; the row spans the full chat width.
fn build_agent_header(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);

    // Identity: the agent name only. The status dot/label is no longer shown
    // in this topbar.
    let identity = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(
            Text::new(state.agent_name.clone())
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        )
        .finish();

    let on_copy = actions.on_copy.clone();
    let on_restart = actions.on_restart.clone();
    let copy_button = TopbarButton::new(
        Icon::new("copy")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_on_click(move || (on_copy.borrow_mut())())
    .finish();
    let restart_button = TopbarButton::new(
        Icon::new("refresh")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_on_click(move || (on_restart.borrow_mut())())
    .finish();
    // Terminal topbar: button on the right opens the agent's crons drawer.
    let on_open_crons = actions.on_open_crons.clone();
    let cron_button = TopbarButton::new(
        Icon::new("terminal")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_on_click(move || (on_open_crons.borrow_mut())())
    .finish();

    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing)
            .with_child(identity)
            .with_child(Spacer::new().finish())
            .with_child(copy_button)
            .with_child(restart_button)
            .with_child(cron_button)
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
    .with_padding(EdgeInsets::new(0.0, md, 0.0, md))
    .finish()
}
