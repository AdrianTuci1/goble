//! Agent chat tab: identity header + message transcript + composer.

use goble_ui::elements::{
    AppContext, Button, ButtonVariant, ChatLayout, ChatSidebar, Container, CrossAxisAlignment,
    EdgeInsets, Element, Fill, Flex, Icon, MainAxisSize, PopupMenu, PopupMenuItem,
    PopupMenuPosition, RoutineItem, Spacer, Text, TopbarButton,
};
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::ChatView;

use super::{PaneChatSnapshot, UiActions, UiSnapshot, WorkspaceRouting};

/// Agent chat tab: a header row with the agent identity/status/copy/restart,
/// then the message transcript + composer (which fills the remaining space).
///
/// `pane_id` selects this pane's own transcript + composer draft + path, so
/// every chat pane renders an independent session instead of the global one.
pub fn build_agent_chat(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    active: bool,
) -> Box<dyn Element> {
    let header = build_agent_header(app, state, actions);

    // This pane's own session data; falls back to the global active view for a
    // pane that somehow has no entry yet.
    let session = state
        .pane_chat
        .get(&pane_id)
        .cloned()
        .unwrap_or_else(|| PaneChatSnapshot {
            conversation_id: state.selected_id.clone().unwrap_or_default(),
            messages: state.chat_messages.clone(),
            composer_draft: state.composer_draft.clone(),
            composer_path: state.composer_path.clone(),
            pending_ask: state.pending_ask.clone(),
            queued_prompt: state.queued_prompt.clone(),
            agent_busy: state.agent_busy,
            inline_screen: None,
            screen_link: None,
        });

    let on_composer_change = actions.on_composer_change.clone();
    let on_composer_focus = actions.on_composer_focus_change.clone();
    let on_pane_activate = actions.on_pane_activate.clone();
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
    let on_close_inline_screen = actions.on_close_inline_screen.clone();
    let on_open_screen_link = actions.on_open_screen_link.clone();
    let on_profile = actions.on_settings.clone();
    let on_select_harness = actions.on_select_harness.clone();
    let on_select_dir = actions.on_select_dir.clone();
    let on_select_branch = actions.on_select_branch.clone();
    let on_composer_slash = actions.on_composer_slash.clone();

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

    let mut chat = ChatView::new()
        .with_header(header)
        .with_messages(session.messages.clone())
        .with_composer_value(session.composer_draft.clone())
        // Only the active pane's composer is focused, so a background chat pane
        // never swallows keys before the active pane (e.g. a terminal) sees them.
        .with_composer_focused(active && state.composer_focused)
        .with_composer_path(session.composer_path.clone())
        .with_composer_model_label(state.selected_model.clone())
        .with_composer_stop_visible(session.agent_busy)
        .with_composer_on_change(move |text| (on_composer_change.borrow_mut())(text))
        // Focusing a pane's composer also activates that pane, so the composer
        // draft / send route to the pane the user is actually typing in.
        .with_composer_on_focus_change(move |focused| {
            if focused {
                (on_pane_activate.borrow_mut())(pane_id);
            }
            (on_composer_focus.borrow_mut())(focused);
        })
        .with_composer_on_attach(move || (on_attach.borrow_mut())())
        .with_composer_on_voice(move || (on_voice.borrow_mut())())
        .with_composer_on_select_model(move || (on_select_model.borrow_mut())())
        .with_composer_on_stop(move || (on_stop.borrow_mut())())
        .with_composer_on_profile(move || (on_profile.borrow_mut())())
        .with_pending_ask(session.pending_ask.clone())
        .with_on_answer_ask(move |resp, cred| (on_answer_ask.borrow_mut())(resp, cred))
        .with_on_skip_ask(move || (on_skip_ask.borrow_mut())())
        .with_auto_approve(state.auto_approve)
        .with_on_toggle_auto_approve(move |on| (on_toggle_auto_approve.borrow_mut())(on))
        .with_queued_prompt(session.queued_prompt.clone())
        .with_on_send_queued(move || (on_send_queued.borrow_mut())())
        .with_on_dismiss_queued(move || (on_dismiss_queued.borrow_mut())())
        .with_screen_link(session.screen_link.clone())
        .with_on_open_screen_link(move |uri| (on_open_screen_link.borrow_mut())(uri))
        .with_on_close_inline_screen(move || (on_close_inline_screen.borrow_mut())())
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
        .with_on_send(move |text| (on_send_message.borrow_mut())(text));

    // A live remote-desktop handoff renders inline at the end of the
    // transcript. The frame's texture key is stable per pane so the screen
    // stream updates in place across rebuilds.
    if let Some(frame) = &session.inline_screen {
        chat = chat.with_inline_screen(
            format!("inline-{pane_id}"),
            frame.frame_seq,
            frame.width,
            frame.height,
            std::sync::Arc::clone(&frame.data),
        );
    }

    // warp-new context pills: harness (the selected medium), working directory
    // (the selected session's path) and git branch. Each is a dropdown that
    // routes its selection back to app state.
    let context = &state.composer_context;
    let harness_ids = context.harness_ids.clone();
    let dir_ids = context.dir_ids.clone();
    chat = chat
        .with_composer_harness_label(context.harness_label.clone())
        .with_composer_harness_menu(
            context.harness_items.clone(),
            context.harness_menu_open.clone(),
            move |idx| {
                if let Some(id) = harness_ids.get(idx) {
                    (on_select_harness.borrow_mut())(id.clone());
                }
            },
        )
        .with_composer_dir_menu(
            context.dir_items.clone(),
            context.dir_menu_open.clone(),
            move |idx| {
                if let Some(id) = dir_ids.get(idx) {
                    (on_select_dir.borrow_mut())(id.clone());
                }
            },
        );
    if !context.branch_label.is_empty() {
        let branch_ids = context.branch_ids.clone();
        chat = chat
            .with_composer_branch_label(context.branch_label.clone())
            .with_composer_branch_menu(
                context.branch_items.clone(),
                context.branch_menu_open.clone(),
                move |idx| {
                    if let Some(id) = branch_ids.get(idx) {
                        (on_select_branch.borrow_mut())(id.clone());
                    }
                },
            );
    }
    let chat = chat
        .with_composer_on_slash(move || (on_composer_slash.borrow_mut())())
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

    // The first-run prompts (configure a model key, pick local/remote) are
    // rendered as modal overlays over the whole app in `build_ui`, so the chat
    // surface here is always the plain transcript + composer.
    chat_el
}

/// First-run modal content shown when no model key is configured yet.
pub(crate) fn build_llm_key_banner(app: &AppContext, actions: &UiActions) -> Box<dyn Element> {
    let on_config = actions.on_config_llm_key.clone();
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);
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
            Text::new("You don't have a model key configured yet.")
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(13.0)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(
            Button::new(Text::new("Configure a key").finish())
                .with_variant(ButtonVariant::Primary)
                .with_on_click(move || (on_config.borrow_mut())())
                .finish(),
        )
        .finish();
    Container::new(label)
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_corner_radius(app.theme.radius_px())
        .with_padding(EdgeInsets::uniform(md))
        .finish()
}

/// First-run compact getting-started hint, shown on the chat workspace once the
/// workspace choice is made. Dismissible; the dismissal survives a restart via
/// the persisted onboarding flag.
pub(crate) fn build_onboarding_tip(app: &AppContext, actions: &UiActions) -> Box<dyn Element> {
    let on_dismiss = actions.on_dismiss_onboarding_tip.clone();
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);
    let tip = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Icon::new("info")
                .with_size(16.0)
                .with_theme_color(ColorToken::Accent, app)
                .finish(),
        )
        .with_child(
            Text::new("You're all set. Use Ctrl/Cmd+Arrows to move between panes, Ctrl/Cmd+Space to split, and Cmd+K for commands.")
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(
            Button::new(Text::new("Got it").finish())
                .with_variant(ButtonVariant::Ghost)
                .with_on_click(move || (on_dismiss.borrow_mut())())
                .finish(),
        )
        .finish();
    Container::new(tip)
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_padding(EdgeInsets::new(md, sm, md, sm))
        .finish()
}

/// First-run choice prompt: pick where the agent should run.
pub(crate) fn build_workspace_choice(app: &AppContext, actions: &UiActions) -> Box<dyn Element> {
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

    // 3-dots menu: rename / clear transcript / toggle fullscreen. The menu
    // items are rebuilt each frame, but the open flag is app-owned so it
    // survives the per-frame element rebuild. The fullscreen item is marked
    // selected when the window is fullscreen (acts as a checked state).
    let on_rename = actions.on_rename_agent.clone();
    let on_clear = actions.on_clear_transcript.clone();
    let on_toggle_fullscreen = actions.on_toggle_fullscreen.clone();
    let menu_open = state.agent_header_menu_open.clone();
    let fullscreen_item = if state.fullscreen {
        PopupMenuItem::new("Fullscreen").with_icon("maximize").selected()
    } else {
        PopupMenuItem::new("Fullscreen").with_icon("maximize")
    };
    let menu_items = vec![
        PopupMenuItem::new("Rename"),
        PopupMenuItem::new("Clear transcript").with_icon("trash"),
        fullscreen_item,
    ];
    let dots_button = TopbarButton::new(
        Icon::new("dots-horizontal")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .finish();
    let menu = PopupMenu::new(dots_button, menu_items)
        .with_open(menu_open)
        .with_position(PopupMenuPosition::Below)
        .with_on_select(move |index| match index {
            0 => (on_rename.borrow_mut())(),
            1 => (on_clear.borrow_mut())(),
            2 => (on_toggle_fullscreen.borrow_mut())(),
            _ => {}
        });
    let header_menu = Box::new(menu);

    // Close button: the chat pane's single topbar carries the conversation name
    // + actions + 3-dots + close, so (with the generic pane header removed) the
    // pane is still closable and the traffic lights are never overlapped.
    let on_close = actions.on_close_pane.clone();
    let close_button = TopbarButton::new(
        Icon::new("x")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_on_click(move || (on_close.borrow_mut())())
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
            .with_child(header_menu)
            .with_child(close_button)
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
    .with_padding(EdgeInsets::new(0.0, md, 0.0, md))
    .finish()
}
