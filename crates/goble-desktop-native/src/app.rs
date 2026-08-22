use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::{CollectingEventBus, DesktopState};
use goble_ui::elements::{
    ActiveView, AppContext, ChatSidebarTab, ConversationEntry, ConversationSidebar,
    ConversationStatus, Element, SettingsTab, ShellState, ShellView,
};
use goble_ui::theme::Theme;
use tokio::runtime::Runtime;

use crate::views::agent::AgentManagementView;
use crate::views::agent_trace::AgentTraceViewPanel;
use crate::views::chat::ChatViewPanel;
use crate::views::connectors::ConnectorsViewPanel;
use crate::views::drive::DriveViewPanel;
use crate::views::executions::ExecutionsViewPanel;
use crate::views::logs::LogsViewPanel;
use crate::views::search::SearchViewPanel;
use crate::views::settings::SettingsViewPanel;
use crate::views::teams::TeamsViewPanel;
use crate::views::threads::ThreadsViewPanel;
use crate::views::workflows::WorkflowsViewPanel;

/// UI-specific state that is independent of the service layer.
#[derive(Default, Clone)]
pub struct UiState {
    pub selected_chat_id: Option<String>,
    pub selected_thread_id: String,
    pub selected_trace_id: Option<String>,
    pub chat_sidebar_tab: ChatSidebarTab,
    pub chat_sidebar_visible: bool,
    pub settings_tab: SettingsTab,
    pub dark_mode: bool,
    pub selected_agent_id: Option<String>,
    pub agent_new_open: bool,
    pub agent_editing: bool,
    pub agent_scheduling: bool,
    pub agent_edit_name: String,
    pub agent_edit_prompt: String,
    pub agent_edit_description: String,
    pub agent_edit_tools: Vec<String>,
    pub agent_edit_mcp_ids: Vec<String>,
    pub agent_schedule_cron: String,
    pub thread_reply_to_id: Option<String>,
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
        ui_state.chat_sidebar_visible = true;
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
        let app_context = Rc::clone(&self.app_context);
        let app_context_for_shell = Rc::clone(&self.app_context);
        let bus = self.bus.clone();

        let event_checker = Rc::new(RefCell::new(move || {
            let events = bus.take_events();
            !events.is_empty()
        }));

        let shell_app = app_context_for_shell.borrow();
        let state_for_sidebar = Arc::clone(&self.state);
        let ui_state_for_sidebar = Rc::clone(&self.ui_state);
        ShellView::with_content_and_event_checker(
            shell_state,
            &*shell_app,
            Box::new(
                move |shell_state: Rc<RefCell<ShellState>>, dirty: Rc<RefCell<bool>>| {
                    {
                        let mut ui = ui_state.borrow_mut();
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
                            Rc::clone(&shell_state),
                            Rc::clone(&ui_state),
                            dirty,
                            &*app,
                        )
                        .finish(),
                        ActiveView::AgentManagement => AgentManagementView::new(
                            Arc::clone(&state),
                            Rc::clone(&ui_state),
                            dirty,
                            &*app,
                        )
                        .finish(),
                        ActiveView::Threads => ThreadsViewPanel::new(
                            Arc::clone(&state),
                            Rc::clone(&ui_state),
                            dirty,
                            &*app,
                        )
                        .finish(),
                        ActiveView::Drive => {
                            DriveViewPanel::new(Arc::clone(&state), dirty, &*app).finish()
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
                        ActiveView::Executions => ExecutionsViewPanel::new(
                            Arc::clone(&state),
                            Rc::clone(&shell_state),
                            Rc::clone(&ui_state),
                            dirty,
                            &*app,
                        )
                        .finish(),
                        ActiveView::AgentTrace => AgentTraceViewPanel::new(
                            Arc::clone(&state),
                            Rc::clone(&shell_state),
                            Rc::clone(&ui_state),
                            dirty,
                            &*app,
                        )
                        .finish(),
                        ActiveView::Connectors => ConnectorsViewPanel::new(
                            Arc::clone(&state),
                            Rc::clone(&ui_state),
                            dirty,
                            &*app,
                        )
                        .finish(),
                        ActiveView::Workflows => WorkflowsViewPanel::new(
                            Arc::clone(&state),
                            Rc::clone(&ui_state),
                            dirty,
                            &*app,
                        )
                        .finish(),
                        ActiveView::Teams => TeamsViewPanel::new(
                            Arc::clone(&state),
                            Rc::clone(&ui_state),
                            dirty,
                            &*app,
                        )
                        .finish(),
                        ActiveView::Logs => LogsViewPanel::new(
                            Arc::clone(&state),
                            Rc::clone(&ui_state),
                            dirty,
                            &*app,
                        )
                        .finish(),
                        ActiveView::Search => SearchViewPanel::new(
                            Arc::clone(&state),
                            Rc::clone(&ui_state),
                            Rc::clone(&shell_state),
                            dirty,
                            &*app,
                        )
                        .finish(),
                    }
                },
            ),
            Some(event_checker),
        )
        .with_conversation_sidebar(move |_app, dirty| {
            let chats = state_for_sidebar.list_chats();
            let entries: Vec<ConversationEntry> = chats
                .iter()
                .map(|chat| {
                    let last_response = state_for_sidebar
                        .list_chat_messages(&chat.id)
                        .ok()
                        .and_then(|msgs| msgs.last().map(|m| m.content.clone()))
                        .unwrap_or_default();
                    let timestamp = &chat.updated_at[..chat.updated_at.len().min(19)];
                    ConversationEntry {
                        id: chat.id.clone(),
                        name: chat.title.clone(),
                        last_response,
                        timestamp: timestamp.to_string(),
                        status: ConversationStatus::Default,
                    }
                })
                .collect();

            let state_for_delete = Arc::clone(&state_for_sidebar);
            let state_for_create = Arc::clone(&state_for_sidebar);
            let ui_state_for_select = Rc::clone(&ui_state_for_sidebar);
            let ui_state_for_delete = Rc::clone(&ui_state_for_sidebar);
            let ui_state_for_create = Rc::clone(&ui_state_for_sidebar);
            let dirty_for_select = Rc::clone(&dirty);
            let dirty_for_delete = Rc::clone(&dirty);
            let dirty_for_create = Rc::clone(&dirty);

            let selected_id = ui_state_for_sidebar.borrow().selected_chat_id.clone();

            ConversationSidebar::new(entries)
                .with_selected(selected_id.unwrap_or_default())
                .with_on_select(move |id| {
                    ui_state_for_select.borrow_mut().selected_chat_id = Some(id);
                    *dirty_for_select.borrow_mut() = true;
                })
                .with_on_delete(move |id| {
                    if let Err(e) = state_for_delete.delete_chat(&id) {
                        log::error!("failed to delete chat: {}", e);
                    }
                    let remaining = state_for_delete.list_chats();
                    ui_state_for_delete.borrow_mut().selected_chat_id =
                        remaining.first().map(|c| c.id.clone());
                    *dirty_for_delete.borrow_mut() = true;
                })
                .with_on_create(move || {
                    match state_for_create.create_chat("New chat", None, None) {
                        Ok(id) => {
                            ui_state_for_create.borrow_mut().selected_chat_id = Some(id.clone());
                            log::info!("created chat {}", id);
                        }
                        Err(e) => log::error!("failed to create chat: {}", e),
                    }
                    *dirty_for_create.borrow_mut() = true;
                })
                .finish()
        })
        .finish()
    }
}
