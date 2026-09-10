use goble_core::store::Store;
use goble_core::thread::{Participant, ThreadKind, UserId};
use goble_core::user::UserProfile;
use goble_desktop_service::{CollectingEventBus, DesktopState};
use goble_ui::elements::chat_content::ChatMessage as UiChatMessage;
use goble_ui::elements::ConstrainedBox;
use goble_ui::geometry::vec2f;
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::{
    AppContext, Button, ChatView, Container, CrossAxisAlignment, EdgeInsets, Element, Fill, Flex,
    LayoutContext, MainAxisAlignment, SettingsPage, SettingsView, SizeConstraint, ThreadListEntry,
    ThreadsContainer,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppTab {
    Threads,
    Chat,
    Settings,
}

struct TabbedApp {
    tab: Rc<RefCell<AppTab>>,
    threads: Vec<ThreadListEntry>,
    thread_messages: Vec<UiChatMessage>,
    chat_messages: Vec<UiChatMessage>,
    root: Option<Box<dyn Element>>,
    size: Option<goble_ui::Vector2F>,
    origin: Option<goble_ui::Point>,
}

impl TabbedApp {
    fn new(
        tab: Rc<RefCell<AppTab>>,
        threads: Vec<ThreadListEntry>,
        thread_messages: Vec<UiChatMessage>,
        chat_messages: Vec<UiChatMessage>,
    ) -> Self {
        Self {
            tab,
            threads,
            thread_messages,
            chat_messages,
            root: None,
            size: None,
            origin: None,
        }
    }

    fn tab_button(
        label: &str,
        tab: AppTab,
        current: AppTab,
        target: Rc<RefCell<AppTab>>,
    ) -> Box<dyn Element> {
        let style = if current == tab {
            goble_ui::ButtonVariant::Primary
        } else {
            goble_ui::ButtonVariant::Default
        };
        Button::new(goble_ui::Text::new(label).finish())
            .with_variant(style)
            .with_on_click(move || {
                *target.borrow_mut() = tab;
            })
            .finish()
    }

    fn rebuild(&mut self, app: &AppContext) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let current = *self.tab.borrow();

        let tab_row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing)
            .with_child(Self::tab_button(
                "Threads",
                AppTab::Threads,
                current,
                self.tab.clone(),
            ))
            .with_child(Self::tab_button(
                "Chat",
                AppTab::Chat,
                current,
                self.tab.clone(),
            ))
            .with_child(Self::tab_button(
                "Settings",
                AppTab::Settings,
                current,
                self.tab.clone(),
            ))
            .finish();

        let header = Container::new(tab_row)
            .with_padding(EdgeInsets::uniform(spacing))
            .finish();

        let body: Box<dyn Element> = match current {
            AppTab::Threads => {
                let selected = self
                    .threads
                    .first()
                    .map(|t| t.id.clone())
                    .unwrap_or_default();
                ThreadsContainer::new(&selected)
                    .with_threads(self.threads.clone())
                    .with_messages(&selected, self.thread_messages.clone())
                    .finish()
            }
            AppTab::Chat => ChatView::new()
                .with_messages(self.chat_messages.clone())
                .finish(),
            AppTab::Settings => SettingsView::new(SettingsPage::Profile)
                .with_profile("Ada", "ada@example.com")
                .with_llm("openai", "gpt-4o", "", "", "")
                .finish(),
        };

        let body = ConstrainedBox::new(body).with_max_height(600.0).finish();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);
        column = column.with_child(header);
        column = column.with_child(body);

        self.root = Some(
            Container::new(column.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
                .with_padding(EdgeInsets::uniform(spacing))
                .finish(),
        );
    }
}

impl Element for TabbedApp {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> goble_ui::Vector2F {
        self.rebuild(app);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(
        &mut self,
        origin: goble_ui::Vector2F,
        ctx: &mut goble_ui::PaintContext,
        app: &AppContext,
    ) {
        self.origin = Some(goble_ui::Point::from_vec2f(origin, Default::default()));
        self.root.as_mut().unwrap().paint(origin, ctx, app);
    }

    fn size(&self) -> Option<goble_ui::Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<goble_ui::Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &goble_ui::event::DispatchedEvent,
        ctx: &mut goble_ui::EventContext,
        app: &AppContext,
    ) -> bool {
        self.root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let _guard = runtime.enter();

    let bus = CollectingEventBus::default();
    let state = DesktopState::new(
        Store::open_in_memory()?,
        goble_desktop_service::ThreadStore::new(PathBuf::from("/tmp/goble-ui-full-example"))?,
    );
    state.set_event_bus(Arc::new(bus.clone()));

    let owner = UserId::generate();
    let thread = state.thread_store().create_thread(
        ThreadKind::Channel,
        "General",
        owner.clone(),
        false,
        vec![Participant::User(owner.clone())],
        vec!["#general".to_string()],
    )?;
    state.thread_store().post_message(
        &thread.id,
        Participant::User(owner.clone()),
        "Hello from **full native app**!",
        None,
        vec![],
        vec![],
        None,
    )?;

    state.thread_store().set_profile(UserProfile::new(
        goble_core::principal::PrincipalId("u1".to_string()),
        "Ada",
        "ada@example.com",
    ))?;

    let chat_id = state.create_chat("Native chat", None, None)?;
    state.add_chat_message(&chat_id, "user", "Hello from chat tab!")?;
    state.add_chat_message(
        &chat_id,
        "assistant",
        "Hi there! I am backed by the shared service layer.",
    )?;

    let threads = vec![ThreadListEntry {
        id: thread.id.0.clone(),
        title: thread.title.clone(),
        kind: goble_ui::ThreadKind::Channel,
        selected: true,
        unread_count: 0,
    }];

    let thread_messages: Vec<UiChatMessage> = state
        .thread_store()
        .list_messages(&thread.id)?
        .iter()
        .map(UiChatMessage::from_thread_message)
        .collect();

    let chat_messages: Vec<UiChatMessage> = state
        .list_chat_messages(&chat_id)?
        .into_iter()
        .map(|m| {
            let role = match m.role.as_str() {
                "user" => goble_ui::ChatRole::User,
                _ => goble_ui::ChatRole::Assistant,
            };
            UiChatMessage::from_markdown(role, m.content)
        })
        .collect();

    let app = AppContext::default();
    let mut tabbed = TabbedApp::new(
        Rc::new(RefCell::new(AppTab::Threads)),
        threads,
        thread_messages,
        chat_messages,
    );
    let size = tabbed.layout(
        SizeConstraint::loose(vec2f(1024.0, 768.0)),
        &mut LayoutContext::default(),
        &app,
    );

    println!("Tabbed native app layout size: {}x{}", size.x, size.y);
    println!("Service events emitted: {}", bus.events().len());

    Ok(())
}
