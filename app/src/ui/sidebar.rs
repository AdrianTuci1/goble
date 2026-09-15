//! Left conversation sidebar.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AgentCardUi, AppContext, Axis, Button, ButtonVariant, Container, ConversationEntry,
    ConversationListItem, CrossAxisAlignment, EdgeInsets, Element, Expanded, Fill, Flex,
    HoverButton, HoverRow, Icon, MainAxisSize, Scrollable, SearchInput, Spacer, Text, Tooltip,
    TooltipPosition,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::explorer::build_explorer;
use super::global_search::build_search;
use super::{SidebarView, UiActions, UiSnapshot};

/// How many conversation cards the collapsed sidebar shows before the
/// "View all" button.
const COLLAPSED_CONVERSATIONS: usize = 4;

/// Gap between a conversation card's 3-dot control and the sidebar's own right
/// edge, in logical points.
///
/// The card does not know the sidebar's width or padding, so it is told only how
/// far in from its *own* trailing edge the control goes; the sidebar's padding
/// (the `xs` in [`build_sidebar`]) is the rest of the 5 pt. With the default
/// spacing that is `5.0 - 4.0 = 1.0`.
const CARD_DOTS_INSET: f32 = 5.0;

/// Extra height on each side of the search field's row: the box is a few pixels
/// taller than its text so it reads as a field rather than a label.
pub(crate) const SEARCH_EXTRA_HEIGHT: f32 = 3.0;

/// The collapsible section that pins starred conversations, above the folder
/// groups.
const STARRED_SECTION: &str = "starred";

/// The collapsed key of a conversation folder's section.
fn folder_section(folder: &str) -> String {
    format!("folder:{folder}")
}

/// Left sidebar: the view toolbelt, then whichever view it selects.
///
/// The surface is flat: no rounded corners and no separator lines. The
/// environment (medium) selector lives in the topbar now, so the agents view
/// only needs the currently selected medium's routing to filter which
/// conversation folder set is shown.
pub fn build_sidebar(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    selected_medium: &str,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let xs = app.theme.spacing_px(SpacingToken::Xs);

    let view: Box<dyn Element> = match state.sidebar_view {
        SidebarView::Agents => agents_view(app, state, actions, selected_medium),
        SidebarView::Explorer => build_explorer(app, state, actions),
        SidebarView::Search => build_search(app, state, actions),
    };

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(view_toolbelt(app, state, actions));
    column = column.with_child(Expanded::new(view).finish());

    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets::new(xs, spacing, xs, spacing))
        .finish()
}

/// The sidebar's own toolbelt: which of its views is showing. Icon-only, with
/// the view's name on the tooltip, so the strip costs one short row.
fn view_toolbelt(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(xs);
    for view in SidebarView::ALL {
        let active = state.sidebar_view == *view;
        let on_select = actions.on_select_sidebar_view.clone();
        let icon = match view {
            SidebarView::Agents => "message-chat-square",
            SidebarView::Explorer => "folder-closed",
            SidebarView::Search => "search",
        };
        let tab = HoverRow::new(
            Container::new(
                Icon::new(icon)
                    .with_size(14.0)
                    .with_theme_color(
                        if active {
                            ColorToken::Text
                        } else {
                            ColorToken::Muted
                        },
                        app,
                    )
                    .finish(),
            )
            .with_padding(EdgeInsets::uniform(5.0))
            .finish(),
        )
        .with_selected(active)
        .with_on_click(move || (on_select.borrow_mut())(*view))
        .finish();
        row = row.with_child(
            Tooltip::new(tab, view.tooltip())
                .with_position(TooltipPosition::Below)
                .finish(),
        );
    }
    row.finish()
}

/// The agents view: the conversation search box, then "new conversation", then
/// the conversation cards — the starred ones pinned at the top, the rest grouped
/// into collapsible folders. A starred conversation is listed once: the folders
/// hold the ones that are not pinned.
fn agents_view(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    selected_medium: &str,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let xs = app.theme.spacing_px(SpacingToken::Xs);

    // Search box: compact, square, taller than its own text, and without the
    // magnifier — the placeholder says what the field is.
    let on_search_change = actions.on_search_change.clone();
    let on_search_focus = actions.on_search_focus_change.clone();
    let search = SearchInput::new()
        .with_value(state.search_query.clone())
        .with_focused(state.search_focused)
        .with_placeholder("Search")
        .with_compact(true)
        .with_icon(false)
        .with_extra_height(SEARCH_EXTRA_HEIGHT)
        .with_on_change(move |value| (on_search_change.borrow_mut())(value))
        .with_on_focus_change(move |focused| (on_search_focus.borrow_mut())(focused))
        .finish();

    // "New conversation" row: a gray "+" box in front of the label. The whole
    // row highlights on hover and creates a new conversation on click. The hover flag
    // lives in app state so the highlight survives the per-frame rebuild.
    let on_create_submit = actions.on_create_submit.clone();
    let plus_box = Container::new(
        Icon::new("plus")
            .with_size(14.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_padding(EdgeInsets::uniform(5.0))
    .finish();
    let header = HoverButton::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(plus_box)
            .with_child(
                Text::new("New conversation")
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish(),
        state.new_agent_hover.clone(),
    )
    .with_padding(EdgeInsets::new(xs, xs, xs, xs))
    .with_corner_radius(0.0)
    .with_on_click(move || (on_create_submit.borrow_mut())())
    .finish();

    // Conversation cards: nothing here is a section of its own until the
    // starred ones and the folders below.
    let on_toggle_section = actions.on_toggle_section.clone();

    // Starred: the conversations the user pinned, pinned above the folders,
    // whichever environment they belong to. The pinned ones are this section's
    // and no one else's — the folder groups below list what is left — so a
    // starred conversation is listed once, where the user put it.
    //
    // A conversation the agent has not answered yet is not listed at all: it is
    // a tab the user opened and typed into, not a thread with anything in it.
    // The rule is on the entry ([`ConversationEntry::has_agent_reply`]) and
    // applies to both sections, so neither can list what the other hides.
    let starred: Vec<&ConversationEntry> = state
        .conversations
        .iter()
        .filter(|entry| entry.has_agent_reply && state.starred.iter().any(|id| id == &entry.id))
        .collect();
    let is_starred =
        |entry: &ConversationEntry| starred.iter().any(|pinned| pinned.id == entry.id);

    // Show only the conversations that belong to the selected environment
    // (Local / Remote). Each conversation carries its `workspace_routing`
    // (`"local"` / `"remote"`); the selected medium maps to one of those via
    // [`crate::media::medium_routing`]. Conversations without an explicit
    // routing default to `local`.
    let routing = crate::media::medium_routing(selected_medium);
    let visible: Vec<&ConversationEntry> = state
        .conversations
        .iter()
        .filter(|entry| {
            entry.has_agent_reply && entry.workspace_routing == routing && !is_starred(entry)
        })
        .collect();

    // Collapsed, the list is a short digest: a few cards plus a "View all"
    // button. Expanding it shows every conversation in a scrollable region.
    let shown: Vec<&ConversationEntry> = if state.conversations_expanded {
        visible.clone()
    } else {
        visible.iter().take(COLLAPSED_CONVERSATIONS).copied().collect()
    };

    // Group conversations into folders (preserving insertion order), each with
    // a collapsible section header, then the items under it.
    let mut ordered: Vec<(String, Vec<&ConversationEntry>)> = Vec::new();
    for entry in &shown {
        if let Some((_, items)) = ordered.iter_mut().find(|(folder, _)| folder == &entry.folder) {
            items.push(entry);
        } else {
            ordered.push((entry.folder.clone(), vec![entry]));
        }
    }

    let mut list = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.0);

    // Starred: the conversations the user pinned, above the folders, whichever
    // environment they belong to. A section with nothing in it is not drawn at
    // all: there is nothing to collapse.
    let starred_collapsed = state.collapsed_sections.iter().any(|key| key == STARRED_SECTION);
    if !starred.is_empty() {
        list = list.with_child(section_header(
            app,
            "Starred",
            starred.len(),
            starred_collapsed,
            on_toggle_section.clone(),
            STARRED_SECTION.to_string(),
        ));
        if !starred_collapsed {
            for entry in &starred {
                list = list.with_child(conversation_card(app, state, actions, entry));
            }
        }
    }

    // The empty state belongs to the whole list, not to the folders alone:
    // pinned conversations are listed here too, so a list that is entirely
    // starred has content and says nothing.
    if visible.is_empty() && starred.is_empty() {
        // The list is empty for one of two reasons, and they read differently:
        // nothing worth listing exists yet (no conversation, or none the agent
        // has answered), or the ones that do exist belong to another
        // environment.
        let any_listable = state
            .conversations
            .iter()
            .any(|entry| entry.has_agent_reply);
        let empty_label = if any_listable {
            "No conversations for this environment."
        } else {
            "No conversations yet. Create one to begin."
        };
        list = list.with_child(
            Container::new(
                Text::new(empty_label)
                    .with_font_size(11.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_padding(EdgeInsets::new(xs, xs, xs, xs))
            .finish(),
        );
    }
    for (folder, entries) in ordered {
        let key = folder_section(&folder);
        let collapsed = state.collapsed_sections.iter().any(|section| section == &key);
        list = list.with_child(section_header(
            app,
            &folder,
            entries.len(),
            collapsed,
            on_toggle_section.clone(),
            key,
        ));
        if collapsed {
            continue;
        }
        for entry in entries {
            list = list.with_child(conversation_card(app, state, actions, entry));
        }
    }

    // "View all" (collapsed, when there is more to see) / "Show less"
    // (expanded) — the switch that turns the list into a scrollable one. Its
    // label and its outline both carry the app's text colour: it is the list's
    // own control, not a raised button, and the surface's hairline border would
    // leave the label as the only thing visible.
    if shown.len() < visible.len() || state.conversations_expanded {
        let on_toggle = actions.on_toggle_conversations_expanded.clone();
        let label = if state.conversations_expanded {
            "Show less".to_string()
        } else {
            format!("View all ({})", visible.len())
        };
        list = list.with_child(
            Button::new(
                Text::new(label)
                    .with_font_size(11.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_variant(ButtonVariant::Default)
            .with_border_color(app.theme.color(ColorToken::Text))
            .with_corner_radius(0.0)
            .with_on_click(move || (on_toggle.borrow_mut())())
            .finish(),
        );
    }

    // Plugins/Screen open from the Cmd+K palette ("Open connectors" /
    // "Open screen"), not from the sidebar.

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(search);
    column = column.with_child(header);
    // The list is the only flexible row: it takes the remaining height and
    // scrolls inside it (its offset lives in app state, so it survives the
    // per-frame rebuild).
    column = column.with_child(
        Expanded::new(
            Scrollable::new(list.finish(), Axis::Vertical)
                .with_state(state.sidebar_scroll.clone())
                .finish(),
        )
        .finish(),
    );

    column.finish()
}

/// A section header: the disclosure chevron, the name and how many rows sit
/// under it. The whole header toggles the section, the way warp-new's do.
fn section_header(
    app: &AppContext,
    label: &str,
    count: usize,
    collapsed: bool,
    on_toggle: Rc<RefCell<dyn FnMut(String)>>,
    key: String,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Icon::new(if collapsed {
                "chevron-right"
            } else {
                "chevron-down"
            })
            .with_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        )
        .with_child(
            Text::new(label.to_string())
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
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
    HoverRow::new(row)
        .with_padding(EdgeInsets::new(0.0, xs, 0.0, xs))
        .with_on_click(move || (on_toggle.borrow_mut())(key.clone()))
        .finish()
}

/// One conversation card. Its menu carries the star toggle, so the section it
/// belongs to and the card always agree about whether it is starred.
fn conversation_card(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    entry: &ConversationEntry,
) -> Box<dyn Element> {
    let click_id = entry.id.clone();
    let delete_id = entry.id.clone();
    let star_id = entry.id.clone();
    let selected = state.selected_id.as_deref() == Some(entry.id.as_str());
    let on_select = actions.on_select_conversation.clone();
    let on_delete = actions.on_agent_delete.clone();
    let on_star = actions.on_toggle_star.clone();
    let ui = state
        .agent_cards
        .get(&entry.id)
        .cloned()
        .unwrap_or_else(|| Rc::new(RefCell::new(AgentCardUi::default())));
    // The card is inset from the sidebar's edges by the sidebar's own padding,
    // so the part of the 5 pt that lies inside the card is what is left over.
    let dots_inset = (CARD_DOTS_INSET - app.theme.spacing_px(SpacingToken::Xs)).max(0.0);
    ConversationListItem::new(
        entry.id.clone(),
        entry.name.clone(),
        entry.last_response.clone(),
        entry.timestamp.clone(),
        ui,
        selected,
    )
    .with_workspace_routing(entry.workspace_routing.clone())
    .with_directory(entry.directory.clone())
    .with_starred(state.starred.iter().any(|id| id == &entry.id))
    .with_dots_inset(dots_inset)
    .with_on_click(move || (on_select.borrow_mut())(click_id.clone()))
    .with_on_delete(move || (on_delete.borrow_mut())(delete_id.clone()))
    .with_on_toggle_star(move || (on_star.borrow_mut())(star_id.clone()))
    .finish()
}

#[cfg(test)]
mod sidebar_surface_tests {
    use super::*;
    use crate::root_view::RootView;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::{AgentCardUi, ConversationStatus, LayoutContext, PaintContext, SizeConstraint};
    use goble_ui::geometry::{vec2f, RectF};
    use goble_ui::render::{RenderCommand, Renderer};
    use goble_ui::test_util::render_element;
    use std::sync::Arc;

    /// A rule between two sections: a hairline spanning the sidebar's width.
    /// The search field's own border is a stroke too, but it is as tall as the
    /// field, so it is not a separator.
    fn separator(command: &RenderCommand, band: f32) -> Option<RectF> {
        match command {
            RenderCommand::StrokeRect { rect, .. }
                if rect.height() <= 2.0 && rect.width() > band / 2.0 =>
            {
                Some(*rect)
            }
            _ => None,
        }
    }

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

    /// Every command the sidebar band drew — a rect fully left of the workspace
    /// (the pane's frame starts at its own edge), or a run whose origin is.
    fn in_band(rect: RectF) -> bool {
        rect.max_x() <= crate::ui::SIDEBAR_WIDTH
    }

    /// The sidebar draws no rounded fill, no separator rule and no magnifier,
    /// and it does draw the runs that make those assertions mean something. The
    /// one card that rounds its corners is the tray an open 3-dot control hangs
    /// beside itself — the card the rich input's controls open, so the two
    /// trays are one surface — and it exists only while that tray is open
    /// (`tray_open`).
    fn assert_the_band_is_flat(commands: &[RenderCommand], case: &str, tray_open: bool) {
        let band = crate::ui::SIDEBAR_WIDTH;

        // The tray's own rows: a rounded fill that holds one of them is the
        // tray's card and not a row that went back to being a widget.
        let tray_rows = ["Star", "Delete agent"];
        let holds_a_tray_row = |rect: &RectF| {
            tray_open
                && commands.iter().any(|other| match other {
                    RenderCommand::DrawText { text, origin, .. } => {
                        tray_rows.contains(&text.as_str())
                            && rect.min_x() <= origin.x
                            && origin.x <= rect.max_x()
                            && rect.min_y() <= origin.y
                            && origin.y <= rect.max_y()
                    }
                    _ => false,
                })
        };

        let rounded: Vec<String> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    corner_radius,
                    ..
                }
                | RenderCommand::FillRectFadeRight {
                    rect,
                    corner_radius,
                    ..
                } if in_band(*rect) && *corner_radius > 0.0 && !holds_a_tray_row(rect) => {
                    Some(format!("fill {rect:?} radius {corner_radius}"))
                }
                _ => None,
            })
            .collect();
        assert!(
            rounded.is_empty(),
            "{case}: the sidebar draws no rounded fill, got {}",
            rounded.join(", ")
        );
        // The tray itself is drawn, and drawn rounded: the rule above is not
        // passing because the band happens to be empty.
        if tray_open {
            let tray_fills = commands
                .iter()
                .filter(|command| match command {
                    RenderCommand::FillRect {
                        rect,
                        corner_radius,
                        ..
                    } => in_band(*rect) && *corner_radius > 0.0 && holds_a_tray_row(rect),
                    _ => false,
                })
                .count();
            assert!(
                tray_fills > 0,
                "{case}: the open tray is the band's rounded card"
            );
        }

        let rules: Vec<String> = commands
            .iter()
            .filter_map(|command| separator(command, band).map(|rect| format!("{rect:?}")))
            .collect();
        assert!(
            rules.is_empty(),
            "{case}: the sidebar draws no separator, got {}",
            rules.join(", ")
        );

        // The field itself carries no magnifier: the one `search` icon in the
        // band is the global-search tab of the view toolbelt.
        let magnifiers = commands
            .iter()
            .filter(|command| {
                matches!(command, RenderCommand::DrawIcon { name, origin, .. }
                    if name == "search" && origin.x < band)
            })
            .count();
        assert_eq!(
            magnifiers, 1,
            "{case}: only the toolbelt's search tab draws a search icon"
        );

        // The band is really the sidebar, and a stroke was available to be a
        // rule: the search field's own border is drawn.
        let field_border = commands.iter().any(|command| {
            matches!(command, RenderCommand::StrokeRect { rect, .. }
                if in_band(*rect) && rect.height() > 2.0)
        });
        assert!(field_border, "{case}: the search field's border is drawn");

        let drawn = |text: &str| {
            commands.iter().any(|command| {
                matches!(command, RenderCommand::DrawText { text: run, origin, .. }
                    if run == text && origin.x < band)
            })
        };
        for run in [
            "Search",
            "New conversation",
            "Frontend",
            "Ada",
            "View all (5)",
        ] {
            assert!(drawn(run), "{case}: the sidebar draws {run:?}");
        }
    }

    /// The sidebar is a flat surface: its field, its rows and its controls draw
    /// no rounded corner anywhere, the two section rules are gone, and the
    /// search field carries no magnifier. Held in every hover state, because a
    /// highlight is where a rounded band would come back.
    #[test]
    fn the_sidebar_is_flat_square_and_free_of_separators_and_the_magnifier() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();

        for (hovered_card, hovered_create) in [
            (false, false),
            (false, true),
            (true, false),
            (true, true),
        ] {
            let view = RootView::new(&app, &desktop, None);
            let state = view.state_rc();
            {
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
                *s.new_agent_hover.borrow_mut() = hovered_create;
                // The hovered card also opens its menu, so the 3-dot control and
                // the delete row are painted too.
                s.agent_cards.insert(
                    "c1".to_string(),
                    Rc::new(RefCell::new(AgentCardUi {
                        hover: hovered_card,
                        menu_open: hovered_card,
                    })),
                );
            }

            let mut root: Box<dyn Element> = Box::new(view);
            let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
            let case = format!("card hovered {hovered_card}, create hovered {hovered_create}");
            assert_the_band_is_flat(&commands, &case, hovered_card);
        }
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
                    conversation(
                        &format!("c{index}"),
                        &format!("Agent {index}"),
                        "Frontend",
                    )
                })
                .collect();
        }

        let mut root: Box<dyn Element> = Box::new(view);
        let window = vec2f(1024.0, 768.0);
        // The wheel is attributed by the last paint (a wheel event carries no
        // position), so the pointer has to be over the band when it is painted.
        let over_the_band = vec2f(crate::ui::SIDEBAR_WIDTH / 2.0, 400.0);
        let paint = |root: &mut Box<dyn Element>| {
            let _ = root.layout(
                SizeConstraint::loose(window),
                &mut LayoutContext::default(),
                &app,
            );
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
            let mut ctx = goble_ui::elements::EventContext::default();
            root.dispatch_event(
                &DispatchedEvent::MouseMove {
                    position: over_the_band,
                },
                &mut ctx,
                &app,
            );
            root.dispatch_event(
                &DispatchedEvent::Scroll {
                    delta: vec2f(0.0, dy),
                },
                &mut ctx,
                &app,
            );
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

    /// The rule above distinguishes a separator from a control's own edge, so
    /// it is not a rule that never fires.
    #[test]
    fn the_separator_rule_reports_a_rule_and_not_a_field_border() {
        let band = crate::ui::SIDEBAR_WIDTH;
        let rule = RenderCommand::StrokeRect {
            rect: RectF::new(
                goble_ui::geometry::PointF::new(4.0, 100.0),
                goble_ui::geometry::Size2F::new(band - 8.0, 1.0),
            ),
            color: goble_ui::color::ColorU::new(255, 255, 255, 255),
            width: 1.0,
            corner_radius: 0.0,
        };
        let field = RenderCommand::StrokeRect {
            rect: RectF::new(
                goble_ui::geometry::PointF::new(4.0, 12.0),
                goble_ui::geometry::Size2F::new(band - 8.0, 25.0),
            ),
            color: goble_ui::color::ColorU::new(255, 255, 255, 255),
            width: 1.0,
            corner_radius: 0.0,
        };
        assert!(separator(&rule, band).is_some(), "a hairline is a rule");
        assert!(separator(&field, band).is_none(), "the field's border is not");
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

        assert!(drawn.iter().any(|run| run == "Frontend"), "the header stays: {drawn:?}");
        for hidden in ["Ada", "Coder"] {
            assert!(
                !drawn.iter().any(|run| run == hidden),
                "the collapsed folder hides {hidden:?}: {drawn:?}"
            );
        }
        assert!(drawn.iter().any(|run| run == "Ops"), "another section is untouched");
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

        let section = drawn.iter().position(|run| run == "Starred").expect("the Starred header");
        let folder = drawn.iter().position(|run| run == "Frontend").expect("a folder header");
        let starred = drawn.iter().position(|run| run == "Ops").expect("the starred card");
        assert!(section < folder && folder < starred || section < starred, "{drawn:?}");
        assert!(section < starred, "the pinned card is under the Starred header: {drawn:?}");
    }

    /// The toolbelt is the sidebar's switch: the explorer draws the working
    /// directory's own children, the search view draws what the query found, and
    /// neither is the conversation list.
    #[test]
    fn the_toolbelt_switches_the_sidebar_between_the_tree_and_the_search() {
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
            drawn.iter().any(|run| run == "notes.md"),
            "the explorer draws the directory's children: {drawn:?}"
        );
        assert!(
            !drawn.iter().any(|run| run == "New conversation"),
            "the explorer is not the conversation list: {drawn:?}"
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

    /// The toolbelt's tabs are real controls, and a view is rewound when it is
    /// entered: a conversations list scrolled halfway down is at the top again
    /// when the user comes back to it.
    #[test]
    fn clicking_a_tab_switches_the_view_and_the_view_opens_at_the_top() {
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
            // Expanded, and long enough to overflow the band, so there is
            // somewhere to scroll.
            s.conversations_expanded = true;
            s.conversations = (0..40)
                .map(|index| {
                    conversation(&format!("c{index}"), &format!("Agent {index}"), "Frontend")
                })
                .collect();
        }

        let mut root: Box<dyn Element> = Box::new(view);
        let window = vec2f(1024.0, 768.0);
        // A wheel event carries no position; it is attributed by the last paint,
        // so the pointer has to be over the band while the tree is painted.
        let over_the_band = vec2f(crate::ui::SIDEBAR_WIDTH / 2.0, 400.0);
        let paint = |root: &mut Box<dyn Element>| -> Vec<RenderCommand> {
            let _ = root.layout(
                SizeConstraint::loose(window),
                &mut LayoutContext::default(),
                &app,
            );
            let mut ctx = PaintContext::new(Renderer::new());
            ctx.cursor_inside = true;
            ctx.cursor_position = over_the_band;
            root.paint(vec2f(0.0, 0.0), &mut ctx, &app);
            ctx.renderer.expect("renderer").commands().to_vec()
        };
        /// Click the toolbelt tab whose icon the last paint drew under `name`.
        fn click_tab(root: &mut Box<dyn Element>, commands: &[RenderCommand], name: &str, app: &AppContext) {
            use goble_ui::event::DispatchedEvent;

            let icon = commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::DrawIcon { name: drawn, origin, .. }
                        if drawn == name && origin.x < crate::ui::SIDEBAR_WIDTH =>
                    {
                        Some(vec2f(origin.x, origin.y))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("the {name} tab's icon is drawn"));
            let at = vec2f(icon.x + 7.0, icon.y + 7.0);
            let mut ctx = goble_ui::elements::EventContext::default();
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
                root.dispatch_event(&event, &mut ctx, app);
            }
        }

        let commands = paint(&mut root);
        let scroll = state.borrow().sidebar_scroll.clone();
        assert!(
            scroll.borrow().max_offset() > 0.0,
            "the list overflows the band: {:?}",
            scroll.borrow().max_offset()
        );

        let mut ctx = goble_ui::elements::EventContext::default();
        root.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: over_the_band,
            },
            &mut ctx,
            &app,
        );
        root.dispatch_event(
            &DispatchedEvent::Scroll {
                delta: vec2f(0.0, -40.0),
            },
            &mut ctx,
            &app,
        );
        assert!(
            scroll.borrow().offset() > 0.0,
            "the list scrolled away from the top"
        );

        click_tab(&mut root, &commands, "folder-closed", &app);
        assert_eq!(
            state.borrow().sidebar_view,
            SidebarView::Explorer,
            "the explorer's tab selected the explorer"
        );

        let commands = paint(&mut root);
        click_tab(&mut root, &commands, "message-chat-square", &app);
        assert_eq!(
            state.borrow().sidebar_view,
            SidebarView::Agents,
            "the agents tab selected the conversations"
        );
        assert_eq!(
            scroll.borrow().offset(),
            0.0,
            "coming back to the conversations starts at the top"
        );
    }

    /// The box a frame painted a card's 3-dot control in: the control's box is
    /// the glyph's own box, so the icon command is its bounds. It is looked for
    /// inside the sidebar's own band, because the pane header draws the same
    /// glyph.
    fn dots_box(commands: &[RenderCommand], band: f32) -> Option<RectF> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawIcon {
                name,
                origin,
                size,
                ..
            } if name == "dots-horizontal" && origin.x < band => Some(RectF::new(
                goble_ui::geometry::PointF::new(origin.x, origin.y),
                goble_ui::geometry::Size2F::new(*size, *size),
            )),
            _ => None,
        })
    }

    /// A subject wider than any sidebar: the card is a full-width band, so this
    /// is the title that overflows the row it sits in.
    const LONG_SUBJECT: &str =
        "A conversation whose subject is far wider than the sidebar it is listed in";

    /// The sidebar's own frame with every conversation titled [`LONG_SUBJECT`],
    /// `width` points wide and the list collapsed or expanded.
    fn long_titled_view(
        app: &AppContext,
        desktop: &Arc<DesktopState>,
        width: f32,
        expanded: bool,
    ) -> crate::root_view::RootView {
        let view = seeded_view(app, desktop);
        {
            let state = view.state_rc();
            let mut s = state.borrow_mut();
            s.sidebar_width = width;
            s.conversations_expanded = expanded;
            s.conversations = (0..5)
                .map(|index| conversation(&format!("c{index}"), LONG_SUBJECT, "Frontend"))
                .collect();
        }
        view
    }

    /// A card's dots are hover affordances: a frame of the untouched sidebar
    /// draws none, the pointer arriving on a card draws that card's, and the
    /// pointer leaving takes them away again — the hover lives in app state, so
    /// it has to be cleared, not merely rebuilt.
    #[test]
    fn the_cards_dots_follow_the_pointer_on_and_off_the_card() {
        use goble_ui::event::DispatchedEvent;

        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let view = seeded_view(&app, &desktop);
        // One shared card-state cell per conversation, the way
        // `refresh_conversations` seeds them: the hover lives in that cell, so a
        // card built without one forgets the pointer when the frame is rebuilt.
        {
            let state = view.state_rc();
            let mut s = state.borrow_mut();
            for id in ["c1", "c2", "c3", "c4", "c5"] {
                s.agent_cards
                    .insert(id.to_string(), Rc::new(RefCell::new(AgentCardUi::default())));
            }
        }
        let mut root: Box<dyn Element> = Box::new(view);
        let window = vec2f(1024.0, 768.0);
        let away = vec2f(700.0, 400.0);
        let paint = |root: &mut Box<dyn Element>, at: goble_ui::geometry::Vector2F| {
            let _ = root.layout(
                SizeConstraint::loose(window),
                &mut LayoutContext::default(),
                &app,
            );
            let mut ctx = PaintContext::new(Renderer::new());
            ctx.cursor_inside = true;
            ctx.cursor_position = at;
            root.paint(vec2f(0.0, 0.0), &mut ctx, &app);
            ctx.renderer.expect("renderer").commands().to_vec()
        };

        // The pointer is out over the main pane: no card draws its dots.
        let idle = paint(&mut root, away);
        assert!(
            dots_box(&idle, crate::ui::SIDEBAR_WIDTH).is_none(),
            "a sidebar no pointer is over draws no dots"
        );

        // Nor over a row of the sidebar that is not a card: the hover is the
        // card's own rectangle, not the band's.
        let on_a_header = idle
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Frontend" && origin.x < crate::ui::SIDEBAR_WIDTH =>
                {
                    Some(vec2f(origin.x, origin.y + 3.0))
                }
                _ => None,
            })
            .expect("the folder header is drawn");
        let mut ctx = goble_ui::elements::EventContext::default();
        root.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: on_a_header,
            },
            &mut ctx,
            &app,
        );
        let on_band = paint(&mut root, on_a_header);
        assert!(
            dots_box(&on_band, crate::ui::SIDEBAR_WIDTH).is_none(),
            "a row that is not a card draws no dots"
        );

        // Aim at the first card's own subject: the card's bounds are its whole
        // rectangle, padding included, so a point on its text is on the card.
        let on_card = idle
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "Ada" => {
                    Some(vec2f(origin.x, origin.y + 3.0))
                }
                _ => None,
            })
            .expect("the first card's subject is drawn");

        root.dispatch_event(
            &DispatchedEvent::MouseMove { position: on_card },
            &mut ctx,
            &app,
        );
        let hovered = paint(&mut root, on_card);
        let dots = dots_box(&hovered, crate::ui::SIDEBAR_WIDTH)
            .expect("the card under the pointer draws its dots");
        assert!(
            dots.max_x() <= crate::ui::SIDEBAR_WIDTH,
            "the dots are inside the sidebar: {dots:?}"
        );
        // The card's own hover band comes with them: the band is the card's
        // rectangle, and it covers the dots it just revealed.
        assert!(
            hovered.iter().any(|command| match command {
                RenderCommand::FillRect { rect, color, .. } => {
                    *color == app.theme.color(ColorToken::Hover)
                        && rect.min_x() <= dots.min_x()
                        && dots.max_x() <= rect.max_x()
                        && rect.min_y() <= dots.min_y()
                        && dots.max_y() <= rect.max_y()
                }
                _ => false,
            }),
            "the hovered card paints its band under the dots"
        );

        // The pointer leaves the sidebar again: the dots go with it.
        root.dispatch_event(
            &DispatchedEvent::MouseMove { position: away },
            &mut ctx,
            &app,
        );
        let left = paint(&mut root, away);
        assert!(
            dots_box(&left, crate::ui::SIDEBAR_WIDTH).is_none(),
            "a card the pointer has left draws no dots"
        );
    }

    /// The dots sit inside the sidebar, 5 pt from its own edge, at every sidebar
    /// width, in the collapsed and the expanded list, and with the card's menu
    /// open — and a title wider than the card cannot move them: the row's text
    /// is clipped clear of them.
    #[test]
    fn the_cards_dots_stay_five_pt_inside_the_sidebar_at_every_width_and_state() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();

        for width in [200.0_f32, 300.0, 480.0] {
            for expanded in [false, true] {
                for (hover, menu_open) in [(true, false), (true, true)] {
                    let case = format!(
                        "width {width}, list expanded {expanded}, menu open {menu_open}"
                    );
                    let view = long_titled_view(&app, &desktop, width, expanded);
                    view.state_rc().borrow_mut().agent_cards.insert(
                        "c0".to_string(),
                        Rc::new(RefCell::new(AgentCardUi { hover, menu_open })),
                    );
                    let mut root: Box<dyn Element> = Box::new(view);
                    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);

                    let dots = dots_box(&commands, width)
                        .unwrap_or_else(|| panic!("{case}: the hovered card draws its dots"));
                    assert_eq!(
                        dots.max_x(),
                        width - CARD_DOTS_INSET,
                        "{case}: the dots' right edge is 5 pt inside the sidebar: {dots:?}"
                    );
                    assert!(
                        dots.min_x() >= 0.0,
                        "{case}: and the control is inside the band: {dots:?}"
                    );

                    // The title is the widest thing in the row, so the row is
                    // clipped before the dots: the band's narrowest clip ends at
                    // or left of the control.
                    let row_clip = commands
                        .iter()
                        .filter_map(|command| match command {
                            RenderCommand::ClipRect(rect) if rect.max_x() <= width => {
                                Some(*rect)
                            }
                            _ => None,
                        })
                        .min_by(|a, b| a.max_x().total_cmp(&b.max_x()))
                        .unwrap_or_else(|| panic!("{case}: the band is painted clipped"));
                    assert!(
                        row_clip.max_x() <= dots.min_x(),
                        "{case}: the row is clipped clear of the dots, clip {row_clip:?} vs dots {dots:?}"
                    );
                    assert!(
                        row_clip.min_y() <= dots.min_y() && row_clip.max_y() >= dots.max_y(),
                        "{case}: and that clip covers the control's band: {row_clip:?}"
                    );
                }
            }
        }
    }

    /// "View all (N)" / "Show less" is the list's own control: its label and its
    /// outline both carry the app's text colour, not the accent or the surface's
    /// hairline border, so it reads on the surface it sits on.
    #[test]
    fn the_view_all_button_draws_its_label_and_border_in_the_text_colour() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let view = seeded_view(&app, &desktop);
        let mut root: Box<dyn Element> = Box::new(view);
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);

        let (label_at, label_color) = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    text, origin, color, ..
                } if text == "View all (5)" => Some((*origin, *color)),
                _ => None,
            })
            .expect("the collapsed list offers View all (5)");
        assert_eq!(
            label_color,
            app.theme.color(ColorToken::Text),
            "the label carries the text colour"
        );

        let outline = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::StrokeRect { rect, color, .. }
                    if rect.min_x() <= label_at.x
                        && label_at.x <= rect.max_x()
                        && rect.min_y() <= label_at.y
                        && label_at.y <= rect.max_y() =>
                {
                    Some(*color)
                }
                _ => None,
            })
            .expect("the button draws its outline around its label");
        assert_eq!(
            outline,
            app.theme.color(ColorToken::Text),
            "and the outline matches the label"
        );
    }

    /// Lay the mounted root out and paint it with the pointer at `at`, the way
    /// the window's own redraw does: hover is read at paint time.
    fn frame_at(
        root: &mut Box<dyn Element>,
        app: &AppContext,
        at: goble_ui::geometry::Vector2F,
    ) -> Vec<RenderCommand> {
        let _ = root.layout(
            SizeConstraint::loose(vec2f(1024.0, 768.0)),
            &mut LayoutContext::default(),
            app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = at;
        root.paint(vec2f(0.0, 0.0), &mut ctx, app);
        ctx.renderer.expect("renderer").commands().to_vec()
    }

    /// A point on the sidebar band's run of `text`, a few points into its box so
    /// it lands on the control rather than on the run's own top-left corner.
    fn on_band_text(commands: &[RenderCommand], text: &str) -> goble_ui::geometry::Vector2F {
        let origin = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    text: run, origin, ..
                } if run == text && origin.x < crate::ui::SIDEBAR_WIDTH => Some(*origin),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{text:?} is drawn in the sidebar's band"));
        vec2f(origin.x + 3.0, origin.y + 3.0)
    }

    /// Where the sidebar's band painted each run of `text`, in paint order. A
    /// box the frame draws twice (the tray, once inside its card and once as the
    /// frame's last layer) has two entries, and the last one is the visible one.
    fn band_runs(commands: &[RenderCommand], text: &str) -> Vec<usize> {
        commands
            .iter()
            .enumerate()
            .filter(|(_, command)| {
                matches!(command, RenderCommand::DrawText { text: run, origin, .. }
                    if run == text && origin.x < crate::ui::SIDEBAR_WIDTH)
            })
            .map(|(index, _)| index)
            .collect()
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

    /// The card's tray opens as a card of its own anchored to the 3-dot control
    /// — below it and trailing it — rather than as rows under the card. It is
    /// drawn by the frame's last layer, so the cards listed after this one
    /// cannot cover it; a pointer on it is still on the card's own surface; and
    /// its rows act on the conversation the way they promise.
    #[test]
    fn the_cards_tray_hangs_from_its_control_above_the_cards_after_it() {
        use goble_ui::event::DispatchedEvent;

        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let view = seeded_view(&app, &desktop);
        let state = view.state_rc();
        {
            let mut s = state.borrow_mut();
            for id in ["c1", "c2", "c3", "c4", "c5"] {
                s.agent_cards
                    .insert(id.to_string(), Rc::new(RefCell::new(AgentCardUi::default())));
            }
        }
        let mut root: Box<dyn Element> = Box::new(view);
        let mut ctx = goble_ui::elements::EventContext::default();
        let away = vec2f(700.0, 400.0);

        // The pointer arrives on the first card, and its control appears.
        let on_card = on_band_text(&frame_at(&mut root, &app, away), "Ada");
        root.dispatch_event(
            &DispatchedEvent::MouseMove { position: on_card },
            &mut ctx,
            &app,
        );
        let hovered = frame_at(&mut root, &app, on_card);
        let dots = dots_box(&hovered, crate::ui::SIDEBAR_WIDTH)
            .expect("the card under the pointer draws its dots");

        // A release on the control opens the tray.
        let on_dots = vec2f(
            dots.min_x() + dots.width() / 2.0,
            dots.min_y() + dots.height() / 2.0,
        );
        root.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: on_dots,
                button: 0,
            },
            &mut ctx,
            &app,
        );
        let open = frame_at(&mut root, &app, on_dots);

        let star_row = on_band_text(&open, "Star");
        let delete_row = on_band_text(&open, "Delete agent");
        assert!(delete_row.y > star_row.y, "star is the tray's first row");

        // The tray's own card, around its rows.
        let tray = open
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                    ..
                } if *color == app.theme.color(ColorToken::SurfaceRaised)
                    && *corner_radius > 0.0
                    && rect.min_x() <= star_row.x
                    && star_row.x <= rect.max_x()
                    && rect.min_y() <= star_row.y
                    && star_row.y <= rect.max_y() =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the tray is a card of its own around its rows");
        assert!(
            tray.min_y() > dots.max_y(),
            "the tray hangs below the control: tray {tray:?} vs dots {dots:?}"
        );
        assert!(
            (tray.max_x() - dots.max_x()).abs() < 0.01,
            "its trailing edge is the control's: tray {tray:?} vs dots {dots:?}"
        );
        assert!(
            tray.max_x() <= crate::ui::SIDEBAR_WIDTH,
            "and the tray stays inside the sidebar: {tray:?}"
        );

        // The cards painted after this one cannot cover it: the tray's own runs
        // come after them.
        let next_card = band_runs(&open, "Coder");
        let next_card = *next_card.first().expect("the next card is drawn");
        let star_runs = band_runs(&open, "Star");
        assert!(
            *star_runs.last().expect("the tray's star row is drawn") > next_card,
            "the tray is drawn after the cards that follow it: {star_runs:?} vs {next_card}"
        );

        // The pointer walks off the card's bottom edge into the tray's strip:
        // the strip is the tray's, so the tray is still open when the pointer
        // arrives at the row. The strip is the few points between the card's
        // bottom and the tray's own top, so its middle is a couple of points
        // above the tray.
        let crossing = vec2f(tray.min_x() + tray.width() / 2.0, tray.min_y() - 2.0);
        root.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: crossing,
            },
            &mut ctx,
            &app,
        );
        assert!(
            state.borrow().agent_cards["c1"].borrow().menu_open,
            "the strip between the card and its tray keeps the tray open"
        );

        // The pointer moves onto the tray: it is the control's own surface, so
        // the tray stays — even though it lies outside the card's own band.
        root.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: delete_row,
            },
            &mut ctx,
            &app,
        );
        assert!(
            state.borrow().agent_cards["c1"].borrow().menu_open,
            "a pointer on the tray does not put it away"
        );
        let still_open = frame_at(&mut root, &app, delete_row);
        assert!(
            !band_runs(&still_open, "Delete agent").is_empty(),
            "and the next frame still draws the tray"
        );

        // Its first row acts: a press and release on Star stars the
        // conversation, which is also what puts the tray away.
        root.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: star_row,
            },
            &mut ctx,
            &app,
        );
        for event in [
            DispatchedEvent::MouseDown {
                position: star_row,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: star_row,
                button: 0,
            },
        ] {
            root.dispatch_event(&event, &mut ctx, &app);
        }
        assert!(
            state.borrow().starred_conversations.contains("c1"),
            "the tray's star row stars the conversation"
        );
        assert!(
            !state.borrow().agent_cards["c1"].borrow().menu_open,
            "and starring puts the tray away"
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
        assert!(s.collapsed_sections.contains("starred"), "the other section is untouched");
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

    /// A file row opens the file in a pane of its own: the tree hands the path
    /// to the pane tree, and the input the user was typing in is left alone.
    #[test]
    fn a_file_click_opens_the_file_in_a_pane_not_in_the_input() {
        let (state, actions, dir) = harness();
        let spaced = dir.path().join("my notes.md");
        std::fs::write(&spaced, "hi").unwrap();
        let path = spaced.to_string_lossy().to_string();

        (actions.on_explorer_file_click.borrow_mut())(path.clone());
        let s = state.borrow();
        let pane_id = s.active_pane_id;
        assert_eq!(
            s.spaces[s.active_space].leaf_kind(pane_id),
            Some(&crate::ui::PaneKind::File { path }),
            "the clicked file opens in a pane of its own"
        );
        assert_eq!(
            s.composer_draft, "",
            "the path is not typed into the pane's input"
        );
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
