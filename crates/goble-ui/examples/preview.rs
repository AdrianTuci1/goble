use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    ActiveView, AppContext, ChatHeader, ChatLayout, ChatMessage, ChatRole, ChatSidebar, Container,
    Element, EventContext, Fill, LayoutContext, PaintContext, Point, ShellState, ShellView,
    SizeConstraint, Text,
};
use goble_ui::theme::ColorToken;
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::{ChatView, SettingsPage, SettingsView, ThreadKind, ThreadListEntry, ThreadsContainer};

fn build_chat_preview(
    shell_state: Rc<RefCell<ShellState>>,
    dirty: Rc<RefCell<bool>>,
    app: &AppContext,
) -> Box<dyn Element> {
    let sidebar_visible = shell_state.borrow().chat_sidebar_visible;
    let toggle_state = Rc::clone(&shell_state);
    let toggle_dirty = Rc::clone(&dirty);
    let header = ChatHeader::new("New chat", app)
        .with_sidebar_toggle(
            sidebar_visible,
            move || {
                let mut s = toggle_state.borrow_mut();
                s.chat_sidebar_visible = !s.chat_sidebar_visible;
                *toggle_dirty.borrow_mut() = true;
            },
        )
        .finish();

    let chat = ChatView::new()
        .with_header(header)
        .with_empty_state("New conversation", "Type a message and press send to begin.")
        .with_on_send(|text| println!("send: {}", text))
        .finish();

    let mut layout = ChatLayout::new(chat);
    if sidebar_visible {
        layout = layout.with_right_sidebar(ChatSidebar::new(app).finish());
    }
    layout.finish()
}

fn build_threads_preview(_app: &AppContext) -> Box<dyn Element> {
    let threads = vec![
        ThreadListEntry {
            id: "t1".to_string(),
            title: "General".to_string(),
            kind: ThreadKind::Channel,
            selected: true,
            unread_count: 0,
        },
        ThreadListEntry {
            id: "t2".to_string(),
            title: "Random".to_string(),
            kind: ThreadKind::Chat,
            selected: false,
            unread_count: 3,
        },
    ];

    let messages = vec![
        ChatMessage::from_markdown(ChatRole::User, "Thread message."),
        ChatMessage::from_markdown(ChatRole::Assistant, "Thread reply."),
    ];

    ThreadsContainer::new("t1")
        .with_threads(threads)
        .with_messages("t1", messages)
        .with_on_send(|text| println!("thread send: {}", text))
        .with_on_select(|id| println!("selected: {}", id))
        .finish()
}

fn build_settings_preview(_app: &AppContext) -> Box<dyn Element> {
    SettingsView::new(SettingsPage::Profile)
        .with_profile("Ada", "ada@example.com")
        .with_llm("openai", "gpt-4o", "", "", "0.7")
        .with_dark_mode(true)
        .with_workers(vec![(
            "w1".to_string(),
            "VPS".to_string(),
            "wss://example.com/ws".to_string(),
            false,
        )])
        .with_on_save_profile(|name, email| println!("save profile: {} <{}>", name, email))
        .with_on_save_llm(|provider, model, _key, _url, temp| {
            println!("save llm: {} / {} / {}", provider, model, temp)
        })
        .with_on_toggle_dark_mode(|enabled| println!("dark mode: {}", enabled))
        .finish()
}

struct PreviewRoot {
    shell: ShellView,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl PreviewRoot {
    fn new(app: &AppContext) -> Self {
        let shell = ShellView::with_content(
            ShellState::default(),
            app,
            Box::new(
                move |state: Rc<RefCell<ShellState>>, dirty: Rc<RefCell<bool>>| {
                    let app = AppContext::default();
                    match state.borrow().active_view {
                        ActiveView::Chat => build_chat_preview(Rc::clone(&state), Rc::clone(&dirty), &app),
                        ActiveView::Threads => build_threads_preview(&app),
                        ActiveView::Settings(_) => build_settings_preview(&app),
                        _ => Container::new(
                            Text::new("Coming soon")
                                .with_theme_color(ColorToken::Muted, &app)
                                .finish(),
                        )
                        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
                        .finish(),
                    }
                },
            ),
        );
        Self {
            shell,
            size: None,
            origin: None,
        }
    }
}

impl Element for PreviewRoot {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.shell.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.shell.paint(origin, ctx, app);
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
        self.shell.dispatch_event(event, ctx, app)
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let app_context = Rc::new(RefCell::new(AppContext::default()));
    let root = PreviewRoot::new(&app_context.borrow());
    goble_ui::platform::run_with_root(root.finish(), app_context)?;
    Ok(())
}
