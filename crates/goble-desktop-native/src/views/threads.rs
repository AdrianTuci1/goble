use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::DesktopState;
use goble_ui::elements::{
    AppContext, ChatMessage as UiChatMessage, Container, Element, EventContext, Fill, LayoutContext,
    PaintContext, Point, SizeConstraint,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::views::thread_list_view::{ThreadKind, ThreadListEntry};
use goble_ui::ThreadsContainer;

use crate::app::UiState;

fn map_thread_kind(kind: goble_core::thread::ThreadKind) -> ThreadKind {
    match kind {
        goble_core::thread::ThreadKind::Direct => ThreadKind::Direct,
        goble_core::thread::ThreadKind::Channel => ThreadKind::Channel,
        goble_core::thread::ThreadKind::Chat => ThreadKind::Chat,
    }
}

pub struct ThreadsViewPanel {
    content: Box<dyn Element>,
}

impl ThreadsViewPanel {
    pub fn new(
        state: Arc<DesktopState>,
        ui_state: Rc<RefCell<UiState>>,
        dirty: Rc<RefCell<bool>>,
        app: &AppContext,
    ) -> Self {
        let bg = app.theme.color(ColorToken::Bg);
        let padding = app.theme.spacing_px(SpacingToken::Md);

        let threads = state.thread_store().list_threads();
        let selected_id = if !ui_state.borrow().selected_thread_id.is_empty() {
            ui_state.borrow().selected_thread_id.clone()
        } else {
            threads.first().map(|t| t.id.0.clone()).unwrap_or_default()
        };

        let selected_id = if selected_id.is_empty() && threads.is_empty() {
            let store = state.thread_store();
            let owner = store
                .get_profile()
                .map(|p| goble_core::thread::UserId(p.id.0))
                .unwrap_or_else(goble_core::thread::UserId::generate);
            let participant = goble_core::thread::Participant::User(owner.clone());
            match store.create_thread(
                goble_core::thread::ThreadKind::Chat,
                "New thread",
                owner,
                false,
                vec![participant],
                vec![],
            ) {
                Ok(thread) => {
                    let id = thread.id.0.clone();
                    ui_state.borrow_mut().selected_thread_id = id.clone();
                    id
                }
                Err(e) => {
                    log::error!("failed to auto-create thread: {}", e);
                    String::new()
                }
            }
        } else {
            selected_id
        };

        let entries: Vec<ThreadListEntry> = threads
            .iter()
            .map(|t| ThreadListEntry {
                id: t.id.0.clone(),
                title: t.title.clone(),
                kind: map_thread_kind(t.kind),
                selected: t.id.0 == selected_id,
                unread_count: 0,
            })
            .collect();

        let messages: Vec<UiChatMessage> = if selected_id.is_empty() {
            Vec::new()
        } else {
            state
                .thread_store()
                .list_messages(&goble_core::thread::ThreadId(selected_id.clone()))
                .map(|msgs| {
                    msgs.iter()
                        .map(|m| UiChatMessage::from_thread_message(m))
                        .collect()
                })
                .unwrap_or_default()
        };

        let state_for_send = Arc::clone(&state);
        let state_for_new = Arc::clone(&state);
        let ui_state_for_select = Rc::clone(&ui_state);
        let ui_state_for_new = Rc::clone(&ui_state);
        let dirty_for_select = Rc::clone(&dirty);
        let dirty_for_send = Rc::clone(&dirty);
        let dirty_for_new = Rc::clone(&dirty);

        let container = ThreadsContainer::new(selected_id.clone())
            .with_threads(entries)
            .with_messages(selected_id.clone(), messages)
            .with_on_select(move |id| {
                ui_state_for_select.borrow_mut().selected_thread_id = id;
                *dirty_for_select.borrow_mut() = true;
            })
            .with_on_send(move |text| {
                if selected_id.is_empty() {
                    log::warn!("no thread selected; cannot send message");
                    return;
                }
                let store = state_for_send.thread_store();
                let thread_id = goble_core::thread::ThreadId(selected_id.clone());
                let author = store
                    .get_profile()
                    .map(|p| goble_core::thread::Participant::User(goble_core::thread::UserId(p.id.0)))
                    .unwrap_or_else(|| {
                        goble_core::thread::Participant::User(goble_core::thread::UserId::generate())
                    });
                if let Err(e) = store.post_message(&thread_id, author, text, None, vec![], vec![], None) {
                    log::error!("failed to post thread message: {}", e);
                }
                *dirty_for_send.borrow_mut() = true;
            })
            .with_on_new(move || {
                let store = state_for_new.thread_store();
                let owner = store
                    .get_profile()
                    .map(|p| goble_core::thread::UserId(p.id.0))
                    .unwrap_or_else(goble_core::thread::UserId::generate);
                let participant = goble_core::thread::Participant::User(owner.clone());
                match store.create_thread(
                    goble_core::thread::ThreadKind::Chat,
                    "New thread",
                    owner,
                    false,
                    vec![participant],
                    vec![],
                ) {
                    Ok(thread) => {
                        ui_state_for_new.borrow_mut().selected_thread_id = thread.id.0.clone();
                        log::info!("created new thread {}", thread.id.0);
                    }
                    Err(e) => log::error!("failed to create thread: {}", e),
                }
                *dirty_for_new.borrow_mut() = true;
            });

        let content = Container::new(container.finish())
            .with_background(Fill::Solid(bg))
            .with_padding(goble_ui::elements::EdgeInsets::uniform(padding))
            .finish();

        Self { content }
    }
}

impl Element for ThreadsViewPanel {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.content.layout(constraint, ctx, app)
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.content.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.content.size()
    }

    fn origin(&self) -> Option<Point> {
        self.content.origin()
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.content.dispatch_event(event, ctx, app)
    }
}
