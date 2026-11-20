//! The window's single top row and the right Overview panel.
//!
//! The general topbar is retired: traffic lights, navigation, the conversation
//! name and the panel actions live on one continuous partitioned strip above
//! the bordered content. The strip owns the top edge for both the sidebar and
//! the panes; the sidebar and the panel header start below it.

use goble_ui::color::ColorU;
use goble_ui::elements::{
    AppContext, Border, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets, Element, Empty,
    Expanded, Fill, Flex, HoverRow, Icon, MainAxisSize, PopupMenu, PopupMenuItem,
    PopupMenuPosition, Scrollable, SearchInput, Spacer, Text, Tooltip, TooltipPosition,
    TopbarButton,
};
use goble_ui::geometry::vec2f;
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::ColorToken;

use crate::ui::{DiffPanelAction, SidebarView, UiActions, UiSnapshot};

/// The strip's height: the one row the window's chrome occupies.
pub const CHROME_HEIGHT: f32 = 48.0;
/// The quiet band above the strip's content. The Surface covers the whole 48 pt,
/// so the padding reads as one continuous material, not as a second bar.
const CHROME_TOP_PADDING: f32 = 5.0;
/// The strip's content band, below [`CHROME_TOP_PADDING`]. The section rules
/// span exactly this height, so every section bottom ends where the content
/// sheet begins.
const CHROME_CONTENT_HEIGHT: f32 = CHROME_HEIGHT - CHROME_TOP_PADDING;
/// The right Overview dock's default width.
pub const OVERVIEW_WIDTH: f32 = 320.0;

/// A top-row control's glyph. 17 pt reads clearly inside the strip without
/// filling its hit box edge to edge.
const CONTROL_ICON_SIZE: f32 = 17.0;
/// A top-row control's square hit box. The 28 pt box is compact enough for the
/// four leading controls to share a 200 pt sidebar section on macOS.
const CONTROL_BUTTON_SIZE: f32 = 28.0;
/// The gap between two strip controls. It is deliberately tight: DSH's leading
/// control cluster reads as one group rather than four separate buttons.
const CONTROL_SPACING: f32 = 2.0;
/// The vertical padding that centers a 28 pt control row inside the 43 pt
/// content band.
const SECTION_VERTICAL_PADDING: f32 = (CHROME_CONTENT_HEIGHT - CONTROL_BUTTON_SIZE) / 2.0;
/// A section rule's width: one physical pixel, painted as a filled Empty.
const SEPARATOR_WIDTH: f32 = 1.0;
/// The main section's leading padding: the title starts where the pane area's
/// own left edge does, not at the rule.
const MAIN_SECTION_LEFT_PADDING: f32 = 16.0;
/// The main section's trailing padding, before the More control.
const MAIN_SECTION_RIGHT_PADDING: f32 = 12.0;
/// The side-panel section's inset on both sides.
const SIDE_SECTION_PADDING: f32 = 12.0;
/// A side-panel tab's glyph and label sizes.
const TAB_ICON_SIZE: f32 = 14.0;
const TAB_FONT_SIZE: f32 = 12.5;

/// The room the window controls take at the strip's leading edge. On macOS the
/// OS draws the real traffic lights over this strip, so the whole band stays
/// clear of the sidebar toggle and navigation. Elsewhere the strip is compact
/// and the preview dots follow this edge inset; the dots' own width is part of
/// the reserved region either way, so the toggle always starts after it.
#[cfg(target_os = "macos")]
const TRAFFIC_INSET: f32 = 78.0;
#[cfg(not(target_os = "macos"))]
const TRAFFIC_INSET: f32 = 20.0;

/// The preview traffic lights: three 12 pt dots with 8 pt gaps (52 pt total).
/// macOS keeps the same clear spacing inside [`TRAFFIC_INSET`] for the
/// OS-drawn lights.
const TRAFFIC_LIGHT_SIZE: f32 = 12.0;
const TRAFFIC_LIGHT_GAP: f32 = 8.0;
const TRAFFIC_LIGHTS_WIDTH: f32 = TRAFFIC_LIGHT_SIZE * 3.0 + TRAFFIC_LIGHT_GAP * 2.0;

fn glyph(app: &AppContext, name: &'static str) -> Box<dyn Element> {
    Icon::new(name)
        .with_size(CONTROL_ICON_SIZE)
        .with_theme_color(ColorToken::Muted, app)
        .finish()
}

fn button(app: &AppContext, name: &'static str, tip: &'static str) -> Box<dyn Element> {
    Tooltip::new(
        TopbarButton::new(glyph(app, name))
            .with_size(CONTROL_BUTTON_SIZE)
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
            .with_size(CONTROL_BUTTON_SIZE)
            .with_corner_radius(8.0)
            .with_on_click(action)
            .finish(),
        tip,
    )
    .with_position(TooltipPosition::Below)
    .finish()
}

/// The single top row, partitioned left to right: the sidebar section (lights,
/// sidebar toggle and navigation), the main section (the active conversation
/// name, More and the side-panel toggle), and — while the side panel is open —
/// the side-panel section (its tab chips, add-tab and window controls).
pub fn build_chrome_row(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    // The body's first column is the sidebar when it is shown; the strip's first
    // section takes exactly that width so the rule falls on the same boundary.
    // With the sidebar retracted the section shrinks to its controls, and the
    // main section starts immediately after them.
    let sidebar_shown =
        state.responsive.sidebar_shown(state.sidebar_visible) && !state.side_panel_maximized;
    let sidebar_width = if sidebar_shown {
        state.sidebar_width
    } else {
        sidebar_controls_width()
    };

    // `traffic_lights` owns the whole leading band (the reserved inset plus the
    // lights' width), so the sidebar toggle and every nav control after it start
    // past the lights instead of underneath them.
    let mut leading = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(CONTROL_SPACING)
        .with_child(traffic_lights(app));
    let on_sidebar = actions.on_toggle_sidebar.clone();
    let sidebar_open = state.sidebar_visible;
    leading = leading.with_child(button_click(
        app,
        if sidebar_open {
            "left-panel-close"
        } else {
            "left-panel-open"
        },
        if sidebar_open {
            "Hide sidebar"
        } else {
            "Show sidebar"
        },
        move || (on_sidebar.borrow_mut())(),
    ));
    // Back/forward are visual navigation controls until the pane history lands;
    // they render disabled rather than pretending to navigate.
    leading = leading.with_child(disabled_button(app, "arrow-left", "Back"));
    leading = leading.with_child(disabled_button(app, "arrow-right", "Forward"));
    let on_search = actions.on_select_sidebar_view.clone();
    leading = leading.with_child(
        Tooltip::new(
            TopbarButton::new(glyph(app, "search"))
                .with_size(CONTROL_BUTTON_SIZE)
                .with_corner_radius(8.0)
                .with_on_click(move || (on_search.borrow_mut())(SidebarView::Search))
                .finish(),
            "Search",
        )
        .with_position(TooltipPosition::Below)
        .finish(),
    );
    let leading = ConstrainedBox::new(
        Container::new(leading.finish())
            .with_padding(EdgeInsets::new(
                0.0,
                SECTION_VERTICAL_PADDING,
                0.0,
                SECTION_VERTICAL_PADDING,
            ))
            .finish(),
    )
    .with_width(sidebar_width)
    .finish();

    // The main section names the active conversation and nothing else: DSH's
    // partitioned strip retired the folder crumb, so there is no "Not in
    // project /" run before the name.
    let title = Text::new(truncate_label(&active_conversation_name(state), 64))
        .with_font_size(14.5)
        .with_weight(FontWeight::Medium)
        .with_theme_color(ColorToken::Text, app)
        .with_max_lines(1)
        .finish();
    let on_rename = actions.on_session_rename_start.clone();
    let on_archive = actions.on_archive_conversation.clone();
    let on_copy = actions.on_copy_conversation_id.clone();
    let on_pin = actions.on_toggle_star.clone();
    let on_split_right = actions.on_split_right.clone();
    let on_split_down = actions.on_split_down.clone();
    let on_close = actions.on_close_pane.clone();
    let on_create = actions.on_create_submit.clone();
    let selected = state.selected_id.clone().unwrap_or_default();
    let pinned = state
        .conversations
        .iter()
        .find(|entry| entry.id == selected)
        .is_some_and(|entry| entry.starred);
    let pin_label = if pinned { "Unpin" } else { "Pin" };
    let id = selected.clone();
    let close_id = selected.clone();
    let more_items = vec![
        PopupMenuItem::new("Rename").with_shortcut("⌘ R"),
        PopupMenuItem::new(pin_label),
        PopupMenuItem::new("Archive").with_shortcut("⌘ ⇧ A"),
        PopupMenuItem::new("Split right")
            .with_shortcut("⌘ Space")
            .with_separator_before(),
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
            1 => (on_pin.borrow_mut())(id.clone()),
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
        if panel_open {
            "left-panel-close"
        } else {
            "left-panel-open"
        },
        "Toggle side panel",
        move || (on_panel.borrow_mut())(),
    );

    let main_section = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(CONTROL_SPACING)
        .with_child(Expanded::new(title).finish())
        .with_child(more)
        .with_child(panel_button)
        .finish();
    let main_section = Container::new(main_section)
        .with_padding(EdgeInsets::new(
            MAIN_SECTION_LEFT_PADDING,
            SECTION_VERTICAL_PADDING,
            MAIN_SECTION_RIGHT_PADDING,
            SECTION_VERTICAL_PADDING,
        ))
        .finish();

    let mut strip = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(leading)
        .with_child(vertical_separator(app))
        .with_child(Expanded::new(main_section).finish());
    if panel_open {
        let maximized = state.side_panel_maximized;
        let side_tabs = side_panel_tabs(app, state);
        let on_max = actions.on_toggle_side_panel_maximized.clone();
        let on_close = actions.on_toggle_right_sidebar.clone();
        let side_section = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(CONTROL_SPACING)
            .with_child(Expanded::new(side_tabs).finish())
            .with_child(button(app, "plus", "Add tab"))
            .with_child(button_click(
                app,
                if maximized {
                    "fullscreen-exit"
                } else {
                    "fullscreen-enter"
                },
                if maximized {
                    "Restore side panel"
                } else {
                    "Maximize side panel"
                },
                move || (on_max.borrow_mut())(),
            ))
            .with_child(button_click(
                app,
                "left-panel-close",
                "Collapse side panel",
                move || (on_close.borrow_mut())(),
            ))
            .finish();
        let side_section = Container::new(side_section)
            .with_padding(EdgeInsets::new(
                SIDE_SECTION_PADDING,
                SECTION_VERTICAL_PADDING,
                SIDE_SECTION_PADDING,
                SECTION_VERTICAL_PADDING,
            ))
            .finish();
        strip = strip
            .with_child(vertical_separator(app))
            .with_child(Expanded::new(side_section).finish());
    }
    // The strip content is exactly 43 pt; the outer container owns the 5 pt
    // quiet band above it. Both are Surface, so the whole 48 pt reads as one
    // continuous material.
    let strip = ConstrainedBox::new(strip.finish())
        .with_height(CHROME_CONTENT_HEIGHT)
        .finish();
    Container::new(strip)
        .with_padding(EdgeInsets::new(0.0, CHROME_TOP_PADDING, 0.0, 0.0))
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .finish()
}

fn traffic_lights(app: &AppContext) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(TRAFFIC_LIGHT_GAP);
    if !cfg!(target_os = "macos") {
        for color in ["#ff5f57", "#febc2e", "#28c840"] {
            row = row.with_child(
                Container::new(
                    Empty::new()
                        .with_size(vec2f(TRAFFIC_LIGHT_SIZE, TRAFFIC_LIGHT_SIZE))
                        .finish(),
                )
                .with_background(Fill::Solid(ColorU::hex(
                    u32::from_str_radix(color.trim_start_matches('#'), 16).unwrap_or(0),
                )))
                .with_corner_radius(TRAFFIC_LIGHT_SIZE / 2.0)
                .finish(),
            );
        }
    }
    let _ = app;
    // The reserved band is this element's own width, so the sidebar toggle and
    // navigation that follow it in the leading flex are always laid out past
    // the lights. macOS keeps the whole inset clear for the OS-drawn lights;
    // elsewhere the preview dots sit after the (smaller) edge inset.
    ConstrainedBox::new(
        Container::new(row.finish())
            .with_padding(EdgeInsets::new(TRAFFIC_INSET, 0.0, 0.0, 0.0))
            .finish(),
    )
    .with_width(TRAFFIC_INSET + traffic_preview_width())
    .with_height(16.0)
    .finish()
}

/// The preview dots' width on platforms where the app draws them. macOS keeps
/// the same space inside [`TRAFFIC_INSET`] for the OS-drawn lights, so it adds
/// nothing after the inset.
fn traffic_preview_width() -> f32 {
    if cfg!(target_os = "macos") {
        0.0
    } else {
        TRAFFIC_LIGHTS_WIDTH
    }
}

/// The width the leading section needs when the sidebar is hidden: the reserved
/// traffic band, the four controls, and the gaps between all five children.
fn sidebar_controls_width() -> f32 {
    TRAFFIC_INSET + traffic_preview_width() + CONTROL_SPACING * 4.0 + CONTROL_BUTTON_SIZE * 4.0
}

/// A 1 px section rule. `Border` only strokes a whole rectangle, so the strip
/// splits sections with an empty 1 pt column painted in the border token. Its
/// height is the full content band, so the three partitions meet the content
/// sheet with one continuous line.
fn vertical_separator(app: &AppContext) -> Box<dyn Element> {
    Container::new(
        Empty::new()
            .with_size(vec2f(SEPARATOR_WIDTH, CHROME_CONTENT_HEIGHT))
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::Border)))
    .finish()
}

/// A nav control whose action is not available yet: the muted glyph reads as
/// disabled, while the tooltip still names the eventual control.
fn disabled_button(app: &AppContext, name: &'static str, tip: &'static str) -> Box<dyn Element> {
    Tooltip::new(
        TopbarButton::new(
            Icon::new(name)
                .with_size(CONTROL_ICON_SIZE)
                .with_theme_color(ColorToken::Border, app)
                .finish(),
        )
        .with_size(CONTROL_BUTTON_SIZE)
        .with_corner_radius(8.0)
        .finish(),
        tip,
    )
    .with_position(TooltipPosition::Below)
    .finish()
}

/// The active conversation's display name, or the fresh-pane placeholder. The
/// strip shows only this run; the folder crumb is deliberately gone.
fn active_conversation_name(state: &UiSnapshot) -> String {
    let id = state.selected_id.as_deref();
    id.and_then(|id| state.conversations.iter().find(|entry| entry.id == id))
        .map(|entry| entry.name.clone())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "New Agent".to_string())
}

/// The side-panel section's tabs: DSH's new-side-question chip, at most one real
/// work tab (the active pane's own unfinished background task), and the shell
/// tab. No label is invented when the pane has no work item.
fn side_panel_tabs(app: &AppContext, state: &UiSnapshot) -> Box<dyn Element> {
    let task = state
        .pane_live_work
        .get(&state.active_pane_id)
        .and_then(|items| {
            items.iter().find(|item| {
                item.kind == crate::state::PaneWorkKind::Task
                    && !item.description.trim().is_empty()
            })
        })
        .map(|item| item.description.clone());
    let mut tabs = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(side_panel_tab(
            app,
            "message-plus-square",
            "New Side Question",
            task.is_none(),
        ));
    if let Some(task) = task {
        tabs = tabs.with_child(side_panel_tab(
            app,
            "terminal-input",
            &truncate_label(&task, 28),
            true,
        ));
    }
    tabs = tabs.with_child(side_panel_tab(app, "terminal", "zsh", false));
    tabs.finish()
}

/// One side-panel tab chip: icon plus a single-line label, selected when it is
/// the surface the panel currently shows.
fn side_panel_tab(
    app: &AppContext,
    icon: &'static str,
    label: &str,
    selected: bool,
) -> Box<dyn Element> {
    let token = if selected {
        ColorToken::Text
    } else {
        ColorToken::Muted
    };
    let content = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(
            Icon::new(icon)
                .with_size(TAB_ICON_SIZE)
                .with_theme_color(token, app)
                .finish(),
        )
        .with_child(
            Text::new(label)
                .with_font_size(TAB_FONT_SIZE)
                .with_theme_color(token, app)
                .with_max_lines(1)
                .finish(),
        )
        .finish();
    HoverRow::new(content)
        .with_selected(selected)
        .with_padding(EdgeInsets::new(9.0, 6.0, 9.0, 6.0))
        .with_corner_radius(8.0)
        .finish()
}

/// Fit a label into a one-line chip without inventing different text: take a
/// prefix and an ellipsis when the run would be too long.
fn truncate_label(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

/// The right Overview panel: real Review/Files/Terminal actions, the live
/// sub-agent and background-task lists, maximize and collapse controls.
pub fn build_side_panel(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
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
            TopbarButton::new(glyph(
                app,
                if state.side_panel_maximized {
                    "fullscreen-exit"
                } else {
                    "fullscreen-enter"
                },
            ))
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
        } else {
            let lower = query.to_lowercase();
            let mut found = false;
            let conversations: Vec<_> = state
                .conversations
                .iter()
                .filter(|entry| entry.name.to_lowercase().contains(&lower))
                .take(6)
                .collect();
            if !conversations.is_empty() {
                found = true;
                results =
                    results.with_child(section_header(app, "Conversations", conversations.len()));
                let select = actions.on_select_conversation.clone();
                for entry in conversations {
                    let id = entry.id.clone();
                    let select = select.clone();
                    results = results.with_child(
                        HoverRow::new(
                            Text::new(entry.name.clone())
                                .with_font_size(12.0)
                                .with_theme_color(ColorToken::Text, app)
                                .with_max_lines(1)
                                .finish(),
                        )
                        .with_padding(EdgeInsets::new(3.0, 4.0, 3.0, 4.0))
                        .with_corner_radius(6.0)
                        .with_on_click(move || (select.borrow_mut())(id.clone()))
                        .finish(),
                    );
                }
            }
            let commands: Vec<_> = crate::ui::palette::slash_commands(state, actions)
                .into_iter()
                .filter(|command| {
                    command.name.to_lowercase().contains(&lower)
                        || command.description.to_lowercase().contains(&lower)
                })
                .take(6)
                .collect();
            if !commands.is_empty() {
                found = true;
                results = results.with_child(section_header(app, "Commands", commands.len()));
                let insert = actions.on_composer_insert.clone();
                for command in commands {
                    let token = format!("/{} ", command.name);
                    let label = format!("/{}  {}", command.name, command.description);
                    let insert = insert.clone();
                    results = results.with_child(
                        HoverRow::new(
                            Text::new(label)
                                .with_font_size(12.0)
                                .with_theme_color(ColorToken::Text, app)
                                .with_max_lines(1)
                                .finish(),
                        )
                        .with_padding(EdgeInsets::new(3.0, 4.0, 3.0, 4.0))
                        .with_corner_radius(6.0)
                        .with_on_click(move || (insert.borrow_mut())(token.clone()))
                        .finish(),
                    );
                }
            }
            let files: Vec<_> = state
                .global_search_rows
                .iter()
                .filter(|row| {
                    row.name.is_empty()
                        || row.name.to_lowercase().contains(&lower)
                        || row.text.to_lowercase().contains(&lower)
                })
                .take(12)
                .collect();
            if !files.is_empty() {
                found = true;
                results = results.with_child(section_header(app, "Files", files.len()));
                let open = actions.on_explorer_file_click.clone();
                for row in files {
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
                                .with_theme_color(
                                    if row.line.is_some() {
                                        ColorToken::Muted
                                    } else {
                                        ColorToken::Text
                                    },
                                    app,
                                )
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
            if !found {
                results = results.with_child(muted_line(
                    app,
                    if state.global_search_searched {
                        "No matches"
                    } else {
                        "Searching…"
                    },
                ));
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
        let disabled = matches!(action, 3);
        let row = if disabled {
            Container::new(panel_row(app, icon, label, true)).finish()
        } else {
            let on_diff = on_diff.clone();
            let on_explorer = on_explorer.clone();
            let on_terminal = on_terminal.clone();
            let on_connectors = on_connectors.clone();
            let on_side_question = actions.on_create_submit.clone();
            HoverRow::new(panel_row(app, icon, label, false))
                .with_padding(EdgeInsets::new(6.0, 4.0, 6.0, 4.0))
                .with_corner_radius(8.0)
                .with_on_click(move || match action {
                    0 => (on_diff.borrow_mut())(active, DiffPanelAction::Open(None)),
                    1 => (on_explorer.borrow_mut())(SidebarView::Explorer),
                    2 => (on_terminal.borrow_mut())(active),
                    4 => (on_side_question.borrow_mut())(),
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
        .map(|items| {
            items
                .iter()
                .filter(|item| item.kind == crate::state::PaneWorkKind::Task)
                .collect()
        })
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
            sections =
                sections.with_child(panel_row(app, "terminal-input", &task.description, false));
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

fn panel_row(
    app: &AppContext,
    icon: &'static str,
    label: &str,
    disabled: bool,
) -> Box<dyn Element> {
    Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(10.0)
        .with_child(
            Icon::new(icon)
                .with_size(15.0)
                .with_theme_color(
                    if disabled {
                        ColorToken::Border
                    } else {
                        ColorToken::Muted
                    },
                    app,
                )
                .finish(),
        )
        .with_child(
            Text::new(label)
                .with_font_size(13.0)
                .with_theme_color(
                    if disabled {
                        ColorToken::Muted
                    } else {
                        ColorToken::Text
                    },
                    app,
                )
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
