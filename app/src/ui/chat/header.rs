use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Container, CrossAxisAlignment,
    EdgeInsets, Element, Fill, Flex, Icon, MainAxisSize, PopupMenu, PopupMenuItem,
    PopupMenuPosition, Spacer, TopbarButton,
};
use goble_ui::theme::{ColorToken, SpacingToken};


use super::super::{UiActions, UiSnapshot};

/// Height of the agent header's tallest control (`TopbarButton`'s default
/// size); the header is padded out to `shell::TOPBAR_HEIGHT` around it.
const HEADER_CONTROL_HEIGHT: f32 = 32.0;

/// Agent header row: a right-aligned 3-dots tray + a close X. Every pane gets
/// its own app-owned tray flag (`agent_header_menus[pane_id]`) so opening the
/// tray in one split pane does not open it in the other panes sharing the
/// view; the copy / restart / scheduled-tasks / side-panel actions are folded
/// into the 3-dots menu instead of cluttering the header.
/// The pane's topbar — the one bar a pane has, shared by both of its surfaces.
///
/// The agent view and the terminal pane's shell view draw this same bar (see
/// `terminal::build_terminal`): a draft's directory and branch are not repeated
/// here, because the rich input at the bottom of the pane already carries them.
/// A terminal pane adds the "Run agent" menu, since that pane can host a real
/// TUI agent, and while its harness is open the bar leads with `esc` — the way
/// back to the plain shell, spelled out at the front.
pub(crate) fn build_agent_header(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    harness_open: bool,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    // The agent header is the pane's topbar, so it is padded out to the same
    // height as the general topbar and the terminal pane header (36px on
    // macOS) instead of hugging its tallest control.
    let v_pad = ((super::super::shell::TOPBAR_HEIGHT - HEADER_CONTROL_HEIGHT) / 2.0).max(0.0);

    let on_rename = actions.on_rename_agent.clone();
    let on_clear = actions.on_clear_transcript.clone();
    let on_toggle_fullscreen = actions.on_toggle_fullscreen.clone();
    let on_copy = actions.on_copy.clone();
    let on_restart = actions.on_restart.clone();
    let on_open_crons = actions.on_open_crons.clone();
    let on_toggle_sidebar = actions.on_toggle_right_sidebar.clone();

    // The open flag is per-pane (app-owned) so it survives the per-frame
    // element rebuild and stays isolated between split panes.
    let menu_open = state
        .agent_header_menus
        .get(&pane_id)
        .cloned()
        .unwrap_or_else(|| Rc::new(RefCell::new(false)));
    let fullscreen_item = if state.fullscreen {
        PopupMenuItem::new("Fullscreen").with_icon("maximize").selected()
    } else {
        PopupMenuItem::new("Fullscreen").with_icon("maximize")
    };
    let panel_icon = if state.right_sidebar_open {
        "left-panel-close"
    } else {
        "left-panel-open"
    };
    let menu_items = vec![
        PopupMenuItem::new("Copy"),
        PopupMenuItem::new("Restart").with_icon("refresh"),
        PopupMenuItem::new("Scheduled tasks").with_icon("terminal"),
        PopupMenuItem::new("Toggle panel").with_icon(panel_icon),
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
        // The dots sit at the pane's right edge, so the tray opens to the left
        // and stays inside the window.
        .with_position(PopupMenuPosition::BelowEnd)
        .with_on_select(move |index| match index {
            0 => (on_copy.borrow_mut())(),
            1 => (on_restart.borrow_mut())(),
            2 => (on_open_crons.borrow_mut())(),
            3 => (on_toggle_sidebar.borrow_mut())(),
            4 => (on_rename.borrow_mut())(),
            5 => (on_clear.borrow_mut())(),
            6 => (on_toggle_fullscreen.borrow_mut())(),
            _ => {}
        });
    let header_menu = Box::new(menu);

    let on_close = actions.on_close_pane.clone();
    let close_button = TopbarButton::new(
        Icon::new("x")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_on_click(move || (on_close.borrow_mut())())
    .finish();

    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(spacing);
    if harness_open {
        // Clicking the hint is the switch itself: the harness closes and the
        // bar's editor lets go of the keys, which is what Esc does.
        let on_harness_mode = actions.on_set_pane_harness_mode.clone();
        let on_composer_focus = actions.on_composer_focus_change.clone();
        let hover = state
            .pane_hover
            .get(&pane_id)
            .cloned()
            .unwrap_or_else(|| Rc::new(RefCell::new(false)));
        row = row.with_child(super::super::panes::build_esc_hint(app, hover, move || {
            (on_harness_mode.borrow_mut())(pane_id, false);
            (on_composer_focus.borrow_mut())(false);
        }));
    }
    let row = row
        .with_child(Spacer::new().finish())
        .with_child(header_menu)
        .with_child(close_button)
        .finish();

    Container::new(row)
    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
    .with_padding(EdgeInsets::new(0.0, v_pad, 0.0, v_pad))
    .finish()
}
