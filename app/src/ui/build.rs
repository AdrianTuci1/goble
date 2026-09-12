use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Container, CrossAxisAlignment, Divider, Element, Empty, Expanded, Fill, Flex,
    MainAxisSize, Stack,
};
use goble_ui::theme::ColorToken;
use goble_ui::{vec2f, Dialog, Sheet, DIALOG_DEFAULT_WIDTH, SHEET_DEFAULT_WIDTH};

use super::actions::{AiActions, MediaActions, ProjectsActions, ScreenActions, UiActions};
use super::palette::PaletteCommand;
use super::snapshot::{AiSnapshot, MediaSnapshot, ProjectsSnapshot, ScreenSnapshot, UiSnapshot};
use super::types::AppTab;
use super::{
    chat, connectors, crons, media, model_form, palette, screen, settings, shell, sidebar, vault,
    CONNECTORS_WIDTH,
};

/// Margin between the Settings overlay panel and the window edges. The panel
/// otherwise fills the surface, so the menu reads as the window's own surface
/// rather than a floating card.
pub const SETTINGS_OVERLAY_INSET: f32 = 12.0;

/// Build the complete app UI: topbar, sidebar, main content, and the
/// auxiliary sheets (crons, connectors, vault) stacked on top.
pub fn build_ui(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    ai: &AiSnapshot,
    ai_actions: &AiActions,
    projects: &ProjectsSnapshot,
    projects_actions: &ProjectsActions,
    media: &MediaSnapshot,
    media_actions: &MediaActions,
    screen: &ScreenSnapshot,
    screen_actions: &ScreenActions,
) -> Box<dyn Element> {
    let topbar = shell::build_topbar(app, state, actions, media, media_actions);
    let sidebar = sidebar::build_sidebar(app, state, actions, &media.selected_medium);
    let main = shell::build_main(
        app,
        state,
        actions,
        projects,
        projects_actions,
        ai,
        ai_actions,
        media,
        media_actions,
    );
    // With the sidebar retracted the main area (and its resizable divider) is
    // gone, so the chat/terminal column fills the whole window.
    let body: Box<dyn Element> = if state.sidebar_visible {
        let on_drag_start = actions.on_sidebar_drag_start.clone();
        let on_drag_move = actions.on_sidebar_drag_move.clone();
        let on_drag_end = actions.on_sidebar_drag_end.clone();
        shell::SidebarLayout::new(sidebar, main, state.sidebar_width)
            .with_dragging(state.sidebar_dragging)
            .with_on_drag_start(move |x| (on_drag_start.borrow_mut())(x))
            .with_on_drag_move(move |x| (on_drag_move.borrow_mut())(x))
            .with_on_drag_end(move || (on_drag_end.borrow_mut())())
            .finish()
    } else {
        drop(sidebar);
        main
    };

    let mut shell_col = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    // The toolbar is layered *above* the shell body (see the stack below) so the
    // trays it opens — the "+ ▾" environment menu above all — paint over the
    // sidebar and the pane surface instead of being covered by them. This
    // placeholder reserves the toolbar's height in the body column so the
    // content still starts below it.
    shell_col = shell_col.with_child(
        Empty::new()
            .with_size(vec2f(0.0, shell::TOPBAR_HEIGHT))
            .finish(),
    );
    // Separator below the topbar, like the sidebar separators. The topbar is
    // a fixed height and is not resizable, so this is purely a visual rule.
    shell_col = shell_col.with_child(Divider::horizontal().finish());
    // First-run: a compact getting-started hint on the chat workspace, shown
    // after the workspace choice is made, until dismissed.
    if state.current_tab == AppTab::Chat && state.show_onboarding_tip {
        shell_col = shell_col.with_child(chat::build_onboarding_tip(app, actions));
        shell_col = shell_col.with_child(Divider::horizontal().finish());
    }
    // The body must consume only the *remaining* height below the topbar, so
    // wrap it in `Expanded`; otherwise the chat composer would be pushed past
    // the bottom edge of the window.
    shell_col = shell_col.with_child(Expanded::new(body).finish());

    let on_close_crons = actions.on_close_crons.clone();
    let crons_sheet = Sheet::new(crons::build_crons_drawer(app, state, actions))
        .with_expanded(state.crons_open)
        .with_width(SHEET_DEFAULT_WIDTH)
        .with_on_close(move || (on_close_crons.borrow_mut())())
        .finish();

    let on_close_connectors = ai_actions.on_close_connectors.clone();
    let connectors_sheet = Sheet::new(connectors::build_connectors_sheet(app, ai, ai_actions))
        .with_expanded(ai.connectors_open)
        .with_width(CONNECTORS_WIDTH)
        .with_on_close(move || (on_close_connectors.borrow_mut())())
        .finish();

    let on_close_vault = ai_actions.on_close_vault.clone();
    let vault_sheet = Sheet::new(vault::build_vault_sheet(app, ai, ai_actions))
        .with_expanded(ai.vault_open)
        .with_width(SHEET_DEFAULT_WIDTH)
        .with_on_close(move || (on_close_vault.borrow_mut())())
        .finish();

    let on_close_screen = screen_actions.on_close.clone();
    let screen_sheet = Sheet::new(screen::build_screen_sheet(app, screen, screen_actions))
        .with_expanded(screen.open)
        .with_width(SHEET_DEFAULT_WIDTH)
        .with_on_close(move || (on_close_screen.borrow_mut())())
        .finish();

    // The agent panel (routines + scheduled tasks) floats over the workspace
    // as a right-anchored sheet, with an explicit X close button plus the
    // standard backdrop-click-to-close. `right_sidebar_open` is its open flag.
    let on_close_chat_panel = actions.on_toggle_right_sidebar.clone();
    let chat_panel_sheet = Sheet::new(chat::build_chat_panel_overlay(app, state, actions))
        .with_expanded(state.right_sidebar_open)
        .with_width(SHEET_DEFAULT_WIDTH)
        .with_on_close(move || (on_close_chat_panel.borrow_mut())())
        .finish();

    // Settings opens as a wide overlay inset a few pixels from the window
    // edges with a dimmed backdrop, so it reads as a settings menu rather than
    // a side panel. It is closed via the X, the navbar, the backdrop click or
    // Escape.
    let on_close_settings = actions.on_settings_close.clone();
    let settings_dialog = Dialog::new(settings::build_settings_overlay(app, state, actions))
        .with_open(state.settings_overlay_open)
        .with_inset(SETTINGS_OVERLAY_INSET)
        .with_on_close(move || (on_close_settings.borrow_mut())())
        .finish();

    let mut stack = Stack::new().with_children(vec![
        shell_col.finish(),
        // The toolbar paints after the body so the popups/trays anchored in it
        // (the "+ ▾" environment menu) sit above the sidebar and the pane
        // surface, and it is dispatched first within the shell for the same
        // reason. Sheets and dialogs below stay above the toolbar.
        topbar,
        crons_sheet,
        connectors_sheet,
        vault_sheet,
        screen_sheet,
        chat_panel_sheet,
        settings_dialog,
    ]);

    // First-run onboarding overlays: the local/remote workspace choice. The
    // missing-API-key error is no longer a dialog — it renders inline at the
    // top of the chat area (see `ChatView::with_notice`), so it stays next to
    // the conversation it blocks.
    let on_dismiss_workspace = actions.on_dismiss_workspace_choice.clone();
    if state.show_workspace_choice && !state.show_llm_key_banner {
        stack = stack.with_overlay(
            Dialog::new(chat::build_workspace_choice(app, actions))
                .with_open(true)
                .with_width(DIALOG_DEFAULT_WIDTH)
                .with_on_close(move || (on_dismiss_workspace.borrow_mut())())
                .finish(),
            vec2f(0.0, 0.0),
        );
    }

    // The model-provider dialog floats above everything (including the sheets).
    stack = stack.with_overlay(
        model_form::build_llm_dialog(app, state, actions),
        vec2f(0.0, 0.0),
    );
    // "Add a new environment" dialog, opened from the topbar "+" menu. It only
    // paints/intercepts events while open (see [`super::media::build_add_medium_dialog`]).
    stack = stack.with_overlay(
        media::build_add_medium_dialog(app, state, media_actions, actions),
        vec2f(0.0, 0.0),
    );

    // The Cmd+K command palette floats above every other overlay. Each command
    // is a closure that runs the underlying UiAction (or a toggle) and then
    // closes the palette.
    let close_palette = actions.on_close_command_palette.clone();
    let run = |action: &Rc<RefCell<dyn FnMut()>>| -> PaletteCommand {
        let act = action.clone();
        let close = close_palette.clone();
        PaletteCommand::new("", "", move || {
            (act.borrow_mut())();
            (close.borrow_mut())();
        })
    };
    let mut command_list: Vec<PaletteCommand> = vec![
        run(&actions.on_add_space).with_label("New space", "Cmd+Shift+N"),
        run(&actions.on_split_right).with_label("Split right", "Ctrl/Cmd+Space"),
        run(&actions.on_split_down).with_label("Split down", "Cmd+Shift+D"),
        run(&actions.on_new_terminal).with_label("New terminal", "Cmd+Shift+T"),
        run(&actions.on_close_pane).with_label("Close pane", "Ctrl/Cmd+W"),
        run(&actions.on_create_submit).with_label("New conversation", ""),
        run(&actions.on_settings).with_label("Open settings", ""),
        run(&actions.on_projects).with_label("Open projects", ""),
        run(&actions.on_open_crons).with_label("Open scheduled tasks", ""),
        run(&actions.on_toggle_right_sidebar).with_label("Toggle right sidebar", ""),
    ];
    {
        let toggle_dark = actions.on_toggle_dark_mode.clone();
        let dark_mode = state.settings_dark_mode;
        let close = close_palette.clone();
        command_list.push(PaletteCommand::new("Toggle dark mode", "", move || {
            (toggle_dark.borrow_mut())(!dark_mode);
            (close.borrow_mut())();
        }));
    }
    {
        // The same switch Settings→Agent carries; either surface turns modal
        // editing on, and the choice persists.
        let toggle_vim = actions.on_toggle_vim_mode.clone();
        let vim_mode = state.vim_mode;
        let close = close_palette.clone();
        command_list.push(PaletteCommand::new("Toggle vim mode", "", move || {
            (toggle_vim.borrow_mut())(!vim_mode);
            (close.borrow_mut())();
        }));
    }
    command_list.push(
        run(&screen_actions.on_open).with_label("Open screen", ""),
    );
    command_list.push(
        run(&ai_actions.on_open_connectors).with_label("Open connectors", ""),
    );
    command_list.push(run(&ai_actions.on_open_vault).with_label("Open vault", ""));
    // One "switch to space N" entry per existing space.
    for (i, space) in state.spaces.iter().enumerate() {
        let on_select_space = actions.on_select_space.clone();
        let close = close_palette.clone();
        let label = format!("Switch to {}", space.name);
        command_list.push(PaletteCommand::new(label, "", move || {
            (on_select_space.borrow_mut())(i);
            (close.borrow_mut())();
        }));
    }
    command_list.push(
        run(&screen_actions.on_open).with_label("Open screen", ""),
    );
    command_list.push(
        run(&ai_actions.on_open_connectors).with_label("Open connectors", ""),
    );
    command_list.push(run(&ai_actions.on_open_vault).with_label("Open vault", ""));
    // Model selection entries: typing `/model` or the model name filters the
    // palette down to these, so the slash path lists the config.toml models.
    // A palette pick applies to the active pane's own composer controls.
    let palette_pane_id = state.active_pane_id;
    for name in &state.models {
        let act = actions.on_model_select.clone();
        let close = close_palette.clone();
        let name = name.clone();
        command_list.push(PaletteCommand::new(format!("Model: {name}"), "", move || {
            (act.borrow_mut())(palette_pane_id, name.clone());
            (close.borrow_mut())();
        }));
    }

    stack = stack.with_overlay(
        palette::build_command_palette(
            app,
            state.command_palette_open,
            &state.command_palette_query,
            state.command_palette_index,
            command_list,
            actions.on_command_palette_change.clone(),
            actions.on_command_palette_move.clone(),
            actions.on_close_command_palette.clone(),
        ),
        vec2f(0.0, 0.0),
    );

    Container::new(stack.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .finish()
}
