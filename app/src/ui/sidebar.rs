//! Left conversation sidebar.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AgentCardUi, AppContext, Axis, Button, ButtonVariant, Container, ConversationEntry,
    ConversationListItem, CrossAxisAlignment, EdgeInsets, Element, Expanded, Fill, Flex,
    HoverButton, Icon, Label, LabelSize, MainAxisSize, Scrollable, SearchInput, Spacer, Text,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::{UiActions, UiSnapshot};

/// How many conversation cards the collapsed sidebar shows before the
/// "View all" button.
const COLLAPSED_CONVERSATIONS: usize = 4;

/// Extra height on each side of the search field's row: the box is a few pixels
/// taller than its text so it reads as a field rather than a label.
const SEARCH_EXTRA_HEIGHT: f32 = 3.0;

/// Left sidebar: search box, then "new conversation", then the list of
/// conversation cards grouped into folders.
///
/// The surface is flat: no rounded corners and no separator lines. The
/// environment (medium) selector lives in the topbar now, so the sidebar
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

#[cfg(test)]
mod sidebar_surface_tests {
    use super::*;
    use crate::root_view::RootView;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::{AgentCardUi, ConversationStatus};
    use goble_ui::geometry::{vec2f, RectF};
    use goble_ui::render::RenderCommand;
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

    /// Every command the sidebar band drew — a rect fully left of the workspace
    /// (the pane's frame starts at its own edge), or a run whose origin is.
    fn in_band(rect: RectF) -> bool {
        rect.max_x() <= crate::ui::SIDEBAR_WIDTH
    }

    /// The sidebar draws no rounded fill, no separator rule and no magnifier,
    /// and it does draw the runs that make those assertions mean something.
    fn assert_the_band_is_flat(commands: &[RenderCommand], case: &str) {
        let band = crate::ui::SIDEBAR_WIDTH;

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
                } if in_band(*rect) && *corner_radius > 0.0 => {
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

        let rules: Vec<String> = commands
            .iter()
            .filter_map(|command| separator(command, band).map(|rect| format!("{rect:?}")))
            .collect();
        assert!(
            rules.is_empty(),
            "{case}: the sidebar draws no separator, got {}",
            rules.join(", ")
        );

        let magnifiers = commands
            .iter()
            .filter(|command| {
                matches!(command, RenderCommand::DrawIcon { name, origin, .. }
                    if name == "search" && origin.x < band)
            })
            .count();
        assert_eq!(magnifiers, 0, "{case}: the sidebar search draws no magnifier");

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
                s.settings_overlay_open = false;
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
            assert_the_band_is_flat(&commands, &case);
        }
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
}
