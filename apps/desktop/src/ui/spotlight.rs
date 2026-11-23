//! Q: the topbar Search opens the centered spotlight window — a scope pill, a
//! query field, a right-hand options menu (sort + filters) and a real results
//! list. It paints above the shell and is dismissed with Escape or an outside
//! press.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::conversation_list_item::sidebar_relative_time;
use goble_ui::elements::{
    caret_beam, AppContext, Border, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets,
    Element, Empty, EventContext, Expanded, Fill, Flex, HoverRow, Icon, KeyHandler, LayoutContext,
    MainAxisSize, PaintContext, Point, Scrollable, SizeConstraint, Text,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{vec2f, Vector2F};
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::ColorToken;
use goble_ui::ColorU;

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

/// The spotlight's own colours. The reference is a near-black app with a
/// slightly lifted card; the shell preview is a touch lighter, so the card keeps
/// the reference's relative lift instead of blending into the pane behind it.
struct SpotPalette {
    card: ColorU,
    border: ColorU,
    raised: ColorU,
    text: ColorU,
    muted: ColorU,
    header_muted: ColorU,
    icon: ColorU,
    menu: ColorU,
}

fn palette(app: &AppContext) -> SpotPalette {
    if app.theme.name == "dark" {
        SpotPalette {
            // X: the reference card fill, so the black content material behind
            // it still reads as a lifted rounded card.
            card: ColorU::hex(0x17161B),
            border: ColorU::hex(0x323136),
            raised: ColorU::hex(0x323136),
            text: ColorU::hex(0xF8F7FC),
            muted: ColorU::hex(0x918F97),
            header_muted: ColorU::hex(0xA6A8AE),
            icon: ColorU::hex(0xA5A4AA),
            menu: ColorU::hex(0x1C1B20),
        }
    } else {
        SpotPalette {
            card: app.theme.color(ColorToken::Surface),
            border: app.theme.color(ColorToken::Border),
            raised: app.theme.color(ColorToken::Selected),
            text: app.theme.color(ColorToken::Text),
            muted: app.theme.color(ColorToken::Muted),
            header_muted: app.theme.color(ColorToken::Muted),
            icon: app.theme.color(ColorToken::Muted),
            menu: app.theme.color(ColorToken::SurfaceRaised),
        }
    }
}

/// Card geometry taken from the 2364x1334 reference (a 2x 1182x667 window).
const CARD_WIDTH_RATIO: f32 = 0.542;
const CARD_TOP_RATIO: f32 = 0.25;
const CARD_RADIUS: f32 = 8.0;
const ROW_HEIGHT: f32 = 36.0;
const ROW_BAND_HEIGHT: f32 = 28.0;
const MENU_TOP: f32 = 33.0;

/// Build the full-window spotlight layer. It is inert (an empty element) while
/// closed; when open it paints the centered window and the two anchored menus
/// above the shell. The outside press target is transparent, as in the
/// reference (the shell behind is not dimmed).
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
    // The scrim only catches presses: it paints nothing at any hover state, so
    // the workspace behind the spotlight stays fully visible.
    let scrim = Scrim::new(window, move || {
        (close.borrow_mut())(SpotlightAction::Close)
    })
    .finish();

    let width = (window.x * CARD_WIDTH_RATIO).clamp(520.0, 1000.0);
    let x = (window.x - width) * 0.5;
    let y = (window.y * CARD_TOP_RATIO).max(48.0);
    // The card lives inside the window: its results scroll in the room that is
    // actually left, never past the bottom edge.
    let content_max_height = (window.y - y - 24.0).max(160.0);

    let scope_width = (width * 0.21).max(132.0);
    let options_width = (width * 0.30).max(178.0);

    let card = build_window(app, state, actions, width, content_max_height);
    let scope_menu = *spotlight.scope_menu_open.borrow();
    let options_menu = *spotlight.options_menu_open.borrow();

    let mut stack = goble_ui::elements::Stack::new().with_children(vec![scrim]);
    stack = stack.with_overlay(card, vec2f(x, y));
    if scope_menu {
        stack = stack.with_overlay(
            build_scope_menu(app, state, actions, scope_width),
            vec2f(x + 4.0, y + MENU_TOP),
        );
    }
    if options_menu {
        stack = stack.with_overlay(
            build_options_menu(app, state, actions, options_width),
            vec2f(x + width - options_width - 4.0, y + MENU_TOP),
        );
    }
    let query = spotlight.query.clone();
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
            "Backspace" => {
                let mut next = query.clone();
                next.pop();
                (keys.borrow_mut())(SpotlightAction::Query(next));
                true
            }
            typed if typed.chars().count() == 1 => {
                let mut next = query.clone();
                next.push_str(typed);
                (keys.borrow_mut())(SpotlightAction::Query(next));
                true
            }
            _ => false,
        }
    })
    .finish();
    element
}

/// The spotlight's outside-press catcher: full-window, paints nothing (the
/// workspace stays visible), consumes the pointer and closes on a press.
struct Scrim {
    size: Option<Vector2F>,
    origin: Option<Point>,
    window: Vector2F,
    on_close: Rc<RefCell<dyn FnMut()>>,
}

impl Scrim {
    fn new(window: Vector2F, on_close: impl FnMut() + 'static) -> Self {
        Self {
            size: None,
            origin: None,
            window,
            on_close: Rc::new(RefCell::new(on_close)),
        }
    }

    fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for Scrim {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        self.size = Some(self.window.min(constraint.max));
        self.window
    }

    fn paint(&mut self, origin: Vector2F, _ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
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
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        match event {
            // The scrim owns the pointer: no hover, scroll or press reaches the
            // workspace behind the open spotlight.
            DispatchedEvent::MouseDown { .. } => {
                (self.on_close.borrow_mut())();
                true
            }
            DispatchedEvent::MouseMove { .. }
            | DispatchedEvent::MouseUp { .. }
            | DispatchedEvent::Scroll { .. } => true,
            _ => false,
        }
    }
}

fn build_window(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    width: f32,
    content_max_height: f32,
) -> Box<dyn Element> {
    let spotlight = &state.spotlight;
    let colors = palette(app);
    let dispatch = actions.on_spotlight.clone();
    let scope = dispatch.clone();
    let options = dispatch.clone();

    let pill = {
        let pill_content = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(7.0)
            .with_child(
                Text::new(spotlight.scope.label())
                    .with_font_size(14.0)
                    .with_weight(FontWeight::Medium)
                    .with_color(colors.text)
                    .finish(),
            )
            .with_child(
                Icon::new("chevron-down")
                    .with_size(9.0)
                    .with_color(colors.muted)
                    .finish(),
            )
            .finish();
        HoverRow::new(
            Container::new(pill_content)
                .with_background(Fill::Solid(colors.raised))
                .with_corner_radius(10.0)
                .with_padding(EdgeInsets::new(10.5, 7.0, 10.5, 7.0))
                .finish(),
        )
        .with_on_click(move || (scope.borrow_mut())(SpotlightAction::ToggleScopeMenu))
        .finish()
    };

    let input = search_input(app, spotlight, &colors);

    let options_button = HoverRow::new(
        Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(1.0)
                .with_child(filter_bar(&colors, 11.0))
                .with_child(filter_bar(&colors, 7.5))
                .with_child(filter_bar(&colors, 3.5))
                .finish(),
        )
        .with_padding(EdgeInsets::uniform(5.0))
        .with_corner_radius(6.0)
        .finish(),
    )
    .with_hover_token(ColorToken::Selected)
    .with_on_click(move || (options.borrow_mut())(SpotlightAction::ToggleOptionsMenu))
    .finish();

    let header = Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(4.0)
            .with_child(pill)
            .with_child(Expanded::new(input).finish())
            .with_child(options_button)
            .finish(),
    )
    .with_padding(EdgeInsets::new(4.0, 3.0, 8.0, 4.0))
    .finish();

    let content_width = (width - 2.0).max(0.0);
    let separator = Container::new(Empty::new().with_size(vec2f(content_width, 1.0)).finish())
        .with_background(Fill::Solid(colors.border))
        .finish();

    let column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header)
        .with_child(separator)
        .with_child(build_results(app, state, actions, content_max_height));
    ConstrainedBox::new(
        Container::new(column.finish())
            .with_background(Fill::Solid(colors.card))
            .with_border(Border::all(1.0).with_border_color(colors.border))
            .with_corner_radius(CARD_RADIUS)
            .finish(),
    )
    .with_width(width)
    .finish()
}

/// The borderless query line: the reference has no input box, just the scope
/// pill, the placeholder and the options button on one row.
fn search_input(
    app: &AppContext,
    spotlight: &SpotlightState,
    colors: &SpotPalette,
) -> Box<dyn Element> {
    let empty = spotlight.query.is_empty();
    let display = if empty {
        "Search for conversations...".to_string()
    } else {
        spotlight.query.clone()
    };
    let color = if empty { colors.muted } else { colors.text };
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center);
    if spotlight.focused {
        row = row.with_child(caret_beam(app));
    }
    row = row.with_child(
        Text::new(display)
            .with_font_size(14.0)
            .with_color(color)
            .with_max_lines(1)
            .finish(),
    );
    Container::new(row.finish())
        .with_padding(EdgeInsets::new(6.0, 0.0, 6.0, 0.0))
        .finish()
}

fn filter_bar(colors: &SpotPalette, width: f32) -> Box<dyn Element> {
    Container::new(Empty::new().with_size(vec2f(width, 1.5)).finish())
        .with_background(Fill::Solid(colors.icon))
        .with_corner_radius(0.8)
        .finish()
}

fn build_results(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    content_max_height: f32,
) -> Box<dyn Element> {
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
                    .with_font_size(14.0)
                    .with_color(palette(app).muted)
                    .finish(),
            )
            .with_padding(EdgeInsets::new(16.0, 9.0, 16.0, 9.0))
            .finish(),
        );
    }
    let height = if count == 0 {
        ROW_HEIGHT
    } else {
        (count as f32 * ROW_HEIGHT).min(340.0).min((content_max_height - 52.0).max(36.0))
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
    let colors = palette(app);
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0)
        .with_child(leading_icon(app, icon, &colors))
        .with_child(
            Expanded::new(
                Text::new(title)
                    .with_font_size(14.0)
                    .with_color(colors.text)
                    .with_max_lines(1)
                    .finish(),
            )
            .finish(),
        );
    if !trailing.is_empty() {
        row = row.with_child(
            Text::new(trailing)
                .with_font_size(14.0)
                .with_color(colors.muted)
                .finish(),
        );
    }
    let band = Container::new(row.finish())
        .with_padding(EdgeInsets::new(12.0, 5.0, 13.5, 5.0))
        .with_corner_radius(6.0);
    let band = if selected {
        band.with_background(Fill::Solid(colors.raised))
    } else {
        band
    };
    HoverRow::new(
        Container::new(band.finish())
            .with_padding(EdgeInsets::new(
                (ROW_HEIGHT - ROW_BAND_HEIGHT) * 0.5,
                4.0,
                (ROW_HEIGHT - ROW_BAND_HEIGHT) * 0.5,
                4.0,
            ))
            .finish(),
    )
    .with_hover_token(ColorToken::Selected)
    .with_corner_radius(6.0)
    .with_on_click(on_click)
    .finish()
}

/// The reference's leading conversation mark: a small outlined speech bubble.
/// Goble has no matching asset, so the bubble is drawn from a rounded casing
/// (with a tail bar) at the row's own 16 pt icon box.
fn leading_icon(app: &AppContext, icon: &'static str, colors: &SpotPalette) -> Box<dyn Element> {
    if icon == "conversation-local" {
        let bubble = Container::new(Empty::new().with_size(vec2f(12.0, 9.0)).finish())
            .with_border(Border::all(1.2).with_border_color(colors.icon))
            .with_corner_radius(3.0);
        let tail = Container::new(Empty::new().with_size(vec2f(3.5, 1.2)).finish())
            .with_background(Fill::Solid(colors.icon))
            .finish();
        let mark = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(0.0)
            .with_child(bubble.finish())
            .with_child(
                Container::new(tail)
                    .with_padding(EdgeInsets::new(1.0, 0.0, 0.0, 0.0))
                    .finish(),
            )
            .finish();
        return Container::new(mark)
            .with_padding(EdgeInsets::new(1.6, 0.0, 1.6, 0.0))
            .finish();
    }
    Icon::new(icon)
        .with_size(16.0)
        .with_color(colors.icon)
        .finish()
}

fn build_scope_menu(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    width: f32,
) -> Box<dyn Element> {
    let colors = palette(app);
    let dispatch = actions.on_spotlight.clone();
    let mut menu = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(0.0);
    for scope in [
        SpotlightScope::Commands,
        SpotlightScope::Files,
        SpotlightScope::Conversations,
        SpotlightScope::Projects,
    ] {
        let on_select = dispatch.clone();
        let selected = state.spotlight.scope == scope;
        menu = menu.with_child(menu_row(
            app,
            &colors,
            scope.label(),
            selected,
            true,
            13.0,
            move || (on_select.borrow_mut())(SpotlightAction::Scope(scope)),
        ));
    }
    menu_card(
        app,
        &colors,
        menu.finish(),
        width,
        EdgeInsets::new(4.0, 9.0, 4.0, 7.0),
    )
}

fn build_options_menu(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    width: f32,
) -> Box<dyn Element> {
    let colors = palette(app);
    let dispatch = actions.on_spotlight.clone();
    let mut menu = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(0.0);
    menu = menu.with_child(menu_header(app, &colors, "Sort Conversations"));
    menu = menu.with_child(menu_spacer(3.0));
    for sort in [SpotlightSort::LastUpdated, SpotlightSort::LastPrompt] {
        let on_select = dispatch.clone();
        let selected = state.spotlight.sort == sort;
        menu = menu.with_child(menu_row(
            app,
            &colors,
            sort.label(),
            selected,
            false,
            9.5,
            move || (on_select.borrow_mut())(SpotlightAction::Sort(sort)),
        ));
    }
    menu = menu.with_child(menu_spacer(4.0));
    menu = menu.with_child(menu_divider(&colors, width - 8.0));
    menu = menu.with_child(menu_header(app, &colors, "Filter"));
    menu = menu.with_child(menu_spacer(3.0));
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
        menu = menu.with_child(menu_row(
            app,
            &colors,
            label,
            selected,
            false,
            9.5,
            move || (on_select.borrow_mut())(SpotlightAction::ToggleFilter(filter)),
        ));
    }
    menu_card(
        app,
        &colors,
        menu.finish(),
        width,
        EdgeInsets::new(4.0, 0.0, 4.0, 6.0),
    )
}

fn menu_header(app: &AppContext, colors: &SpotPalette, label: &str) -> Box<dyn Element> {
    let _ = app;
    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new(label)
                    .with_font_size(13.0)
                    .with_color(colors.header_muted)
                    .finish(),
            )
            .finish(),
    )
    .with_padding(EdgeInsets::new(8.0, 3.5, 8.0, 3.5))
    .finish()
}

fn menu_spacer(height: f32) -> Box<dyn Element> {
    Container::new(Empty::new().with_size(vec2f(0.0, height)).finish()).finish()
}

fn menu_divider(colors: &SpotPalette, width: f32) -> Box<dyn Element> {
    Container::new(Empty::new().with_size(vec2f(width.max(0.0), 1.0)).finish())
        .with_background(Fill::Solid(colors.border))
        .finish()
}

fn menu_row(
    app: &AppContext,
    colors: &SpotPalette,
    label: &str,
    selected: bool,
    show_band: bool,
    right_pad: f32,
    on_click: impl FnMut() + 'static,
) -> Box<dyn Element> {
    let _ = app;
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(0.0)
        .with_child(
            Text::new(label)
                .with_font_size(13.0)
                .with_color(colors.text)
                .finish(),
        )
        .with_child(Expanded::new(Empty::new().finish()).finish());
    if selected {
        row = row.with_child(
            Icon::new("check")
                .with_size(11.0)
                .with_color(colors.text)
                .finish(),
        );
    }
    let band = Container::new(row.finish())
        .with_padding(EdgeInsets::new(7.0, 4.6, right_pad, 4.6))
        .with_corner_radius(6.0);
    let band = if selected && show_band {
        band.with_background(Fill::Solid(colors.raised))
    } else {
        band
    };
    HoverRow::new(
        Container::new(band.finish())
            .with_padding(EdgeInsets::new(2.0, 1.0, 2.0, 1.0))
            .finish(),
    )
    .with_hover_token(ColorToken::Selected)
    .with_corner_radius(6.0)
    .with_on_click(on_click)
    .finish()
}

fn menu_card(
    app: &AppContext,
    colors: &SpotPalette,
    content: Box<dyn Element>,
    width: f32,
    padding: EdgeInsets,
) -> Box<dyn Element> {
    ConstrainedBox::new(
        Container::new(content)
            .with_background(Fill::Solid(colors.menu))
            .with_border(Border::all(1.0).with_border_color(colors.border))
            .with_corner_radius(10.0)
            .with_padding(padding)
            .finish(),
    )
    .with_width(width)
    .finish()
}
