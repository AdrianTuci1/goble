//! Left conversation sidebar.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AgentCardUi, AppContext, Axis, Button, ButtonVariant, Container, ConversationEntry,
    ConversationListItem, CrossAxisAlignment, Divider, EdgeInsets, Element, Expanded, Fill, Flex,
    HoverButton, Icon, Label, LabelSize, MainAxisSize, Scrollable, SearchInput, Spacer, Text,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::{UiActions, UiSnapshot};

/// How many conversation cards the collapsed sidebar shows before the
/// "View all" button.
const COLLAPSED_CONVERSATIONS: usize = 4;

/// Left sidebar: search box, then "new conversation", then the list of
/// conversation cards grouped into folders. Divider lines separate the sections.
///
/// The environment (medium) selector lives in the topbar now, so the sidebar
/// only needs the currently selected medium's routing to filter which
/// conversation folder set is shown.
pub fn build_sidebar(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    selected_medium: &str,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let xs = app.theme.spacing_px(SpacingToken::Xs);

    // Search box: compact, slightly rounded, short placeholder.
    let on_search_change = actions.on_search_change.clone();
    let on_search_focus = actions.on_search_focus_change.clone();
    let search = SearchInput::new()
        .with_value(state.search_query.clone())
        .with_focused(state.search_focused)
        .with_placeholder("Search")
        .with_compact(true)
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
    .with_corner_radius(6.0)
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
    .with_on_click(move || (on_create_submit.borrow_mut())())
    .finish();

    // Conversation cards.
    let section_label = Label::new("Conversations")
        .with_size(LabelSize::Xs)
        .with_theme_color(ColorToken::Muted, app)
        .finish();

    // Show only the conversations that belong to the selected environment
    // (Local / Remote). Each conversation carries its `workspace_routing`
    // (`"local"` / `"remote"`); the selected medium maps to one of those via
    // [`crate::media::medium_routing`]. Conversations without an explicit
    // routing default to `local`.
    let routing = crate::media::medium_routing(selected_medium);
    let visible: Vec<&ConversationEntry> = state
        .conversations
        .iter()
        .filter(|entry| entry.workspace_routing == routing)
        .collect();

    // Collapsed, the list is a short digest: a few cards plus a "View all"
    // button. Expanding it shows every conversation in a scrollable region.
    let shown: Vec<&ConversationEntry> = if state.conversations_expanded {
        visible.clone()
    } else {
        visible.iter().take(COLLAPSED_CONVERSATIONS).copied().collect()
    };

    // Group conversations into folders (preserving insertion order), each with
    // a small section header, then the items indented under it.
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
    if visible.is_empty() {
        let empty_label = if state.conversations.is_empty() {
            "No conversations yet. Create one to begin."
        } else {
            "No conversations for this environment."
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
        let folder_header = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(
                Text::new(folder.clone())
                    .with_font_size(11.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(Spacer::new().finish())
            .with_child(
                Text::new(entries.len().to_string())
                    .with_font_size(10.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .finish();
        list = list.with_child(folder_header);
        for entry in entries {
            let click_id = entry.id.clone();
            let delete_id = entry.id.clone();
            let selected = state.selected_id.as_deref() == Some(entry.id.as_str());
            let on_select = actions.on_select_conversation.clone();
            let on_delete = actions.on_agent_delete.clone();
            let ui = state
                .agent_cards
                .get(&entry.id)
                .cloned()
                .unwrap_or_else(|| Rc::new(RefCell::new(AgentCardUi::default())));
            let item = ConversationListItem::new(
                entry.id.clone(),
                entry.name.clone(),
                entry.last_response.clone(),
                entry.timestamp.clone(),
                ui,
                selected,
            )
            .with_workspace_routing(entry.workspace_routing.clone())
            .with_on_click(move || (on_select.borrow_mut())(click_id.clone()))
            .with_on_delete(move || (on_delete.borrow_mut())(delete_id.clone()))
            .finish();
            list = list.with_child(item);
        }
    }

    // "View all" (collapsed, when there is more to see) / "Show less"
    // (expanded) — the switch that turns the list into a scrollable one.
    if shown.len() < visible.len() || state.conversations_expanded {
        let on_toggle = actions.on_toggle_conversations_expanded.clone();
        let label = if state.conversations_expanded {
            "Show less".to_string()
        } else {
            format!("View all ({})", visible.len())
        };
        list = list.with_child(
            Button::new(Text::new(label).with_font_size(11.0).finish())
                .with_variant(ButtonVariant::Ghost)
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
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(header);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(section_label);
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

    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets::new(xs, spacing, xs, spacing))
        .finish()
}
