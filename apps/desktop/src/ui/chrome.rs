//! The window's single top row and the right Overview panel.
//!
//! The general topbar is retired: traffic lights, navigation, the conversation
//! breadcrumb and the panel actions live on one continuous strip above the
//! bordered content. The strip owns the top edge for both the sidebar and the
//! panes; the sidebar and the panel header start below it.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::color::ColorU;
use goble_ui::elements::chat_content::changed_files_for_message;
use goble_ui::elements::{
    AppContext, Border, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets, Element,
    Empty, Expanded, Fill, Flex, HoverRow, Icon, MainAxisAlignment, MainAxisSize, PopupMenu,
    PopupMenuItem, PopupMenuPosition, Scrollable, SearchInput, Spacer, Text, Tooltip,
    TooltipPosition, TopbarButton,
};
use goble_ui::geometry::vec2f;
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::{ColorToken, SpacingToken};

use crate::ui::{DiffPanelAction, SidebarView, UiActions, UiSnapshot};

/// The strip's height: the one row the window's chrome occupies.
pub const CHROME_HEIGHT: f32 = 48.0;
/// The right Overview dock's default width.
pub const OVERVIEW_WIDTH: f32 = 320.0;

fn glyph(app: &AppContext, name: &'static str) -> Box<dyn Element> {
    Icon::new(name)
        .with_size(15.0)
        .with_theme_color(ColorToken::Muted, app)
        .finish()
}

fn button(app: &AppContext, name: &'static str, tip: &'static str) -> Box<dyn Element> {
    Tooltip::new(
        TopbarButton::new(glyph(app, name))
            .with_size(28.0)
            .with_corner_radius(8.0)
            .finish(),
        tip,
    )
    .with_position(TooltipPosition::Below)
    .finish()
}

fn button_click<F: FnMut() + 'static>(
    app: &AppContext,
    name: &'static str,
    tip: &'static str,
    action: F,
) -> Box<dyn Element> {
    Tooltip::new(
        TopbarButton::new(glyph(app, name))
            .with_size(28.0)
            .with_corner_radius(8.0)
            .with_on_click(action)
            .finish(),
        tip,
    )
    .with_position(TooltipPosition::Below)
    .finish()
}

/// The single top row, left to right: lights, sidebar toggle, navigation,
/// breadcrumb, More, Overview and the right-panel toggle.
pub fn build_chrome_row(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let mut leading = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0)
        .with_child(Empty::new().with_size(vec2f(12.0, 0.0)).finish())
        .with_child(traffic_lights(app));
    let on_sidebar = actions.on_toggle_sidebar.clone();
    let sidebar_open = state.sidebar_visible;
    leading = leading.with_child(button_click(
        app,
        if sidebar_open { "left-panel-close" } else { "left-panel-open" },
        if sidebar_open { "Hide sidebar" } else { "Show sidebar" },
        move || (on_sidebar.borrow_mut())(),
    ));
    // Back/forward are visual navigation controls until the pane history lands;
    // they render disabled rather than pretending to navigate.
    leading = leading.with_child(
        Tooltip::new(
            TopbarButton::new(
                Icon::new("arrow-left")
                    .with_size(15.0)
                    .with_theme_color(ColorToken::Border, app)
                    .finish(),
            )
                .with_size(28.0)
                .finish(),
            "Back",
        )
        .with_position(TooltipPosition::Below)
        .finish(),
    );
    leading = leading.with_child(
        Tooltip::new(
            TopbarButton::new(
                Icon::new("arrow-right")
                    .with_size(15.0)
                    .with_theme_color(ColorToken::Border, app)
                    .finish(),
            )
                .with_size(28.0)
                .finish(),
            "Forward",
        )
        .with_position(TooltipPosition::Below)
        .finish(),
    );
    let on_search = actions.on_select_sidebar_view.clone();
    leading = leading.with_child(
        Tooltip::new(
            TopbarButton::new(glyph(app, "search"))
                .with_size(28.0)
                .with_corner_radius(8.0)
                .with_on_click(move || (on_search.borrow_mut())(SidebarView::Search))
                .finish(),
            "Search",
        )
        .with_position(TooltipPosition::Below)
        .finish(),
    );

    let (crumb_a, crumb_b) = breadcrumb(state);
    let mut crumb = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0);
    if !crumb_a.is_empty() {
        crumb = crumb
            .with_child(
                Text::new(crumb_a)
                    .with_font_size(13.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .with_max_lines(1)
                    .finish(),
            )
            .with_child(
                Text::new("/")
                    .with_font_size(13.0)
                    .with_theme_color(ColorToken::Border, app)
                    .finish(),
            );
    }
    crumb = crumb.with_child(
        ConstrainedBox::new(
            Text::new(crumb_b)
                .with_font_size(13.0)
                .with_weight(FontWeight::Medium)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
        )
        .with_max_width(520.0)
        .finish(),
    );

    let active = state.active_pane_id;
    let on_overview = actions.on_toggle_right_sidebar.clone();
    let overview = button_click(app, "flat-list", "Overview", move || {
        (on_overview.borrow_mut())()
    });
    let on_rename = actions.on_session_rename_start.clone();
    let on_archive = actions.on_archive_conversation.clone();
    let on_copy = actions.on_copy_conversation_id.clone();
    let on_split_right = actions.on_split_right.clone();
    let on_split_down = actions.on_split_down.clone();
    let on_close = actions.on_close_pane.clone();
    let on_create = actions.on_create_submit.clone();
    let selected = state.selected_id.clone().unwrap_or_default();
    let id = selected.clone();
    let close_id = selected.clone();
    let more_items = vec![
        PopupMenuItem::new("Rename").with_shortcut("⌘ R"),
        PopupMenuItem::new("Pin").disabled(),
        PopupMenuItem::new("Archive").with_shortcut("⌘ ⇧ A"),
        PopupMenuItem::new("Split right").with_shortcut("⌘ Space").with_separator_before(),
        PopupMenuItem::new("Split down").with_shortcut("⌘ ⇧ D"),
        PopupMenuItem::new("Replace with new"),
        PopupMenuItem::new("Copy ID").with_shortcut("⌘ ⇧ C"),
        PopupMenuItem::new("Close pane").with_separator_before(),
    ];
    let more = PopupMenu::new(button(app, "ellipsis", "More"), more_items)
        .with_open(state.chrome_more_open.clone())
        .with_position(PopupMenuPosition::BelowEnd)
        .with_max_width(240.0)
        .with_on_select(move |index| match index {
            0 => (on_rename.borrow_mut())(id.clone()),
            2 => (on_archive.borrow_mut())(close_id.clone()),
            3 => (on_split_right.borrow_mut())(),
            4 => (on_split_down.borrow_mut())(),
            5 => (on_create.borrow_mut())(),
            6 => (on_copy.borrow_mut())(close_id.clone()),
            7 => (on_close.borrow_mut())(),
            _ => {}
        })
        .finish();
    let on_panel = actions.on_toggle_right_sidebar.clone();
    let panel_open = state.right_sidebar_open;
    let panel_button = button_click(
        app,
        if panel_open { "left-panel-close" } else { "left-panel-open" },
        if panel_open { "Hide overview" } else { "Show overview" },
        move || (on_panel.borrow_mut())(),
    );

    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(10.0)
        .with_child(leading.finish())
        .with_child(Expanded::new(crumb.finish()).finish())
        .with_child(more)
        .with_child(overview)
        .with_child(panel_button)
        .with_child(Empty::new().with_size(vec2f(10.0, 0.0)).finish())
        .finish();
    let row = ConstrainedBox::new(row).with_height(CHROME_HEIGHT).finish();
    Container::new(row)
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .finish()
}

fn traffic_lights(app: &AppContext) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0);
    if !cfg!(target_os = "macos") {
        for color in ["#ff5f57", "#febc2e", "#28c840"] {
            row = row.with_child(
                Container::new(Empty::new().with_size(vec2f(12.0, 12.0)).finish())
                    .with_background(Fill::Solid(ColorU::hex(
                        u32::from_str_radix(color.trim_start_matches('#'), 16).unwrap_or(0),
                    )))
                    .with_corner_radius(6.0)
                    .finish(),
            );
        }
    }
    let _ = app;
    ConstrainedBox::new(row.finish()).with_height(16.0).finish()
}

fn breadcrumb(state: &UiSnapshot) -> (String, String) {
    let id = state.selected_id.as_deref();
    let entry = id.and_then(|id| state.conversations.iter().find(|entry| entry.id == id));
    match entry {
        Some(entry) => (entry.folder.clone(), entry.name.clone()),
        None => (String::new(), "New Agent".to_string()),
    }
}

/// The right Overview panel: real Review/Files/Terminal actions, the live
/// sub-agent and background-task lists, maximize and collapse controls.
pub fn build_side_panel(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let pane = state.pane_chat.get(&state.active_pane_id);
    let on_max = actions.on_toggle_side_panel_maximized.clone();
    let on_close = actions.on_toggle_right_sidebar.clone();
    let header = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(
            Text::new("Overview")
                .with_font_size(13.0)
                .with_weight(FontWeight::Medium)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(
            TopbarButton::new(glyph(app, if state.side_panel_maximized {
                "fullscreen-exit"
            } else {
                "fullscreen-enter"
            }))
            .with_size(26.0)
            .with_on_click(move || (on_max.borrow_mut())())
            .finish(),
        )
        .with_child(
            TopbarButton::new(glyph(app, "left-panel-close"))
                .with_size(26.0)
                .with_on_click(move || (on_close.borrow_mut())())
                .finish(),
        )
        .finish();

    let on_search_change = actions.on_global_search_change.clone();
    let on_search_focus = actions.on_global_search_focus_change.clone();
    let search = SearchInput::new()
        .with_value(state.global_search_query.clone())
        .with_focused(state.global_search_focused)
        .with_caret(state.global_search_caret.clone())
        .with_placeholder("Search or enter a URL")
        .with_text_size(12.0)
        .with_extra_height(12.0)
        .with_on_change(move |value| (on_search_change.borrow_mut())(value))
        .with_on_focus_change(move |focused| (on_search_focus.borrow_mut())(focused))
        .finish();
    let mut results = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(1.0);
    let query = state.global_search_query.trim();
    if !query.is_empty() {
        if query.starts_with("http://") || query.starts_with("https://") {
            results = results.with_child(muted_line(app, "Browser is not available in this build"));
        } else if state.global_search_rows.is_empty() {
            results = results.with_child(muted_line(
                app,
                if state.global_search_searched {
                    "No matches"
                } else {
                    "Searching…"
                },
            ));
        } else {
            let open = actions.on_explorer_file_click.clone();
            for row in state.global_search_rows.iter().take(20) {
                let label = if row.name.is_empty() {
                    row.text.clone()
                } else {
                    row.name.clone()
                };
                let path = row.path.clone();
                let open = open.clone();
                results = results.with_child(
                    HoverRow::new(
                        Text::new(label)
                            .with_font_size(12.0)
                            .with_theme_color(if row.line.is_some() {
                                ColorToken::Muted
                            } else {
                                ColorToken::Text
                            }, app)
                            .with_max_lines(1)
                            .finish(),
                    )
                    .with_padding(EdgeInsets::new(3.0, 4.0, 3.0, 4.0))
                    .with_corner_radius(6.0)
                    .with_on_click(move || (open.borrow_mut())(path.clone()))
                    .finish(),
                );
            }
        }
    }

    let on_diff = actions.on_diff_panel.clone();
    let on_explorer = actions.on_select_sidebar_view.clone();
    let on_terminal = actions.on_toggle_terminal_dock.clone();
    let on_connectors = actions.on_settings.clone();
    let active = state.active_pane_id;
    let mut rows = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.0);
    for (icon, label, action) in [
        ("file", "Review", 0u8),
        ("folder", "Open File", 1),
        ("terminal", "Terminal", 2),
        ("environment-cloud", "Browser", 3),
        ("message-plus-square", "Side Question", 4),
        ("layers-three-01", "Extensions", 5),
    ] {
        let disabled = matches!(action, 3 | 4);
        let row = if disabled {
            Container::new(panel_row(app, icon, label, true)).finish()
        } else {
            let on_diff = on_diff.clone();
            let on_explorer = on_explorer.clone();
            let on_terminal = on_terminal.clone();
            let on_connectors = on_connectors.clone();
            HoverRow::new(panel_row(app, icon, label, false))
                .with_padding(EdgeInsets::new(6.0, 4.0, 6.0, 4.0))
                .with_corner_radius(8.0)
                .with_on_click(move || match action {
                    0 => (on_diff.borrow_mut())(active, DiffPanelAction::Open(None)),
                    1 => (on_explorer.borrow_mut())(SidebarView::Explorer),
                    2 => (on_terminal.borrow_mut())(active),
                    5 => (on_connectors.borrow_mut())(),
                    _ => {}
                })
                .finish()
        };
        rows = rows.with_child(row);
    }

    let mut sections = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(4.0);
    let subagents: Vec<_> = pane
        .map(|pane| pane.sub_agents.values().cloned().collect())
        .unwrap_or_default();
    sections = sections.with_child(section_header(app, "Subagents", subagents.len()));
    if subagents.is_empty() {
        sections = sections.with_child(muted_line(app, "No subagents"));
    } else {
        for row in &subagents {
            let open = actions.on_open_sub_agent.clone();
            let active = state.active_pane_id;
            let child = row.child_id.clone();
            let (mark, token) = row.status_affordance();
            let line = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.0)
                .with_child(
                    Text::new(mark)
                        .with_font_size(11.0)
                        .with_theme_color(token, app)
                        .finish(),
                )
                .with_child(
                    Text::new(if row.description.trim().is_empty() {
                        row.subagent_type.clone()
                    } else {
                        row.description.clone()
                    })
                    .with_font_size(12.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
                )
                .finish();
            sections = sections.with_child(
                HoverRow::new(line)
                    .with_padding(EdgeInsets::new(4.0, 4.0, 4.0, 4.0))
                    .with_corner_radius(6.0)
                    .with_on_click(move || (open.borrow_mut())(active, child.clone()))
                    .finish(),
            );
        }
    }
    let tasks: Vec<_> = state
        .pane_live_work
        .get(&active)
        .map(|items| items.iter().filter(|item| item.kind == crate::state::PaneWorkKind::Task).collect())
        .unwrap_or_default();
    sections = sections.with_child(
        Container::new(section_header(app, "Background Tasks", tasks.len()))
            .with_padding(EdgeInsets::new(12.0, 0.0, 0.0, 0.0))
            .finish(),
    );
    if tasks.is_empty() {
        sections = sections.with_child(muted_line(app, "No background tasks"));
    } else {
        for task in tasks {
            sections = sections.with_child(panel_row(app, "terminal-input", &task.description, false));
        }
    }

    let body = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(10.0)
        .with_child(header)
        .with_child(search)
        .with_child(results.finish())
        .with_child(rows.finish())
        .with_child(
            Container::new(sections.finish())
                .with_padding(EdgeInsets::new(12.0, 0.0, 12.0, 0.0))
                .finish(),
        )
        .finish();
    Container::new(Scrollable::new(body, goble_ui::elements::Axis::Vertical).finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
        .with_corner_radius(12.0)
        .with_padding(EdgeInsets::new(12.0, 12.0, 12.0, 12.0))
        .finish()
}

fn panel_row(app: &AppContext, icon: &'static str, label: &str, disabled: bool) -> Box<dyn Element> {
    Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(10.0)
        .with_child(
            Icon::new(icon)
                .with_size(15.0)
                .with_theme_color(if disabled { ColorToken::Border } else { ColorToken::Muted }, app)
                .finish(),
        )
        .with_child(
            Text::new(label)
                .with_font_size(13.0)
                .with_theme_color(if disabled { ColorToken::Muted } else { ColorToken::Text }, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(
            Text::new(if disabled { "not available" } else { "" })
                .with_font_size(10.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish()
}

fn section_header(app: &AppContext, label: &str, count: usize) -> Box<dyn Element> {
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(
            Text::new(label)
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Text::new(count.to_string())
                .with_font_size(10.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish()
}

fn muted_line(app: &AppContext, text: &str) -> Box<dyn Element> {
    Text::new(text)
        .with_font_size(12.0)
        .with_theme_color(ColorToken::Muted, app)
        .finish()
}
