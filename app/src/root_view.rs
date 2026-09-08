//! Root element of the native UI.
//!
//! Owns only the element tree. The data lives in [`crate::state`], the
//! callbacks live in [`crate::actions`], and runtime orchestration (where a
//! turn executes) lives in [`crate::runtime`].

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::{CollectingEventBus, DesktopState};
use goble_ui::elements::{Empty, PopupMenuItem};
use goble_ui::event::DispatchedEvent;
use goble_ui::platform::WindowControl;
use goble_ui::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint, Vector2F,
};

use crate::actions::make_actions;
use crate::ai::{make_ai_actions, AiState};
use crate::media::{make_media_actions, MediaState};
use crate::projects::{make_projects_actions, ProjectsState};
use crate::screen::{make_screen_actions, ScreenState};
use crate::state::UiState;
use crate::ui::{
    build_ui, AiSnapshot, ComposerContext, MediaSnapshot, ProjectsSnapshot, ScreenFrameSnapshot,
    ScreenSnapshot, UiActions, UiSnapshot,
};

/// Root element that renders the app UI and drives the event loop.
pub struct RootView {
    element: Box<dyn Element>,
    state: Rc<RefCell<UiState>>,
    ai_state: Rc<RefCell<AiState>>,
    projects_state: Rc<RefCell<ProjectsState>>,
    media_state: Rc<RefCell<MediaState>>,
    screen_state: Rc<RefCell<ScreenState>>,
    desktop: Option<Arc<DesktopState>>,
    event_bus: Option<CollectingEventBus>,
    /// Handle used to request window-level changes (e.g. toggling fullscreen).
    /// Cloned from `AppContext`, so the handler installed by the platform event
    /// loop (once the window exists) is visible to the actions built each frame.
    window_control: WindowControl,
    /// The callbacks built on the last rebuild; kept here so the root can
    /// dispatch global keyboard shortcuts (split/space) before the tree does.
    actions: Option<UiActions>,
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
        let projects_state = Rc::new(RefCell::new(match &desktop {
            Some(d) => ProjectsState::from_desktop(d),
            None => ProjectsState::mock(),
        }));
        let media_state = Rc::new(RefCell::new(match &desktop {
            Some(d) => MediaState::from_desktop(d),
            None => MediaState::default(),
        }));
        let screen_state = Rc::new(RefCell::new(match &desktop {
            Some(d) => ScreenState::from_desktop(d),
            None => ScreenState::mock(),
        }));

        let mut view = Self {
            element: Box::new(Empty::new()),
            state,
            ai_state,
            projects_state,
            media_state,
            screen_state,
            desktop,
            event_bus,
            window_control: app.window_control.clone(),
            actions: None,
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

    /// Expose the backing screen state so integration render tests can open the
    /// screen sheet (broadcast + computer-use) before mounting.
    #[doc(hidden)]
    pub fn screen_state_rc(&self) -> Rc<RefCell<ScreenState>> {
        Rc::clone(&self.screen_state)
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
                    // Route the finished turn to the pane that owns its
                    // conversation (falling back to the active pane).
                    let pane_id = payload
                        .get("chat_id")
                        .and_then(|v| v.as_str())
                        .and_then(|cid| state.pane_id_for_conversation(cid))
                        .unwrap_or(state.active_pane_id);
                    let conv = state.pane_conversation_id(pane_id);
                    if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                        rt.busy = false;
                    }
                    // A prompt queued while the agent was running is submitted
                    // automatically now that the turn finished (warp-new model).
                    let queued = state
                        .pane_runtime
                        .get_mut(&pane_id)
                        .and_then(|rt| rt.queued_prompt.take());
                    if let (Some(prompt), Some(conv)) = (queued, conv) {
                        let model = if state.selected_model.trim().is_empty() {
                            state.settings_llm_model.clone()
                        } else {
                            state.selected_model.clone()
                        };
                        let (medium_id, project_id, session_id) = {
                            let media_b = self.media_state.borrow();
                            let session_id = if media_b.selected_session_id().is_empty() {
                                conv.clone()
                            } else {
                                media_b.selected_session_id().to_string()
                            };
                            (
                                media_b.selected_medium_id().to_string(),
                                media_b.selected_project_id().to_string(),
                                session_id,
                            )
                        };
                        let cwd = state
                            .pane_sessions
                            .get(&pane_id)
                            .map(|s| s.path.clone())
                            .unwrap_or_default();
                        if let Err(e) = crate::runtime::run_turn(
                            &desktop,
                            &conv,
                            &prompt,
                            &state.settings_llm_provider,
                            &model,
                            state.workspace_routing,
                            &medium_id,
                            &project_id,
                            &session_id,
                            &cwd,
                            Some(state.selected_harness.as_str()),
                        ) {
                            log::warn!("auto-submit queued prompt failed: {e}");
                        } else if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                            rt.busy = true;
                        }
                    }
                    state.sync_active_view();
                }
                // The agent suspended to ask the user a question; render the
                // inline ask card in the transcript.
                "chat:ask_user" => {
                    let pane_id = payload
                        .get("chat_id")
                        .and_then(|v| v.as_str())
                        .and_then(|cid| state.pane_id_for_conversation(cid))
                        .unwrap_or(state.active_pane_id);
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
                        if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                            rt.pending_ask = Some(goble_ui::AskUserUi::new(
                                question.to_string(),
                                quick,
                            ));
                        }
                    }
                    state.refresh_messages(&desktop);
                }
                "workflows:updated" => state.refresh_crons(&desktop),
                "agents:updated" => state.refresh_agent_name(&desktop),
                "vault:updated" => self.ai_state.borrow_mut().refresh_vault(&desktop),
                "executions:updated" => {
                    self.projects_state.borrow_mut().refresh(&desktop);
                    // Sessions/projects come from the store; re-derive the tree
                    // so a new session shows up in the environment selector.
                    self.media_state.borrow_mut().refresh(Some(&desktop));
                }
                // The agent handed the desktop to a remote screen: open the
                // screen panel, refresh the source list (so the fresh remote
                // source shows up), then select it. The same source is marked
                // as this conversation's inline handoff, so its live frame
                // renders inside the chat's harness area too.
                "screen:handoff" => {
                    if let Some(source) = payload.get("source").and_then(|v| v.as_str()) {
                        let mut s = self.screen_state.borrow_mut();
                        s.open = true;
                        s.refresh_sources(&desktop);
                        s.select_source(source);
                        let pane_id = payload
                            .get("chat_id")
                            .and_then(|v| v.as_str())
                            .and_then(|cid| state.pane_id_for_conversation(cid))
                            .unwrap_or(state.active_pane_id);
                        if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                            rt.inline_screen_source = Some(source.to_string());
                        }
                    }
                }
                _ => {}
            }
        }

        // A clicked BYOH handoff link: open the screen sheet, selecting the
        // source derived from the URI. The root owns the screen sheet, so the
        // action only stashes the URI here.
        if let Some(link) = state.pending_screen_link_open.take() {
            let source = crate::state::screen_source_from_link(&link);
            let mut s = self.screen_state.borrow_mut();
            s.open = true;
            s.refresh_sources(&desktop);
            if let Some(src) = source {
                s.select_source(&src);
            }
        }
    }

    /// Rebuild the element tree from the current app state.
    fn rebuild(&mut self, app: &AppContext) {
        self.drain_events();
        // Advance any running screen-event replay by real time (one step per
        // frame). The schedule itself is deterministic; only the pacing uses
        // the wall clock, and tests drive it through `step_replay`.
        self.screen_state
            .borrow_mut()
            .tick(self.desktop.as_deref());
        // Derive the composer context pills (harness = the selected harness,
        // working dir = the selected session's path, branch = git branch) from
        // the environment tree + the active pane's cwd. The harness pill lists
        // the work environment (medium) the active pane runs on; the sidebar
        // hosts the same selector, this pill is the quick overlay.
        let composer_context = {
            let media = self.media_state.borrow();
            let state_s = self.state.borrow();
            let harness_label = media
                .mediums
                .iter()
                .find(|m| m.id == media.selected_medium)
                .map(|m| m.label.clone())
                .unwrap_or_else(|| media.selected_medium.clone());
            let mut harness_ids = Vec::new();
            let mut harness_items = Vec::new();
            for m in &media.mediums {
                harness_ids.push(m.id.clone());
                let mut item = PopupMenuItem::new(m.label.clone()).with_icon("computer");
                if m.id == media.selected_medium {
                    item = item.selected();
                }
                harness_items.push(item);
            }
            let session_path = media.selected_session_path();
            let dir_label = if session_path.is_empty() {
                state_s.composer_path.clone()
            } else {
                session_path
            };
            let mut dir_ids = Vec::new();
            let mut dir_items = Vec::new();
            if let Some(m) = media.mediums.iter().find(|m| m.id == media.selected_medium) {
                if let Some(proj) = m.projects.iter().find(|p| p.id == media.selected_project) {
                    for s in &proj.sessions {
                        dir_ids.push(s.id.clone());
                        let mut item = PopupMenuItem::new(s.path.clone()).with_icon("folder");
                        if s.id == media.selected_session {
                            item = item.selected();
                        }
                        dir_items.push(item);
                    }
                }
            }
            let branch_label = state_s.composer_branch.clone();
            let branch_ids = if branch_label.is_empty() {
                Vec::new()
            } else {
                vec![branch_label.clone()]
            };
            let branch_items = if branch_label.is_empty() {
                Vec::new()
            } else {
                vec![PopupMenuItem::new(branch_label.clone())
                    .with_icon("git-branch")
                    .selected()]
            };
            ComposerContext {
                harness_label,
                harness_ids,
                harness_items,
                harness_menu_open: state_s.harness_menu_open.clone(),
                dir_label,
                dir_ids,
                dir_items,
                dir_menu_open: state_s.dir_menu_open.clone(),
                branch_label,
                branch_ids,
                branch_items,
                branch_menu_open: state_s.branch_menu_open.clone(),
            }
        };
        let snapshot = {
            let s = self.state.borrow();
            let mut pane_chat = s.pane_chat_snapshot();
            // Overlay the live inline handoff frame onto panes whose harness
            // handed control to the selected screen source, so the remote
            // desktop renders inside the chat's harness area.
            {
                let screen = self.screen_state.borrow();
                if let Some(frame) = &screen.frame {
                    let selected = screen.selected_source.clone();
                    let frame_snap = ScreenFrameSnapshot {
                        frame_seq: screen.frame_seq,
                        width: frame.width,
                        height: frame.height,
                        data: Arc::from(frame.data.clone()),
                    };
                    for (pid, snap) in pane_chat.iter_mut() {
                        let active = s
                            .pane_runtime
                            .get(pid)
                            .and_then(|rt| rt.inline_screen_source.as_ref())
                            .map(|src| src == &selected)
                            .unwrap_or(false);
                        if active {
                            snap.inline_screen = Some(frame_snap.clone());
                        }
                    }
                }
            }
            UiSnapshot {
                current_tab: s.current_tab,
                conversations: s.conversations.clone(),
                selected_id: s.selected_id.clone(),
                search_query: s.search_query.clone(),
                search_focused: s.search_focused,
                new_conversation_draft: s.new_conversation_draft.clone(),
                create_focused: s.create_focused,
                chat_messages: s.chat_messages.clone(),
                pending_ask: s.pending_ask.clone(),
                queued_prompt: s.queued_prompt.clone(),
                composer_draft: s.composer_draft.clone(),
                composer_focused: s.composer_focused,
                models: s.models.clone(),
                selected_model: s.selected_model.clone(),
                harnesses: s.harnesses.clone(),
                selected_harness: s.selected_harness.clone(),
                model_menu_open: s.model_menu_open.clone(),
                profile_menu_open: s.profile_menu_open.clone(),
                agent_name: s.agent_name.clone(),
                agent_busy: s.agent_busy,
                auto_approve: s.auto_approve,
                right_sidebar_open: s.right_sidebar_open,
                fullscreen: s.fullscreen,
                agent_header_menu_open: s.agent_header_menu_open.clone(),
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
                spaces: s.spaces.clone(),
                active_space: s.active_space,
                space_hover: s.space_hover,
                space_press: s.space_press,
                space_drag: s.space_drag,
                active_pane_id: s.active_pane_id,
                dragging_pane_id: s.dragging_pane_id,
                composer_path: s.composer_path.clone(),
                composer_context,
                pane_hover: s.pane_hover.clone(),
                pane_chat,
                terminal: s.terminal.clone(),
                sidebar_width: s.sidebar_width,
                sidebar_dragging: s.sidebar_dragging,
                agent_cards: s.agent_cards.clone(),
                new_agent_hover: s.new_agent_hover.clone(),
                command_palette_open: s.command_palette_open,
                command_palette_query: s.command_palette_query.clone(),
                command_palette_index: s.command_palette_index,
                show_onboarding_tip: s.show_onboarding_tip,
                add_space_menu_open: s.add_space_menu_open.clone(),
                env_selector_open: s.env_selector_open.clone(),
                add_medium_dialog_open: s.add_medium_dialog_open,
                add_medium_draft: s.add_medium_draft.clone(),
                add_medium_focused: s.add_medium_focused,
            }
        };
        let actions = make_actions(
            Rc::clone(&self.state),
            self.desktop.clone(),
            Rc::clone(&self.media_state),
            self.window_control.clone(),
        );
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
        let projects_snapshot = {
            let s = self.projects_state.borrow();
            ProjectsSnapshot {
                projects: s.projects.clone(),
            }
        };
        let projects_actions =
            make_projects_actions(Rc::clone(&self.projects_state), self.desktop.clone());
        let media_snapshot = {
            let s = self.media_state.borrow();
            MediaSnapshot {
                mediums: s.mediums.clone(),
                selected_medium: s.selected_medium.clone(),
                selected_project: s.selected_project.clone(),
                selected_session: s.selected_session.clone(),
                expanded: s.expanded.clone(),
            }
        };
        let media_actions = make_media_actions(
            Rc::clone(&self.media_state),
            Rc::clone(&self.state),
            self.desktop.clone(),
        );
        let screen_snapshot = {
            let s = self.screen_state.borrow();
            ScreenSnapshot {
                open: s.open,
                broadcast: s.broadcast,
                computer_use: s.computer_use,
                selected_source: s.selected_source.clone(),
                sources: s.sources.clone(),
                last_capture: s.last_capture.clone(),
                recording: s.recording,
                replaying: s.replaying,
                replay_loop: s.replay_loop,
                recorded_count: s.recorded.len(),
                replay_status: s.replay_status.clone(),
                frame: s.frame.as_ref().map(|f| ScreenFrameSnapshot {
                    frame_seq: s.frame_seq,
                    width: f.width,
                    height: f.height,
                    data: Arc::from(f.data.clone()),
                }),
            }
        };
        let screen_actions =
            make_screen_actions(Rc::clone(&self.screen_state), self.desktop.clone());
        self.element = build_ui(
            app,
            &snapshot,
            &actions,
            &ai_snapshot,
            &ai_actions,
            &projects_snapshot,
            &projects_actions,
            &media_snapshot,
            &media_actions,
            &screen_snapshot,
            &screen_actions,
        );
        self.actions = Some(actions);
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
        // Global shortcuts (warp-new style pane splitting + pane focus
        // navigation + the command palette). These are handled before the tree
        // so a focused composer cannot swallow the keys.
        if let Some(actions) = &self.actions {
            if let DispatchedEvent::KeyDown { key, modifiers } = event {
                let has_ctrl_cmd = modifiers.ctrl || modifiers.command;
                // Cmd+K toggles the command palette (no other modifier).
                if has_ctrl_cmd && !modifiers.shift && key.eq_ignore_ascii_case("k") {
                    (actions.on_toggle_command_palette.borrow_mut())();
                    return true;
                }
                if has_ctrl_cmd && !modifiers.shift && key == " " {
                    (actions.on_split_right.borrow_mut())();
                    return true;
                }
                if modifiers.command && modifiers.shift && key.eq_ignore_ascii_case("d") {
                    (actions.on_split_down.borrow_mut())();
                    return true;
                }
                if has_ctrl_cmd && modifiers.shift && key.eq_ignore_ascii_case("t") {
                    (actions.on_new_terminal.borrow_mut())();
                    return true;
                }
                if has_ctrl_cmd && !modifiers.shift && key.eq_ignore_ascii_case("w") {
                    (actions.on_close_pane.borrow_mut())();
                    return true;
                }
                // Pane focus navigation: Ctrl/Cmd+Arrows move between panes.
                if has_ctrl_cmd && !modifiers.shift && !modifiers.alt {
                    let dir = match key.as_str() {
                        "ArrowLeft" => Some(crate::ui::NavDir::Left),
                        "ArrowRight" => Some(crate::ui::NavDir::Right),
                        "ArrowUp" => Some(crate::ui::NavDir::Up),
                        "ArrowDown" => Some(crate::ui::NavDir::Down),
                        _ => None,
                    };
                    if let Some(dir) = dir {
                        (actions.on_pane_navigate.borrow_mut())(dir);
                        return true;
                    }
                }
            }
        }
        self.element.dispatch_event(event, ctx, app)
    }
}
