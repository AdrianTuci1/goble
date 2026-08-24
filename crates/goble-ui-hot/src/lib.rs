//! Hot-reloadable UI for the native Goble app.
//!
//! This crate is compiled as a `cdylib` and loaded at runtime by
//! `hot-lib-reloader` (see `app/src/hot_ui.rs`). `build_ui` is the only
//! reloadable function; keep its signature and the shapes of [`UiSnapshot`] /
//! [`UiActions`] stable during a dev session — changing them requires
//! rebuilding `goble-app` (the executable bakes in the ABI).

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Axis, Button, ButtonVariant, ChatMessage as UiChatMessage, Container,
    ConversationEntry, ConversationListItem, CrossAxisAlignment, Divider, EdgeInsets, Element,
    EventContext, Fill, Flex, Icon, Label, LabelSize, LayoutContext, MainAxisAlignment,
    MainAxisSize, PaintContext, Point, Rect, Scrollable, SearchInput, SizeConstraint, Spacer, Text,
    TextInput, TopbarButton,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{vec2f, Vector2F};
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::{ChatView, SettingsPage, SettingsView};

/// Width of the left conversation sidebar.
pub const SIDEBAR_WIDTH: f32 = 260.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppTab {
    Threads,
    Chat,
    Settings,
}

/// Plain snapshot of the UI state used to build the tree. Owned by the host
/// app; rendered from scratch every frame so state changes show up live.
#[derive(Clone, Debug)]
pub struct UiSnapshot {
    pub current_tab: AppTab,
    pub conversations: Vec<ConversationEntry>,
    pub selected_id: Option<String>,
    pub search_query: String,
    pub search_focused: bool,
    pub new_conversation_draft: String,
    pub create_focused: bool,
    pub thread_messages: Vec<UiChatMessage>,
    pub chat_messages: Vec<UiChatMessage>,
    pub composer_draft: String,
    pub composer_focused: bool,
    pub agent_name: String,
    pub agent_busy: bool,
}

/// Callbacks supplied by the host app. Created fresh on every rebuild; they
/// mutate app-owned state, which is rendered back on the next frame.
pub struct UiActions {
    pub on_search_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_search_focus_change: Rc<RefCell<dyn FnMut(bool)>>,
    pub on_create_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_create_focus_change: Rc<RefCell<dyn FnMut(bool)>>,
    pub on_create_submit: Rc<RefCell<dyn FnMut()>>,
    pub on_select_conversation: Rc<RefCell<dyn FnMut(String)>>,
    pub on_select_tab: Rc<RefCell<dyn FnMut(AppTab)>>,
    pub on_composer_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_composer_focus_change: Rc<RefCell<dyn FnMut(bool)>>,
    pub on_send_message: Rc<RefCell<dyn FnMut(String)>>,
    pub on_attach: Rc<RefCell<dyn FnMut()>>,
    pub on_voice: Rc<RefCell<dyn FnMut()>>,
    pub on_select_model: Rc<RefCell<dyn FnMut()>>,
    pub on_copy: Rc<RefCell<dyn FnMut()>>,
    pub on_restart: Rc<RefCell<dyn FnMut()>>,
    pub on_stop: Rc<RefCell<dyn FnMut()>>,
}

/// Build the complete app UI: a full-height sidebar on the left and the main
/// content on the right.
pub fn build_ui(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let sidebar = build_sidebar(app, state, actions);
    let main = build_main(app, state, actions);
    let layout = SidebarLayout::new(sidebar, main, SIDEBAR_WIDTH);
    Container::new(layout.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .finish()
}

/// Left sidebar: search input on top, then a "new conversation" input, then
/// the list of conversation cards.
fn build_sidebar(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    // Search
    let on_search_change = actions.on_search_change.clone();
    let on_search_focus = actions.on_search_focus_change.clone();
    let search = SearchInput::new()
        .with_value(state.search_query.clone())
        .with_focused(state.search_focused)
        .with_placeholder("Search conversations…")
        .with_on_change(move |value| (on_search_change.borrow_mut())(value))
        .with_on_focus_change(move |focused| (on_search_focus.borrow_mut())(focused))
        .finish();

    // New conversation input + create button
    let on_create_change = actions.on_create_change.clone();
    let on_create_focus = actions.on_create_focus_change.clone();
    let on_create_submit = actions.on_create_submit.clone();
    let create_input = TextInput::new()
        .with_value(state.new_conversation_draft.clone())
        .with_focused(state.create_focused)
        .with_placeholder("New conversation…")
        .with_on_change(move |value| (on_create_change.borrow_mut())(value))
        .with_on_focus_change(move |focused| (on_create_focus.borrow_mut())(focused))
        .with_on_submit(move || (on_create_submit.borrow_mut())())
        .finish();

    let on_create_submit = actions.on_create_submit.clone();
    let create_button = TopbarButton::new(
        Icon::new("plus")
            .with_size(16.0)
            .with_theme_color(ColorToken::Text, app)
            .finish(),
    )
    .with_size(32.0)
    .with_on_click(move || (on_create_submit.borrow_mut())())
    .finish();

    let header = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(create_input)
        .with_child(create_button)
        .finish();

    // Conversation cards
    let section_label = Label::new("Conversations")
        .with_size(LabelSize::Xs)
        .with_theme_color(ColorToken::Muted, app)
        .finish();

    let mut list = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.0);
    for entry in &state.conversations {
        let selected = state.selected_id.as_deref() == Some(entry.id.as_str());
        let on_select = actions.on_select_conversation.clone();
        let id = entry.id.clone();
        let item = ConversationListItem::new(
            entry.id.clone(),
            entry.name.clone(),
            entry.last_response.clone(),
            entry.timestamp.clone(),
            entry.status,
            selected,
        )
        .with_on_click(move || (on_select.borrow_mut())(id.clone()))
        .finish();
        list = list.with_child(item);
    }

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(search);
    column = column.with_child(header);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(section_label);
    column = column.with_child(Scrollable::new(list.finish(), Axis::Vertical).finish());

    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
}

/// Main content area: tab bar + tab body.
fn build_main(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);

    let tab_row = Flex::row()
        .with_main_axis_alignment(MainAxisAlignment::Start)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(spacing)
        .with_child(tab_button(
            "Threads",
            AppTab::Threads,
            state.current_tab,
            actions,
        ))
        .with_child(tab_button("Chat", AppTab::Chat, state.current_tab, actions))
        .with_child(tab_button(
            "Settings",
            AppTab::Settings,
            state.current_tab,
            actions,
        ))
        .finish();

    let body: Box<dyn Element> = match state.current_tab {
        AppTab::Threads => ChatView::new()
            .with_messages(state.thread_messages.clone())
            .finish(),
        AppTab::Chat => build_agent_chat(app, state, actions),
        AppTab::Settings => SettingsView::new(SettingsPage::Profile)
            .with_profile("Ada", "ada@example.com")
            .with_llm("openai", "gpt-4o", "", "", "")
            .finish(),
    };

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(
        Container::new(tab_row)
            .with_padding(EdgeInsets::uniform(spacing))
            .finish(),
    );
    column = column.with_child(body);

    Container::new(column.finish())
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
}

/// Agent chat tab: a header row with the agent avatar/status/copy/restart,
/// then the message transcript + composer (which fills the remaining space).
fn build_agent_chat(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let header = build_agent_header(app, state, actions);

    let on_composer_change = actions.on_composer_change.clone();
    let on_composer_focus = actions.on_composer_focus_change.clone();
    let on_send_message = actions.on_send_message.clone();
    let on_attach = actions.on_attach.clone();
    let on_voice = actions.on_voice.clone();
    let on_select_model = actions.on_select_model.clone();
    let on_stop = actions.on_stop.clone();

    ChatView::new()
        .with_header(header)
        .with_messages(state.chat_messages.clone())
        .with_composer_value(state.composer_draft.clone())
        .with_composer_focused(state.composer_focused)
        .with_composer_model_label("goble-agent")
        .with_composer_stop_visible(state.agent_busy)
        .with_composer_on_change(move |text| (on_composer_change.borrow_mut())(text))
        .with_composer_on_focus_change(move |focused| (on_composer_focus.borrow_mut())(focused))
        .with_composer_on_attach(move || (on_attach.borrow_mut())())
        .with_composer_on_voice(move || (on_voice.borrow_mut())())
        .with_composer_on_select_model(move || (on_select_model.borrow_mut())())
        .with_composer_on_stop(move || (on_stop.borrow_mut())())
        .with_on_send(move |text| (on_send_message.borrow_mut())(text))
        .finish()
}

/// Agent identity row: avatar, name + status dot, then copy/restart actions.
fn build_agent_header(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);
    let radius = app.theme.radius_px();

    let avatar = Container::new(
        Icon::new("ai-assistant")
            .with_size(20.0)
            .with_theme_color(ColorToken::Text, app)
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_padding(EdgeInsets::uniform(spacing))
    .with_corner_radius(radius)
    .finish();

    let status_color = if state.agent_busy {
        ColorToken::Accent
    } else {
        ColorToken::Success
    };
    let status_label = if state.agent_busy {
        "working…"
    } else {
        "online"
    };
    let status_row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(
            Container::new(Rect::new().with_size(vec2f(8.0, 8.0)).finish())
                .with_background(Fill::Solid(app.theme.color(status_color)))
                .with_corner_radius(4.0)
                .finish(),
        )
        .with_child(
            Text::new(status_label)
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(12.0)
                .finish(),
        )
        .finish();

    let identity = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(2.0)
        .with_child(
            Text::new(state.agent_name.clone())
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(15.0)
                .finish(),
        )
        .with_child(status_row)
        .finish();

    let on_copy = actions.on_copy.clone();
    let on_restart = actions.on_restart.clone();
    let copy_button = TopbarButton::new(
        Icon::new("copy")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_on_click(move || (on_copy.borrow_mut())())
    .finish();
    let restart_button = TopbarButton::new(
        Icon::new("refresh")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_on_click(move || (on_restart.borrow_mut())())
    .finish();

    Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing)
            .with_child(avatar)
            .with_child(identity)
            .with_child(Spacer::new().finish())
            .with_child(copy_button)
            .with_child(restart_button)
            .finish(),
    )
    .with_padding(EdgeInsets::new(0.0, md, 0.0, md))
    .finish()
}

fn tab_button(label: &str, tab: AppTab, current: AppTab, actions: &UiActions) -> Box<dyn Element> {
    let style = if tab == current {
        ButtonVariant::Primary
    } else {
        ButtonVariant::Default
    };
    let on_select_tab = actions.on_select_tab.clone();
    Button::new(Text::new(label).finish())
        .with_variant(style)
        .with_on_click(move || (on_select_tab.borrow_mut())(tab))
        .finish()
}

/// Splits horizontal space: fixed-width sidebar + main area filling the rest.
/// The engine's `Flex` cannot yet distribute remaining space, so this custom
/// element does the split at layout time.
struct SidebarLayout {
    sidebar: Box<dyn Element>,
    main: Box<dyn Element>,
    width: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SidebarLayout {
    fn new(sidebar: Box<dyn Element>, main: Box<dyn Element>, width: f32) -> Self {
        Self {
            sidebar,
            main,
            width,
            size: None,
            origin: None,
        }
    }
}

impl Element for SidebarLayout {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let sidebar_constraint =
            SizeConstraint::new(vec2f(0.0, 0.0), vec2f(self.width, constraint.max.y));
        let _ = self.sidebar.layout(sidebar_constraint, ctx, app);

        let main_width = (constraint.max.x - self.width).max(0.0);
        let main_constraint =
            SizeConstraint::new(vec2f(0.0, 0.0), vec2f(main_width, constraint.max.y));
        let _ = self.main.layout(main_constraint, ctx, app);

        let size = vec2f(constraint.max.x, constraint.max.y);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.sidebar.paint(origin, ctx, app);
        self.main.paint(origin + vec2f(self.width, 0.0), ctx, app);
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
        if self.sidebar.dispatch_event(event, ctx, app) {
            return true;
        }
        self.main.dispatch_event(event, ctx, app)
    }
}
