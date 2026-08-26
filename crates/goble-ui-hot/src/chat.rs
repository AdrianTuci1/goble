//! Agent chat tab: identity header + message transcript + composer.

use goble_ui::elements::{
    AppContext, Button, ButtonVariant, ChatLayout, ChatSidebar, Container, CrossAxisAlignment,
    EdgeInsets, Element, Expanded, Fill, Flex, Icon, MainAxisSize, PopupMenuItem, RoutineItem,
    Spacer, Text, TopbarButton,
};
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::ChatView;

use crate::{UiActions, UiSnapshot, WorkspaceRouting};

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
    let on_answer_ask = actions.on_answer_ask.clone();
    let on_skip_ask = actions.on_skip_ask.clone();
    let on_toggle_auto_approve = actions.on_toggle_auto_approve.clone();
    let on_send_queued = actions.on_send_queued.clone();
    let on_dismiss_queued = actions.on_dismiss_queued.clone();
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

    let chat = ChatView::new()
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
        .with_pending_ask(state.pending_ask.clone())
        .with_on_answer_ask(move |resp, cred| (on_answer_ask.borrow_mut())(resp, cred))
        .with_on_skip_ask(move || (on_skip_ask.borrow_mut())())
        .with_auto_approve(state.auto_approve)
        .with_on_toggle_auto_approve(move |on| (on_toggle_auto_approve.borrow_mut())(on))
        .with_queued_prompt(state.queued_prompt.clone())
        .with_on_send_queued(move || (on_send_queued.borrow_mut())())
        .with_on_dismiss_queued(move || (on_dismiss_queued.borrow_mut())())
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
        .finish();

    // Right chat-sidebar: hidden by default, toggled from the chat header.
    // When open, wrap the chat surface in a ChatLayout with a ChatSidebar whose
    // routines come from the agent's real scheduled tasks. The sidebar's "+"
    // button opens the crons drawer (which can create a new scheduled task).
    let chat_el = if state.right_sidebar_open {
        let on_add = actions.on_open_crons.clone();
        let routines = state
            .crons
            .iter()
            .map(|cron| {
                let schedule = if cron.last_run == "unknown" {
                    cron.schedule.clone()
                } else {
                    cron.last_run.clone()
                };
                RoutineItem::new(cron.name.clone(), schedule, cron.enabled)
            })
            .collect();
        let sidebar = ChatSidebar::new(app)
            .with_routines(routines)
            .with_on_add(move || (on_add.borrow_mut())())
            .finish();
        ChatLayout::new(chat).with_right_sidebar(sidebar).finish()
    } else {
        chat
    };

    // First-run overlays: a "configure a model key" banner and/or the
    // "local or remote workspace?" choice sit above the transcript. They only
    // render when the app state asks for them, so the normal chat path is
    // unchanged.
    if state.show_llm_key_banner || state.show_workspace_choice {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let mut wrapper = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);
        if state.show_llm_key_banner {
            wrapper = wrapper.with_child(build_llm_key_banner(app, actions));
        }
        if state.show_workspace_choice {
            wrapper = wrapper.with_child(build_workspace_choice(app, actions));
        }
        wrapper = wrapper.with_child(Expanded::new(chat_el).finish());
        Container::new(wrapper.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .finish()
    } else {
        chat_el
    }
}

/// First-run banner shown in the chat when no model key is configured yet.
fn build_llm_key_banner(app: &AppContext, actions: &UiActions) -> Box<dyn Element> {
    let on_config = actions.on_config_llm_key.clone();
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let label = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Icon::new("key")
                .with_size(16.0)
                .with_theme_color(ColorToken::Warning, app)
                .finish(),
        )
        .with_child(
            Text::new(
                "You don't have any key configured, please click here to configure a model key.",
            )
            .with_theme_color(ColorToken::Text, app)
            .with_font_size(12.0)
            .finish(),
        )
        .finish();
    Button::new(label)
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || (on_config.borrow_mut())())
        .finish()
}

/// First-run choice prompt: pick where the agent should run.
fn build_workspace_choice(app: &AppContext, actions: &UiActions) -> Box<dyn Element> {
    let md = app.theme.spacing_px(SpacingToken::Md);
    let local = {
        let on_choose = actions.on_choose_workspace.clone();
        Button::new(Text::new("Local").finish())
            .with_variant(ButtonVariant::Primary)
            .with_on_click(move || (on_choose.borrow_mut())(WorkspaceRouting::Local))
            .finish()
    };
    let remote = {
        let on_choose = actions.on_choose_workspace.clone();
        Button::new(Text::new("Remote").finish())
            .with_variant(ButtonVariant::Ghost)
            .with_on_click(move || (on_choose.borrow_mut())(WorkspaceRouting::Remote))
            .finish()
    };
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(md)
        .with_child(
            Text::new("Run the agent locally or on a remote worker?")
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(local)
        .with_child(remote)
        .finish();
    Container::new(row)
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_corner_radius(app.theme.radius_px())
        .with_padding(EdgeInsets::uniform(md))
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
    // Right chat-sidebar toggle: shows/hides the Computer Use + Routines
    // panel. The icon reflects the current state (open handler vs. closed).
    let on_toggle_sidebar = actions.on_toggle_right_sidebar.clone();
    let panel_icon = if state.right_sidebar_open {
        "left-panel-close"
    } else {
        "left-panel-open"
    };
    let panel_button = TopbarButton::new(
        Icon::new(panel_icon)
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_on_click(move || (on_toggle_sidebar.borrow_mut())())
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
            .with_child(panel_button)
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
    .with_padding(EdgeInsets::new(0.0, md, 0.0, md))
    .finish()
}
