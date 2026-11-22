//! Q: the topbar Search opens the centered spotlight window — a scope pill, a
//! query field, a right-hand options menu (sort + filters) and a real results
//! list. It paints above the shell and is dismissed with Escape or an outside
//! press.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::conversation_list_item::sidebar_relative_time;
use goble_ui::elements::{
    AppContext, Border, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets, Element, Empty,
    Expanded, Fill, Flex, HoverRow, Icon, KeyHandler, MainAxisSize, Padding, Scrollable, Text,
    TextInput,
};
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::theme::{ColorToken, FontFamily};

use goble_ui::ConversationEntry;

use super::{UiActions, UiSnapshot};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpotlightScope {
    Commands,
    Files,
    #[default]
    Conversations,
    Projects,
}

impl SpotlightScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Commands => "Commands",
            Self::Files => "Files",
            Self::Conversations => "Conversations",
            Self::Projects => "Projects",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpotlightSort {
    #[default]
    LastUpdated,
    LastPrompt,
}

impl SpotlightSort {
    pub fn label(self) -> &'static str {
        match self {
            Self::LastUpdated => "Last Updated",
            Self::LastPrompt => "Last Prompt",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotlightFilter {
    Unread,
    Archived,
    Automations,
}

/// The spotlight's whole app-owned state.
#[derive(Clone, Debug)]
pub struct SpotlightState {
    pub open: bool,
    pub scope: SpotlightScope,
    pub sort: SpotlightSort,
    pub only_unread: bool,
    pub only_archived: bool,
    pub only_automations: bool,
    pub query: String,
    pub focused: bool,
    pub selected: usize,
    pub scope_menu_open: Rc<RefCell<bool>>,
    pub options_menu_open: Rc<RefCell<bool>>,
}

impl Default for SpotlightState {
    fn default() -> Self {
        Self {
            open: false,
            scope: SpotlightScope::Conversations,
            sort: SpotlightSort::LastUpdated,
            only_unread: false,
            only_archived: false,
            only_automations: false,
            query: String::new(),
            focused: false,
            selected: 0,
            scope_menu_open: Rc::new(RefCell::new(false)),
            options_menu_open: Rc::new(RefCell::new(false)),
        }
    }
}

#[derive(Clone, Debug)]
pub enum SpotlightAction {
    Open,
    Close,
    Escape,
    Query(String),
    Scope(SpotlightScope),
    Sort(SpotlightSort),
    ToggleFilter(SpotlightFilter),
    ToggleScopeMenu,
    ToggleOptionsMenu,
    Select(usize),
    Move(i32),
    Activate,
}

/// The conversation rows the current query/filters allow, in display order.
pub fn filtered_conversations<'a>(
    conversations: &'a [ConversationEntry],
    spotlight: &SpotlightState,
) -> Vec<&'a ConversationEntry> {
    let query = spotlight.query.trim().to_lowercase();
    let mut rows: Vec<&ConversationEntry> = conversations
        .iter()
        .filter(|entry| {
            if spotlight.only_archived {
                entry.archived
            } else {
                !entry.archived
            }
        })
        .filter(|entry| {
            !spotlight.only_unread
                || matches!(
                    entry.status,
                    goble_ui::ConversationStatus::Waiting | goble_ui::ConversationStatus::Running
                )
        })
        .filter(|entry| {
            !spotlight.only_automations
                || entry.folder.eq_ignore_ascii_case("automations")
                || entry.name.to_lowercase().contains("automation")
        })
        .filter(|entry| {
            query.is_empty()
                || entry.name.to_lowercase().contains(&query)
                || entry.last_response.to_lowercase().contains(&query)
        })
        .collect();
    match spotlight.sort {
        SpotlightSort::LastUpdated => rows.sort_by(|a, b| {
            b.updated_at
                .as_deref()
                .unwrap_or(&b.timestamp)
                .cmp(a.updated_at.as_deref().unwrap_or(&a.timestamp))
        }),
        SpotlightSort::LastPrompt => rows.sort_by(|a, b| a.name.cmp(&b.name)),
    }
    rows
}

/// The unique projects (conversation folders) the query allows.
pub fn filtered_projects<'a>(conversations: &'a [ConversationEntry], query: &str) -> Vec<&'a str> {
    let query = query.trim().to_lowercase();
    let mut folders: Vec<&str> = conversations
        .iter()
        .map(|entry| entry.folder.as_str())
        .filter(|folder| !folder.trim().is_empty())
        .filter(|folder| query.is_empty() || folder.to_lowercase().contains(&query))
        .collect();
    folders.sort_unstable();
    folders.dedup();
    folders
}

/// The commands the spotlight can run.
pub fn commands() -> [(&'static str, &'static str); 3] {
    [
        ("Review changes", "Review the working tree"),
        ("Toggle sidebar", "Show or hide the tools sidebar"),
        ("Open Settings", "Open the Settings sheet"),
    ]
}

/// Build the full-window spotlight layer. It is inert (an empty element) while
/// closed; when open it paints a scrim, the centered window and the two
/// anchored menus above the shell.
pub fn build_spotlight(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    window: Vector2F,
) -> Box<dyn Element> {
    let spotlight = &state.spotlight;
    if !spotlight.open {
        return Empty::new().finish();
    }
    let dispatch = actions.on_spotlight.clone();
    let close = dispatch.clone();
    let scrim = HoverRow::new(
        Container::new(Empty::new().with_size(window).finish())
            .with_background(Fill::Solid(goble_ui::ColorU::new(0, 0, 0, 70)))
            .finish(),
    )
    .with_on_click(move || (close.borrow_mut())(SpotlightAction::Close))
    .finish();

    let width = (window.x * 0.39).clamp(520.0, 920.0);
    let x = ((window.x - width) * 0.5).max(24.0);
    let y = (window.y * 0.18).clamp(60.0, 200.0);

    let card = build_window(app, state, actions, width);
    let scope_menu = *spotlight.scope_menu_open.borrow();
    let options_menu = *spotlight.options_menu_open.borrow();

    let mut stack = goble_ui::elements::Stack::new().with_children(vec![scrim]);
    stack = stack.with_overlay(card, vec2f(x, y));
    if scope_menu {
        stack = stack.with_overlay(
            build_scope_menu(app, state, actions),
            vec2f(x + 10.0, y + 50.0),
        );
    }
    if options_menu {
        stack = stack.with_overlay(
            build_options_menu(app, state, actions),
            vec2f(x + width - 304.0, y + 50.0),
        );
    }
    let dispatch = actions.on_spotlight.clone();
    let keys = dispatch.clone();
    let element = KeyHandler::new(stack.finish(), move |key: &str, modifiers| {
        if modifiers.command || modifiers.ctrl || modifiers.alt {
            return false;
        }
        match key {
            "ArrowDown" => {
                (keys.borrow_mut())(SpotlightAction::Move(1));
                true
            }
            "ArrowUp" => {
                (keys.borrow_mut())(SpotlightAction::Move(-1));
                true
            }
            "Enter" => {
                (keys.borrow_mut())(SpotlightAction::Activate);
                true
            }
            "Escape" => {
                (keys.borrow_mut())(SpotlightAction::Escape);
                true
            }
            _ => false,
        }
    })
    .finish();
    let _ = dispatch;
    element
}

fn build_window(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    width: f32,
) -> Box<dyn Element> {
    let spotlight = &state.spotlight;
    let dispatch = actions.on_spotlight.clone();
    let scope = dispatch.clone();
    let options = dispatch.clone();
    let pill = {
        let scope = scope.clone();
        HoverRow::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(6.0)
                .with_child(
                    Text::new(spotlight.scope.label())
                        .with_font_size(14.0)
                        .with_weight(FontWeight::Medium)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .with_child(
                    Icon::new("chevron-down")
                        .with_size(11.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .finish(),
        )
        .with_padding(EdgeInsets::new(7.0, 11.0, 6.0, 11.0))
        .with_corner_radius(9.0)
        .with_hover_token(ColorToken::Selected)
        .with_on_click(move || (scope.borrow_mut())(SpotlightAction::ToggleScopeMenu))
        .finish()
    };
    let input = {
        let query = dispatch.clone();
        let input = TextInput::new()
            .with_value(spotlight.query.clone())
            .with_placeholder("Search for conversations...")
            .with_focused(spotlight.focused)
            .with_full_width(true)
            .with_on_change(move |value| (query.borrow_mut())(SpotlightAction::Query(value)));
        input.finish()
    };
    let options_button = HoverRow::new(
        Icon::new("menu-01")
            .with_size(15.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(7.0))
    .with_corner_radius(8.0)
    .with_on_click(move || (options.borrow_mut())(SpotlightAction::ToggleOptionsMenu))
    .finish();
    let header = Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(10.0)
            .with_child(pill)
            .with_child(Expanded::new(input).finish())
            .with_child(options_button)
            .finish(),
    )
    .with_padding(EdgeInsets::new(10.0, 12.0, 8.0, 12.0))
    .finish();

    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header)
        .with_child(build_results(app, state, actions));
    ConstrainedBox::new(
        Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
            .with_corner_radius(14.0)
            .finish(),
    )
    .with_width(width)
    .finish()
}

fn build_results(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let spotlight = &state.spotlight;
    let dispatch = actions.on_spotlight.clone();
    let mut rows = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(1.0);
    let mut count = 0usize;
    match spotlight.scope {
        SpotlightScope::Conversations => {
            for (index, entry) in filtered_conversations(&state.conversations, spotlight)
                .into_iter()
                .enumerate()
            {
                count += 1;
                let on_select = dispatch.clone();
                let date = entry
                    .updated_at
                    .as_deref()
                    .map(sidebar_relative_time)
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| entry.timestamp.clone());
                rows = rows.with_child(result_row(
                    app,
                    "conversation-local",
                    &entry.name,
                    &date,
                    index == spotlight.selected,
                    move || (on_select.borrow_mut())(SpotlightAction::Select(index)),
                ));
            }
        }
        SpotlightScope::Projects => {
            for (index, folder) in filtered_projects(&state.conversations, &spotlight.query)
                .into_iter()
                .enumerate()
            {
                count += 1;
                let on_select = dispatch.clone();
                rows = rows.with_child(result_row(
                    app,
                    "folder",
                    folder,
                    "",
                    index == spotlight.selected,
                    move || (on_select.borrow_mut())(SpotlightAction::Select(index)),
                ));
            }
        }
        SpotlightScope::Files => {
            let query = spotlight.query.trim().to_lowercase();
            for (index, row) in state.global_search_rows.iter().enumerate() {
                let haystack = format!("{} {} {}", row.path, row.name, row.text).to_lowercase();
                if !query.is_empty() && !haystack.contains(&query) {
                    continue;
                }
                count += 1;
                let on_select = dispatch.clone();
                let label = if row.name.is_empty() {
                    row.path.clone()
                } else {
                    row.name.clone()
                };
                rows = rows.with_child(result_row(
                    app,
                    "file",
                    &label,
                    "",
                    index == spotlight.selected,
                    move || (on_select.borrow_mut())(SpotlightAction::Select(index)),
                ));
            }
        }
        SpotlightScope::Commands => {
            let query = spotlight.query.trim().to_lowercase();
            for (index, (name, description)) in commands().into_iter().enumerate() {
                if !query.is_empty()
                    && !name.to_lowercase().contains(&query)
                    && !description.to_lowercase().contains(&query)
                {
                    continue;
                }
                count += 1;
                let on_select = dispatch.clone();
                rows = rows.with_child(result_row(
                    app,
                    "terminal-input",
                    name,
                    description,
                    index == spotlight.selected,
                    move || (on_select.borrow_mut())(SpotlightAction::Select(index)),
                ));
            }
        }
    }
    if count == 0 {
        rows = rows.with_child(
            Container::new(
                Text::new("No results found")
                    .with_font_size(13.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_padding(EdgeInsets::new(20.0, 16.0, 20.0, 18.0))
            .finish(),
        );
    }
    let height = if count == 0 {
        64.0
    } else {
        (count as f32 * 36.0 + 8.0).min(340.0)
    };
    ConstrainedBox::new(Scrollable::new(rows.finish(), goble_ui::elements::Axis::Vertical).finish())
        .with_height(height)
        .finish()
}

fn result_row(
    app: &AppContext,
    icon: &'static str,
    title: &str,
    trailing: &str,
    selected: bool,
    on_click: impl FnMut() + 'static,
) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(10.0)
        .with_child(
            Icon::new(icon)
                .with_size(16.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Expanded::new(
                Text::new(title)
                    .with_font_size(14.0)
                    .with_theme_color(ColorToken::Text, app)
                    .with_max_lines(1)
                    .finish(),
            )
            .finish(),
        );
    if !trailing.is_empty() {
        row = row.with_child(
            Text::new(trailing)
                .with_font_family(FontFamily::Mono)
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    HoverRow::new(
        Container::new(row.finish())
            .with_background(if selected {
                Fill::Solid(app.theme.color(ColorToken::Selected))
            } else {
                Fill::None
            })
            .with_padding(EdgeInsets::new(9.0, 16.0, 9.0, 16.0))
            .finish(),
    )
    .with_on_click(on_click)
    .finish()
}

fn build_scope_menu(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let dispatch = actions.on_spotlight.clone();
    let mut menu = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(1.0);
    for scope in [
        SpotlightScope::Commands,
        SpotlightScope::Files,
        SpotlightScope::Conversations,
        SpotlightScope::Projects,
    ] {
        let on_select = dispatch.clone();
        let selected = state.spotlight.scope == scope;
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.0)
            .with_child(
                Text::new(scope.label())
                    .with_font_size(13.5)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_child(Expanded::new(Empty::new().finish()).finish());
        let row = if selected {
            row.with_child(
                Icon::new("check")
                    .with_size(12.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
        } else {
            row
        };
        menu = menu.with_child(
            HoverRow::new(
                Container::new(row.finish())
                    .with_padding(EdgeInsets::new(7.0, 12.0, 7.0, 12.0))
                    .with_corner_radius(6.0)
                    .finish(),
            )
            .with_on_click(move || (on_select.borrow_mut())(SpotlightAction::Scope(scope)))
            .finish(),
        );
    }
    menu_card(app, menu.finish(), 200.0)
}

fn build_options_menu(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let dispatch = actions.on_spotlight.clone();
    let mut menu = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(1.0);
    menu = menu.with_child(menu_header(app, "Sort Conversations"));
    for sort in [SpotlightSort::LastUpdated, SpotlightSort::LastPrompt] {
        let on_select = dispatch.clone();
        let selected = state.spotlight.sort == sort;
        menu = menu.with_child(menu_row(app, sort.label(), selected, move || {
            (on_select.borrow_mut())(SpotlightAction::Sort(sort))
        }));
    }
    menu = menu.with_child(menu_header(app, "Filter"));
    for (filter, label, selected) in [
        (
            SpotlightFilter::Unread,
            "Only Unread",
            state.spotlight.only_unread,
        ),
        (
            SpotlightFilter::Archived,
            "Only Archived",
            state.spotlight.only_archived,
        ),
        (
            SpotlightFilter::Automations,
            "Only Automations",
            state.spotlight.only_automations,
        ),
    ] {
        let on_select = dispatch.clone();
        menu = menu.with_child(menu_row(app, label, selected, move || {
            (on_select.borrow_mut())(SpotlightAction::ToggleFilter(filter))
        }));
    }
    menu_card(app, menu.finish(), 292.0)
}

fn menu_header(app: &AppContext, label: &str) -> Box<dyn Element> {
    Container::new(
        Text::new(label)
            .with_font_size(11.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_padding(EdgeInsets::new(9.0, 12.0, 5.0, 12.0))
    .finish()
}

fn menu_row(
    app: &AppContext,
    label: &str,
    selected: bool,
    on_click: impl FnMut() + 'static,
) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Text::new(label)
                .with_font_size(13.5)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(Expanded::new(Empty::new().finish()).finish());
    if selected {
        row = row.with_child(
            Icon::new("check")
                .with_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        );
    }
    HoverRow::new(
        Container::new(row.finish())
            .with_padding(EdgeInsets::new(6.0, 12.0, 6.0, 12.0))
            .with_corner_radius(6.0)
            .finish(),
    )
    .with_on_click(on_click)
    .finish()
}

fn menu_card(app: &AppContext, content: Box<dyn Element>, width: f32) -> Box<dyn Element> {
    ConstrainedBox::new(
        Container::new(content)
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
            .with_corner_radius(10.0)
            .with_padding(EdgeInsets::uniform(4.0))
            .finish(),
    )
    .with_width(width)
    .finish()
}

// Quiet import used by the result rows' icon labels.
use goble_ui::platform::text_atlas::FontWeight;
