use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::Empty;
use goble_ui::event::DispatchedEvent;
use goble_ui::{
    AppContext, ChatFragment, ChatMessage, ChatRole, ConversationEntry, ConversationStatus,
    Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint, TerminalData,
    TerminalLine, TerminalStatus, Vector2F,
};

use crate::hot_ui::{build_ui, AppTab, UiActions, UiSnapshot};

#[cfg(feature = "hot-reload")]
use std::sync::mpsc;

/// App-owned UI state.
///
/// The element tree is rebuilt from this state on every frame, so state lives
/// here (in the executable) instead of inside the hot-reloaded dylib. That
/// keeps text input focus/value across rebuilds and survives library swaps.
#[derive(Clone)]
struct UiState {
    current_tab: AppTab,
    conversations: Vec<ConversationEntry>,
    selected_id: Option<String>,
    search_query: String,
    search_focused: bool,
    new_conversation_draft: String,
    create_focused: bool,
    thread_messages: Vec<ChatMessage>,
    chat_messages: Vec<ChatMessage>,
    composer_draft: String,
    composer_focused: bool,
    agent_name: String,
    agent_busy: bool,
}

impl UiState {
    fn mock() -> Self {
        let conversations = vec![
            ConversationEntry::new("c1", "Ada", "Let's ship hot reload today", "10:42")
                .with_status(ConversationStatus::Success),
            ConversationEntry::new("c2", "Coder", "PR #12 is merged", "09:30")
                .with_status(ConversationStatus::Success),
            ConversationEntry::new("c3", "Ops", "Worker deployment done", "Yesterday")
                .with_status(ConversationStatus::Default),
            ConversationEntry::new("c4", "Research", "Drafting the plan", "2 days ago")
                .with_status(ConversationStatus::Error),
        ];

        let thread_messages = vec![ChatMessage::from_markdown(
            ChatRole::Assistant,
            "Bine ai venit! Selectează o conversație din sidebar.",
        )];
        let chat_messages = vec![
            ChatMessage::from_markdown(ChatRole::User, "Salut! Cum legăm goble-ui de app?"),
            ChatMessage::new(
                ChatRole::Assistant,
                vec![
                    ChatFragment::text("Am pornit aplicația:"),
                    ChatFragment::terminal(
                        TerminalData::new(
                            "cargo run",
                            vec![
                                TerminalLine::command("cargo run"),
                                TerminalLine::output("Compiling goble-ui v0.1.0"),
                                TerminalLine::output("Finished `dev` profile in 1.2s"),
                                TerminalLine::success("Running `target/debug/goble`"),
                            ],
                        )
                        .with_status(TerminalStatus::Success),
                    ),
                ],
            ),
            ChatMessage::from_markdown(
                ChatRole::Assistant,
                "UI-ul rulează cu hot reload — modificările din `goble-ui-hot` apar instant.",
            ),
        ];

        Self {
            current_tab: AppTab::Chat,
            selected_id: conversations.first().map(|c| c.id.clone()),
            conversations,
            search_query: String::new(),
            search_focused: false,
            new_conversation_draft: String::new(),
            create_focused: false,
            thread_messages,
            chat_messages,
            composer_draft: String::new(),
            composer_focused: false,
            agent_name: "Goble Agent".to_string(),
            agent_busy: false,
        }
    }
}

#[cfg(feature = "hot-reload")]
struct ReloadHandles {
    drop_request: mpsc::Receiver<()>,
    drop_done: mpsc::Sender<()>,
}

/// Root element that renders the hot-reloadable UI and coordinates library
/// swaps with the running `hot-lib-reloader` watcher.
pub struct RootView {
    element: Box<dyn Element>,
    state: Rc<RefCell<UiState>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    #[cfg(feature = "hot-reload")]
    reload: ReloadHandles,
}

impl RootView {
    pub fn new(app: &AppContext) -> Self {
        let state = Rc::new(RefCell::new(UiState::mock()));

        #[cfg(feature = "hot-reload")]
        let reload = spawn_reload_thread();

        let mut view = Self {
            element: Box::new(Empty::new()),
            state,
            size: None,
            origin: None,
            #[cfg(feature = "hot-reload")]
            reload,
        };
        view.rebuild(app);
        view
    }

    /// Rebuild the element tree from the current app state.
    ///
    /// This drops the previous tree *before* calling into the hot library, so
    /// old dylib-owned objects are released while the old library is still
    /// loaded (`hot-lib-reloader` may swap it during the next `build_ui` call).
    fn rebuild(&mut self, app: &AppContext) {
        let snapshot = {
            let s = self.state.borrow();
            UiSnapshot {
                current_tab: s.current_tab,
                conversations: s.conversations.clone(),
                selected_id: s.selected_id.clone(),
                search_query: s.search_query.clone(),
                search_focused: s.search_focused,
                new_conversation_draft: s.new_conversation_draft.clone(),
                create_focused: s.create_focused,
                thread_messages: s.thread_messages.clone(),
                chat_messages: s.chat_messages.clone(),
                composer_draft: s.composer_draft.clone(),
                composer_focused: s.composer_focused,
                agent_name: s.agent_name.clone(),
                agent_busy: s.agent_busy,
            }
        };
        let actions = Self::actions(Rc::clone(&self.state));
        self.element = build_ui(app, &snapshot, &actions);
    }

    fn actions(state: Rc<RefCell<UiState>>) -> UiActions {
        let on_search_change = Rc::clone(&state);
        let on_search_focus_change = Rc::clone(&state);
        let on_create_change = Rc::clone(&state);
        let on_create_focus_change = Rc::clone(&state);
        let on_create_submit = Rc::clone(&state);
        let on_select_conversation = Rc::clone(&state);
        let on_select_tab = Rc::clone(&state);
        let on_composer_change = Rc::clone(&state);
        let on_composer_focus_change = Rc::clone(&state);
        let on_send_message = Rc::clone(&state);
        let on_attach = Rc::clone(&state);
        let on_voice = Rc::clone(&state);
        let on_stop = Rc::clone(&state);

        UiActions {
            on_search_change: Rc::new(RefCell::new(move |value: String| {
                on_search_change.borrow_mut().search_query = value;
            })),
            on_search_focus_change: Rc::new(RefCell::new(move |focused: bool| {
                on_search_focus_change.borrow_mut().search_focused = focused;
            })),
            on_create_change: Rc::new(RefCell::new(move |value: String| {
                on_create_change.borrow_mut().new_conversation_draft = value;
            })),
            on_create_focus_change: Rc::new(RefCell::new(move |focused: bool| {
                on_create_focus_change.borrow_mut().create_focused = focused;
            })),
            on_create_submit: Rc::new(RefCell::new(move || {
                let mut state = on_create_submit.borrow_mut();
                let title = state.new_conversation_draft.trim().to_string();
                if !title.is_empty() {
                    let id = format!("c-{}", state.conversations.len() + 1);
                    state.conversations.insert(
                        0,
                        ConversationEntry::new(id.clone(), title, "New conversation", "now"),
                    );
                    state.selected_id = Some(id);
                    state.new_conversation_draft.clear();
                }
            })),
            on_select_conversation: Rc::new(RefCell::new(move |id: String| {
                on_select_conversation.borrow_mut().selected_id = Some(id);
            })),
            on_select_tab: Rc::new(RefCell::new(move |tab: AppTab| {
                on_select_tab.borrow_mut().current_tab = tab;
            })),
            on_composer_change: Rc::new(RefCell::new(move |value: String| {
                on_composer_change.borrow_mut().composer_draft = value;
            })),
            on_composer_focus_change: Rc::new(RefCell::new(move |focused: bool| {
                on_composer_focus_change.borrow_mut().composer_focused = focused;
            })),
            on_send_message: Rc::new(RefCell::new(move |text: String| {
                let mut state = on_send_message.borrow_mut();
                state
                    .chat_messages
                    .push(ChatMessage::from_markdown(ChatRole::User, text.clone()));
                state.chat_messages.push(ChatMessage::from_markdown(
                    ChatRole::Assistant,
                    format!("Am primit mesajul tău. Rulez acum comanda pentru „{text}”."),
                ));
                state.composer_draft.clear();
                state.agent_busy = false;
            })),
            on_attach: Rc::new(RefCell::new(move || {
                on_attach
                    .borrow_mut()
                    .chat_messages
                    .push(ChatMessage::from_markdown(
                        ChatRole::Assistant,
                        "(attach — file picker coming soon)",
                    ));
            })),
            on_voice: Rc::new(RefCell::new(move || {
                on_voice
                    .borrow_mut()
                    .chat_messages
                    .push(ChatMessage::from_markdown(
                        ChatRole::Assistant,
                        "(voice input coming soon)",
                    ));
            })),
            on_select_model: Rc::new(RefCell::new(move || {
                log::info!("model selector pressed");
            })),
            on_copy: Rc::new(RefCell::new(move || {
                log::info!("copy transcript pressed");
            })),
            on_restart: Rc::new(RefCell::new(move || {
                log::info!("restart agent pressed");
            })),
            on_stop: Rc::new(RefCell::new(move || {
                on_stop.borrow_mut().agent_busy = false;
            })),
        }
    }

    /// Drain the reload handshake: when the dylib is about to be swapped,
    /// replace the old tree with an app-owned placeholder *before* the swap is
    /// allowed to proceed, so no dylib-owned objects outlive the library.
    #[cfg(feature = "hot-reload")]
    fn handle_reload(&mut self) {
        if self.reload.drop_request.try_recv().is_ok() {
            self.element = Empty::new().finish();
            let _ = self.reload.drop_done.send(());
        }
    }

    #[cfg(not(feature = "hot-reload"))]
    fn handle_reload(&mut self) {}
}

/// Watches `hot-lib-reloader` reload events.
///
/// `wait_for_about_to_reload` returns a token that *blocks the swap* while it
/// is alive. We hold it until the main thread confirms the old element tree
/// was dropped, then release it and wait for the new library to be loaded.
#[cfg(feature = "hot-reload")]
fn spawn_reload_thread() -> ReloadHandles {
    let observer = crate::hot_ui::ui_hot::subscribe();
    let (drop_tx, drop_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();

    std::thread::spawn(move || loop {
        let blocker = observer.wait_for_about_to_reload();
        // Ask the main thread to drop the old element tree (safe: the old dylib
        // is still mapped and the swap is blocked).
        if drop_tx.send(()).is_err() {
            return;
        }
        if done_rx.recv().is_err() {
            return;
        }
        drop(blocker);
        observer.wait_for_reload();
    });

    ReloadHandles {
        drop_request: drop_rx,
        drop_done: done_tx,
    }
}

impl Element for RootView {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.handle_reload();
        self.rebuild(app);
        let size = self.element.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.element.paint(origin, ctx, app);
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
        self.element.dispatch_event(event, ctx, app)
    }
}
