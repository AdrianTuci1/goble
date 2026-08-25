//! Root element of the native UI.
//!
//! Owns only the element tree + the hot-reload handshake. The data lives in
//! [`crate::state`] and the callbacks live in [`crate::actions`].

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::{CollectingEventBus, DesktopState};
use goble_ui::elements::Empty;
use goble_ui::event::DispatchedEvent;
use goble_ui::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint, Vector2F,
};

use crate::actions::make_actions;
use crate::ai::{make_ai_actions, AiState};
use crate::hot_ui::{build_ui, AiSnapshot, UiSnapshot};
use crate::state::UiState;

#[cfg(feature = "hot-reload")]
use std::sync::mpsc;

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
    ai_state: Rc<RefCell<AiState>>,
    desktop: Option<Arc<DesktopState>>,
    event_bus: Option<CollectingEventBus>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    #[cfg(feature = "hot-reload")]
    reload: ReloadHandles,
}

impl RootView {
    pub fn new(
        app: &AppContext,
        desktop: Option<Arc<DesktopState>>,
        event_bus: Option<CollectingEventBus>,
    ) -> Self {
        let state = Rc::new(RefCell::new(match &desktop {
            Some(d) => UiState::from_desktop(d),
            None => UiState::mock(),
        }));
        let ai_state = Rc::new(RefCell::new(match &desktop {
            Some(d) => AiState::from_desktop(d),
            None => AiState::mock(),
        }));

        #[cfg(feature = "hot-reload")]
        let reload = spawn_reload_thread();

        let mut view = Self {
            element: Box::new(Empty::new()),
            state,
            ai_state,
            desktop,
            event_bus,
            size: None,
            origin: None,
            #[cfg(feature = "hot-reload")]
            reload,
        };
        view.rebuild(app);
        view
    }

    /// Poll the event bus and refresh state from the backend when something
    /// changed (chats, messages, workflows, agents). Called on every frame
    /// before the tree is rebuilt, so backend updates show up live.
    fn drain_events(&mut self) {
        let Some(bus) = self.event_bus.clone() else {
            return;
        };
        let events = bus.take_events();
        if events.is_empty() {
            return;
        }
        let Some(desktop) = self.desktop.clone() else {
            return;
        };
        let mut state = self.state.borrow_mut();
        for (name, _payload) in events {
            match name.as_str() {
                "chats:updated" | "chat:updated" => state.refresh_conversations(&desktop),
                "workflows:updated" => state.refresh_crons(&desktop),
                "agents:updated" => state.refresh_agent_name(&desktop),
                "vault:updated" => self.ai_state.borrow_mut().refresh_vault(&desktop),
                _ => {}
            }
        }
    }

    /// Rebuild the element tree from the current app state.
    ///
    /// This drops the previous tree *before* calling into the hot library, so
    /// old dylib-owned objects are released while the old library is still
    /// loaded (`hot-lib-reloader` may swap it during the next `build_ui` call).
    fn rebuild(&mut self, app: &AppContext) {
        self.drain_events();
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
                models: s.models.clone(),
                selected_model: s.selected_model.clone(),
                model_menu_open: s.model_menu_open.clone(),
                profile_menu_open: s.profile_menu_open.clone(),
                agent_name: s.agent_name.clone(),
                agent_busy: s.agent_busy,
                crons_open: s.crons_open,
                crons: s.crons.clone(),
                sidebar_width: s.sidebar_width,
                sidebar_dragging: s.sidebar_dragging,
                agent_cards: s.agent_cards.clone(),
                new_agent_hover: s.new_agent_hover.clone(),
            }
        };
        let actions = make_actions(Rc::clone(&self.state), self.desktop.clone());
        let ai_snapshot = {
            let s = self.ai_state.borrow();
            AiSnapshot {
                connectors_open: s.connectors_open,
                vault_open: s.vault_open,
                vault_unlocked: s.vault_unlocked,
                vault_secrets: s.vault_secrets.clone(),
                vault_unlock_draft: s.vault_unlock_draft.clone(),
                vault_new_key: s.vault_new_key.clone(),
                vault_new_value: s.vault_new_value.clone(),
                vault_error: s.vault_error.clone(),
                connector_search: s.connector_search.clone(),
                connectors: s.connectors.clone(),
                install_open: s.install_open,
                install_editing_id: s.install_editing_id.clone(),
                install_name: s.install_name.clone(),
                install_source: s.install_source.clone(),
                install_source_value: s.install_source_value.clone(),
                install_search_query: s.install_search_query.clone(),
                install_search_results: s.install_search_results.clone(),
                install_selected_secrets: s.install_selected_secrets.clone(),
                install_error: s.install_error.clone(),
                installing: s.installing,
            }
        };
        let ai_actions = make_ai_actions(Rc::clone(&self.ai_state), self.desktop.clone());
        self.element = build_ui(app, &snapshot, &actions, &ai_snapshot, &ai_actions);
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
