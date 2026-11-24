//! Left conversation sidebar.
//!
//! The conversation view is the DSH-parity "Workspaces" section: a 36pt header
//! (title plus the settings/new controls), the restyled new-conversation row,
//! and 32pt group/session rows with DeepSeek Harness's hover, selected and
//! menu-open states. The view toolbelt and the search field above it are
//! Goble's own chrome and are kept (the owner only skipped DSH's magnifier).
//! Numbers come from `/tmp/goble-work/sidebar-design-spec.md`; the layout
//! constants live in `conversation_list_item.rs` beside the row widgets.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::color::ColorU;
use goble_ui::elements::conversation_list_item::{
    sidebar_relative_time, SidebarActionRow, SidebarGroupRow, SidebarIconButton, SidebarList,
    SidebarMenuItem, SidebarSessionRow, SIDEBAR_GROUP_GAP, SIDEBAR_HEADER_BUTTON,
    SIDEBAR_HEADER_HEIGHT, SIDEBAR_HEADER_ICON, SIDEBAR_HEADER_PADDING_START,
    SIDEBAR_ICON_FOLDER_OPEN, SIDEBAR_ICON_NEW_CHAT, SIDEBAR_ICON_PIN_FILL,
    SIDEBAR_ICON_PROJECT_ADD, SIDEBAR_ICON_SLIDERS, SIDEBAR_LIST_BOTTOM_PAD, SIDEBAR_SECTION_TITLE,
    SIDEBAR_SECTION_TOP_INSET, SIDEBAR_TITLE_LINE, SIDEBAR_TITLE_SIZE,
};
use goble_ui::elements::{
    AgentCardUi, AppContext, Axis, ConstrainedBox, Container, ConversationEntry,
    CrossAxisAlignment, EdgeInsets, Element, Empty, Expanded, Fill, Flex, HoverRow, MainAxisSize,
    Padding, PopupMenu, PopupMenuItem, PopupMenuPosition, Scrollable, SearchInput, Spacer, Text,
    Tooltip, TooltipPosition,
};
use goble_ui::geometry::vec2f;
use goble_ui::theme::{ColorToken, SpacingToken};

use super::explorer::build_explorer;
use super::global_search::build_search;
use super::{ArchivedFilter, SidebarView, UiActions, UiSnapshot};

/// How many conversation rows the collapsed sidebar shows before the
/// "View all" button.
const COLLAPSED_CONVERSATIONS: usize = 4;

/// Extra height on the search field so the compact box is the spec's 32pt.
pub(crate) const SEARCH_EXTRA_HEIGHT: f32 = 4.2;

/// The collapsible section that pins starred conversations, above the folder
/// groups.
const STARRED_SECTION: &str = "starred";

/// Hover/focus keys the rows report into the shared app cells.
const KEY_HEADER_SETTINGS: &str = "header:settings";
const KEY_HEADER_NEW: &str = "header:new";
const KEY_NEW_CONVERSATION: &str = "action:new-conversation";

/// The collapsed key of a conversation folder's section.
fn folder_section(folder: &str) -> String {
    format!("folder:{folder}")
}

/// The hover cell key of a group row.
fn group_hover_key(section: &str) -> String {
    format!("group:{section}")
}

/// The hover cell key of a session row.
fn session_hover_key(id: &str) -> String {
    format!("session:{id}")
}

/// Whether `key` is the row the pointer was over on the last frame.
fn is_hovered(state: &UiSnapshot, key: &str) -> bool {
    state.sidebar_hover_row.borrow().as_deref() == Some(key)
}

/// Whether `key` is the row the sidebar's keyboard focus is on.
fn is_focused(state: &UiSnapshot, key: &str) -> bool {
    state.sidebar_focus_row.borrow().as_deref() == Some(key)
}

/// Left sidebar: the view toolbelt, then whichever view it selects.
///
/// The surface is flat: no rounded corners and no separator lines. The column's
/// padding matches DeepSeek Harness's sidebar (8pt inline, 4pt vertical, Goble's
/// spacing tokens).
pub fn build_sidebar(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    selected_medium: &str,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let xs = app.theme.spacing_px(SpacingToken::Xs);

    let view: Box<dyn Element> = match state.sidebar_view {
        SidebarView::Agents => agents_view(app, state, actions, selected_medium),
        // Z: the left sidebar no longer lists files; an old Explorer state
        // falls back to the conversations view.
        SidebarView::Explorer => agents_view(app, state, actions, selected_medium),
        SidebarView::Search => build_search(app, state, actions),
    };

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    // The general topbar is gone: the sidebar owns the window's top edge, so
    // it starts with the traffic lights (native on macOS, drawn elsewhere) and
    // then the conversation rows. The old view toolbelt lived on the general
    // bar; the panel header's sidebar toggle replaces it.
    column = column.with_child(Expanded::new(view).finish());

    // The Surface fill is the sidebar's backdrop, and it must reach the window's
    // bottom edge: the column is `MainAxisSize::Max`, so this container takes
    // the whole height the shell gives the sidebar (see `SidebarLayout`), and
    // every view under it puts its list in an `Expanded` scroll region. The
    // scroller paints nothing of its own, so after the last row — even with the
    // list shorter than the window — the area below is the sidebar surface
    // again, not the shell's page background.
    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets::new(sm, xs, sm, xs))
        .finish()
}

/// The sidebar's own toolbelt: which of its views is showing, and the hide
/// control at the trailing edge. Restyled to the section header's 28x28 icon
/// controls; the view names stay on the tooltips.
fn view_toolbelt(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(xs);
    for view in SidebarView::ALL {
        let active = state.sidebar_view == *view;
        let key = format!("toolbelt:{}", view.label());
        let on_select = actions.on_select_sidebar_view.clone();
        let icon = match view {
            SidebarView::Agents => "message-chat-square",
            SidebarView::Explorer => "folder-closed",
            SidebarView::Search => "search",
        };
        let tab = SidebarIconButton::new(icon)
            .with_button_size(SIDEBAR_HEADER_BUTTON)
            .with_icon_size(14.0)
            .with_radius(12.0)
            .with_active(active)
            .with_hovered(is_hovered(state, &key))
            .with_hover_cell(state.sidebar_hover_row.clone(), key)
            .with_on_click(move || (on_select.borrow_mut())(*view))
            .finish();
        row = row.with_child(
            Tooltip::new(tab, view.tooltip())
                .with_position(TooltipPosition::Below)
                .finish(),
        );
    }

    // Settings lives here, with the other Goble chrome controls.
    let settings_key = "toolbelt:settings";
    let on_settings = actions.on_settings.clone();
    let settings = SidebarIconButton::new("settings")
        .with_button_size(SIDEBAR_HEADER_BUTTON)
        .with_icon_size(14.0)
        .with_radius(12.0)
        .with_hovered(is_hovered(state, settings_key))
        .with_hover_cell(state.sidebar_hover_row.clone(), settings_key)
        .with_on_click(move || (on_settings.borrow_mut())())
        .finish();
    let close_key = "toolbelt:close";
    let on_close = actions.on_toggle_sidebar.clone();
    let close = SidebarIconButton::new("x-close")
        .with_button_size(SIDEBAR_HEADER_BUTTON)
        .with_icon_size(14.0)
        .with_radius(app.theme.radius_px())
        .with_hovered(is_hovered(state, close_key))
        .with_hover_cell(state.sidebar_hover_row.clone(), close_key)
        .with_on_click(move || (on_close.borrow_mut())())
        .finish();
    row = row
        .with_child(Spacer::new().finish())
        .with_child(
            Tooltip::new(settings, "View options")
                .with_position(TooltipPosition::Below)
                .finish(),
        )
        .with_child(
            Tooltip::new(close, "Hide sidebar")
                .with_position(TooltipPosition::Below)
                .finish(),
        );
    row.finish()
}

/// The three window controls at the sidebar's leading edge. On macOS the OS
/// draws the real lights over this reserved row; elsewhere the same three dots
/// are painted so the layout still reads as the reference.
fn traffic_lights(app: &AppContext) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0)
        .with_child(Empty::new().with_size(vec2f(8.0, 0.0)).finish());
    if !cfg!(target_os = "macos") {
        for color in ["#ff5f57", "#febc2e", "#28c840"] {
            row = row.with_child(
                Container::new(Empty::new().with_size(vec2f(10.0, 10.0)).finish())
                    .with_background(Fill::Solid(ColorU::hex(
                        u32::from_str_radix(color.trim_start_matches('#'), 16).unwrap_or(0),
                    )))
                    .with_corner_radius(5.0)
                    .finish(),
            );
        }
    }
    ConstrainedBox::new(row.finish()).with_height(24.0).finish()
}

/// One of the sidebar's plain top-level rows (Automations, Customizations).
fn sidebar_action(
    _app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    icon: &'static str,
    label: &'static str,
    key: &'static str,
) -> Box<dyn Element> {
    let on_settings = actions.on_settings.clone();
    SidebarActionRow::new(icon, label)
        .with_label_color(ColorToken::Text)
        .with_hovered(is_hovered(state, key))
        .with_hover_cell(state.sidebar_hover_row.clone(), key)
        .with_on_click(move || (on_settings.borrow_mut())())
        .finish()
}

/// The conversation view: search, the Workspaces header and its rows.
fn agents_view(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    selected_medium: &str,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    // Search box: compact, no magnifier (Goble keeps its own field; the
    // owner's one deliberate omission from the reference).
    let on_search_change = actions.on_search_change.clone();
    let on_search_focus = actions.on_search_focus_change.clone();
    let search = SearchInput::new()
        .with_value(state.search_query.clone())
        .with_focused(state.search_focused)
        .with_caret(state.sidebar_search_caret.clone())
        .with_placeholder("Search")
        .with_compact(true)
        .with_icon(false)
        .with_text_size(13.0)
        .with_corner_radius(app.theme.radius_px())
        .with_extra_height(SEARCH_EXTRA_HEIGHT)
        .with_on_change(move |value| (on_search_change.borrow_mut())(value))
        .with_on_focus_change(move |focused| (on_search_focus.borrow_mut())(focused))
        .finish();

    // The old "New conversation" row, rounded to the list's anatomy: a 32pt
    // full-width action under the search field (README: the owner wants the
    // affordance kept, just restyled). Starting a conversation is this row's
    // job; the Workspaces header's square-plus adds a workspace instead.
    let new_conversation = Padding::new(
        new_conversation_row(state, actions),
        EdgeInsets::new(0.0, sm, 0.0, 0.0),
    )
    .finish();

    // The spec's 8pt inset is the gap ABOVE the header; the first group row sits
    // flush under it (DSH's toolbar has no bottom margin).

    // The list takes the rest and scrolls inside it (its offset lives in app
    // state so it survives the per-frame rebuild).
    let list = workspaces_list(app, state, actions, selected_medium);
    let list = SidebarList::new(list, state.sidebar_drag.clone()).finish();
    let list = Scrollable::new(list, Axis::Vertical)
        .with_state(state.sidebar_scroll.clone())
        .finish();

    let automations = sidebar_action(
        app,
        state,
        actions,
        "sparkle",
        "Automations",
        "sidebar.automations",
    );
    let customizations = sidebar_action(
        app,
        state,
        actions,
        "sliders",
        "Customizations",
        "sidebar.customizations",
    );
    let on_settings = actions.on_settings.clone();
    let settings = SidebarActionRow::new("settings", "Settings")
        .with_label_color(ColorToken::Text)
        .with_hovered(is_hovered(state, "sidebar.settings"))
        .with_hover_cell(state.sidebar_hover_row.clone(), "sidebar.settings")
        .with_on_click(move || (on_settings.borrow_mut())())
        .finish();
    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    let _ = search;
    column = column.with_child(new_conversation);
    column = column.with_child(automations);
    column = column.with_child(customizations);
    column = column.with_child(Expanded::new(list).finish());
    column = column.with_child(settings);
    column.finish()
}

/// The sidebar's "New conversation" row: the old Goble affordance, restyled to
/// the Workspaces rows' anatomy — a 32pt rounded (radius 12) band with a 16pt
/// glyph and a 14pt label, borderless, that highlights on hover. The painted
/// band fills the column's content width (the 8pt sidebar padding is the
/// container's), while its glyph and label sit in the same leading column as
/// the list rows' so the sidebar reads as one grid.
///
/// It calls `on_create_submit` (the "start a conversation" action); the hover
/// flag lives in the shared sidebar hover cell so the highlight survives the
/// per-frame rebuild.
fn new_conversation_row(state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let on_create = actions.on_create_submit.clone();
    SidebarActionRow::new(SIDEBAR_ICON_NEW_CHAT, "New conversation")
        .with_label_color(ColorToken::Text)
        .with_paint_inset(0.0)
        .with_hovered(is_hovered(state, KEY_NEW_CONVERSATION))
        .with_hover_cell(state.sidebar_hover_row.clone(), KEY_NEW_CONVERSATION)
        .with_on_click(move || (on_create.borrow_mut())())
        .finish()
}

/// The section header: "Workspaces" and its two controls (settings and new
/// conversation). 36pt tall, 12pt inset, no magnifier.
/// A section heading without controls: "Conversations" sits at the same level
/// as the Projects header, in the same muted type.
fn plain_section_header(app: &AppContext, label: &str) -> Box<dyn Element> {
    let vertical = (SIDEBAR_HEADER_HEIGHT - SIDEBAR_HEADER_BUTTON) / 2.0;
    let title = Text::new(label.to_string())
        .with_font_size(SIDEBAR_TITLE_SIZE)
        .with_line_height(SIDEBAR_TITLE_LINE / SIDEBAR_TITLE_SIZE)
        .with_theme_color(ColorToken::Muted, app)
        .finish();
    Container::new(title)
        .with_padding(EdgeInsets::new(
            SIDEBAR_HEADER_PADDING_START,
            vertical,
            0.0,
            vertical,
        ))
        .finish()
}

fn workspaces_header(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    let vertical = (SIDEBAR_HEADER_HEIGHT - SIDEBAR_HEADER_BUTTON) / 2.0;

    let title = Text::new(SIDEBAR_SECTION_TITLE)
        .with_font_size(SIDEBAR_TITLE_SIZE)
        .with_line_height(SIDEBAR_TITLE_LINE / SIDEBAR_TITLE_SIZE)
        .with_theme_color(ColorToken::Muted, app)
        .finish();

    // The sliders button opens DSH's view-options menu: grouping and ordering,
    // the choices this build's data supports. Views/hide/Settings stay on the
    // toolbelt controls, per the review's parity correction. It is the shared
    // popup casing from the composer's menus (220 wide, radius 12).
    let flat_cell = state.sidebar_flat_list.clone();
    let recent_cell = state.sidebar_recent_first.clone();
    let flat_now = *flat_cell.borrow();
    let recent_now = *recent_cell.borrow();
    let archived_now = state.sidebar_archived_filter;
    let items = vec![
        PopupMenuItem {
            selected: !flat_now,
            ..PopupMenuItem::new("Folder")
                .with_icon("folder-close")
                .with_description("Group by folder")
        },
        PopupMenuItem {
            selected: flat_now,
            ..PopupMenuItem::new("Flat list")
                .with_icon("flat-list")
                .with_description("One list, no folder headers")
        },
        PopupMenuItem {
            selected: recent_now,
            ..PopupMenuItem::new("Recent first")
                .with_icon("clock")
                .with_description("Newest first")
        },
        PopupMenuItem {
            selected: !recent_now,
            ..PopupMenuItem::new("Manual order")
                .with_icon("chevrons-up-down")
                .with_description("Drag rows to reorder")
        },
        // The archived filter: DSH keeps it in the same view-options menu,
        // under the grouping/order choices and in its own group.
        PopupMenuItem {
            selected: archived_now == ArchivedFilter::Default,
            ..PopupMenuItem::new("Hide archived")
                .with_icon("archive-off")
                .with_description(ArchivedFilter::Default.description())
                .with_separator_before()
        },
        PopupMenuItem {
            selected: archived_now == ArchivedFilter::Show,
            ..PopupMenuItem::new("Show archived")
                .with_icon("layers-three-01")
                .with_description(ArchivedFilter::Show.description())
        },
        PopupMenuItem {
            selected: archived_now == ArchivedFilter::Only,
            ..PopupMenuItem::new("Archived only")
                .with_icon("archive-check")
                .with_description(ArchivedFilter::Only.description())
        },
    ];
    let menu_flat = state.sidebar_flat_list.clone();
    let menu_recent = state.sidebar_recent_first.clone();
    let menu_archived = actions.on_archived_filter_change.clone();
    let settings_trigger = SidebarIconButton::new(SIDEBAR_ICON_SLIDERS)
        .with_button_size(SIDEBAR_HEADER_BUTTON)
        .with_icon_size(SIDEBAR_HEADER_ICON)
        .with_radius(12.0)
        .with_hovered(is_hovered(state, KEY_HEADER_SETTINGS))
        .with_hover_cell(state.sidebar_hover_row.clone(), KEY_HEADER_SETTINGS)
        .finish();
    let menu_open = state.sidebar_menu_open.clone();
    let settings = PopupMenu::new(settings_trigger, items)
        .with_open(state.sidebar_menu_open.clone())
        .with_corner_radius(12.0)
        .with_max_width(220.0)
        .with_row_height(44.0)
        .with_icon_size(16.0)
        // The current choice reads in the web's accent cyan, as bright as its
        // neighbours' white labels (the shared casing's dimmer Accent is kept
        // for the composer menus).
        .with_selected_color(ColorToken::Focus)
        .with_position(PopupMenuPosition::BelowEnd)
        // The view-options panel is a menu like every other: the window's
        // open-menu registry knows it under this id.
        .with_menu_id("sidebar.view-options")
        .with_on_select(move |index| match index {
            0 => *menu_flat.borrow_mut() = false,
            1 => *menu_flat.borrow_mut() = true,
            2 => *menu_recent.borrow_mut() = true,
            3 => *menu_recent.borrow_mut() = false,
            4 => (menu_archived.borrow_mut())(ArchivedFilter::Default),
            5 => (menu_archived.borrow_mut())(ArchivedFilter::Show),
            6 => (menu_archived.borrow_mut())(ArchivedFilter::Only),
            _ => {}
        })
        .finish();
    let settings = DeferredOverlay::new(settings, menu_open).finish();

    // The Workspaces header's "+" adds a WORKSPACE, not a conversation (DSH's
    // `workspace.add`): the click opens the platform's directory picker, and the
    // picked folder becomes a row under this header. The separate "New
    // conversation" row above the header is what starts a conversation.
    let on_add_workspace = actions.on_add_workspace.clone();
    let on_open_ssh_target = actions.on_open_ssh_target.clone();
    let add = SidebarIconButton::new(SIDEBAR_ICON_PROJECT_ADD)
        .with_button_size(SIDEBAR_HEADER_BUTTON)
        .with_icon_size(SIDEBAR_HEADER_ICON)
        .with_radius(12.0)
        .with_hovered(is_hovered(state, KEY_HEADER_NEW))
        .with_hover_cell(state.sidebar_hover_row.clone(), KEY_HEADER_NEW)
        .finish();
    // DSH/T3's add-workspace menu: a small anchored casing with the local
    // directory flow and the SSH host flow. The popup owns the press on the
    // trigger, so the button itself takes no click.
    let add_menu = PopupMenu::new(
        Tooltip::new(add, "Add workspace")
            .with_position(TooltipPosition::Below)
            .finish(),
        vec![
            PopupMenuItem::new("New Project").with_icon("folder"),
            PopupMenuItem::new("SSH Host…").with_icon("terminal-input"),
        ],
    )
    .with_open(state.sidebar_add_menu_open.clone())
    .with_corner_radius(12.0)
    .with_max_width(220.0)
    .with_row_height(34.0)
    .with_icon_size(14.0)
    .with_position(PopupMenuPosition::BelowEnd)
    .with_menu_id("sidebar.add-workspace")
    .with_on_select(move |index| match index {
        0 => (on_add_workspace.borrow_mut())(),
        1 => (on_open_ssh_target.borrow_mut())(),
        _ => {}
    })
    .finish();
    let add_menu = DeferredOverlay::new(add_menu, state.sidebar_add_menu_open.clone()).finish();

    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(xs)
        .with_child(title)
        .with_child(Spacer::new().finish())
        .with_child(
            Tooltip::new(settings, "View options")
                .with_position(TooltipPosition::Below)
                .finish(),
        )
        .with_child(add_menu)
        .finish();
    Container::new(row)
        .with_padding(EdgeInsets::new(
            SIDEBAR_HEADER_PADDING_START,
            vertical,
            0.0,
            vertical,
        ))
        .finish()
}

/// The directory a row's hover card labels itself with. A conversation that has
/// run somewhere names its own directory; one that has not names where it would
/// run — the pane already holding it, or the focused pane's, which is where a
/// conversation opened from the list starts.
fn card_directory(state: &UiSnapshot, entry: &ConversationEntry) -> String {
    let directory = entry.directory.trim();
    if !directory.is_empty() {
        return directory.to_string();
    }
    let pane_path = state
        .pane_chat
        .values()
        .find(|pane| pane.conversation_id == entry.id)
        .map(|pane| pane.composer_path.trim())
        .filter(|path| !path.is_empty());
    match pane_path.unwrap_or_else(|| state.composer_path.trim()) {
        "" => crate::state::current_dir_display(),
        path => path.to_string(),
    }
}

/// One conversation entry as a session row.
fn session_row(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    entry: &ConversationEntry,
    section: &str,
    group_ids: &[String],
) -> Box<dyn Element> {
    let _ = app;
    let key = session_hover_key(&entry.id);
    let ui = state
        .agent_cards
        .get(&entry.id)
        .cloned()
        .unwrap_or_else(|| Rc::new(RefCell::new(AgentCardUi::default())));
    let time_label = entry
        .updated_at
        .as_deref()
        .map(sidebar_relative_time)
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| entry.timestamp.clone());

    let click_id = entry.id.clone();
    let on_select = actions.on_select_conversation.clone();
    // The hover strip's quick actions (DSH's `rowActions`): Pin and Archive,
    // wired to the same durable actions the tray's rows use. The pin is the
    // store's `chats.starred` (Goble's pin), so this and the Pin session row
    // are one action; an archived row's archive control is its restore.
    let quick_pin = actions.on_toggle_star.clone();
    let quick_pin_id = entry.id.clone();
    let quick_archive = if entry.archived {
        actions.on_unarchive_conversation.clone()
    } else {
        actions.on_archive_conversation.clone()
    };
    let quick_archive_id = entry.id.clone();

    // The drop handler writes the group's new order: the displayed ids are
    // captured here, so a drop reorders exactly the list the user saw.
    let order_cell = state.sidebar_session_order.clone();
    let recent_cell = state.sidebar_recent_first.clone();
    let section_key = section.to_string();
    let displayed: Vec<String> = group_ids.to_vec();
    let drop_handler = move |source: &str, target: &str, after: bool| {
        if !displayed.iter().any(|id| id == source) {
            // Cross-group drags are parked: a row may only move inside its own
            // group.
            return;
        }
        let mut ids = displayed.clone();
        ids.retain(|id| id != source);
        let at = ids
            .iter()
            .position(|id| id == target)
            .map(|position| if after { position + 1 } else { position })
            .unwrap_or(ids.len());
        ids.insert(at.min(ids.len()), source.to_string());
        order_cell.borrow_mut().insert(section_key.clone(), ids);
        // A drag is an explicit order choice: the list switches to Manual so the
        // move is visible instead of being overridden by recency.
        *recent_cell.borrow_mut() = false;
    };

    SidebarSessionRow::new(entry.id.clone(), entry.name.clone(), time_label)
        .with_group(section.to_string())
        .with_status(entry.status)
        .with_directory(card_directory(state, entry))
        .with_updated_at(entry.updated_at.clone().unwrap_or_default())
        .with_harness(entry.harness.clone())
        .with_remote(entry.workspace_routing == "remote")
        .with_starred(state.starred.iter().any(|id| id == &entry.id))
        .with_selected(state.selected_id.as_deref() == Some(entry.id.as_str()))
        .with_hovered(is_hovered(state, &key))
        .with_focus(is_focused(state, &key))
        .with_ui(ui)
        .with_hover_cell(state.sidebar_hover_row.clone(), key)
        .with_drag(state.sidebar_drag.clone())
        .with_drop_handler(drop_handler)
        .with_on_click(move || (on_select.borrow_mut())(click_id.clone()))
        .with_on_quick_pin(move || (quick_pin.borrow_mut())(quick_pin_id.clone()))
        .with_on_quick_archive(move || (quick_archive.borrow_mut())(quick_archive_id.clone()))
        // Archived rows keep the row's geometry but read as history; the app
        // blocks opening them and their menu has no Pin/Archive rows.
        .with_archived(entry.archived)
        // The tray is tall now: a row near the window's bottom flips it above.
        .with_viewport_height(state.window_size.y)
        // The app owns the tray: labels, icons, separators and availability.
        .with_menu_items(session_menu_items(state, actions, entry))
        .finish()
}

/// The DSH session menu for one conversation, in the order the shipped
/// `ui-workspace` registers its rows:
///
/// Pin session/Unpin session (100) — Rename (200) — Fork session (300) —
/// separator — Copy Session ID (450) — separator — Archive session/Unarchive
/// session (600) — separator — Delete conversation (Goble's own row, last).
/// An archived row shows Rename, Fork, Copy and Unarchive only: no pin, no
/// Archive (DSH resolves the same menu for an archived session).
fn session_menu_items(
    state: &UiSnapshot,
    actions: &UiActions,
    entry: &ConversationEntry,
) -> Vec<SidebarMenuItem> {
    let mut items = Vec::new();
    let starred = state.starred.iter().any(|id| id == &entry.id);
    if !entry.archived {
        let id = entry.id.clone();
        let on_star = actions.on_toggle_star.clone();
        items.push(SidebarMenuItem::new(
            if starred {
                "Unpin session"
            } else {
                "Pin session"
            },
            if starred {
                SIDEBAR_ICON_PIN_FILL
            } else {
                "pin-outline"
            },
            move || (on_star.borrow_mut())(id.clone()),
        ));
    }
    let id = entry.id.clone();
    let rename = actions.on_session_rename_start.clone();
    items.push(SidebarMenuItem::new("Rename", "edit", move || {
        (rename.borrow_mut())(id.clone())
    }));
    let id = entry.id.clone();
    let fork = actions.on_fork_conversation_by_id.clone();
    items.push(SidebarMenuItem::new("Fork session", "branch", move || {
        (fork.borrow_mut())(id.clone())
    }));
    let id = entry.id.clone();
    let copy = actions.on_copy_conversation_id.clone();
    items.push(
        SidebarMenuItem::new("Copy Session ID", "copy", move || {
            (copy.borrow_mut())(id.clone())
        })
        .with_separator_before(true),
    );
    if entry.archived {
        let id = entry.id.clone();
        let unarchive = actions.on_unarchive_conversation.clone();
        items.push(
            SidebarMenuItem::new("Unarchive session", "unarchive", move || {
                (unarchive.borrow_mut())(id.clone())
            })
            .with_separator_before(true),
        );
    } else {
        let id = entry.id.clone();
        let archive = actions.on_archive_conversation.clone();
        items.push(
            SidebarMenuItem::new("Archive session", "archive", move || {
                (archive.borrow_mut())(id.clone())
            })
            .with_separator_before(true),
        );
    }
    let id = entry.id.clone();
    let delete = actions.on_agent_delete.clone();
    items.push(
        SidebarMenuItem::new("Delete conversation", "trash-02", move || {
            (delete.borrow_mut())(id.clone())
        })
        .with_separator_before(true)
        .with_danger(true),
    );
    items
}

/// What a workspace row's controls act on: the directory behind the section
/// (the stored Workspace's path, or the directory the section's sessions
/// imply) and whether the user actually added it. A folder that only exists
/// through its conversations has no stored row to delete, so its tray offers no
/// Delete.
#[derive(Clone, Debug)]
struct WorkspaceSection {
    folder: String,
    path: String,
    title: String,
    stored: bool,
    /// `local` (empty) or `ssh:<alias|user@host:port>`.
    target: String,
}

impl WorkspaceSection {
    fn is_remote(&self) -> bool {
        self.target.starts_with("ssh:")
    }
}

/// The DSH workspace menu for one row, in DSH's registered order: Rename
/// (`ui-workspace` order 100), Show in Finder (`ui-open-in-app` order 200),
/// then — for a stored Workspace — Delete workspace (order 300, danger, behind
/// a separator). The labels are DSH's own: "Rename", "Show in Finder", "Delete
/// workspace".
fn workspace_menu_items(actions: &UiActions, workspace: &WorkspaceSection) -> Vec<SidebarMenuItem> {
    let rename = actions.on_sidebar_workspace_rename_start.clone();
    let path = workspace.path.clone();
    let title = workspace.title.clone();
    let mut items = vec![SidebarMenuItem::new("Rename", "edit", move || {
        (rename.borrow_mut())(path.clone(), title.clone())
    })];
    if !workspace.path.trim().is_empty() && !workspace.is_remote() {
        let reveal = actions.on_sidebar_workspace_show_in_finder.clone();
        let path = workspace.path.clone();
        items.push(SidebarMenuItem::new(
            show_in_file_manager_label(),
            SIDEBAR_ICON_FOLDER_OPEN,
            move || (reveal.borrow_mut())(path.clone()),
        ));
    }
    if workspace.stored {
        let delete = actions.on_sidebar_workspace_delete.clone();
        let path = workspace.path.clone();
        items.push(
            SidebarMenuItem::new("Delete workspace", "trash-02", move || {
                (delete.borrow_mut())(path.clone())
            })
            .with_separator_before(true)
            .with_danger(true),
        );
    }
    items
}

/// The reveal row's label: DSH localizes the same action per platform
/// (`menu.finder` "Show in Finder" on macOS, `menu.reveal` "Show in folder"
/// elsewhere), so the row reads the platform's own file manager.
fn show_in_file_manager_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in Finder"
    } else {
        "Show in folder"
    }
}

/// A group row plus its expanded rows, as one block.
///
/// `workspace` is the directory behind the row when the row is a Workspace: its
/// hover strip then carries the context-menu trigger (Rename / Delete, supplied
/// by [`workspace_menu_items`]) and the New-session control, and its tray state
/// is app-owned. The Starred block is not a Workspace and passes `None`.
#[allow(clippy::too_many_arguments)]
fn group_block(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    label: &str,
    section: &str,
    entries: &[&ConversationEntry],
    shown_ids: &std::collections::HashSet<&str>,
    fixed_glyph: Option<&'static str>,
    contains_selected: bool,
    workspace: Option<&WorkspaceSection>,
) -> Box<dyn Element> {
    let collapsed = state.collapsed_sections.iter().any(|key| key == section);
    let key = group_hover_key(section);
    let on_toggle = actions.on_toggle_section.clone();
    let toggle_key = section.to_string();
    let mut header = SidebarGroupRow::new(label, !collapsed)
        .with_active(contains_selected)
        .with_hovered(is_hovered(state, &key))
        .with_hover_cell(state.sidebar_hover_row.clone(), key)
        .with_on_click(move || (on_toggle.borrow_mut())(toggle_key.clone()));
    if label == "Conversations" {
        header = header.without_leading_icon();
    } else if let Some(icon) = fixed_glyph {
        header = header.with_leading_icon(icon);
    } else if workspace.is_some_and(WorkspaceSection::is_remote) {
        // The remote target is a badge, not a second line: the row's leading
        // glyph becomes the cloud/terminal mark and the title stays the label.
        header = header.with_leading_icon("environment-cloud");
    }
    if let Some(workspace) = workspace {
        let menu = actions.on_sidebar_workspace_menu.clone();
        let menu_section = section.to_string();
        let create = actions.on_sidebar_workspace_new_session.clone();
        let create_path = workspace.path.clone();
        let create_target = workspace.target.clone();
        header = header
            .with_ui(state.group_ui(section))
            .with_viewport_height(state.window_size.y)
            .with_menu_items(workspace_menu_items(actions, workspace))
            .with_on_create_session(move || {
                (create.borrow_mut())(create_path.clone(), create_target.clone())
            })
            .with_on_menu_open(move || (menu.borrow_mut())(Some(menu_section.clone())));
    }
    let mut block = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(GOBLE_ROW_GAP)
        .with_child(header.finish());
    if !collapsed {
        let group_ids: Vec<String> = entries.iter().map(|entry| entry.id.clone()).collect();
        for entry in entries {
            if !shown_ids.contains(entry.id.as_str()) {
                continue;
            }
            block = block.with_child(session_row(app, state, actions, entry, section, &group_ids));
        }
    }
    block.finish()
}

/// The row gap inside a group (2pt per the spec).
const GOBLE_ROW_GAP: f32 = goble_ui::elements::conversation_list_item::SIDEBAR_ROW_GAP;

/// A child whose open popup must paint above the rest of the sidebar.
///
/// The immediate-mode tree paints in child order, so the header's view-options
/// panel (painted with the header) would be covered by the rows below it. This
/// wrapper paints the child normally and, while its flag is set, queues the same
/// element into the frame's top layer, which the root paints last — exactly the
/// mechanism the row trays and hover cards use.
struct DeferredOverlay {
    inner: Rc<RefCell<Box<dyn Element>>>,
    open: Rc<RefCell<bool>>,
    size: Option<goble_ui::geometry::Vector2F>,
    origin: Option<goble_ui::elements::Point>,
}

impl DeferredOverlay {
    fn new(child: Box<dyn Element>, open: Rc<RefCell<bool>>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(child)),
            open,
            size: None,
            origin: None,
        }
    }

    fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for DeferredOverlay {
    fn layout(
        &mut self,
        constraint: goble_ui::elements::SizeConstraint,
        ctx: &mut goble_ui::elements::LayoutContext,
        app: &AppContext,
    ) -> goble_ui::geometry::Vector2F {
        let size = self.inner.borrow_mut().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(
        &mut self,
        origin: goble_ui::geometry::Vector2F,
        ctx: &mut goble_ui::elements::PaintContext,
        app: &AppContext,
    ) {
        self.origin = Some(goble_ui::elements::Point::from_vec2f(
            origin,
            Default::default(),
        ));
        self.inner.borrow_mut().paint(origin, ctx, app);
        if *self.open.borrow() {
            app.hover_chips
                .borrow_mut()
                .push_shared(origin, Rc::clone(&self.inner));
        }
    }

    fn size(&self) -> Option<goble_ui::geometry::Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<goble_ui::elements::Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &goble_ui::event::DispatchedEvent,
        ctx: &mut goble_ui::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        self.inner.borrow_mut().dispatch_event(event, ctx, app)
    }
}

/// The conversations under the groups, ordered by the user's manual order then
/// by recency.
fn ordered_entries<'a>(
    entries: &[&'a ConversationEntry],
    order: Option<&Vec<String>>,
) -> Vec<&'a ConversationEntry> {
    let mut ordered = entries.to_vec();
    let rank = |entry: &ConversationEntry| {
        order
            .and_then(|ids| ids.iter().position(|id| id == &entry.id))
            .unwrap_or(usize::MAX)
    };
    ordered.sort_by(|left, right| {
        rank(left)
            .cmp(&rank(right))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.name.cmp(&right.name))
    });
    ordered
}

/// The Workspaces list: the starred group first, then the folder groups in
/// alphabetical order, the digest overflow control and the empty state.
fn workspaces_list(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    selected_medium: &str,
) -> Box<dyn Element> {
    let xs = app.theme.spacing_px(SpacingToken::Xs);

    // The archived filter applies to both blocks: a hidden row is hidden even
    // from the Starred block, an archived row never leads a pinned block (DSH
    // keeps its pinned slot for a restore but does not pin an archived row),
    // and under "Archived only" only archived rows are listed.
    let filter = state.sidebar_archived_filter;
    let passes_filter = |entry: &ConversationEntry| match filter {
        ArchivedFilter::Default => !entry.archived,
        ArchivedFilter::Show => true,
        ArchivedFilter::Only => entry.archived,
    };
    let is_starred = |entry: &ConversationEntry| {
        !entry.archived && state.starred.iter().any(|id| id == &entry.id)
    };
    let starred: Vec<&ConversationEntry> = state
        .conversations
        .iter()
        .filter(|entry| entry.has_agent_reply && passes_filter(entry) && is_starred(entry))
        .collect();

    let routing = crate::media::medium_routing(selected_medium);
    let visible: Vec<&ConversationEntry> = state
        .conversations
        .iter()
        .filter(|entry| {
            entry.has_agent_reply
                && passes_filter(entry)
                && entry.workspace_routing == routing
                && !is_starred(entry)
        })
        .collect();

    // Recency order across the whole list drives the digest's first rows.
    let order_cell = state.sidebar_session_order.borrow();
    let mut by_recency = visible.clone();
    by_recency.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.name.cmp(&right.name))
    });
    // Every conversation is listed: the sidebar no longer collapses the list
    // behind a "View all / Show less" row.
    let shown: Vec<&ConversationEntry> = visible.clone();
    let shown_ids: std::collections::HashSet<&str> =
        shown.iter().map(|entry| entry.id.as_str()).collect();

    let recent_first = *state.sidebar_recent_first.borrow();
    let flat_list = *state.sidebar_flat_list.borrow();

    let mut blocks: Vec<Box<dyn Element>> = Vec::new();

    // Starred first: the pinned block.
    if !starred.is_empty() {
        let entries = ordered_entries(
            &starred,
            recent_first
                .then(|| order_cell.get(STARRED_SECTION))
                .flatten(),
        );
        let contains_selected = entries
            .iter()
            .any(|entry| state.selected_id.as_deref() == Some(entry.id.as_str()));
        let all_shown: std::collections::HashSet<&str> =
            entries.iter().map(|entry| entry.id.as_str()).collect();
        blocks.push(group_block(
            app,
            state,
            actions,
            "Starred",
            STARRED_SECTION,
            &entries,
            &all_shown,
            Some(SIDEBAR_ICON_PIN_FILL),
            contains_selected,
            None,
        ));
    }

    // Folder groups, alphabetical, with the stored Workspaces folded in: a
    // directory the user added with the header's "+" draws its row even before
    // a conversation has run in it (that is the whole point of adding it), and
    // a folder the conversations imply matches its stored row by name so the
    // user's rename shows.
    let mut sections: Vec<WorkspaceSection> = Vec::new();
    for entry in &visible {
        if sections.iter().any(|row| row.folder == entry.folder) {
            continue;
        }
        let stored = state
            .sidebar_workspaces
            .iter()
            .find(|workspace| workspace.folder() == entry.folder);
        let path = stored
            .map(|workspace| workspace.path.clone())
            .unwrap_or_else(|| card_directory(state, entry));
        let title = stored
            .map(|workspace| workspace.display_title())
            .unwrap_or_else(|| entry.folder.clone());
        sections.push(WorkspaceSection {
            folder: entry.folder.clone(),
            path,
            title,
            stored: stored.is_some(),
            target: stored
                .map(|workspace| workspace.target.clone())
                .unwrap_or_default(),
        });
    }
    for workspace in &state.sidebar_workspaces {
        let folder = workspace.folder();
        if sections.iter().any(|row| row.folder == folder) {
            continue;
        }
        sections.push(WorkspaceSection {
            folder,
            path: workspace.path.clone(),
            title: workspace.display_title(),
            stored: true,
            target: workspace.target.clone(),
        });
    }
    // A conversation that has not run in a project is its own list, under the
    // "Conversations" heading; it is not a workspace block.
    sections.retain(|workspace| workspace.folder != "Conversations");
    // "The rest reorders by most recent": project groups follow the newest
    // conversation they contain, read off the same recency order the flat list
    // uses, not the alphabet. A stored workspace with no sessions goes last.
    let latest_stamp = |folder: &str| -> String {
        visible
            .iter()
            .filter(|entry| entry.folder == folder)
            .map(|entry| entry.timestamp.clone())
            .max()
            .unwrap_or_default()
    };
    sections.sort_by(|left, right| {
        latest_stamp(&right.folder).cmp(&latest_stamp(&left.folder))
    });
    if flat_list {
        // One flat run of sessions instead of the project blocks; the manual
        // order keys off the flat section, and recency otherwise.
        let flat_order = order_cell.get("flat");
        let entries = ordered_entries(&by_recency, if recent_first { None } else { flat_order });
        let all_shown: std::collections::HashSet<&str> =
            entries.iter().map(|entry| entry.id.as_str()).collect();
        let flat_shown: std::collections::HashSet<&str> = if state.conversations_expanded {
            all_shown.clone()
        } else {
            entries
                .iter()
                .take(COLLAPSED_CONVERSATIONS)
                .map(|entry| entry.id.as_str())
                .collect()
        };
        let group_ids: Vec<String> = entries.iter().map(|entry| entry.id.clone()).collect();
        let mut flat_block = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(GOBLE_ROW_GAP);
        for entry in &entries {
            if !flat_shown.contains(entry.id.as_str()) {
                continue;
            }
            flat_block =
                flat_block.with_child(session_row(app, state, actions, entry, "flat", &group_ids));
        }
        blocks.push(flat_block.finish());
    }
    // The Projects section comes first, with its view-options and add controls.
    if !flat_list {
        blocks.push(
            Padding::new(
                workspaces_header(app, state, actions),
                EdgeInsets::new(0.0, SIDEBAR_SECTION_TOP_INSET, 0.0, 0.0),
            )
            .finish(),
        );
    }

    for workspace in sections.iter().filter(|_| !flat_list) {
        let entries: Vec<&ConversationEntry> = visible
            .iter()
            .filter(|entry| entry.folder == workspace.folder)
            .copied()
            .collect();
        let section = folder_section(&workspace.folder);
        let collapsed = state.collapsed_sections.iter().any(|key| key == &section);
        let draws_a_card = entries
            .iter()
            .any(|entry| shown_ids.contains(entry.id.as_str()));
        // A folder that only exists through its sessions disappears when the
        // digest hides every card it has; a stored Workspace keeps its row (its
        // "+" would otherwise add nothing visible to the list).
        if !workspace.stored && !collapsed && !draws_a_card {
            continue;
        }
        let ordered = ordered_entries(
            &entries,
            if recent_first {
                None
            } else {
                order_cell.get(&section)
            },
        );
        let contains_selected = entries
            .iter()
            .any(|entry| state.selected_id.as_deref() == Some(entry.id.as_str()));
        blocks.push(group_block(
            app,
            state,
            actions,
            &workspace.title,
            &section,
            &ordered,
            &shown_ids,
            None,
            contains_selected,
            // The fallback "Conversations" group is a plain heading: no
            // workspace icon, no three-dot menu, the label starts where the
            // leading icon used to be.
            (workspace.folder != "Conversations").then_some(workspace),
        ));
    }

    // "Conversations" always sits UNDER the Projects groups; it disappears
    // entirely when there is nothing loose to list.
    if !flat_list {
        let mut loose: Vec<&ConversationEntry> = visible
            .iter()
            .filter(|entry| entry.folder == "Conversations")
            .copied()
            .collect();
        if !loose.is_empty() {
            loose.sort_by(|a, b| {
                let a_key = a.updated_at.as_deref().unwrap_or(&a.timestamp);
                let b_key = b.updated_at.as_deref().unwrap_or(&b.timestamp);
                b_key.cmp(a_key)
            });
            let mut block = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(GOBLE_ROW_GAP)
                .with_child(plain_section_header(app, "Conversations"));
            let group_ids: Vec<String> = loose.iter().map(|entry| entry.id.clone()).collect();
            for entry in &loose {
                if !shown_ids.contains(entry.id.as_str()) {
                    continue;
                }
                block = block.with_child(session_row(
                    app,
                    state,
                    actions,
                    entry,
                    "conversations",
                    &group_ids,
                ));
            }
            blocks.push(block.finish());
        }
    }

    // The list is empty for one of two reasons, and they read differently:
    // nothing worth listing exists yet, or the ones that do belong to another
    // environment.
    let empty = visible.is_empty() && starred.is_empty() && state.sidebar_workspaces.is_empty();
    let any_listable = state
        .conversations
        .iter()
        .any(|entry| entry.has_agent_reply);
    let any_archived = state
        .conversations
        .iter()
        .any(|entry| entry.has_agent_reply && entry.archived);
    let empty_label =
        if filter == ArchivedFilter::Only || (filter == ArchivedFilter::Show && !any_archived) {
            // DSH's archived empty state, under the filter that asks for them.
            "No archived conversations yet."
        } else if any_listable {
            "No conversations for this environment."
        } else {
            "No conversations yet. Create one to begin."
        };

    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(SIDEBAR_GROUP_GAP);
    for block in blocks {
        column = column.with_child(block);
    }
    if empty {
        column = column.with_child(
            Container::new(
                Text::new(empty_label)
                    .with_font_size(13.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_padding(EdgeInsets::new(xs * 3.0, xs * 4.0, xs * 3.0, xs * 4.0))
            .finish(),
        );
    } else if shown.len() < visible.len() || state.conversations_expanded {
    }

    Padding::new(
        column.finish(),
        EdgeInsets::new(0.0, 0.0, 0.0, SIDEBAR_LIST_BOTTOM_PAD),
    )
    .finish()
}

#[cfg(test)]
mod sidebar_surface_tests {
    use super::*;
    use crate::root_view::RootView;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::{ConversationStatus, LayoutContext, PaintContext, SizeConstraint};
    use goble_ui::geometry::{vec2f, Vector2F};
    use goble_ui::render::{RenderCommand, Renderer};
    use goble_ui::test_util::render_element;
    use std::sync::Arc;

    fn conversation(id: &str, name: &str, folder: &str) -> ConversationEntry {
        ConversationEntry::new(id, name, "Let's ship hot reload today", "10:42")
            .with_status(ConversationStatus::Success)
            .with_folder(folder)
    }

    /// Every run of text the sidebar drew, in paint order.
    fn drawn_text(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if origin.x < crate::ui::SIDEBAR_WIDTH =>
                {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// Every run the sidebar's list drew: left of the workspace and below the
    /// topbar, which names the active tab's conversation at the same x.
    fn sidebar_list_text(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if origin.x < crate::ui::SIDEBAR_WIDTH
                        && origin.y > crate::ui::shell::TOPBAR_HEIGHT =>
                {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// The sidebar's Surface fill reaches the window's bottom edge: the list
    /// scrolls inside a region that takes the remaining height, and the fill
    /// behind it keeps going past the last row, so the page background never
    /// shows under a short list.
    #[test]
    fn the_sidebar_surface_reaches_the_bottom_of_the_window() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let view = seeded_view(&app, &desktop);
        let mut root: Box<dyn Element> = Box::new(view);
        let window = vec2f(1024.0, 768.0);
        let commands = render_element(&mut root, window, &app);
        let surface = app.theme.color(ColorToken::Surface);
        let covers_bottom = commands.iter().any(|command| match command {
            RenderCommand::FillRect { rect, color, .. } => {
                *color == surface
                    && rect.min_x() <= 0.5
                    && rect.max_x() >= crate::ui::SIDEBAR_WIDTH
                    && rect.max_y() >= window.y - 0.5
            }
            _ => false,
        });
        assert!(
            covers_bottom,
            "the sidebar's Surface fill reaches the window bottom: {commands:?}"
        );
    }

    /// The restored "New conversation" row is a real control again: it is drawn
    /// under the search field (outside the scroller), it is a 32pt full-width
    /// rounded band that highlights on hover, its glyph starts in the list
    /// rows' leading column, and it creates a conversation through the same
    /// action the Workspaces header's square-plus uses.
    #[test]
    fn the_new_conversation_row_is_drawn_and_creates_a_conversation() {
        use goble_ui::event::DispatchedEvent;

        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let pad = app.theme.spacing_px(SpacingToken::Sm);
        let view = seeded_view(&app, &desktop);
        let state = view.state_rc();
        // Hover comes from the shared sidebar hover cell, so the next build
        // paints the row's fill the way the live app does.
        *state.borrow().sidebar_hover_row.borrow_mut() = Some(KEY_NEW_CONVERSATION.to_string());

        let mut root: Box<dyn Element> = Box::new(view);
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
        let in_band = |origin: &Vector2F| origin.x < crate::ui::SIDEBAR_WIDTH;
        let label = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "New conversation" && in_band(origin) =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the row's label is drawn");
        let workspaces = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == SIDEBAR_SECTION_TITLE => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the Workspaces header");
        assert!(
            label.y < workspaces.y,
            "the row sits above the Workspaces header: {label:?} against {workspaces:?}"
        );

        let glyph = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon { name, origin, .. }
                    if name == SIDEBAR_ICON_NEW_CHAT && in_band(origin) =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the row's glyph is drawn");
        assert!(
            (glyph.x
                - (pad
                    + goble_ui::elements::conversation_list_item::SIDEBAR_PAINT_INSET
                    + goble_ui::elements::conversation_list_item::SIDEBAR_ROW_PADDING_START))
                .abs()
                < 0.5,
            "the glyph sits in the list rows' leading column: {glyph:?}"
        );

        // The hovered band: the column's whole content width (the 8pt sidebar
        // padding on each side is the container's), one 32pt row high, radius 12.
        let hover = app.theme.color(ColorToken::Hover);
        let band = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == hover
                    && *corner_radius > 11.0
                    && (rect.height() - 32.0).abs() < 0.5
                    && rect.width() > 200.0 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the hovered row paints its rounded band");
        assert!(
            (band.min_x() - pad).abs() < 0.5
                && (band.max_x() - (crate::ui::SIDEBAR_WIDTH - pad)).abs() < 0.5,
            "the band fills the column inside its 8pt padding: {band:?}"
        );

        // Clicking it runs the shared create action: one more tab/space opens.
        let before = state.borrow().spaces.len();
        let at = vec2f(label.x + 10.0, label.y + 10.0);
        let mut ctx = goble_ui::elements::EventContext;
        for event in [
            DispatchedEvent::MouseDown {
                position: at,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: at,
                button: 0,
            },
        ] {
            root.dispatch_event(&event, &mut ctx, &app);
        }
        assert_eq!(
            state.borrow().spaces.len(),
            before + 1,
            "the row creates a conversation through on_create_submit"
        );
    }

    /// A root view with the overlays and banners off and five conversations, the
    /// way the sidebar's own tests seed it.
    fn seeded_view(app: &AppContext, desktop: &Arc<DesktopState>) -> crate::root_view::RootView {
        let view = RootView::new(app, desktop, None);
        {
            let state = view.state_rc();
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.conversations = vec![
                conversation("c1", "Ada", "Frontend"),
                conversation("c2", "Coder", "Frontend"),
                conversation("c3", "Ops", "Infra"),
                conversation("c4", "Research", "Research"),
                conversation("c5", "Docs", "Research"),
            ];
        }
        view
    }

    /// The wheel over the sidebar follows the platform's sign, the way the
    /// transcript and the terminal do: winit reports how far the *content*
    /// should move and positive is down, so a negative delta — the swipe that
    /// moves the content up — carries the list's offset on, and a positive one
    /// walks it back towards the top.
    #[test]
    fn the_wheels_sign_scrolls_the_sidebar() {
        use goble_ui::event::DispatchedEvent;

        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let view = RootView::new(&app, &desktop, None);
        let state = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            // Expanded, so every row is in the scrollable region (collapsed is
            // a short digest that fits the band).
            s.conversations_expanded = true;
            // More rows than the band holds, so there is somewhere to scroll.
            s.conversations = (0..40)
                .map(|index| {
                    conversation(&format!("c{index}"), &format!("Agent {index}"), "Frontend")
                })
                .collect();
        }

        let mut root: Box<dyn Element> = Box::new(view);
        let window = vec2f(1024.0, 768.0);
        // The wheel is attributed by the last paint (a wheel event carries no
        // position), so the pointer has to be over the band when it is painted.
        let over_the_band = vec2f(crate::ui::SIDEBAR_WIDTH / 2.0, 400.0);
        let paint = |root: &mut Box<dyn Element>| {
            let _ = root.layout(SizeConstraint::loose(window), &mut LayoutContext, &app);
            let mut ctx = PaintContext::new(Renderer::new());
            ctx.cursor_inside = true;
            ctx.cursor_position = over_the_band;
            root.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        };
        paint(&mut root);
        let condition = state.borrow().sidebar_scroll.clone();
        assert!(
            condition.borrow().max_offset() > 0.0,
            "the seeded list overflows the band: {:?}",
            condition.borrow().max_offset()
        );
        assert_eq!(condition.borrow().offset(), 0.0, "it opens at the top");

        let wheel = |root: &mut Box<dyn Element>, dy: f32| {
            let mut ctx = goble_ui::elements::EventContext;
            root.dispatch_event(
                &DispatchedEvent::MouseMove {
                    position: over_the_band,
                },
                &mut ctx,
                &app,
            );
            root.dispatch_event(&DispatchedEvent::scroll(vec2f(0.0, dy)), &mut ctx, &app);
        };

        wheel(&mut root, 40.0);
        assert_eq!(
            condition.borrow().offset(),
            0.0,
            "a delta that pulls the content down keeps the list at its top"
        );
        wheel(&mut root, -40.0);
        let scrolled = condition.borrow().offset();
        assert!(
            scrolled > 0.0,
            "and one that moves the content up carries the offset on: {scrolled}"
        );
        // The tree is rebuilt every frame; the offset lives in app state.
        paint(&mut root);
        assert_eq!(
            condition.borrow().offset(),
            scrolled,
            "the offset survives the rebuild"
        );
    }

    /// A trackpad reports how far the fingers travelled, and the sidebar spends
    /// it to the point: a 4 pt delta moves the list 4 pt, not a row. The frame
    /// accounting is the transcript's too — a delta that moves nothing (the
    /// list is already at its end) is consumed but earns no frame.
    #[test]
    fn a_precise_delta_moves_the_sidebar_by_its_own_distance_and_only_a_move_earns_a_frame() {
        use goble_ui::event::DispatchedEvent;

        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let view = RootView::new(&app, &desktop, None);
        let state = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.conversations_expanded = true;
            s.conversations = (0..40)
                .map(|index| {
                    conversation(&format!("c{index}"), &format!("Agent {index}"), "Frontend")
                })
                .collect();
        }

        let mut root: Box<dyn Element> = Box::new(view);
        let window = vec2f(1024.0, 768.0);
        let over_the_band = vec2f(crate::ui::SIDEBAR_WIDTH / 2.0, 400.0);
        let _ = root.layout(SizeConstraint::loose(window), &mut LayoutContext, &app);
        let mut paint_ctx = PaintContext::new(Renderer::new());
        paint_ctx.cursor_inside = true;
        paint_ctx.cursor_position = over_the_band;
        root.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);

        let scroll = state.borrow().sidebar_scroll.clone();
        assert!(
            scroll.borrow().max_offset() > 0.0,
            "the seeded list overflows the band: {:?}",
            scroll.borrow().max_offset()
        );

        let mut ctx = goble_ui::elements::EventContext;
        root.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: over_the_band,
            },
            &mut ctx,
            &app,
        );
        let moves = app.scroll_moves();
        let wheel = |root: &mut Box<dyn Element>, dy: f32| {
            let mut ctx = goble_ui::elements::EventContext;
            root.dispatch_event(
                &DispatchedEvent::Scroll {
                    delta: vec2f(0.0, dy),
                    precise: true,
                },
                &mut ctx,
                &app,
            );
        };
        wheel(&mut root, -4.0);
        assert_eq!(
            scroll.borrow().offset(),
            4.0,
            "a 4 pt trackpad delta is 4 pt of list, not a rounded row"
        );
        assert_eq!(app.scroll_moves(), moves + 1, "the move earns a frame");

        scroll.borrow_mut().scroll_by(f32::MAX);
        let at_end = scroll.borrow().offset();
        assert_eq!(at_end, scroll.borrow().max_offset());
        let moves = app.scroll_moves();
        wheel(&mut root, -4.0);
        assert_eq!(
            scroll.borrow().offset(),
            at_end,
            "the wheel is at the end and moves nothing"
        );
        assert_eq!(
            app.scroll_moves(),
            moves,
            "a wheel that moves nothing earns no frame"
        );
    }

    /// The wheel belongs to the conversation cards and nothing above them: the
    /// toolbelt, the search field and the `New conversation` row stay put, and
    /// a wheel over them scrolls nothing.
    #[test]
    fn a_wheel_over_the_sidebar_header_does_not_scroll_the_list() {
        use goble_ui::event::DispatchedEvent;

        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let view = RootView::new(&app, &desktop, None);
        let state = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.conversations_expanded = true;
            s.conversations = (0..40)
                .map(|index| {
                    conversation(&format!("c{index}"), &format!("Agent {index}"), "Frontend")
                })
                .collect();
        }

        let mut root: Box<dyn Element> = Box::new(view);
        let window = vec2f(1024.0, 768.0);
        // Below the toolbelt and over the search field / New conversation row,
        // above the list's own viewport.
        let over_the_header = vec2f(crate::ui::SIDEBAR_WIDTH / 2.0, 110.0);
        let _ = root.layout(SizeConstraint::loose(window), &mut LayoutContext, &app);
        let mut paint_ctx = PaintContext::new(Renderer::new());
        paint_ctx.cursor_inside = true;
        paint_ctx.cursor_position = over_the_header;
        root.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);

        let scroll = state.borrow().sidebar_scroll.clone();
        assert!(
            scroll.borrow().max_offset() > 0.0,
            "the seeded list overflows the band: {:?}",
            scroll.borrow().max_offset()
        );

        let mut ctx = goble_ui::elements::EventContext;
        root.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: over_the_header,
            },
            &mut ctx,
            &app,
        );
        let moves = app.scroll_moves();
        let consumed = root.dispatch_event(
            &DispatchedEvent::Scroll {
                delta: vec2f(0.0, -20.0),
                precise: true,
            },
            &mut ctx,
            &app,
        );
        assert!(
            !consumed,
            "the header does not take the wheel away from the rest of the window"
        );
        assert_eq!(
            scroll.borrow().offset(),
            0.0,
            "and the conversation list stays at its top"
        );
        assert_eq!(app.scroll_moves(), moves, "so no frame is owed");
    }

    /// A collapsed folder section keeps its header — which says how much is
    /// hidden — and drops its cards, so a long list can be folded away without
    /// leaving the sidebar.
    #[test]
    fn a_collapsed_section_keeps_its_header_and_drops_its_cards() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let view = seeded_view(&app, &desktop);
        view.state_rc()
            .borrow_mut()
            .collapsed_sections
            .insert("folder:Frontend".to_string());

        let mut root: Box<dyn Element> = Box::new(view);
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
        let drawn = drawn_text(&commands);

        assert!(
            drawn.iter().any(|run| run == "Frontend"),
            "the header stays: {drawn:?}"
        );
        for hidden in ["Ada", "Coder"] {
            assert!(
                !drawn.iter().any(|run| run == hidden),
                "the collapsed folder hides {hidden:?}: {drawn:?}"
            );
        }
        assert!(
            drawn.iter().any(|run| run == "Ops"),
            "another section is untouched"
        );
    }

    /// A starred conversation is listed in the Starred section, which sits above
    /// the folders — whatever folder the conversation itself belongs to.
    #[test]
    fn a_starred_conversation_is_drawn_in_the_starred_section_above_the_folders() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let view = seeded_view(&app, &desktop);
        view.state_rc()
            .borrow_mut()
            .starred_conversations
            .insert("c3".to_string());

        let mut root: Box<dyn Element> = Box::new(view);
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
        let drawn = drawn_text(&commands);

        let section = drawn
            .iter()
            .position(|run| run == "Starred")
            .expect("the Starred header");
        let folder = drawn
            .iter()
            .position(|run| run == "Frontend")
            .expect("a folder header");
        let starred = drawn
            .iter()
            .position(|run| run == "Ops")
            .expect("the starred card");
        assert!(
            section < folder && folder < starred || section < starred,
            "{drawn:?}"
        );
        assert!(
            section < starred,
            "the pinned card is under the Starred header: {drawn:?}"
        );
    }

    /// The toolbelt is the sidebar's switch: the explorer draws the working
    /// directory's own children, the search view draws what the query found, and
    /// neither is the conversation list.
    #[test]
    fn the_sidebar_has_no_file_tree_and_keeps_search() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let home = tempfile::tempdir().expect("temp working directory");
        std::fs::write(home.path().join("notes.md"), "needle here\n").unwrap();

        let view = seeded_view(&app, &desktop);
        {
            let state = view.state_rc();
            let mut s = state.borrow_mut();
            s.set_active_pane_path(home.path().to_string_lossy().to_string());
            s.sidebar_view = SidebarView::Explorer;
        }
        let mut root: Box<dyn Element> = Box::new(view);
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
        let drawn = drawn_text(&commands);
        assert!(
            !drawn.iter().any(|run| run == "notes.md"),
            "the sidebar draws no file tree: {drawn:?}"
        );
        assert!(
            drawn.iter().any(|run| run == "New conversation"),
            "an old Explorer state falls back to the conversations list: {drawn:?}"
        );

        // The search view draws the query it ran and the rows it found.
        let view = seeded_view(&app, &desktop);
        {
            let state = view.state_rc();
            let mut s = state.borrow_mut();
            s.sidebar_view = SidebarView::Search;
            s.global_search_query = "needle".to_string();
            s.global_search_searched = true;
            s.global_search_rows = vec![
                crate::ui::SearchRow {
                    path: home.path().join("notes.md").to_string_lossy().to_string(),
                    name: "notes.md".to_string(),
                    line: None,
                    text: String::new(),
                },
                crate::ui::SearchRow {
                    path: home.path().join("notes.md").to_string_lossy().to_string(),
                    name: String::new(),
                    line: Some(1),
                    text: "needle here".to_string(),
                },
            ];
        }
        let mut root: Box<dyn Element> = Box::new(view);
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
        let drawn = drawn_text(&commands);
        for expected in ["needle", "notes.md", "1 result in 1 file"] {
            assert!(
                drawn.iter().any(|run| run == expected),
                "the search view draws {expected:?}: {drawn:?}"
            );
        }
    }

    /// A pinned conversation is listed once, under Starred: the folder groups
    /// below hold what is left. A list that is entirely pinned is not empty, so
    /// it says nothing about an empty environment either.
    #[test]
    fn a_starred_conversation_is_listed_under_starred_only() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();

        let view = seeded_view(&app, &desktop);
        view.state_rc()
            .borrow_mut()
            .starred_conversations
            .insert("c1".to_string());
        let mut root: Box<dyn Element> = Box::new(view);
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
        let drawn = drawn_text(&commands);
        assert_eq!(
            drawn.iter().filter(|text| *text == "Ada").count(),
            1,
            "the pinned card is listed once: {drawn:?}"
        );
        assert!(
            drawn.iter().any(|text| text == "Starred"),
            "under the Starred header: {drawn:?}"
        );
        assert!(
            drawn.iter().any(|text| text == "Frontend"),
            "and its folder still lists the cards that are not pinned: {drawn:?}"
        );

        // Every conversation pinned: there is content, so the environment's
        // empty state is not what the user reads.
        let view = seeded_view(&app, &desktop);
        {
            let state = view.state_rc();
            let mut s = state.borrow_mut();
            s.conversations.truncate(1);
            s.starred_conversations.insert("c1".to_string());
        }
        let mut root: Box<dyn Element> = Box::new(view);
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
        let drawn = drawn_text(&commands);
        assert!(
            drawn.iter().any(|text| text == "Ada"),
            "the pinned conversation is drawn: {drawn:?}"
        );
        assert!(
            !drawn.iter().any(|text| text.contains("No conversations")),
            "an all-pinned list has content: {drawn:?}"
        );
    }

    /// A conversation nobody has answered is a place to type rather than
    /// history, so the sidebar lists it only once the agent has produced
    /// something in it: an assistant reply or a tool result.
    #[test]
    fn a_conversation_the_agent_has_not_answered_is_not_listed() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();

        let asked = desktop
            .create_chat("Only asked", None, None)
            .expect("a conversation with an unanswered prompt");
        desktop
            .add_chat_message(&asked, "user", "salut")
            .expect("the user's prompt");
        let answered = desktop
            .create_chat("Answered", None, None)
            .expect("a conversation the agent answered");
        desktop
            .add_chat_message(&answered, "user", "salut")
            .expect("the user's prompt");
        desktop
            .add_chat_message(&answered, "assistant", "Bine ai venit!")
            .expect("the agent's reply");
        let tooled = desktop
            .create_chat("Tooled", None, None)
            .expect("a conversation a tool ran in");
        desktop
            .add_chat_message(&tooled, "tool", "call_1\nfile.txt")
            .expect("a tool result");

        let view = RootView::new(&app, &desktop, None);
        {
            let state = view.state_rc();
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.refresh_conversations(&desktop);
        }
        let mut root: Box<dyn Element> = Box::new(view);
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
        let drawn = sidebar_list_text(&commands);
        assert!(
            drawn.iter().any(|text| text == "Answered"),
            "the answered conversation is listed: {drawn:?}"
        );
        assert!(
            drawn.iter().any(|text| text == "Tooled"),
            "a conversation a tool has run in is listed: {drawn:?}"
        );
        assert!(
            !drawn.iter().any(|text| text == "Only asked"),
            "a conversation nobody has answered is left out: {drawn:?}"
        );
    }

    /// The subject is not re-derived from later turns: a conversation keeps the
    /// subject its first prompt named, however much is said afterwards.
    #[test]
    fn a_later_turn_keeps_the_subject_the_first_prompt_named() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let chat = desktop
            .create_chat(crate::state::NEW_CONVERSATION_TITLE, None, None)
            .expect("a conversation nobody has named");
        let prompt = "Arata un text colorat in chat";
        desktop
            .add_chat_message(&chat, "user", prompt)
            .expect("the user's prompt");
        desktop
            .add_chat_message(&chat, "assistant", "Sure.")
            .expect("the agent's reply");

        let view = RootView::new(&app, &desktop, None);
        let state = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.name_conversation_from_prompt(&chat, prompt, &desktop);
            s.refresh_conversations(&desktop);
            assert_eq!(s.conversations[0].name, "Text colorat in chat");
            // A second prompt is not the conversation's first: it does not
            // rename it, and it does not ask for a new subject.
            assert!(!s.name_conversation_from_prompt(
                &chat,
                "Now make the sidebar show conversation titles",
                &desktop
            ));
        }

        desktop
            .add_chat_message(
                &chat,
                "user",
                "Now make the sidebar show conversation titles",
            )
            .expect("the follow-up prompt");
        desktop
            .add_chat_message(&chat, "assistant", "Sure, the card reads it now.")
            .expect("the agent's second reply");
        {
            let mut s = state.borrow_mut();
            s.refresh_conversations(&desktop);
            assert_eq!(
                s.conversations[0].name, "Text colorat in chat",
                "a later turn leaves the first prompt's subject alone"
            );
        }
        assert_eq!(
            desktop.list_chats()[0].title,
            "Text colorat in chat",
            "the store keeps the first prompt's subject"
        );
        assert_eq!(
            desktop
                .store_clone()
                .get_chat_title(&chat)
                .expect("read the stored title"),
            Some("Text colorat in chat".to_string())
        );
    }

    /// The list is empty for one of two reasons and they read differently: no
    /// conversation has been answered yet, or the ones that have belong to
    /// another environment. An unanswered conversation is the first, not the
    /// second — it is not a conversation this environment lacks.
    #[test]
    fn an_unanswered_conversation_leaves_the_first_empty_state() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));

        let asked = desktop
            .create_chat("Only asked", None, None)
            .expect("a conversation with an unanswered prompt");
        desktop
            .add_chat_message(&asked, "user", "salut")
            .expect("the user's prompt");

        let app = AppContext::default();
        let view = RootView::new(&app, &desktop, None);
        let state = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.refresh_conversations(&desktop);
        }
        let mut root: Box<dyn Element> = Box::new(view);
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
        let drawn = sidebar_list_text(&commands);
        assert!(
            !drawn
                .iter()
                .any(|text| text == "No conversations for this environment."),
            "an unanswered conversation is not one this environment lacks: {drawn:?}"
        );
        assert!(
            drawn
                .iter()
                .any(|text| text == "No conversations yet. Create one to begin."),
            "the list has nothing to show yet: {drawn:?}"
        );

        // An answered conversation that belongs to the other environment is
        // the second reason: the list holds something, elsewhere.
        let elsewhere = desktop
            .create_chat("Elsewhere", None, None)
            .expect("a conversation in another environment");
        desktop
            .add_chat_message(&elsewhere, "assistant", "Bine ai venit!")
            .expect("the agent's reply");
        desktop
            .set_chat_workspace_routing(&elsewhere, Some("remote"))
            .expect("a remote environment");
        state.borrow_mut().refresh_conversations(&desktop);
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
        let drawn = sidebar_list_text(&commands);
        assert!(
            drawn
                .iter()
                .any(|text| text == "No conversations for this environment."),
            "the answered conversation belongs to the other environment: {drawn:?}"
        );
        assert!(
            !drawn
                .iter()
                .any(|text| text == "No conversations yet. Create one to begin."),
            "and the list is not untouched: {drawn:?}"
        );
    }
}

/// What the sidebar's new controls do to app state: the actions are the only
/// writers, and the views read exactly the fields they write.
#[cfg(test)]
mod sidebar_action_tests {
    use super::*;
    use crate::actions::make_actions;
    use crate::media::MediaState;
    use crate::state::UiState;
    use crate::ui::UiActions;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use std::sync::Arc;

    fn harness() -> (Rc<RefCell<UiState>>, UiActions, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let state = Rc::new(RefCell::new(UiState::from_desktop(&desktop)));
        let media = Rc::new(RefCell::new(MediaState::mock()));
        state.borrow_mut().active_pane_id = 1;
        let actions = make_actions(
            Rc::clone(&state),
            Some(Arc::clone(&desktop)),
            media,
            goble_ui::platform::WindowControl::default(),
            Rc::new(RefCell::new(1.0)),
        );
        (state, actions, dir)
    }

    /// The toolbelt selects a view; the view's own list picks it up on the next
    /// frame, which is why the selection is state rather than a local flag.
    #[test]
    fn selecting_a_view_switches_it() {
        let (state, actions, _dir) = harness();

        (actions.on_select_sidebar_view.borrow_mut())(SidebarView::Explorer);
        assert_eq!(state.borrow().sidebar_view, SidebarView::Explorer);

        (actions.on_select_sidebar_view.borrow_mut())(SidebarView::Search);
        assert_eq!(state.borrow().sidebar_view, SidebarView::Search);
    }

    /// Opening a view puts its field's caret at the end of the query it holds:
    /// an index from the query the field last typed into never lands in it.
    #[test]
    fn opening_a_view_puts_its_caret_at_the_end_of_the_query() {
        let (state, actions, _dir) = harness();
        state.borrow_mut().search_query = "abc".to_string();
        *state.borrow().sidebar_search_caret.borrow_mut() = 0;
        state.borrow_mut().global_search_query = "xy".to_string();
        *state.borrow().global_search_caret.borrow_mut() = 0;

        (actions.on_select_sidebar_view.borrow_mut())(SidebarView::Agents);
        assert_eq!(
            *state.borrow().sidebar_search_caret.borrow(),
            3,
            "the sidebar's field opens with its caret at the query's end"
        );

        (actions.on_select_sidebar_view.borrow_mut())(SidebarView::Search);
        assert_eq!(
            *state.borrow().global_search_caret.borrow(),
            2,
            "and so does the global search's"
        );
    }

    /// A section header toggles that one section; the others keep their state.
    #[test]
    fn a_section_key_toggles_only_that_section() {
        let (state, actions, _dir) = harness();

        (actions.on_toggle_section.borrow_mut())("folder:General".to_string());
        (actions.on_toggle_section.borrow_mut())("starred".to_string());
        assert_eq!(state.borrow().collapsed_sections.len(), 2);

        (actions.on_toggle_section.borrow_mut())("folder:General".to_string());
        let s = state.borrow();
        assert!(!s.collapsed_sections.contains("folder:General"));
        assert!(
            s.collapsed_sections.contains("starred"),
            "the other section is untouched"
        );
    }

    /// Starring is a toggle on the conversation id, which is what both the card
    /// and the Starred section read.
    #[test]
    fn starring_a_conversation_toggles_it() {
        let (state, actions, _dir) = harness();

        (actions.on_toggle_star.borrow_mut())("c1".to_string());
        assert!(state.borrow().starred_conversations.contains("c1"));

        (actions.on_toggle_star.borrow_mut())("c1".to_string());
        assert!(!state.borrow().starred_conversations.contains("c1"));
    }

    /// Y/Z: a file row/click opens the side panel's Files tab; it never
    /// retargets or splits a main pane, and the sidebar no longer owns a tree.
    #[test]
    fn a_file_click_opens_the_side_files_tab_not_a_pane() {
        let (state, actions, dir) = harness();
        let spaced = dir.path().join("my notes.md");
        std::fs::write(&spaced, "hi").unwrap();
        let path = spaced.to_string_lossy().to_string();
        let pane_id = state.borrow().active_pane_id;

        (actions.on_explorer_file_click.borrow_mut())(path.clone());
        let s = state.borrow();
        assert_ne!(
            s.spaces[s.active_space].leaf_kind(pane_id),
            Some(&crate::ui::PaneKind::File { path: path.clone() }),
            "the main pane is not retargeted to the file"
        );
        assert_eq!(s.side_file.as_deref(), Some(path.as_str()));
        assert_eq!(s.active_side_tab_kind(), crate::state::SideTabKind::Files);
        assert!(s.right_sidebar_open, "the Files tab is revealed");
        assert_eq!(
            s.composer_draft, "",
            "the path is not typed into the pane's input"
        );
    }

    /// Y: the "Open File" launcher shows the Files side tab without touching a
    /// pane.
    #[test]
    fn open_file_launcher_shows_the_side_files_tab() {
        let (state, actions, _dir) = harness();
        let pane = state.borrow().active_pane_id;
        let max = state.borrow().spaces[0].root.max_id();
        (actions.on_open_files_tab.borrow_mut())();
        let s = state.borrow();
        assert_eq!(s.active_side_tab_kind(), crate::state::SideTabKind::Files);
        assert!(s.right_sidebar_open);
        assert_eq!(s.active_pane_id, pane);
        assert_eq!(s.spaces[0].root.max_id(), max);
    }

    /// The query runs the search off the UI thread, and clearing it abandons the
    /// walk and clears the results rather than searching for nothing. It runs
    /// wherever the machine has ripgrep or not: the search falls back to the
    /// app's own walk.
    #[test]
    fn a_query_runs_the_search_and_an_empty_one_clears_it() {
        let (state, actions, _dir) = harness();
        let home = tempfile::tempdir().expect("temp working directory");
        std::fs::write(home.path().join("main.rs"), "fn main() {}\n").unwrap();
        state
            .borrow_mut()
            .set_active_pane_path(home.path().to_string_lossy().to_string());

        (actions.on_global_search_change.borrow_mut())("fn main".to_string());
        {
            let s = state.borrow();
            assert!(
                !s.global_search_searched,
                "the field says it is still searching while the walk runs"
            );
        }
        // Collect the answer the way the frame loop does.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            state.borrow_mut().take_global_search_result();
            if state.borrow().global_search_searched {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the worker answers the query"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        {
            let s = state.borrow();
            assert_eq!(s.global_search_rows.len(), 2, "a file row and its match");
            assert_eq!(s.global_search_rows[0].name, "main.rs");
            assert_eq!(s.global_search_rows[1].line, Some(1));
            assert!(s.global_search_error.is_none());
        }

        (actions.on_global_search_change.borrow_mut())("   ".to_string());
        let s = state.borrow();
        assert!(s.global_search_rows.is_empty());
        assert!(!s.global_search_searched, "an empty query has not searched");
        assert!(s.global_search_error.is_none());
    }
}

#[cfg(test)]
mod workspace_row_tests {
    //! The Workspaces rows' controls, the header's add-workspace flow, and the
    //! session rows' hover Pin/Archive — each wired to the real durable action
    //! (the store's `chats.starred` / `chats.archived_at` and the persisted
    //! Workspaces list in `ui.workspaces`), not to a mock.

    use super::*;
    use crate::root_view::RootView;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::EventContext;
    use goble_ui::event::DispatchedEvent;
    use goble_ui::geometry::{vec2f, Vector2F};
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;
    use std::sync::Arc;

    fn window() -> Vector2F {
        vec2f(1024.0, 768.0)
    }

    /// A root view over a real store with two answered conversations, in two
    /// different directories (so the list has two real folder groups).
    fn seeded() -> (
        Box<dyn Element>,
        Rc<RefCell<crate::state::UiState>>,
        Arc<DesktopState>,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        );
        for (title, workspace) in [("Alpha work", "alpha"), ("Beta work", "beta")] {
            let id = desktop
                .create_chat(title, None, None)
                .expect("create a conversation");
            desktop
                .add_chat_message(&id, "assistant", "done")
                .expect("the agent's reply");
            desktop
                .set_chat_working_dir(&id, &format!("/tmp/goble-work/{workspace}"))
                .expect("the conversation's directory");
        }
        let app = AppContext::default();
        let view = RootView::new(&app, &desktop, None);
        let state = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.show_onboarding_tip = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.refresh_conversations(&desktop);
        }
        (Box::new(view), state, desktop, dir)
    }

    fn render(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<RenderCommand> {
        render_element(root, window(), app)
    }

    fn dispatch(root: &mut Box<dyn Element>, app: &AppContext, event: DispatchedEvent) {
        let mut ctx = EventContext::default();
        root.dispatch_event(&event, &mut ctx, app);
    }

    fn click(root: &mut Box<dyn Element>, app: &AppContext, at: Vector2F) {
        for event in [
            DispatchedEvent::MouseDown {
                position: at,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: at,
                button: 0,
            },
        ] {
            dispatch(root, app, event);
        }
    }

    /// The origin of the first atlas glyph with this name that is drawn in the
    /// sidebar's own column, below the topbar.
    fn sidebar_icon(commands: &[RenderCommand], name: &str) -> Vector2F {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    name: drawn,
                    origin,
                    ..
                } if drawn == name
                    && origin.x < crate::ui::SIDEBAR_WIDTH
                    && origin.y > crate::ui::shell::TOPBAR_HEIGHT =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("the sidebar draws the {name:?} glyph: {commands:?}"))
    }

    /// The origin of the first run of exactly this text drawn in the sidebar's
    /// list.
    fn sidebar_text(commands: &[RenderCommand], text: &str) -> Vector2F {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    text: drawn,
                    origin,
                    ..
                } if drawn == text
                    && origin.x < crate::ui::SIDEBAR_WIDTH
                    && origin.y > crate::ui::shell::TOPBAR_HEIGHT =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("the sidebar draws {text:?}: {commands:?}"))
    }

    fn hovered_row(commands: &[RenderCommand], text: &str) -> Vector2F {
        sidebar_text(commands, text)
    }

    /// The header's "+" opens the two-row add menu: New Project (the local
    /// picker flow) and SSH Host… (the remote overlay). A remote workspace's
    /// row carries the cloud badge without growing a second line.
    #[test]
    fn the_add_menu_offers_project_and_ssh_and_remote_rows_wear_a_badge() {
        let app = AppContext::default();
        let (mut root, state, desktop, _dir) = seeded();
        state
            .borrow_mut()
            .add_ssh_workspace("controlpanel", "controlpanel", Some(&desktop));
        *state.borrow().sidebar_add_menu_open.borrow_mut() = true;
        let commands = render(&mut root, &app);
        let project = sidebar_text(&commands, "New Project");
        let ssh = sidebar_text(&commands, "SSH Host…");
        assert!(
            project.y < ssh.y,
            "New Project is the first row, SSH Host… the second: {project:?} / {ssh:?}"
        );
        let _badge = sidebar_icon(&commands, "environment-cloud");

        // The SSH row opens the overlay on its config and focused field.
        click(&mut root, &app, ssh + vec2f(8.0, 8.0));
        let open = state.borrow();
        assert!(open.ssh_target_open, "the SSH Host… row opens the overlay");
        assert!(open.ssh_target_focused, "the target field starts focused");
        assert!(
            open.ssh_config_path.ends_with("/.ssh/config"),
            "the overlay starts on the default config: {:?}",
            open.ssh_config_path
        );
    }

    /// The Workspaces header's "+" adds a workspace: the picked directory is
    /// persisted (`ui.workspaces`), lists as a row, survives a restart, and is
    /// renamed and deleted through the row's own actions. It never creates a
    /// conversation — the separate "New conversation" row does that.
    #[test]
    fn the_header_plus_adds_and_persists_a_workspace() {
        let app = AppContext::default();
        let (mut root, state, desktop, _dir) = seeded();

        // The button no longer creates a conversation: a press on it leaves the
        // spaces/panes alone. (On a test thread the native picker answers None,
        // so a cancelled pick adds nothing either — both halves are the point.)
        let before_spaces = state.borrow().spaces.len();
        let before_conversations = state.borrow().conversations.len();
        let commands = render(&mut root, &app);
        let plus = sidebar_icon(&commands, "project-add");
        click(&mut root, &app, plus + vec2f(8.0, 8.0));
        assert!(
            *state.borrow().sidebar_add_menu_open.borrow(),
            "the plus opens the add-workspace menu"
        );
        let menu = render(&mut root, &app);
        sidebar_text(&menu, "New Project");
        sidebar_text(&menu, "SSH Host…");
        assert_eq!(
            state.borrow().spaces.len(),
            before_spaces,
            "the header \"+\" does not open a conversation space"
        );
        assert_eq!(
            state.borrow().conversations.len(),
            before_conversations,
            "the header \"+\" does not create a conversation"
        );

        // The flow behind it (what the picker answer feeds) persists the row.
        state
            .borrow_mut()
            .add_workspace("/tmp/goble-work/delta/", Some(&desktop))
            .expect("the directory is adopted");
        let persisted = desktop
            .get_ui_workspaces()
            .expect("the workspaces list is persisted");
        assert!(
            persisted.contains("/tmp/goble-work/delta"),
            "the picked directory persists: {persisted}"
        );
        let commands = render(&mut root, &app);
        hovered_row(&commands, "delta");

        // A restart restores it from the store.
        let restarted = crate::state::UiState::from_desktop(&desktop);
        assert!(
            restarted.workspace_for_folder("delta").is_some(),
            "the workspace comes back after a restart"
        );

        // Rename through the row's dialog: the row, and the store, follow.
        state
            .borrow_mut()
            .begin_workspace_rename("/tmp/goble-work/delta", "delta");
        state
            .borrow_mut()
            .workspace_rename_draft("Delta App".to_string());
        state.borrow_mut().commit_workspace_rename(Some(&desktop));
        let commands = render(&mut root, &app);
        hovered_row(&commands, "Delta App");
        assert!(
            desktop
                .get_ui_workspaces()
                .expect("persisted")
                .contains("Delta App"),
            "the renamed title persists"
        );

        // Delete drops the row from the list and the store.
        state
            .borrow_mut()
            .delete_workspace("/tmp/goble-work/delta", Some(&desktop));
        let commands = render(&mut root, &app);
        assert!(
            commands.iter().all(|command| !matches!(
                command,
                RenderCommand::DrawText { text, .. } if text == "Delta App"
            )),
            "the deleted workspace row is gone"
        );
        assert!(!desktop
            .get_ui_workspaces()
            .expect("persisted")
            .contains("delta"));
    }

    /// A workspace row's New-session control is real: it opens a conversation
    /// whose working directory is the workspace's own path, so the row's
    /// folder group (the directory's last segment) holds the new conversation.
    #[test]
    fn a_workspace_rows_new_session_control_opens_a_conversation_in_its_directory() {
        let app = AppContext::default();
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let view = RootView::new(&app, &desktop, None);
        let state = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.show_onboarding_tip = false;
            s.right_sidebar_open = false;
            s.refresh_conversations(&desktop);
        }
        let actions = crate::actions::make_actions(
            state.clone(),
            Some(Arc::clone(&desktop)),
            view.media_state_rc(),
            goble_ui::platform::WindowControl::default(),
            Rc::new(RefCell::new(1.0)),
        );
        (actions.on_sidebar_workspace_new_session.borrow_mut())(
            "/tmp/goble-work/gamma".to_string(),
            String::new(),
        );
        let opened = desktop
            .list_chats()
            .into_iter()
            .find(|chat| chat.working_dir.as_deref() == Some("/tmp/goble-work/gamma"))
            .expect("the new conversation works in the workspace's directory");
        assert!(
            state
                .borrow()
                .conversations
                .iter()
                .any(|entry| entry.id == opened.id && entry.folder == "gamma"),
            "the conversation is listed under the workspace's folder group"
        );
        assert_eq!(
            state.borrow().composer_path,
            "/tmp/goble-work/gamma",
            "the pane the conversation opened in works in the workspace"
        );
    }

    /// A conversation row's hover strip offers Archive and Pin next to the
    /// tray's dots, and both are the real durable actions: Pin writes
    /// `chats.starred`, Archive writes `chats.archived_at`.
    #[test]
    fn a_hovered_conversation_row_pins_and_archives_for_real() {
        let app = AppContext::default();
        let (mut root, state, desktop, _dir) = seeded();
        let id = state.borrow().conversations[0].id.clone();
        let name = state.borrow().conversations[0].name.clone();

        *state.borrow().sidebar_hover_row.borrow_mut() = Some(format!("session:{id}"));
        let commands = render(&mut root, &app);
        let pin = sidebar_icon(&commands, "pin-outline");
        let archive = sidebar_icon(&commands, "archive");
        assert!(
            pin.x > archive.x,
            "DSH's order is dots, Archive, Pin: {archive:?} then {pin:?}"
        );
        let dotted = sidebar_icon(&commands, "dots-horizontal");
        assert!(
            archive.x > dotted.x,
            "the tray's trigger leads the quick actions: {dotted:?}"
        );

        // Pin: the store's starred column, and the row's own pin state.
        click(&mut root, &app, pin + vec2f(8.0, 8.0));
        assert!(
            desktop.chat_starred(&id).expect("the starred column"),
            "the hover Pin writes the durable star"
        );
        assert!(
            state.borrow().starred_conversations.contains(&id),
            "the Starred block follows the pin"
        );

        // Archive: the store's archived_at column.
        *state.borrow().sidebar_hover_row.borrow_mut() = Some(format!("session:{id}"));
        let commands = render(&mut root, &app);
        let archive = sidebar_icon(&commands, "archive");
        click(&mut root, &app, archive + vec2f(8.0, 8.0));
        assert!(
            desktop
                .chat_archived(&id)
                .expect("the archived column")
                .is_some(),
            "the hover Archive writes the durable archive stamp"
        );

        // The archived row's strip offers the restore, not the archive, and
        // Pin is gone (DSH resolves the same list for an archived session). The
        // default filter hides the row, so show archived rows first (DSH's
        // archive action offers the same filter switch).
        state
            .borrow_mut()
            .set_archived_filter(crate::ui::ArchivedFilter::Show);
        *state.borrow().sidebar_hover_row.borrow_mut() = Some(format!("session:{id}"));
        let commands = render(&mut root, &app);
        let unarchive = sidebar_icon(&commands, "unarchive");
        assert!(
            commands.iter().all(|command| !matches!(
                command,
                RenderCommand::DrawIcon { name, origin, .. }
                    if name == "pin-outline"
                        && origin.x < crate::ui::SIDEBAR_WIDTH
                        && origin.y > crate::ui::shell::TOPBAR_HEIGHT
            )),
            "an archived row offers no pin control"
        );
        click(&mut root, &app, unarchive + vec2f(8.0, 8.0));
        assert!(
            desktop
                .chat_archived(&id)
                .expect("the archived column")
                .is_none(),
            "the restore clears the archive stamp"
        );
        let _ = name;
    }
}
