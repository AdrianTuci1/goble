//! The window's single top row and the right Overview panel.
//!
//! The general topbar is retired: traffic lights, navigation, the conversation
//! name and the panel actions live on one continuous partitioned strip above
//! the bordered content. The strip owns the top edge for both the sidebar and
//! the panes; the sidebar and the panel header start below it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use goble_ui::color::ColorU;
use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, Axis, Border, ConstrainedBox, Container, CrossAxisAlignment, Divider, EdgeInsets,
    Element, Empty, EventContext, Expanded, Fill, Flex, HoverRow, Icon, KeyHandler, LayoutContext,
    MainAxisSize, MenuId, MenuPointer, PaintContext, Point, PopupMenu, PopupMenuItem,
    PopupMenuPosition, ScrollState, Scrollable, SearchInput, SizeConstraint, Spacer, Stack, Text,
    TextInput, Tooltip, TooltipPosition, TopbarButton,
};
use goble_ui::event::{DispatchedEvent, BUTTON_PRIMARY};
use goble_ui::geometry::{vec2f, PointF, RectF, Size2F, Vector2F};
use goble_ui::platform::text_atlas::{advance_width, FontWeight};
use goble_ui::theme::{ColorToken, FontFamily};

use crate::state::{SideTabAction, SideTabKind};
use crate::ui::{DiffPanelAction, PaneKind, SideChatAction, SidebarView, UiActions, UiSnapshot};

#[path = "side_tabs.rs"]
mod side_tabs;

/// The strip's height: the one row the window's chrome occupies.
pub const CHROME_HEIGHT: f32 = 44.0;
/// The quiet band above the strip's content. The Surface covers the whole 42 pt,
/// so the padding reads as one continuous material, not as a second bar.
const CHROME_TOP_PADDING: f32 = 4.0;
/// The strip's content band, below [`CHROME_TOP_PADDING`]. The section rules
/// span exactly this height, so every section bottom ends where the content
/// sheet begins.
const CHROME_CONTENT_HEIGHT: f32 = CHROME_HEIGHT - CHROME_TOP_PADDING;
/// The right Overview dock's default width.
pub const OVERVIEW_WIDTH: f32 = 320.0;

/// A top-row control's glyph. 15.5 pt reads clearly inside the strip without
/// filling its hit box edge to edge.
const CONTROL_ICON_SIZE: f32 = 15.5;
/// A top-row control's square hit box. The 30 pt box is the compact end of the
/// 29–30 pt range and still lets the four leading controls share the 200 pt
/// sidebar section.
const CONTROL_BUTTON_SIZE: f32 = 30.0;
/// The gap between two strip controls. It is deliberately tight: DSH's leading
/// control cluster reads as one group rather than four separate buttons.
const CONTROL_SPACING: f32 = 2.0;
/// The vertical padding that centers a 30 pt control row inside the 40 pt
/// content band.
const SECTION_VERTICAL_PADDING: f32 = (CHROME_HEIGHT - CONTROL_BUTTON_SIZE) / 2.0;
/// A section's top inset: the quiet band above the content plus the padding that
/// centers its controls in the rest of the band. The traffic lights are the one
/// exception: they center on the full bar instead (see [`traffic_lights`]).
const SECTION_TOP_PADDING: f32 = (CHROME_HEIGHT - CONTROL_BUTTON_SIZE) / 2.0;
/// A section rule's width: one physical pixel, painted as a filled Empty.
const SEPARATOR_WIDTH: f32 = 1.0;
/// The main section's leading padding: the title starts where the pane area's
/// own left edge does, not at the rule.
const MAIN_SECTION_LEFT_PADDING: f32 = 16.0;
/// The main section's trailing padding, before the More control.
const MAIN_SECTION_RIGHT_PADDING: f32 = 12.0;
/// The side-panel section's inset on both sides.
const SIDE_SECTION_PADDING: f32 = 12.0;

/// The Overview popover's width, corner radius and the gap under its trigger.
/// It is a list of the active pane's actions and live work, not a document, so
/// it stays narrower than the Overview side tab it duplicates.
const OVERVIEW_POPOVER_WIDTH: f32 = 344.0;
const OVERVIEW_POPOVER_RADIUS: f32 = 12.0;
const OVERVIEW_POPOVER_GAP: f32 = 6.0;
/// The room the floating panel keeps from the window's edges, and the most of
/// the window's height its scrolled body may take.
const OVERVIEW_EDGE_MARGIN: f32 = 8.0;
const OVERVIEW_MAX_HEIGHT_FRACTION: f32 = 0.6;
/// The popover's label in the window's shared open-menu state: it takes part in
/// the one-menu-at-a-time rule and the root's outside-press/Escape dismissal.
const OVERVIEW_MENU_ID: MenuId = "chrome.overview";
/// The `collapsed_sections` keys the three collapsible Overview groups use.
/// They live in app state (the same cell the sidebar groups use), so a fold
/// survives the per-frame rebuild and a closed/reopened popover.
const OVERVIEW_TASKS_KEY: &str = "chrome.overview.background-tasks";
const OVERVIEW_SUBAGENTS_KEY: &str = "chrome.overview.subagents";
const OVERVIEW_WORKFLOWS_KEY: &str = "chrome.overview.workflows";

thread_local! {
    // Session-only element state the snapshot has no field for: the tab
    // strip's horizontal scroll offset and the Overview popover's keyboard
    // highlight. The tree is rebuilt every frame, so a value held in the
    // element would reset each frame; a `UiActions`/snapshot field would be
    // the durable home, but that is outside this file's scope. These cells
    // live for the process's UI thread, so they persist for the session (and
    // are deliberately not saved to disk).
    static SIDE_TAB_SCROLL: Rc<RefCell<ScrollState>> =
        Rc::new(RefCell::new(ScrollState::default()));
    static OVERVIEW_SCROLL: Rc<RefCell<ScrollState>> =
        Rc::new(RefCell::new(ScrollState::default()));
    static OVERVIEW_HIGHLIGHT: Cell<usize> = Cell::new(0);
    static SIDE_TAB_REVEAL: Cell<Option<(usize, usize, f32)>> = Cell::new(None);
}

fn side_tab_scroll() -> Rc<RefCell<ScrollState>> {
    SIDE_TAB_SCROLL.with(Rc::clone)
}

fn overview_scroll() -> Rc<RefCell<ScrollState>> {
    OVERVIEW_SCROLL.with(Rc::clone)
}

/// The More menu's open submenu row and the row the keyboard is on inside it:
/// session-only state, like the menu's own open flag, so it survives the
/// per-frame element rebuild.
fn more_submenu_state() -> (
    Rc<RefCell<Option<usize>>>,
    Rc<RefCell<usize>>,
    Rc<std::cell::Cell<bool>>,
) {
    thread_local! {
        static MORE_SUBMENU: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
        static MORE_SUBMENU_HIGHLIGHT: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));
        static MORE_SUBMENU_KEYBOARD: Rc<std::cell::Cell<bool>> =
            Rc::new(std::cell::Cell::new(false));
    }
    (
        MORE_SUBMENU.with(Rc::clone),
        MORE_SUBMENU_HIGHLIGHT.with(Rc::clone),
        MORE_SUBMENU_KEYBOARD.with(Rc::clone),
    )
}

/// The side panel's kebab menu open flag. The panel is rebuilt every frame, so
/// the flag lives with the other session-only strip state.
fn side_panel_menu_open() -> Rc<RefCell<bool>> {
    thread_local! {
        static SIDE_PANEL_MENU_OPEN: Rc<RefCell<bool>> =
            Rc::new(RefCell::new(false));
    }
    SIDE_PANEL_MENU_OPEN.with(Rc::clone)
}

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
    // past the lights instead of underneath them. The nav controls live in their
    // own top-padded group: they center in the content band, while the lights
    // center on the full bar.
    let mut controls = Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(CONTROL_SPACING);
    let on_sidebar = actions.on_toggle_sidebar.clone();
    let sidebar_open = state.sidebar_visible;
    // Q: DSH's panel-left artwork, lit while the sidebar is open and muted
    // while closed (the same glyph carries both states).
    let sidebar_button = Tooltip::new(
        TopbarButton::new(
            Icon::new("panel-left-outline")
                .with_size(CONTROL_ICON_SIZE)
                .with_theme_color(
                    if sidebar_open {
                        ColorToken::Text
                    } else {
                        ColorToken::Muted
                    },
                    app,
                )
                .finish(),
        )
        .with_size(CONTROL_BUTTON_SIZE)
        .with_corner_radius(8.0)
        .with_active(sidebar_open)
        .with_on_click(move || (on_sidebar.borrow_mut())())
        .finish(),
        if sidebar_open {
            "Hide sidebar"
        } else {
            "Show sidebar"
        },
    )
    .with_position(TooltipPosition::Below)
    .finish();
    controls = controls.with_child(sidebar_button);
    // Back/forward are visual navigation controls until the pane history lands;
    // they render disabled rather than pretending to navigate.
    controls = controls.with_child(disabled_button(app, "arrow-left", "Back"));
    controls = controls.with_child(disabled_button(app, "arrow-right", "Forward"));
    let on_search = actions.on_spotlight.clone();
    controls = controls.with_child(
        Tooltip::new(
            TopbarButton::new(glyph(app, "search"))
                .with_size(CONTROL_BUTTON_SIZE)
                .with_corner_radius(8.0)
                .with_on_click(move || {
                    (on_search.borrow_mut())(crate::ui::spotlight::SpotlightAction::Open)
                })
                .finish(),
            "Search",
        )
        .with_position(TooltipPosition::Below)
        .finish(),
    );
    let controls = Container::new(controls.finish())
        .with_padding(EdgeInsets::new(
            0.0,
            SECTION_TOP_PADDING,
            0.0,
            SECTION_VERTICAL_PADDING,
        ))
        .finish();
    let leading = ConstrainedBox::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(CONTROL_SPACING)
            .with_child(traffic_lights(app))
            .with_child(controls)
            .finish(),
    )
    .with_width(sidebar_width)
    .finish();

    // The main section names the active conversation in a rounded pill (V).
    // Clicking it turns the same pill into an inline editor; the modal rename
    // surface is suppressed for this inline mode.
    let selected_id = state.selected_id.clone();
    let inline_rename = state
        .sidebar_rename
        .as_ref()
        .filter(|rename| rename.inline && Some(&rename.id) == selected_id.as_ref());
    let title: Box<dyn Element> = if let Some(rename) = inline_rename {
        let on_change = actions.on_session_rename_draft.clone();
        let on_submit = actions.on_session_rename_commit.clone();
        let on_cancel = actions.on_session_rename_cancel.clone();
        let on_blur = actions.on_session_rename_commit.clone();
        // Editing keeps the title's exact footprint: no input frame, surface
        // or border — the bare text line is drawn in place, with only the
        // caret appearing to show that it is editable.
        let input = TextInput::new()
            .with_value(rename.draft.clone())
            .with_placeholder("Conversation name")
            .with_focused(rename.focused)
            .with_select_all(true)
            .with_plain(true)
            .with_on_change(move |value| (on_change.borrow_mut())(value))
            .with_on_submit(move || (on_submit.borrow_mut())())
            .with_on_focus_change(move |focused| {
                if !focused {
                    (on_blur.borrow_mut())();
                }
            })
            .finish();
        let editor = Container::new(input)
            .with_padding(EdgeInsets::new(3.0, 10.0, 3.0, 10.0))
            .finish();
        let editor = ConstrainedBox::new(editor)
            .with_max_width(360.0)
            .with_height(26.0)
            .finish();
        let key = actions.on_session_rename_commit.clone();
        let cancel = actions.on_session_rename_cancel.clone();
        KeyHandler::new(editor, move |pressed, modifiers| {
            if modifiers.command || modifiers.ctrl || modifiers.alt {
                return false;
            }
            match pressed {
                "Enter" => {
                    (key.borrow_mut())();
                    true
                }
                "Escape" => {
                    (cancel.borrow_mut())();
                    true
                }
                _ => false,
            }
        })
        .finish()
    } else {
        let name = active_conversation_name(state);
        let on_start = actions.on_inline_title_rename.clone();
        let id = selected_id.clone().unwrap_or_default();
        let label = Text::new(truncate_label(&name, 64))
            .with_font_size(14.5)
            .with_weight(FontWeight::Medium)
            .with_theme_color(ColorToken::Text, app)
            .with_max_lines(1)
            .finish();
        // No chip at rest: the title is plain text and the rounded band only
        // appears under the pointer (where the tooltip names the action).
        let pill = HoverRow::new(label)
            .with_padding(EdgeInsets::new(3.0, 10.0, 3.0, 10.0))
            .with_corner_radius(8.0)
            .with_on_click(move || (on_start.borrow_mut())(id.clone()))
            .finish();
        let pill = ConstrainedBox::new(pill).with_height(26.0).finish();
        let pill = Tooltip::new(pill, "Edit Conversation Title")
            .with_position(TooltipPosition::Below)
            .finish();
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(pill)
            .with_child(Spacer::new().finish())
            .finish()
    };
    let on_rename = actions.on_inline_title_rename.clone();
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
    // The top level keeps only leaf actions and the two rows that open
    // flyouts; their options live in the flyout, not inline (DSH's behaviour).
    let entry = state
        .conversations
        .iter()
        .find(|entry| entry.id == selected)
        .cloned();
    let conversation_name = entry
        .as_ref()
        .map(|entry| entry.name.clone())
        .unwrap_or_default();
    let project_name = entry
        .as_ref()
        .map(|entry| {
            if !entry.folder.trim().is_empty() {
                entry.folder.clone()
            } else {
                std::path::Path::new(&entry.directory)
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default()
            }
        })
        .unwrap_or_default();
    let copy_value = actions.on_copy_menu_value.clone();
    let copy_split_right = on_split_right.clone();
    let copy_split_down = on_split_down.clone();
    let copy_create = on_create.clone();
    let copy_name_action = copy_value.clone();
    let copy_id_action = copy_value.clone();
    let copy_project_action = copy_value;
    let name_for_copy = conversation_name.clone();
    let id_for_copy = selected.clone();
    let project_for_copy = project_name.clone();
    let split_children = vec![
        PopupMenuItem::new("Split right")
            .with_shortcut("⌘ Space")
            .with_on_select(move || (copy_split_right.borrow_mut())()),
        PopupMenuItem::new("Split down")
            .with_shortcut("⌘ ⇧ D")
            .with_on_select(move || (copy_split_down.borrow_mut())()),
        PopupMenuItem::new("Replace with new").with_on_select(move || (copy_create.borrow_mut())()),
    ];
    let mut name_item = PopupMenuItem::new("Conversation name").with_on_select(move || {
        (copy_name_action.borrow_mut())("Conversation name".to_string(), name_for_copy.clone())
    });
    if conversation_name.trim().is_empty() {
        name_item = name_item.disabled();
    }
    let id_available = !id_for_copy.trim().is_empty();
    let mut id_item = PopupMenuItem::new("Conversation id").with_on_select(move || {
        (copy_id_action.borrow_mut())("Conversation id".to_string(), id_for_copy.clone())
    });
    if !id_available {
        id_item = id_item.disabled();
    }
    let mut project_item = PopupMenuItem::new("Project name").with_on_select(move || {
        (copy_project_action.borrow_mut())("Project name".to_string(), project_for_copy.clone())
    });
    if project_name.trim().is_empty() {
        project_item = project_item.disabled();
    }
    let copy_children = vec![name_item, id_item, project_item];
    let more_items = vec![
        PopupMenuItem::new("Rename").with_shortcut("⌘ R"),
        PopupMenuItem::new(pin_label),
        PopupMenuItem::new("Archive").with_shortcut("⌘ ⇧ A"),
        PopupMenuItem::new("Split")
            .with_icon("split-horizontal")
            .with_children(split_children)
            .with_separator_before(),
        PopupMenuItem::new("Copy")
            .with_icon("copy")
            .with_children(copy_children),
        PopupMenuItem::new("Close pane").with_separator_before(),
    ];
    let (submenu_index, submenu_highlight, submenu_keyboard) = more_submenu_state();
    let more = PopupMenu::new(button(app, "dots-vertical", "More"), more_items)
        .with_open(state.chrome_more_open.clone())
        .with_position(PopupMenuPosition::BelowEnd)
        .with_max_width(240.0)
        .with_window_width(state.window_size.x)
        .with_submenu_state(submenu_index, submenu_highlight, submenu_keyboard)
        .with_on_select(move |index| match index {
            0 => (on_rename.borrow_mut())(id.clone()),
            1 => (on_pin.borrow_mut())(id.clone()),
            2 => (on_archive.borrow_mut())(close_id.clone()),
            3 => (on_close.borrow_mut())(),
            _ => {}
        })
        .finish();
    // The panel must not be covered by the panes painted after the chrome.
    let mut more = Some(TopLayer::new(more).finish());
    let panel_open = state.right_sidebar_open;

    // Panel 1's trailing controls are the More menu and the Overview popover
    // only. Maximize/collapse and the side-panel toggle moved into panel 2's
    // section below (the panel's own section owns its window controls).
    let overview_anchor = Rc::new(RefCell::new(None::<RectF>));
    let overview_open = state.chrome_overview_open.clone();
    let overview_active = *overview_open.borrow();
    let toggle_overview = overview_open.clone();
    let overview_button = Tooltip::new(
        TopbarButton::new(glyph(app, "flat-list"))
            .with_size(CONTROL_BUTTON_SIZE)
            .with_corner_radius(8.0)
            .with_active(overview_active)
            .with_on_click(move || {
                let mut open = toggle_overview.borrow_mut();
                *open = !*open;
            })
            .finish(),
        "Overview",
    )
    .with_position(TooltipPosition::Below)
    .finish();
    let overview_button = AnchorProbe::new(overview_button, overview_anchor.clone()).finish();

    // Panel 1 always keeps its trailing controls — the 3-dot More menu and the
    // Overview launcher — whether the side panel is open or not: the agent's
    // own topbar must never lose them.
    let mut main_section = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(CONTROL_SPACING)
        .with_child(
            Expanded::new(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(title)
                    .finish(),
            )
            .finish(),
        );
    // Panel 1's trailing controls are always its own two icons — the More menu
    // and the Overview launcher — whether the side panel is open or closed.
    main_section = main_section
        .with_child(more.take().expect("the More menu"))
        .with_child(overview_button);
    // Panel 1 keeps the control that OPENS the side panel: with the panel
    // closed nothing in the strip could bring it back (the close control lives
    // in panel 2's section, which is only built while the panel is open).
    if !panel_open {
        let on_open = actions.on_toggle_right_sidebar.clone();
        let open_button = Tooltip::new(
            TopbarButton::new(
                Icon::new("panel-right-outline")
                    .with_size(CONTROL_ICON_SIZE)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_size(CONTROL_BUTTON_SIZE)
            .with_corner_radius(8.0)
            .with_on_click(move || (on_open.borrow_mut())())
            .finish(),
            "Open side panel",
        )
        .with_position(TooltipPosition::Below)
        .finish();
        main_section = main_section.with_child(open_button);
    }
    let main_section = main_section.finish();
    let main_section = Container::new(main_section)
        .with_padding(EdgeInsets::new(
            MAIN_SECTION_LEFT_PADDING,
            SECTION_TOP_PADDING,
            MAIN_SECTION_RIGHT_PADDING,
            SECTION_VERTICAL_PADDING,
        ))
        .finish();

    // The title starts at its panel's own left edge: the separator that
    // used to sit between the sidebar section and the main section is gone.
    let mut strip = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(leading)
        .with_child(Expanded::new(main_section).finish());
    if panel_open {
        let maximized = state.side_panel_maximized;
        // The chip strip and its `+` are one measured row: the `+` sits
        // immediately after the last chip and travels/scrolls with it, so it is
        // never pinned over the window controls. Only the maximize/toggle
        // controls stay at the far right. With no open tab the section shows
        // nothing but those controls: Overview is the panel-1 launcher, not a
        // chip.
        let has_tabs = state
            .side_tabs
            .iter()
            .any(|kind| *kind != SideTabKind::Overview);
        let tab_strip: Box<dyn Element> = if has_tabs {
            TabStripScroller::new(app, state, actions).finish()
        } else {
            Empty::new().finish()
        };
        let on_close = actions.on_toggle_right_sidebar.clone();
        // The general topbar keeps only the chips plus maximize and the
        // side-panel toggle; the review's own icons live in the review row.
        let on_max = actions.on_toggle_side_panel_maximized.clone();
        let max_button = Tooltip::new(
            TopbarButton::new(
                Icon::new(if maximized {
                    "fullscreen-exit"
                } else {
                    "fullscreen-enter"
                })
                .with_size(CONTROL_ICON_SIZE)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
            )
            .with_size(CONTROL_BUTTON_SIZE)
            .with_corner_radius(8.0)
            .with_on_click(move || (on_max.borrow_mut())())
            .finish(),
            if maximized {
                "Restore side panel"
            } else {
                "Maximize side panel"
            },
        )
        .with_position(TooltipPosition::Below)
        .finish();
        let close_button = Tooltip::new(
            TopbarButton::new(
                Icon::new("panel-right-outline")
                    .with_size(CONTROL_ICON_SIZE)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_size(CONTROL_BUTTON_SIZE)
            .with_corner_radius(8.0)
            .with_active(true)
            .with_on_click(move || (on_close.borrow_mut())())
            .finish(),
            "Toggle side panel",
        )
        .with_position(TooltipPosition::Below)
        .finish();
        let side_section = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(CONTROL_SPACING)
            .with_child(Expanded::new(tab_strip).finish())
            .with_child(max_button)
            .with_child(close_button)
            .finish();
        let side_section = Container::new(side_section)
            .with_padding(EdgeInsets::new(
                SIDE_SECTION_PADDING,
                SECTION_TOP_PADDING,
                SIDE_SECTION_PADDING,
                SECTION_VERTICAL_PADDING,
            ))
            .finish();
        // The strip's panel-2 section must start on the body's own split
        // boundary, so the topbar rule and the panel hairline are the same
        // vertical line. Mirror the body's geometry exactly: the sheet keeps
        // `WORKSPACE_MARGIN` inside the shell (sidebar or window edge), the pane
        // area cancels its right inset by `PANE_INSET + 1`, and the split takes
        // its ratio of that area.
        let window_width = state.window_size.x;
        let body_sidebar_width = if sidebar_shown {
            state.sidebar_width
        } else {
            0.0
        };
        let area_width =
            (window_width - body_sidebar_width - 2.0 * crate::ui::shell::WORKSPACE_MARGIN).max(0.0);
        let side_width = area_width
            * crate::ui::build::resolved_side_split_ratio(state.side_split_ratio, area_width);
        let boundary_x =
            body_sidebar_width + crate::ui::shell::WORKSPACE_MARGIN + (area_width - side_width);
        let side_section_width = (window_width - boundary_x - SEPARATOR_WIDTH).max(0.0);
        // The first frame (and a headless test) can build before the window
        // measures itself: fall back to the flexible section until the width is
        // known, so the chips are never laid out at zero width.
        let side_section = if window_width > 0.0 {
            ConstrainedBox::new(side_section)
                .with_width(side_section_width)
                .finish()
        } else {
            Expanded::new(side_section).finish()
        };
        // 206: one 1 px rule separates panel 1 from panel 2.
        strip = strip
            .with_child(vertical_separator(app))
            .with_child(side_section);
    }
    // The strip is the full bar height; each section carries the 2 pt quiet band
    // as its own top inset, so its controls center in the 40 pt content band.
    // The traffic lights span the full bar and center on its middle line.
    let strip = ConstrainedBox::new(strip.finish())
        .with_height(CHROME_HEIGHT)
        .finish();
    let row = Container::new(strip)
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .finish();

    // The Overview panel is a real anchored popover, not a menu bolted onto the
    // button: the button records its rect and the popover lays itself below it.
    // It is queued into the window's last-painted hover layer, so it paints
    // above the body, the sidebar and the side panel rather than being covered
    // by the next sibling.
    let close_overview: Rc<RefCell<dyn FnMut()>> = {
        let open = state.chrome_overview_open.clone();
        Rc::new(RefCell::new(move || *open.borrow_mut() = false))
    };
    let (popover_content, popover_actions) =
        build_overview_popover_content(app, state, actions, close_overview.clone());
    let popover = OverviewPopover::new(
        state.chrome_overview_open.clone(),
        overview_anchor,
        popover_content,
        popover_actions,
        close_overview,
    )
    .finish();
    Stack::new()
        .with_children(vec![row])
        .with_overlay(popover, vec2f(0.0, 0.0))
        .finish()
}

fn traffic_lights(app: &AppContext) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(TRAFFIC_LIGHT_GAP);
    // A zero-width sizer fixes the row's height at the dot size on macOS, where
    // no preview dots are drawn, so the band is always CHROME_HEIGHT tall.
    if cfg!(target_os = "macos") {
        row = row.with_child(
            Empty::new()
                .with_size(vec2f(0.0, TRAFFIC_LIGHT_SIZE))
                .finish(),
        );
    }
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
    //
    // The vertical padding makes this band exactly CHROME_HEIGHT tall, so the
    // leading flex centers the 12 pt dots on the full bar's middle line rather
    // than pinning them to the top or to the controls' content band.
    let vertical = (CHROME_HEIGHT - TRAFFIC_LIGHT_SIZE) / 2.0;
    ConstrainedBox::new(
        Container::new(row.finish())
            .with_padding(EdgeInsets::new(TRAFFIC_INSET, vertical, 0.0, vertical))
            .finish(),
    )
    .with_width(TRAFFIC_INSET + traffic_preview_width())
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
/// height is the full content band, starting under the 2 pt quiet band, so the
/// partitions meet the content sheet with one continuous line.
fn vertical_separator(app: &AppContext) -> Box<dyn Element> {
    Container::new(
        Container::new(
            Empty::new()
                .with_size(vec2f(SEPARATOR_WIDTH, CHROME_CONTENT_HEIGHT))
                .finish(),
        )
        .with_background(Fill::Solid(app.theme.color(ColorToken::Border)))
        .finish(),
    )
    .with_padding(EdgeInsets::new(0.0, CHROME_TOP_PADDING, 0.0, 0.0))
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

/// The side-panel section's "+": a popup menu that appends one of the active
/// pane's surfaces to the tab strip. Selecting a kind that is already a tab
/// shows that tab instead of stacking a duplicate.
fn side_tab_add_menu(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let on_add = actions.on_side_tab.clone();
    let items = vec![
        PopupMenuItem::new(SideTabKind::Agent.title()).with_icon(SideTabKind::Agent.icon()),
        PopupMenuItem::new(SideTabKind::Files.title()).with_icon(SideTabKind::Files.icon()),
        PopupMenuItem::new(SideTabKind::Terminal.title()).with_icon(SideTabKind::Terminal.icon()),
        PopupMenuItem::new(SideTabKind::TaskOutput.title())
            .with_icon(SideTabKind::TaskOutput.icon()),
    ];
    let menu = PopupMenu::new(button(app, "plus", "Add tab"), items)
        .with_open(state.side_tab_add_menu_open.clone())
        .with_position(PopupMenuPosition::BelowEnd)
        .with_max_width(200.0)
        .with_on_select(move |index| {
            let kind = match index {
                0 => SideTabKind::Agent,
                1 => SideTabKind::Files,
                2 => SideTabKind::Terminal,
                3 => SideTabKind::TaskOutput,
                _ => return,
            };
            (on_add.borrow_mut())(SideTabAction::Add(kind));
        })
        .finish();
    // The `+` panel is above the panes, like the More menu.
    TopLayer::new(menu).finish()
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

/// The chip strip's width measurements, used to reveal the tab the state says
/// is selected. These mirror `side_tabs.rs`'s chip geometry: the strip measures
/// each chip at its natural label width, then clips and offsets the whole row.
const TAB_STRIP_GAP: f32 = 4.0;
/// The chip row's height: the strip centres it on the bar's middle line.
const TAB_STRIP_CHIP_HEIGHT: f32 = 30.0;
const TAB_STRIP_ICON: f32 = 13.0;
const TAB_STRIP_PADDING_X: f32 = 8.0;
const TAB_STRIP_LABEL_GAP: f32 = 6.0;
const TAB_STRIP_CLOSE: f32 = 14.0;
const TAB_STRIP_CLOSE_GAP: f32 = 4.0;

/// The panel-2 tab strip: the measured chip row inside a clipped, horizontally
/// scrollable viewport. It reveals the selected chip when the selection or the
/// viewport changes, so choosing an off-screen tab still shows its chip; a
/// wheel after that moves the offset freely (the reveal does not re-run until
/// the selection changes again).
struct TabStripScroller {
    scrollable: Scrollable,
    add: Box<dyn Element>,
    widths: Vec<f32>,
    active: usize,
    tabs: usize,
    scroll: Rc<RefCell<ScrollState>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    scroll_size: Vector2F,
}

/// The gap between the chips' viewport and the fixed `+` control.
const TAB_STRIP_ADD_GAP: f32 = 4.0;

impl TabStripScroller {
    fn new(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Self {
        let mut widths = Vec::new();
        let mut active = 0;
        for (index, kind) in state.side_tabs.iter().enumerate() {
            if *kind == SideTabKind::Overview {
                continue;
            }
            if index == state.active_side_tab {
                active = widths.len();
            }
            let label = advance_width(
                kind.title(),
                12.5,
                FontWeight::Medium,
                FontFamily::System,
                false,
            );
            widths.push(
                TAB_STRIP_PADDING_X * 2.0
                    + TAB_STRIP_ICON
                    + TAB_STRIP_LABEL_GAP
                    + label
                    + TAB_STRIP_CLOSE_GAP
                    + TAB_STRIP_CLOSE,
            );
        }
        let tabs = widths.len();
        let scroll = side_tab_scroll();
        // Only the chips live inside the clipped, scrollable viewport. The `+`
        // control is a sibling OUTSIDE it: it keeps its place right beside the
        // visible chips until the room runs out, and its add menu is never
        // clipped by the strip's viewport (it used to scroll out of view and
        // paint behind the window controls).
        let scrollable = Scrollable::new(
            side_tabs::build_tab_strip(app, state, actions),
            Axis::Horizontal,
        )
        .with_state(Rc::clone(&scroll));
        let add = side_tab_add_menu(app, state, actions);
        Self {
            scrollable,
            add,
            widths,
            active,
            tabs,
            scroll,
            size: None,
            origin: None,
            scroll_size: Vector2F::zero(),
        }
    }

    /// The natural width of the chip row, gaps included.
    fn chips_width(&self) -> f32 {
        if self.tabs == 0 {
            0.0
        } else {
            self.widths.iter().sum::<f32>() + TAB_STRIP_GAP * (self.tabs - 1) as f32
        }
    }

    fn reveal_selected(&self, viewport: f32) {
        if viewport <= 0.0 || self.widths.is_empty() {
            return;
        }
        let key = (self.active, self.tabs, viewport);
        let already = SIDE_TAB_REVEAL.with(|last| last.get() == Some(key));
        if already {
            return;
        }
        SIDE_TAB_REVEAL.with(|last| last.set(Some(key)));

        let mut state = self.scroll.borrow_mut();
        let current = state.offset();
        let max = state.max_offset();
        let start =
            self.widths.iter().take(self.active).sum::<f32>() + TAB_STRIP_GAP * self.active as f32;
        let end = start + self.widths.get(self.active).copied().unwrap_or(0.0);
        let target = if start < current {
            start
        } else if end > current + viewport {
            end - viewport
        } else {
            current
        }
        .clamp(0.0, max);
        if (target - current).abs() > 0.5 {
            state.scroll_by(target - current);
        }
    }
}

impl Element for TabStripScroller {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let add_size = self.add.layout(
            SizeConstraint::new(Vector2F::zero(), constraint.max),
            ctx,
            app,
        );
        let height = constraint.max.y.max(add_size.y);
        // Chips centre on the bar's own middle line: the scroll viewport is the
        // chip row's height, not the full strip.
        let chips_height = TAB_STRIP_CHIP_HEIGHT.min(height);
        let viewport_width = (constraint.max.x - add_size.x - TAB_STRIP_ADD_GAP).max(0.0);
        self.scroll_size = self.scrollable.layout(
            SizeConstraint::new(
                vec2f(viewport_width, chips_height),
                vec2f(viewport_width, chips_height),
            ),
            ctx,
            app,
        );
        self.reveal_selected(self.scroll_size.x);
        let size = vec2f(
            (self.scroll_size.x + TAB_STRIP_ADD_GAP + add_size.x).min(constraint.max.x),
            height,
        );
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let chips_y = origin.y + ((size.y - self.scroll_size.y) / 2.0).max(0.0);
        self.scrollable.paint(vec2f(origin.x, chips_y), ctx, app);
        let add_size = self.add.size().unwrap_or(Vector2F::zero());
        // The + sits right beside the visible chips; only when they overflow
        // the viewport is it pushed to the viewport's fixed trailing edge.
        let room = (size.x - add_size.x - TAB_STRIP_ADD_GAP).max(0.0);
        let add_x = origin.x + self.chips_width().min(room) + TAB_STRIP_ADD_GAP;
        self.add.paint(
            vec2f(add_x, origin.y + (size.y - add_size.y) / 2.0),
            ctx,
            app,
        );
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        if self.scrollable.dispatch_event(event, ctx, app) {
            return true;
        }
        self.add.dispatch_event(event, ctx, app)
    }
}

/// Records the rect the Overview trigger painted at. The popover is anchored
/// to the control that opens it, and the control paints before the popover in
/// the same frame.
struct AnchorProbe {
    child: Box<dyn Element>,
    rect: Rc<RefCell<Option<RectF>>>,
}

impl AnchorProbe {
    fn new(child: Box<dyn Element>, rect: Rc<RefCell<Option<RectF>>>) -> Self {
        Self { child, rect }
    }
}

impl Element for AnchorProbe {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.child.layout(constraint, ctx, app)
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        let size = self.child.size().unwrap_or(Vector2F::zero());
        *self.rect.borrow_mut() = Some(RectF::new(
            PointF::new(origin.x, origin.y),
            Size2F::new(size.x, size.y),
        ));
        self.child.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.child.size()
    }

    fn origin(&self) -> Option<Point> {
        self.child.origin()
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.child.dispatch_event(event, ctx, app)
    }
}

/// One row of the Overview popover that the keyboard can land on.
struct OverviewAction {
    run: Rc<RefCell<dyn FnMut()>>,
    /// A section fold stays open; an action row closes the popover after it
    /// runs, the way choosing a menu item does.
    closes: bool,
}

/// The Overview popover's anchored panel.
///
/// The panel is queued into `AppContext::hover_chips`, the window's last paint,
/// so it is drawn above the shell body rather than under the next sibling. The
/// element itself has the panel's laid-out size so `bounds()` is the panel's
/// rect, but `Stack` overlays do not grow the strip.
struct OverviewPopover {
    open: Rc<RefCell<bool>>,
    anchor: Rc<RefCell<Option<RectF>>>,
    content: Rc<RefCell<Box<dyn Element>>>,
    actions: Vec<OverviewAction>,
    on_close: Rc<RefCell<dyn FnMut()>>,
    window: Option<Vector2F>,
    pointer_inside: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl OverviewPopover {
    fn new(
        open: Rc<RefCell<bool>>,
        anchor: Rc<RefCell<Option<RectF>>>,
        content: Rc<RefCell<Box<dyn Element>>>,
        actions: Vec<OverviewAction>,
        on_close: Rc<RefCell<dyn FnMut()>>,
    ) -> Self {
        Self {
            open,
            anchor,
            content,
            actions,
            on_close,
            window: None,
            pointer_inside: false,
            size: None,
            origin: None,
        }
    }

    fn width(&self) -> f32 {
        match self.window {
            Some(window) => {
                OVERVIEW_POPOVER_WIDTH.min((window.x - OVERVIEW_EDGE_MARGIN * 2.0).max(220.0))
            }
            None => OVERVIEW_POPOVER_WIDTH,
        }
    }

    fn cap(&self) -> Option<f32> {
        self.window
            .map(|window| (window.y * OVERVIEW_MAX_HEIGHT_FRACTION).max(160.0))
    }

    /// Below the trigger and trailing-aligned with it, then clamped inside the
    /// window's edges. `None` for the window (an offline tree laid out loose)
    /// means there is no edge to clamp against.
    fn place(&self, size: Vector2F) -> Vector2F {
        let anchor = *self.anchor.borrow();
        let Some(window) = self.window else {
            return match anchor {
                Some(anchor) => vec2f(
                    anchor.max_x() - size.x,
                    anchor.max_y() + OVERVIEW_POPOVER_GAP,
                ),
                None => vec2f(0.0, CHROME_HEIGHT + OVERVIEW_POPOVER_GAP),
            };
        };
        let (x, y) = match anchor {
            Some(anchor) => (
                anchor.max_x() - size.x,
                anchor.max_y() + OVERVIEW_POPOVER_GAP,
            ),
            None => (
                window.x - size.x - OVERVIEW_EDGE_MARGIN,
                CHROME_HEIGHT + OVERVIEW_POPOVER_GAP,
            ),
        };
        vec2f(
            x.min(window.x - size.x - OVERVIEW_EDGE_MARGIN)
                .max(OVERVIEW_EDGE_MARGIN),
            y.min(window.y - size.y - OVERVIEW_EDGE_MARGIN)
                .max(OVERVIEW_EDGE_MARGIN),
        )
    }

    fn close(&self) {
        (self.on_close.borrow_mut())();
        OVERVIEW_HIGHLIGHT.with(|highlight| highlight.set(0));
        overview_scroll().borrow_mut().reset();
    }

    fn handle_key(&mut self, event: &DispatchedEvent) -> bool {
        if !*self.open.borrow() {
            return false;
        }
        let DispatchedEvent::KeyDown { key, modifiers } = event else {
            return false;
        };
        // A chorded key belongs to the workspace behind the popover; only the
        // popover's own navigation keys are taken.
        if modifiers.command || modifiers.ctrl || modifiers.alt {
            return false;
        }
        match key.as_str() {
            "Escape" => {
                self.close();
                true
            }
            "ArrowDown" | "ArrowUp" => {
                let len = self.actions.len();
                if len == 0 {
                    return true;
                }
                let delta = if key == "ArrowDown" { 1 } else { -1 };
                let current = OVERVIEW_HIGHLIGHT.with(Cell::get).min(len - 1) as isize;
                let next = (current + delta).rem_euclid(len as isize) as usize;
                OVERVIEW_HIGHLIGHT.with(|highlight| highlight.set(next));
                true
            }
            "Enter" => {
                let len = self.actions.len();
                if len == 0 {
                    return true;
                }
                let index = OVERVIEW_HIGHLIGHT.with(Cell::get).min(len - 1);
                let action = &self.actions[index];
                (action.run.borrow_mut())();
                if action.closes {
                    self.close();
                }
                true
            }
            _ => false,
        }
    }
}

impl Element for OverviewPopover {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.window = constraint.max.x.is_finite().then_some(constraint.max);
        if !*self.open.borrow() {
            self.pointer_inside = false;
            OVERVIEW_HIGHLIGHT.with(|highlight| highlight.set(0));
            overview_scroll().borrow_mut().reset();
            self.size = None;
            return Vector2F::zero();
        }
        let width = self.width();
        let cap = self
            .cap()
            .filter(|cap| cap.is_finite())
            .unwrap_or(f32::INFINITY);
        let size = self.content.borrow_mut().layout(
            SizeConstraint::new(vec2f(width, 0.0), vec2f(width, cap)),
            ctx,
            app,
        );
        self.size = Some(size);
        size
    }

    fn paint(&mut self, _origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        if !*self.open.borrow() {
            self.size = None;
            return;
        }
        let Some(size) = self.size else {
            return;
        };
        let at = self.place(size);
        self.origin = Some(Point::from_vec2f(at, Default::default()));
        let panel = RectF::new(PointF::new(at.x, at.y), Size2F::new(size.x, size.y));
        self.pointer_inside = ctx.cursor_inside && contains(panel, ctx.cursor_position);
        // The popover is one of the window's menus: it claims the shared open
        // state, so opening another menu closes it and an outside press or
        // Escape is handled by the root before the tree.
        if app.open_menus.id() != Some(OVERVIEW_MENU_ID) {
            app.open_menus.open(OVERVIEW_MENU_ID, self.on_close.clone());
        }
        app.open_menus.report(
            OVERVIEW_MENU_ID,
            self.on_close.clone(),
            vec![panel],
            MenuPointer::Paint,
        );
        if let Some(anchor) = *self.anchor.borrow() {
            app.open_menus.report_anchor(OVERVIEW_MENU_ID, anchor);
        }
        // The layer paints it last, after the whole shell; the owner keeps the
        // handle for layout and events.
        app.hover_chips
            .borrow_mut()
            .push_shared(at, Rc::clone(&self.content));
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        if !*self.open.borrow() {
            return false;
        }
        if matches!(event, DispatchedEvent::KeyDown { .. }) {
            return self.handle_key(event);
        }
        // Wheel events carry no position: only the panel under the pointer
        // receives them, so a wheel over the workspace keeps going to the
        // workspace.
        if let DispatchedEvent::Scroll { .. } = event {
            if !self.pointer_inside {
                return false;
            }
            return self.content.borrow_mut().dispatch_event(event, ctx, app);
        }
        let Some(position) = event_position(event) else {
            return false;
        };
        let Some(panel) = self.bounds() else {
            return false;
        };
        if contains(panel, position) {
            let consumed = self.content.borrow_mut().dispatch_event(event, ctx, app);
            return consumed
                || matches!(
                    event,
                    DispatchedEvent::MouseDown { .. } | DispatchedEvent::MouseUp { .. }
                );
        }
        // The trigger owns its own press: it toggles the popover through the
        // button instead of being swallowed here.
        if self
            .anchor
            .borrow()
            .is_some_and(|anchor| contains(anchor, position))
        {
            return false;
        }
        if matches!(
            event,
            DispatchedEvent::MouseDown {
                button: BUTTON_PRIMARY,
                ..
            }
        ) {
            self.close();
        }
        false
    }
}

/// Paints its child in the window's last-painted layer.
///
/// The topbar is painted before the body, so a `PopupMenu` mounted in the
/// chrome would have its panel covered by the panes and the side panel (the
/// owner reported the More and `+` menus opening behind them in the real app;
/// the previews only looked right because they reordered the command list).
/// The trigger still lays out in the chrome, while the child paints through
/// `AppContext::hover_chips` — the same layer the Overview popover uses — so
/// the panel is above every pane, panel and overlay while it stays in the tree
/// for hit-testing and keyboard routing.
pub(crate) struct TopLayer {
    inner: Rc<RefCell<Box<dyn Element>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TopLayer {
    pub(crate) fn new(inner: Box<dyn Element>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(inner)),
            size: None,
            origin: None,
        }
    }
}

impl Element for TopLayer {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.inner.borrow_mut().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, _ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        app.hover_chips
            .borrow_mut()
            .push_shared(origin, Rc::clone(&self.inner));
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn wants_animation(&self) -> bool {
        self.inner.borrow().wants_animation()
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.inner.borrow_mut().dispatch_event(event, ctx, app)
    }
}

/// The pointer position a mouse event carries, if any.
fn event_position(event: &DispatchedEvent) -> Option<Vector2F> {
    match event {
        DispatchedEvent::MouseDown { position, .. }
        | DispatchedEvent::MouseUp { position, .. }
        | DispatchedEvent::MouseMove { position } => Some(*position),
        _ => None,
    }
}

fn section_collapsed(state: &UiSnapshot, key: &str) -> bool {
    state
        .collapsed_sections
        .iter()
        .any(|section| section == key)
}

fn overview_popover_header(
    app: &AppContext,
    on_close: Rc<RefCell<dyn FnMut()>>,
) -> Box<dyn Element> {
    let close = TopbarButton::new(
        Icon::new("x-close")
            .with_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(22.0)
    .with_corner_radius(6.0)
    .with_on_click(move || (on_close.borrow_mut())())
    .finish();
    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.0)
            .with_child(
                Text::new("Overview")
                    .with_font_size(13.0)
                    .with_weight(FontWeight::Medium)
                    .with_theme_color(ColorToken::Text, app)
                    .with_max_lines(1)
                    .finish(),
            )
            .with_child(Spacer::new().finish())
            .with_child(close)
            .finish(),
    )
    .with_padding(EdgeInsets::new(2.0, 0.0, 2.0, 6.0))
    .finish()
}

fn overview_row_content(
    app: &AppContext,
    icon: &'static str,
    label: &str,
    trailing: Option<String>,
    label_color: ColorToken,
) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0)
        .with_child(
            Icon::new(icon)
                .with_size(14.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Expanded::new(
                Text::new(label.to_string())
                    .with_font_size(12.5)
                    .with_theme_color(label_color, app)
                    .with_max_lines(1)
                    .finish(),
            )
            .finish(),
        );
    if let Some(trailing) = trailing.filter(|value| !value.trim().is_empty()) {
        row = row.with_child(
            Text::new(trailing)
                .with_font_size(10.5)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(1)
                .finish(),
        );
    }
    row.finish()
}

fn overview_action_row(
    app: &AppContext,
    icon: &'static str,
    label: &str,
    selected: bool,
    run: Rc<RefCell<dyn FnMut()>>,
    closes: bool,
    on_close: Rc<RefCell<dyn FnMut()>>,
) -> (Box<dyn Element>, OverviewAction) {
    let content = overview_row_content(app, icon, label, None, ColorToken::Text);
    let click_run = Rc::clone(&run);
    let click_close = Rc::clone(&on_close);
    let row = HoverRow::new(content)
        .with_selected(selected)
        .with_padding(EdgeInsets::new(6.0, 5.0, 6.0, 5.0))
        .with_corner_radius(6.0)
        .with_on_click(move || {
            (click_run.borrow_mut())();
            if closes {
                (click_close.borrow_mut())();
            }
        })
        .finish();
    (row, OverviewAction { run, closes })
}

fn overview_section_header(
    app: &AppContext,
    label: &str,
    count: usize,
    collapsed: bool,
    selected: bool,
    key: &'static str,
    on_toggle: Rc<RefCell<dyn FnMut(String)>>,
) -> (Box<dyn Element>, OverviewAction) {
    let run: Rc<RefCell<dyn FnMut()>> = {
        let on_toggle = Rc::clone(&on_toggle);
        let key = key.to_string();
        Rc::new(RefCell::new(move || (on_toggle.borrow_mut())(key.clone())))
    };
    let click_run = Rc::clone(&run);
    let content = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(
            Icon::new(if collapsed {
                "chevron-right"
            } else {
                "chevron-down"
            })
            .with_size(11.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        )
        .with_child(
            Text::new(label.to_string())
                .with_font_size(11.0)
                .with_weight(FontWeight::Medium)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(1)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(
            Text::new(count.to_string())
                .with_font_size(10.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish();
    let row = HoverRow::new(content)
        .with_selected(selected)
        .with_padding(EdgeInsets::new(8.0, 4.0, 6.0, 4.0))
        .with_corner_radius(6.0)
        .with_on_click(move || (click_run.borrow_mut())())
        .finish();
    (row, OverviewAction { run, closes: false })
}

fn overview_empty_line(app: &AppContext, text: &str) -> Box<dyn Element> {
    Container::new(
        Text::new(text.to_string())
            .with_font_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .with_max_lines(1)
            .finish(),
    )
    .with_padding(EdgeInsets::new(24.0, 3.0, 6.0, 5.0))
    .finish()
}

fn overview_info_row(app: &AppContext, icon: &'static str, label: &str) -> Box<dyn Element> {
    Container::new(overview_row_content(
        app,
        icon,
        label,
        None,
        ColorToken::Text,
    ))
    .with_padding(EdgeInsets::new(24.0, 4.0, 6.0, 4.0))
    .finish()
}

/// The Overview popover's body: the active pane's real Review/Files/Terminal
/// actions, then the live background-task and sub-agent rows and the registered
/// workflows. The last three groups fold through the same app-owned
/// `collapsed_sections` cell the sidebar uses.
fn build_overview_popover_content(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    on_close: Rc<RefCell<dyn FnMut()>>,
) -> (Rc<RefCell<Box<dyn Element>>>, Vec<OverviewAction>) {
    let highlight = OVERVIEW_HIGHLIGHT.with(Cell::get);
    let active = state.active_pane_id;
    let mut flat: Vec<OverviewAction> = Vec::new();
    let mut body = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.0)
        .with_child(overview_popover_header(app, Rc::clone(&on_close)));

    let on_diff = actions.on_diff_panel.clone();
    let review = Rc::new(RefCell::new(move || {
        (on_diff.borrow_mut())(active, DiffPanelAction::Open(None))
    }));
    let (row, action) = overview_action_row(
        app,
        "file",
        "Review",
        flat.len() == highlight,
        review,
        true,
        Rc::clone(&on_close),
    );
    body = body.with_child(row);
    flat.push(action);

    let on_files_tab = actions.on_open_files_tab.clone();
    let files = Rc::new(RefCell::new(move || (on_files_tab.borrow_mut())()));
    let (row, action) = overview_action_row(
        app,
        "folder",
        "Files",
        flat.len() == highlight,
        files,
        true,
        Rc::clone(&on_close),
    );
    body = body.with_child(row);
    flat.push(action);

    let on_terminal = actions.on_toggle_terminal_dock.clone();
    let terminal = Rc::new(RefCell::new(move || (on_terminal.borrow_mut())(active)));
    let (row, action) = overview_action_row(
        app,
        "terminal",
        "Terminal",
        flat.len() == highlight,
        terminal,
        true,
        Rc::clone(&on_close),
    );
    body = body.with_child(row);
    flat.push(action);

    let on_toggle_section = actions.on_toggle_section.clone();
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
    if !tasks.is_empty() {
        let tasks_collapsed = section_collapsed(state, OVERVIEW_TASKS_KEY);
        let (header, action) = overview_section_header(
            app,
            "Background Tasks",
            tasks.len(),
            tasks_collapsed,
            flat.len() == highlight,
            OVERVIEW_TASKS_KEY,
            Rc::clone(&on_toggle_section),
        );
        body = body.with_child(header);
        flat.push(action);
        if !tasks_collapsed {
            for task in tasks {
                let label = if task.description.trim().is_empty() {
                    "Background task"
                } else {
                    task.description.as_str()
                };
                body = body.with_child(overview_info_row(app, "terminal-input", label));
            }
        }
    }

    let mut subagents: Vec<_> = state
        .pane_chat
        .get(&active)
        .map(|pane| pane.sub_agents.values().cloned().collect())
        .unwrap_or_default();
    subagents.sort_by(|left, right| {
        left.description
            .cmp(&right.description)
            .then_with(|| left.child_id.cmp(&right.child_id))
    });
    if !subagents.is_empty() {
        let subagents_collapsed = section_collapsed(state, OVERVIEW_SUBAGENTS_KEY);
        let (header, action) = overview_section_header(
            app,
            "Subagents",
            subagents.len(),
            subagents_collapsed,
            flat.len() == highlight,
            OVERVIEW_SUBAGENTS_KEY,
            Rc::clone(&on_toggle_section),
        );
        body = body.with_child(header);
        flat.push(action);
        if !subagents_collapsed {
            for subagent in subagents {
                let label = if subagent.description.trim().is_empty() {
                    subagent.subagent_type.clone()
                } else {
                    subagent.description.clone()
                };
                let on_open = actions.on_open_sub_agent.clone();
                let child = subagent.child_id.clone();
                let run: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(move || {
                    (on_open.borrow_mut())(active, child.clone())
                }));
                let selected = flat.len() == highlight;
                let (row, action) = overview_action_row(
                    app,
                    "message-plus-square",
                    &label,
                    selected,
                    run,
                    true,
                    Rc::clone(&on_close),
                );
                body = body.with_child(row);
                flat.push(action);
            }
        }
    }

    let workflows_collapsed = section_collapsed(state, OVERVIEW_WORKFLOWS_KEY);
    let (header, action) = overview_section_header(
        app,
        "Workflows",
        state.workflows.len(),
        workflows_collapsed,
        flat.len() == highlight,
        OVERVIEW_WORKFLOWS_KEY,
        Rc::clone(&on_toggle_section),
    );
    body = body.with_child(header);
    flat.push(action);
    if !workflows_collapsed {
        if state.workflows.is_empty() {
            // Honest empty state: this build has no registered workflow source
            // to show, so the section says so instead of inventing a row.
            body = body.with_child(overview_empty_line(app, "No workflows"));
        } else {
            for workflow in &state.workflows {
                let on_workflows = actions.on_workflows.clone();
                let run: Rc<RefCell<dyn FnMut()>> =
                    Rc::new(RefCell::new(move || (on_workflows.borrow_mut())()));
                let selected = flat.len() == highlight;
                let (row, action) = overview_action_row(
                    app,
                    "workflow",
                    &workflow.name,
                    selected,
                    run,
                    true,
                    Rc::clone(&on_close),
                );
                body = body.with_child(row);
                flat.push(action);
            }
        }
    }

    let scroll = Scrollable::new(body.finish(), Axis::Vertical)
        .with_state(overview_scroll())
        .with_fit_content(true)
        .finish();
    let panel = Container::new(scroll)
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
        .with_corner_radius(OVERVIEW_POPOVER_RADIUS)
        .with_padding(EdgeInsets::uniform(8.0))
        .finish();
    (Rc::new(RefCell::new(panel)), flat)
}

/// The right Overview panel: real Review/Files/Terminal actions, the live
/// sub-agent and background-task lists, maximize and collapse controls.
pub fn build_side_panel(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let active = state.active_pane_id;
    let kind = state
        .side_tabs
        .get(state.active_side_tab)
        .copied()
        .unwrap_or(SideTabKind::Overview);
    // The panel's window controls (maximize, toggle) live at the far right of
    // the topbar's panel-2 section, so the body starts directly on the tab's
    // surface: no second header row repeating the controls.

    // Every tab draws a real surface. Overview keeps the original
    // search/actions/subagent/task body; the Agent tab is a conversation agent
    // of its own (the reserved side pane id, never the active pane's chat);
    // Terminal and Files mount the real pane views rather than a static
    // overview.
    let content: Box<dyn Element> = match kind {
        SideTabKind::Overview => build_overview_content(app, state, actions),
        SideTabKind::Agent => {
            // The side composer routes to the side conversation's own session.
            // The pane target is the reserved id, so the active pane's draft and
            // transcript are untouched; activating the reserved id would be
            // meaningless (it is not a leaf), so the side chat's submit/focus
            // paths are wired to no-op activation and side actions instead.
            let mut side_actions = actions.clone();
            let on_side = actions.on_side_chat.clone();
            let on_side_change = on_side.clone();
            side_actions.on_composer_change = Rc::new(RefCell::new(move |value: String| {
                (on_side_change.borrow_mut())(SideChatAction::Draft(value))
            }));
            let on_side_send = on_side.clone();
            side_actions.on_send_message = Rc::new(RefCell::new(move |text: String| {
                (on_side_send.borrow_mut())(SideChatAction::Send(text))
            }));
            let on_side_enter = on_side.clone();
            side_actions.on_cmd_enter = Rc::new(RefCell::new(move |text: String| {
                (on_side_enter.borrow_mut())(SideChatAction::Send(text))
            }));
            let on_side_attach = on_side.clone();
            side_actions.on_attach = Rc::new(RefCell::new(move || {
                (on_side_attach.borrow_mut())(SideChatAction::Attach)
            }));
            let on_side_focus = on_side;
            side_actions.on_composer_focus_change = Rc::new(RefCell::new(move |focused: bool| {
                (on_side_focus.borrow_mut())(SideChatAction::Focus(focused))
            }));
            // Focusing or submitting in the side chat must not move the pane
            // tree's active pane to the reserved id.
            side_actions.on_pane_activate = Rc::new(RefCell::new(|_pane: u64| {}));
            crate::ui::chat::build_agent_chat(
                app,
                state,
                &side_actions,
                crate::state::SIDE_AGENT_PANE_ID,
                true,
                false,
                None,
            )
        }
        SideTabKind::Terminal => {
            let cwd = state
                .pane_chat
                .get(&active)
                .map(|pane| pane.composer_path.clone())
                .filter(|cwd| !cwd.trim().is_empty())
                .unwrap_or_else(|| state.composer_path.clone());
            let grid =
                crate::ui::terminal::build_terminal_dock(app, state, actions, active, cwd, true);
            if state.side_rail_open {
                side_body_with_rail(app, state, actions, grid)
            } else {
                grid
            }
        }
        SideTabKind::Files => {
            // Y: the tab shows the file a click named (SIDE_FILE_PANE_ID), or
            // the file the active pane already shows. It never retargets the
            // main pane just to read a file.
            let (pane_id, path) = if let Some(path) = state.side_file.clone() {
                (crate::state::SIDE_FILE_PANE_ID, Some(path))
            } else {
                let path = state
                    .spaces
                    .get(state.active_space)
                    .and_then(|space| space.leaf_kind(active))
                    .and_then(|kind| match kind {
                        PaneKind::File { path } => Some(path.clone()),
                        _ => None,
                    });
                (active, path)
            };
            match (state.pane_files.contains_key(&pane_id), path) {
                (true, Some(path)) => {
                    crate::ui::file_view::build_file_view(app, state, actions, pane_id, &path)
                }
                _ => side_tabs::no_file_message(app),
            }
        }
        SideTabKind::TaskOutput => {
            let output = task_output_view(app, state);
            if state.side_rail_open {
                side_body_with_rail(app, state, actions, output)
            } else {
                output
            }
        }
        SideTabKind::Review => crate::ui::diff_panel::build_review_tab(app, state, actions, active),
    };
    // AF: every tab except Review (which already owns its row) gets its own
    // 1 pt-separated topbar inside the panel content.
    let content: Box<dyn Element> = if kind == SideTabKind::Review {
        content
    } else {
        let topbar = side_tab_topbar(app, state, actions, kind);
        let inner_pad = if kind == SideTabKind::Terminal {
            EdgeInsets::new(0.0, 0.0, 0.0, 0.0)
        } else {
            EdgeInsets::uniform(
                crate::ui::panes::PANE_AREA_INSET + crate::ui::panes::PANE_INSET + 8.0,
            )
        };
        let mut column = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(topbar)
            .with_child(Divider::horizontal().finish());
        column = column.with_child(
            Expanded::new(Container::new(content).with_padding(inner_pad).finish()).finish(),
        );
        column.finish()
    };
    // A horizontal swipe on the content steps the strip; vertical travel goes
    // to the mounted surface unchanged.
    let content = side_tabs::swipe_content(
        content,
        actions.on_side_tab.clone(),
        state.side_swipe_accum.clone(),
    );

    let body = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(Expanded::new(content).finish())
        .finish();
    // The side panel uses the main panel's own `Bg` material — flat, with no
    // card casing, border or radius — so the two panels read as one surface at
    // the shared boundary. The sheet's `Surface` frame stays around the panel
    // on its top, right and bottom edges, exactly the frame the main pane card
    // keeps, and the content gets the same 8 pt inset. The left edge has no
    // frame: the `Bg` runs to the boundary so no gap or line separates it from
    // the main card (which crosses under it by the canceled gutter).
    let inset = crate::ui::panes::PANE_AREA_INSET + crate::ui::panes::PANE_INSET;
    let body = Container::new(body)
        .with_background(Fill::Solid(app.theme.color(ColorToken::ContentBg)))
        .finish();
    // The side panel is its own black surface (agent, terminal, files and
    // review tabs all sit on `Bg`), so it reads apart from the grey sidebar and
    // topbar material.
    Container::new(body)
        .with_background(Fill::Solid(app.theme.color(ColorToken::ContentBg)))
        .finish()
}

/// The Overview tab's original body: search over conversations, commands and
/// files, the active pane's real actions, then its sub-agents and background
/// tasks. Scrolled by its own viewport inside the panel.
fn build_overview_content(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let pane = state.pane_chat.get(&state.active_pane_id);
    let on_search_change = actions.on_global_search_change.clone();
    let on_search_focus = actions.on_global_search_focus_change.clone();
    let search = SearchInput::new()
        .with_value(state.global_search_query.clone())
        .with_focused(state.global_search_focused)
        .with_caret(state.global_search_caret.clone())
        .with_placeholder("Search or enter a URL")
        .with_text_size(13.0)
        .with_extra_height(0.0)
        .with_corner_radius(22.0)
        .with_background(ColorToken::ContentBg)
        .with_on_change(move |value| (on_search_change.borrow_mut())(value))
        .with_on_focus_change(move |focused| (on_search_focus.borrow_mut())(focused))
        .finish();
    // The reference keeps one kebab at the search row's trailing edge: the
    // side panel's own actions (maximize/restore, close), painted above the
    // panel content through the window's last layer.
    let on_max = actions.on_toggle_side_panel_maximized.clone();
    let on_close = actions.on_toggle_right_sidebar.clone();
    let maximized = state.side_panel_maximized;
    let kebab = PopupMenu::new(
        button(app, "dots-vertical", "Side panel actions"),
        vec![
            PopupMenuItem::new(if maximized {
                "Restore side panel"
            } else {
                "Maximize side panel"
            })
            .with_icon(if maximized {
                "fullscreen-exit"
            } else {
                "fullscreen-enter"
            }),
            PopupMenuItem::new("Close side panel").with_icon("left-panel-close"),
        ],
    )
    .with_open(side_panel_menu_open())
    .with_menu_id("chrome.side-panel")
    .with_position(PopupMenuPosition::BelowEnd)
    .with_max_width(210.0)
    .with_on_select(move |index| match index {
        0 => (on_max.borrow_mut())(),
        1 => (on_close.borrow_mut())(),
        _ => {}
    })
    .finish();
    let search_row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(Expanded::new(search).finish())
        .with_child(TopLayer::new(kebab).finish())
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
    // Terminal and Side Question open as panel-2 tabs from this launcher (the
    // Overview icon is the way to add a surface without the strip's `+`).
    let on_side_tab = actions.on_side_tab.clone();
    let on_plugins = actions.on_plugins.clone();
    let active = state.active_pane_id;
    let mut rows = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(4.0);
    for (icon, label, action, trailing) in [
        ("file", "Review", 0u8, None),
        ("folder", "Open File", 1, None),
        ("terminal", "Terminal", 2, None),
        ("environment-cloud", "Browser", 3, None),
        ("message-plus-square", "Side Question", 4, None),
        ("plug", "Plugins", 5, Some("chevron-right")),
    ] {
        let disabled = matches!(action, 3);
        let row = if disabled {
            HoverRow::new(panel_row(app, icon, label, true, trailing))
                .with_padding(EdgeInsets::new(6.0, 4.0, 6.0, 4.0))
                .with_corner_radius(8.0)
                .finish()
        } else {
            let on_diff = on_diff.clone();
            let on_files_tab = actions.on_open_files_tab.clone();
            let on_side_tab = on_side_tab.clone();
            let on_plugins = on_plugins.clone();
            HoverRow::new(panel_row(app, icon, label, false, trailing))
                .with_padding(EdgeInsets::new(6.0, 4.0, 6.0, 4.0))
                .with_corner_radius(8.0)
                .with_on_click(move || match action {
                    0 => (on_diff.borrow_mut())(active, DiffPanelAction::Open(None)),
                    1 => (on_files_tab.borrow_mut())(),
                    2 => (on_side_tab.borrow_mut())(SideTabAction::Add(SideTabKind::Terminal)),
                    4 => (on_side_tab.borrow_mut())(SideTabAction::Add(SideTabKind::Agent)),
                    5 => (on_plugins.borrow_mut())(),
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
    if !subagents.is_empty() {
        sections = sections.with_child(section_header(app, "Subagents", subagents.len()));
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
    if !tasks.is_empty() {
        sections = sections.with_child(
            Container::new(section_header(app, "Background Tasks", tasks.len()))
                .with_padding(EdgeInsets::new(12.0, 0.0, 0.0, 0.0))
                .finish(),
        );
        for (index, task) in tasks.iter().take(4).enumerate() {
            sections = sections.with_child(
                HoverRow::new(panel_row(
                    app,
                    if index == 0 {
                        "circle-x"
                    } else {
                        "terminal-input"
                    },
                    &task.description,
                    false,
                    None,
                ))
                .with_padding(EdgeInsets::new(10.0, 7.0, 10.0, 7.0))
                .with_corner_radius(8.0)
                .finish(),
            );
        }
        if tasks.len() > 4 {
            let on_side_tab = on_side_tab.clone();
            sections = sections.with_child(
                HoverRow::new(
                    Container::new(muted_line(app, "See all"))
                        .with_padding(EdgeInsets::new(10.0, 4.0, 4.0, 4.0))
                        .finish(),
                )
                .with_corner_radius(6.0)
                .with_on_click(move || {
                    (on_side_tab.borrow_mut())(SideTabAction::Add(SideTabKind::TaskOutput))
                })
                .finish(),
            );
        }
    }

    let body = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(10.0)
        .with_child(search_row)
        .with_child(results.finish())
        .with_child(rows.finish())
        .with_child(
            Container::new(sections.finish())
                .with_padding(EdgeInsets::new(12.0, 0.0, 12.0, 0.0))
                .finish(),
        )
        .finish();
    // The reference launcher is a content column, not full-bleed: cap and
    // inset it while keeping the search field and kebab inside the same column.
    let body = ConstrainedBox::new(
        Container::new(body)
            .with_padding(EdgeInsets::new(20.0, 0.0, 20.0, 0.0))
            .finish(),
    )
    .with_max_width(980.0)
    .finish();
    Scrollable::new(body, goble_ui::elements::Axis::Vertical).finish()
}

/// AF: the per-tab content topbar. The Review tab owns a richer one inside
/// `diff_panel`, so it is not wrapped here.
fn side_tab_topbar(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    kind: SideTabKind,
) -> Box<dyn Element> {
    let on_side_tab = actions.on_side_tab.clone();
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(2.0);
    let (label, trailing): (String, Option<Box<dyn Element>>) = match kind {
        SideTabKind::Terminal => {
            let toggle = {
                let on_side_tab = on_side_tab.clone();
                move || (on_side_tab.borrow_mut())(SideTabAction::ToggleRail)
            };
            (
                "zsh".to_string(),
                Some(
                    Tooltip::new(
                        TopbarButton::new(
                            Icon::new("menu-01")
                                .with_size(14.0)
                                .with_theme_color(ColorToken::Muted, app)
                                .finish(),
                        )
                        .with_size(26.0)
                        .with_corner_radius(13.0)
                        .with_on_click(toggle)
                        .finish(),
                        "Toggle tasks",
                    )
                    .with_position(TooltipPosition::Below)
                    .finish(),
                ),
            )
        }
        SideTabKind::TaskOutput => {
            let text = task_output_text(state);
            let on_copy = actions.on_copy_terminal.clone();
            let copy = Tooltip::new(
                TopbarButton::new(
                    Icon::new("copy")
                        .with_size(14.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_size(26.0)
                .with_corner_radius(6.0)
                .with_on_click(move || (on_copy.borrow_mut())(text.clone()))
                .finish(),
                "Copy output",
            )
            .with_position(TooltipPosition::Below)
            .finish();
            let toggle = {
                let on_side_tab = on_side_tab.clone();
                move || (on_side_tab.borrow_mut())(SideTabAction::ToggleRail)
            };
            let chevron = Tooltip::new(
                TopbarButton::new(
                    Icon::new(if state.side_rail_open {
                        "chevron-right"
                    } else {
                        "chevron-left"
                    })
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
                )
                .with_size(26.0)
                .with_corner_radius(6.0)
                .with_on_click(toggle)
                .finish(),
                "Toggle tasks",
            )
            .with_position(TooltipPosition::Below)
            .finish();
            (
                "Background Task Output".to_string(),
                Some(
                    Flex::row()
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_spacing(2.0)
                        .with_child(copy)
                        .with_child(chevron)
                        .finish(),
                ),
            )
        }
        SideTabKind::Agent => ("New side question".to_string(), None),
        SideTabKind::Files => {
            let label = state
                .side_file
                .clone()
                .or_else(|| {
                    state
                        .spaces
                        .get(state.active_space)
                        .and_then(|space| space.leaf_kind(state.active_pane_id))
                        .and_then(|kind| match kind {
                            PaneKind::File { path } => Some(path.clone()),
                            _ => None,
                        })
                })
                .unwrap_or_else(|| "Files".to_string());
            (label, None)
        }
        SideTabKind::Overview => ("Overview".to_string(), None),
        SideTabKind::Review => ("Review".to_string(), None),
    };
    row = row.with_child(
        Expanded::new(
            Text::new(label)
                .with_font_size(12.5)
                .with_weight(FontWeight::Medium)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
        )
        .finish(),
    );
    if let Some(trailing) = trailing {
        row = row.with_child(trailing);
    }
    ConstrainedBox::new(
        Container::new(row.finish())
            .with_padding(EdgeInsets::new(10.0, 2.0, 8.0, 2.0))
            .finish(),
    )
    .with_height(36.0)
    .finish()
}

fn task_output_text(state: &UiSnapshot) -> String {
    state
        .pane_live_work
        .get(&state.active_pane_id)
        .map(|items| {
            items
                .iter()
                .map(|item| item.description.clone())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// AF: the Task Output body as a monospace, line-numbered code view.
fn task_output_view(app: &AppContext, state: &UiSnapshot) -> Box<dyn Element> {
    let items = state
        .pane_live_work
        .get(&state.active_pane_id)
        .cloned()
        .unwrap_or_default();
    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.0);
    if items.is_empty() {
        column = column.with_child(
            Text::new("No task output")
                .with_font_family(FontFamily::Mono)
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    } else {
        for (index, item) in items.iter().enumerate() {
            column = column.with_child(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(10.0)
                    .with_child(
                        ConstrainedBox::new(
                            Text::new(format!("{}", index + 1))
                                .with_font_family(FontFamily::Mono)
                                .with_font_size(11.0)
                                .with_theme_color(ColorToken::Muted, app)
                                .finish(),
                        )
                        .with_width(28.0)
                        .finish(),
                    )
                    .with_child(
                        Expanded::new(
                            Text::new(item.description.clone())
                                .with_font_family(FontFamily::Mono)
                                .with_font_size(11.5)
                                .with_theme_color(ColorToken::Text, app)
                                .with_max_lines(1)
                                .finish(),
                        )
                        .finish(),
                    )
                    .finish(),
            );
        }
    }
    Scrollable::new(
        Container::new(column.finish())
            .with_padding(EdgeInsets::uniform(10.0))
            .finish(),
        Axis::Vertical,
    )
    .finish()
}

/// AF: Terminal / Task Output body with the shared Terminals + Background Tasks
/// rail on the right, separated by one full-height rule.
fn side_body_with_rail(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    body: Box<dyn Element>,
) -> Box<dyn Element> {
    let rail = side_tasks_rail(app, state, actions);
    let rule = ConstrainedBox::new(
        Container::new(
            Flex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .finish(),
        )
        .with_background(Fill::Solid(app.theme.color(ColorToken::Border)))
        .finish(),
    )
    .with_width(1.0)
    .finish();
    Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(Expanded::new(body).finish())
        .with_child(rule)
        .with_child(rail)
        .finish()
}

fn side_tasks_rail(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let _ = actions;
    let terminal_count = state.terminal.borrow().sessions.len();
    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(1.0)
        .with_child(muted_line(app, "Terminals"));
    for index in 0..terminal_count.max(1) {
        let selected = index == 0;
        column = column.with_child(
            HoverRow::new(
                Container::new(
                    Text::new("zsh")
                        .with_font_family(FontFamily::Mono)
                        .with_font_size(12.0)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .with_padding(EdgeInsets::new(6.0, 8.0, 6.0, 8.0))
                .finish(),
            )
            .with_selected(selected)
            .finish(),
        );
    }
    column = column.with_child(
        Container::new(muted_line(app, "Background Tasks"))
            .with_padding(EdgeInsets::new(12.0, 0.0, 0.0, 0.0))
            .finish(),
    );
    let tasks = state
        .pane_live_work
        .get(&state.active_pane_id)
        .cloned()
        .unwrap_or_default();
    for task in &tasks {
        let icon = if task.description.contains("failed") {
            "x-circle"
        } else if task.description.contains("finished") || task.description.contains("done") {
            "check"
        } else {
            "terminal-input"
        };
        column = column.with_child(
            HoverRow::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(7.0)
                    .with_child(
                        Icon::new(icon)
                            .with_size(12.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_child(
                        Expanded::new(
                            Text::new(task.description.clone())
                                .with_font_size(11.5)
                                .with_theme_color(ColorToken::Text, app)
                                .with_max_lines(1)
                                .finish(),
                        )
                        .finish(),
                    )
                    .finish(),
            )
            .with_padding(EdgeInsets::new(5.0, 8.0, 5.0, 8.0))
            .finish(),
        );
    }
    if tasks.is_empty() {
        column = column.with_child(
            Container::new(muted_line(app, "No background tasks"))
                .with_padding(EdgeInsets::new(4.0, 8.0, 4.0, 8.0))
                .finish(),
        );
    }
    let scroll = Scrollable::new(column.finish(), Axis::Vertical).finish();
    let rail = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(Expanded::new(scroll).finish())
        .finish();
    ConstrainedBox::new(
        Container::new(rail)
            .with_border(Border::left(1.0).with_border_color(app.theme.color(ColorToken::Border)))
            .finish(),
    )
    .with_width(188.0)
    .finish()
}

fn panel_row(
    app: &AppContext,
    icon: &'static str,
    label: &str,
    disabled: bool,
    trailing: Option<&'static str>,
) -> Box<dyn Element> {
    let mut row = Flex::row()
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
        .with_child(Spacer::new().finish());
    if let Some(trailing) = trailing {
        row = row.with_child(
            Icon::new(trailing)
                .with_size(14.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    row.finish()
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
            Container::new(
                Text::new(count.to_string())
                    .with_font_size(10.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_corner_radius(9.0)
            .with_padding(EdgeInsets::new(2.0, 6.0, 2.0, 6.0))
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
