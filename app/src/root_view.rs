//! Root element of the native UI.
//!
//! Owns only the element tree. The data lives in [`crate::state`], the
//! callbacks live in [`crate::actions`], and runtime orchestration (where a
//! turn executes) lives in [`crate::runtime`].

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
use crate::state::UiState;
use crate::ui::{build_ui, AiSnapshot, UiSnapshot};

/// Root element that renders the app UI and drives the event loop.
pub struct RootView {
    element: Box<dyn Element>,
    state: Rc<RefCell<UiState>>,
    ai_state: Rc<RefCell<AiState>>,
    desktop: Option<Arc<DesktopState>>,
    event_bus: Option<CollectingEventBus>,
    size: Option<Vector2F>,
    origin: Option<Point>,
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

        let mut view = Self {
            element: Box::new(Empty::new()),
            state,
            ai_state,
            desktop,
            event_bus,
            size: None,
            origin: None,
        };
        view.rebuild(app);
        view
    }

    /// Expose the backing UI state so integration render tests can drive
    /// first-run flags (e.g. the model-key banner overlay) before mounting.
    #[doc(hidden)]
    pub fn state_rc(&self) -> Rc<RefCell<UiState>> {
        Rc::clone(&self.state)
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
        for (name, payload) in events {
            match name.as_str() {
                "chats:updated" | "chat:updated" => state.refresh_conversations(&desktop),
                "chat:turn_finished" => {
                    state.agent_busy = false;
                    // A prompt queued while the agent was running is submitted
                    // automatically now that the turn finished (warp-new model).
                    if let Some(prompt) = state.queued_prompt.take() {
                        let model = if state.selected_model.trim().is_empty() {
                            state.settings_llm_model.clone()
                        } else {
                            state.selected_model.clone()
                        };
                        if let Some(chat_id) = state.selected_id.clone() {
                            if let Err(e) = crate::runtime::run_turn(
                                &desktop,
                                &chat_id,
                                &prompt,
                                &state.settings_llm_provider,
                                &model,
                                state.workspace_routing,
                            ) {
                                log::warn!("auto-submit queued prompt failed: {e}");
                            } else {
                                state.agent_busy = true;
                            }
                        }
                    }
                }
                // The agent suspended to ask the user a question; render the
                // inline ask card in the transcript.
                "chat:ask_user" => {
                    if let Some(question) = payload.get("question").and_then(|v| v.as_str()) {
                        let quick: Vec<String> = payload
                            .get("quick_replies")
                            .and_then(|q| q.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        state.pending_ask = Some(goble_ui::AskUserUi::new(question, quick));
                        state.refresh_messages(&desktop);
                    }
                }
                "workflows:updated" => state.refresh_crons(&desktop),
                "agents:updated" => state.refresh_agent_name(&desktop),
                "vault:updated" => self.ai_state.borrow_mut().refresh_vault(&desktop),
                _ => {}
            }
        }
    }

    /// Rebuild the element tree from the current app state.
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
                pending_ask: s.pending_ask.clone(),
                queued_prompt: s.queued_prompt.clone(),
                composer_draft: s.composer_draft.clone(),
                composer_focused: s.composer_focused,
                models: s.models.clone(),
                selected_model: s.selected_model.clone(),
                model_menu_open: s.model_menu_open.clone(),
                profile_menu_open: s.profile_menu_open.clone(),
                agent_name: s.agent_name.clone(),
                agent_busy: s.agent_busy,
                auto_approve: s.auto_approve,
                right_sidebar_open: s.right_sidebar_open,
                crons_open: s.crons_open,
                crons: s.crons.clone(),
                settings_page: s.settings_page,
                settings_profile_name: s.settings_profile_name.clone(),
                settings_profile_email: s.settings_profile_email.clone(),
                settings_dark_mode: s.settings_dark_mode,
                settings_llm_provider: s.settings_llm_provider.clone(),
                settings_llm_model: s.settings_llm_model.clone(),
                settings_llm_api_key: s.settings_llm_api_key.clone(),
                settings_llm_base_url: s.settings_llm_base_url.clone(),
                settings_llm_temperature: s.settings_llm_temperature.clone(),
                settings_workers: s.settings_workers.clone(),
                settings_cluster_name: s.settings_cluster_name.clone(),
                settings_cluster_configured: s.settings_cluster_configured,
                settings_authorized_keys: s.settings_authorized_keys.clone(),
                settings_vault_unlocked: s.settings_vault_unlocked,
                show_llm_key_banner: s.show_llm_key_banner,
                show_workspace_choice: s.show_workspace_choice,
                workspace_routing: s.workspace_routing,
                llm_dialog_open: s.llm_dialog_open,
                llm_dialog_provider: s.llm_dialog_provider.clone(),
                llm_dialog_model: s.llm_dialog_model.clone(),
                llm_dialog_api_key: s.llm_dialog_api_key.clone(),
                llm_dialog_base_url: s.llm_dialog_base_url.clone(),
                llm_dialog_temperature: s.llm_dialog_temperature.clone(),
                llm_dialog_focus: s.llm_dialog_focus.clone(),
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
}

impl Element for RootView {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
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
