//! Left conversation sidebar.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AgentCardUi, AppContext, Axis, Container, ConversationListItem, CrossAxisAlignment, Divider,
    EdgeInsets, Element, Fill, Flex, HoverButton, Icon, Label, LabelSize, MainAxisSize, Scrollable,
    SearchInput, Spacer, Text, TopbarButton,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::{AiActions, UiActions, UiSnapshot};

/// Left sidebar: search box on top, then "new conversation", then the list of
/// conversation cards. Divider lines separate the three sections.
pub fn build_sidebar(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    ai_actions: &AiActions,
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

    // "New agent" row: a gray "+" box in front of the label. The whole row
    // highlights on hover and creates a new agent on click. The hover flag
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
                Text::new("New agent")
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

    let mut list = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.0);
    for entry in &state.conversations {
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
        .with_on_click(move || (on_select.borrow_mut())(click_id.clone()))
        .with_on_delete(move || (on_delete.borrow_mut())(delete_id.clone()))
        .finish();
        list = list.with_child(item);
    }

    // Plugins footer -> opens the MCP connectors panel.
    let on_plugins = ai_actions.on_open_connectors.clone();
    let plugins_button = TopbarButton::new(
        Icon::new("plus")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_on_click(move || (on_plugins.borrow_mut())())
    .finish();

    let footer = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Label::new("Plugins")
                .with_size(LabelSize::Xs)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(plugins_button)
        .finish();

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(search);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(header);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(section_label);
    column = column.with_child(Scrollable::new(list.finish(), Axis::Vertical).finish());
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(footer);

    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets::new(xs, spacing, xs, spacing))
        .finish()
}
