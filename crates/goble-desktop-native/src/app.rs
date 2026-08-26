use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::agent::Trigger;
use goble_desktop_service::{CollectingEventBus, DesktopState, WorkflowInfo};
use goble_ui::elements::{
    ActiveView, AppContext, Element, RoutineEntry, RoutineSidebar, RoutineStatus, RoutineTrigger,
    SettingsTab, ShellState, ShellView,
};
use goble_ui::theme::Theme;
use tokio::runtime::Runtime;

use crate::state_api;
use crate::views::agent::AgentManagementView;
use crate::views::chat::ChatViewPanel;
use crate::views::drive::DriveViewPanel;
use crate::views::settings::SettingsViewPanel;
use crate::views::threads::ThreadsViewPanel;

/// UI-specific state that is independent of the service layer.
#[derive(Default, Clone)]
pub struct UiState {
    pub selected_chat_id: Option<String>,
    pub selected_routine_id: Option<String>,
    pub selected_thread_id: String,
    pub settings_tab: SettingsTab,
    pub dark_mode: bool,
    pub chat_streaming: HashMap<String, String>,
    pub thread_streaming: HashMap<String, String>,
    pub agent_edit_name: HashMap<String, String>,
    pub agent_edit_prompt: HashMap<String, String>,
    pub agent_edit_description: HashMap<String, String>,
    pub workflow_edit_name: HashMap<String, String>,
    pub workflow_edit_description: HashMap<String, String>,
    pub team_edit_name: HashMap<String, String>,
    pub mcp_edit_name: HashMap<String, String>,
}

pub struct GobleApp {
    pub runtime: Runtime,
    pub state: Arc<DesktopState>,
    pub bus: CollectingEventBus,
    pub app_context: Rc<RefCell<AppContext>>,
    pub ui_state: Rc<RefCell<UiState>>,
}

impl GobleApp {
    pub fn new() -> anyhow::Result<Self> {
        let runtime = tokio::runtime::Runtime::new()?;
        let _guard = runtime.enter();

        let bus = CollectingEventBus::default();
        let state = DesktopState::open_default()?;
        state.set_event_bus(Arc::new(bus.clone()));

        let app_context = Rc::new(RefCell::new(AppContext::default()));
        app_context.borrow_mut().theme = Theme::default();

        let mut ui_state = UiState::default();
        ui_state.dark_mode = true;
        let ui_state = Rc::new(RefCell::new(ui_state));

        Ok(Self {
            runtime,
            state,
            bus,
            app_context,
            ui_state,
        })
    }

    pub fn run(self) -> anyhow::Result<()> {
        log::info!("Starting Goble native desktop app");
        let root = self.build_root();
        goble_ui::platform::run_with_root(root, Rc::clone(&self.app_context))?;
        Ok(())
    }

    fn build_root(&self) -> Box<dyn Element> {
        let shell_state = ShellState::default();
        let state = Arc::clone(&self.state);
        let ui_state = Rc::clone(&self.ui_state);
        let ui_state_for_event = Rc::clone(&self.ui_state);
        let app_context = Rc::clone(&self.app_context);
        let bus = self.bus.clone();
        let event_checker = Rc::new(RefCell::new(move || {
            let events = bus.take_events();
            let mut dirty = false;
            let mut ui = ui_state_for_event.borrow_mut();
            for (event, payload) in &events {
                match event.as_str() {
                    "harness:event" => {
                        let target_id = payload
                            .get("chat_id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .or_else(|| {
                                payload
                                    .get("thread_id")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                            });
                        let is_thread = payload.get("thread_id").is_some();
                        if let Some(target_id) = target_id {
                            if let Some(ev) = payload.get("event") {
                                if let Some(typ) = ev.get("type").and_then(|v| v.as_str()) {
                                    match typ {
                                        "AssistantDelta" => {
                                            let delta = ev
                                                .get("payload")
                                                .and_then(|p| p.get("delta"))
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            if is_thread {
                                                ui.thread_streaming
                                                    .entry(target_id)
                                                    .or_default()
                                                    .push_str(delta);
                                            } else {
                                                ui.chat_streaming
                                                    .entry(target_id)
                                                    .or_default()
                                                    .push_str(delta);
                                            }
                                            dirty = true;
                                        }
                                        "Done" | "Error" => {
                                            if is_thread {
                                                ui.thread_streaming.remove(&target_id);
                                            } else {
                                                ui.chat_streaming.remove(&target_id);
                                            }
                                            dirty = true;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    "chat:updated" | "chats:updated" | "agents:updated" | "workflows:updated"
                    | "teams:updated" | "executions:updated" | "vault:updated"
                    | "workers:updated" | "cluster:updated" | "threads:updated"
                    | "thread:updated" | "thread:messages:updated" | "thread:message:created"
                    | "agent:log" | "agent:started" | "agent:finished" | "agent:state_update"
                    | "agent:tool_result" | "logs:updated" => {
                        dirty = true;
                    }
                    _ => {}
                }
            }
            dirty
        }));

        let shell_app = app_context.borrow().clone();

        fn map_workflow_to_routine(info: &WorkflowInfo) -> RoutineEntry {
            let trigger = match &info.trigger {
                Trigger::Cron { .. } => RoutineTrigger::Cron,
                _ => RoutineTrigger::Manual,
            };
            RoutineEntry::new(&info.id, &info.name)
                .with_trigger(trigger)
                .with_enabled(info.enabled)
                .with_status(RoutineStatus::Idle)
        }

        let state_for_sidebar = Arc::clone(&state);
        let ui_state_for_sidebar = Rc::clone(&ui_state);
        let sidebar_resolver =
            move |_shell_state: Rc<RefCell<ShellState>>, dirty: Rc<RefCell<bool>>| {
                let workflows = state_api::list_workflows(&state_for_sidebar);
                let routines: Vec<RoutineEntry> = workflows
                    .iter()
                    .map(map_workflow_to_routine)
                    .collect();
                let selected_id = ui_state_for_sidebar.borrow().selected_routine_id.clone();
                let state_for_create = Arc::clone(&state_for_sidebar);
                let ui_state_for_create = Rc::clone(&ui_state_for_sidebar);
                let dirty_for_create = Rc::clone(&dirty);
                let _state_for_select = Arc::clone(&state_for_sidebar);
                let ui_state_for_select = Rc::clone(&ui_state_for_sidebar);
                let dirty_for_select = Rc::clone(&dirty);
                let state_for_delete = Arc::clone(&state_for_sidebar);
                let dirty_for_delete = Rc::clone(&dirty);
                let state_for_toggle = Arc::clone(&state_for_sidebar);
                let dirty_for_toggle = Rc::clone(&dirty);

                RoutineSidebar::new(routines)
                    .with_selected(selected_id)
                    .with_on_create(move || {
                        let req = state_api::CreateWorkflowRequest {
                            name: "New routine".to_string(),
                            description: "".to_string(),
                            steps: vec![],
                            trigger: Trigger::Manual,
                        };
                        if let Ok(info) = state_api::create_workflow(&state_for_create, req) {
                            ui_state_for_create.borrow_mut().selected_routine_id = Some(info.id);
                            *dirty_for_create.borrow_mut() = true;
                        }
                    })
                    .with_on_select(move |id| {
                        ui_state_for_select.borrow_mut().selected_routine_id = Some(id);
                        *dirty_for_select.borrow_mut() = true;
                    })
                    .with_on_delete(move |id| {
                        if let Err(e) = state_api::delete_workflow(&state_for_delete, &id) {
                            log::error!("failed to delete routine {}: {}", id, e);
                        }
                        *dirty_for_delete.borrow_mut() = true;
                    })
                    .with_on_toggle_enabled(move |id| {
                        if let Err(e) = state_api::toggle_workflow_enabled(&state_for_toggle, &id) {
                            log::error!("failed to toggle routine {}: {}", id, e);
                        }
                        *dirty_for_toggle.borrow_mut() = true;
                    })
                    .finish()
            };

        ShellView::with_content_and_event_checker(
            shell_state,
            &shell_app,
            Box::new(
                move |shell_state: Rc<RefCell<ShellState>>, dirty: Rc<RefCell<bool>>| {
                    {
                        let mut ui = ui_state.borrow_mut();
                        app_context.borrow_mut().theme =
                            if ui.dark_mode { Theme::dark() } else { Theme::light() };
                        ui.settings_tab = match shell_state.borrow().active_view {
                            ActiveView::Settings(tab) => tab,
                            _ => ui.settings_tab,
                        };
                    }
                    let app = app_context.borrow();
                    let active_view = shell_state.borrow().active_view;
                    match active_view {
                        ActiveView::Chat => ChatViewPanel::new(
                            Arc::clone(&state),
                            Rc::clone(&ui_state),
                            dirty,
                            &*app,
                        )
                        .finish(),
                        ActiveView::AgentManagement => {
                            AgentManagementView::new(
                                Arc::clone(&state),
                                Rc::clone(&ui_state),
                                dirty,
                                &*app,
                            )
                            .finish()
                        }
                        ActiveView::Threads => ThreadsViewPanel::new(
                            Arc::clone(&state),
                            Rc::clone(&ui_state),
                            dirty,
                            &*app,
                        )
                        .finish(),
                        ActiveView::Drive => {
                            DriveViewPanel::new(
                                Arc::clone(&state),
                                Rc::clone(&ui_state),
                                dirty,
                                &*app,
                            )
                            .finish()
                        }
                        ActiveView::Settings(tab) => SettingsViewPanel::new(
                            Arc::clone(&state),
                            Rc::clone(&ui_state),
                            shell_state,
                            dirty,
                            tab,
                            &*app,
                            Rc::clone(&app_context),
                        )
                        .finish(),
                    }
                },
            ),
            Some(event_checker),
        )
        .with_sidebar(sidebar_resolver)
        .finish()
    }
}
