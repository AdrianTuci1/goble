//! Hot-reloadable UI for the native Goble app.
//!
//! This crate is compiled as a `cdylib` and loaded at runtime by
//! `hot-lib-reloader` (see `app/src/hot_ui.rs`). `build_ui` is the only
//! reloadable function; keep its signature and the shapes of [`UiSnapshot`] /
//! [`UiActions`] (and [`AiSnapshot`] / [`AiActions`]) stable during a dev
//! session — changing them requires rebuilding `goble-app` (the executable
//! bakes in the ABI).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goble_ui::elements::{
    AgentCardUi, AppContext, ChatMessage as UiChatMessage, Container, ConversationEntry,
    CrossAxisAlignment, Divider, Element, Expanded, Fill, Flex, MainAxisSize,
};
use goble_ui::theme::ColorToken;
use goble_ui::{SettingsPage, Sheet, Stack, SHEET_DEFAULT_WIDTH};

pub mod chat;
pub mod connectors;
pub mod crons;
pub mod shell;
pub mod sidebar;
pub mod vault;

/// Width of the left conversation sidebar.
pub const SIDEBAR_WIDTH: f32 = 300.0;

/// Width of the connectors sheet (wider than the default panels).
pub const CONNECTORS_WIDTH: f32 = 480.0;

/// A scheduled task (cron) shown in the agent's crons drawer.
#[derive(Clone, Debug)]
pub struct CronEntry {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub enabled: bool,
    pub last_run: String,
}

impl CronEntry {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        schedule: impl Into<String>,
        last_run: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            schedule: schedule.into(),
            enabled: true,
            last_run: last_run.into(),
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// A vault secret shown in the vault panel.
#[derive(Clone, Debug)]
pub struct VaultSecretEntry {
    pub key: String,
    pub updated_at: String,
}

/// An installed MCP server shown in the connectors panel.
#[derive(Clone, Debug)]
pub struct McpServerEntry {
    pub id: String,
    pub name: String,
    pub source: String,
    pub source_value: Option<String>,
    pub capabilities: Vec<String>,
    pub auth_required: bool,
    pub discovered_tools: Vec<String>,
    pub secret_ids: Vec<String>,
    pub enabled_tools: Vec<String>,
}

/// A registry search result shown in the install drawer.
#[derive(Clone, Debug)]
pub struct McpSearchEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub auth_required: bool,
    pub source_kind: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppTab {
    Threads,
    Chat,
    Settings,
}

/// Plain snapshot of the main UI state used to build the tree. Owned by the
/// host app; rendered from scratch every frame so state changes show up live.
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
    /// Model choices shown in the composer's model dropdown.
    pub models: Vec<String>,
    /// Currently selected model (shown as the composer's model label).
    pub selected_model: String,
    /// App-owned open flags for the composer model / account menus, so open
    /// state survives the per-frame element rebuild.
    pub model_menu_open: Rc<RefCell<bool>>,
    pub profile_menu_open: Rc<RefCell<bool>>,
    pub agent_name: String,
    pub agent_busy: bool,
    pub right_sidebar_open: bool,
    pub crons_open: bool,
    pub crons: Vec<CronEntry>,
    pub settings_page: SettingsPage,
    pub settings_profile_name: String,
    pub settings_profile_email: String,
    pub settings_dark_mode: bool,
    pub settings_llm_provider: String,
    pub settings_llm_model: String,
    pub settings_llm_api_key: String,
    pub settings_llm_base_url: String,
    pub settings_llm_temperature: String,
    pub settings_workers: Vec<(String, String, String, bool)>,
    pub settings_cluster_name: String,
    pub settings_cluster_configured: bool,
    pub settings_authorized_keys: Vec<(String, String, String)>,
    pub settings_vault_unlocked: bool,
    pub sidebar_width: f32,
    pub sidebar_dragging: bool,
    /// Per-card interaction state (hover / delete menu), shared with the
    /// hot-reloaded card elements so selections and menus persist across frames.
    pub agent_cards: HashMap<String, Rc<RefCell<AgentCardUi>>>,
    /// Hover flag for the sidebar's "New agent" row, owned here so the row's
    /// highlight survives the per-frame element rebuild.
    pub new_agent_hover: Rc<RefCell<bool>>,
}

/// Plain snapshot of the AI domain state (vault + MCP connectors) used to
/// build the auxiliary panels. Owned by the host app.
#[derive(Clone, Debug)]
pub struct AiSnapshot {
    pub connectors_open: bool,
    pub vault_open: bool,
    pub vault_unlocked: bool,
    pub vault_secrets: Vec<VaultSecretEntry>,
    pub vault_unlock_draft: String,
    pub vault_new_key: String,
    pub vault_new_value: String,
    pub vault_error: Option<String>,
    pub connector_search: String,
    pub connectors: Vec<McpServerEntry>,
    pub install_open: bool,
    pub install_editing_id: Option<String>,
    pub install_name: String,
    pub install_source: String,
    pub install_source_value: String,
    pub install_search_query: String,
    pub install_search_results: Vec<McpSearchEntry>,
    pub install_selected_secrets: Vec<String>,
    pub install_error: Option<String>,
    pub installing: bool,
}

/// Callbacks supplied by the host app for the main view. Created fresh on
/// every rebuild; they mutate app-owned state, which is rendered back on the
/// next frame.
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
    /// Select a specific model from the composer dropdown by display name.
    pub on_model_select: Rc<RefCell<dyn FnMut(String)>>,
    pub on_copy: Rc<RefCell<dyn FnMut()>>,
    pub on_restart: Rc<RefCell<dyn FnMut()>>,
    pub on_stop: Rc<RefCell<dyn FnMut()>>,
    pub on_menu: Rc<RefCell<dyn FnMut()>>,
    pub on_threads: Rc<RefCell<dyn FnMut()>>,
    pub on_inbox: Rc<RefCell<dyn FnMut()>>,
    pub on_settings: Rc<RefCell<dyn FnMut()>>,
    pub on_plugins: Rc<RefCell<dyn FnMut()>>,
    pub on_open_crons: Rc<RefCell<dyn FnMut()>>,
    pub on_close_crons: Rc<RefCell<dyn FnMut()>>,
    pub on_toggle_right_sidebar: Rc<RefCell<dyn FnMut()>>,
    pub on_cron_create: Rc<RefCell<dyn FnMut()>>,
    pub on_cron_delete: Rc<RefCell<dyn FnMut(String)>>,
    pub on_cron_trigger: Rc<RefCell<dyn FnMut(String)>>,
    /// Begin dragging the sidebar divider at the given pointer x.
    pub on_sidebar_drag_start: Rc<RefCell<dyn FnMut(f32)>>,
    /// Move the divider while dragging, given the pointer x.
    pub on_sidebar_drag_move: Rc<RefCell<dyn FnMut(f32)>>,
    /// Finish a drag; the divider settles at the last width.
    pub on_sidebar_drag_end: Rc<RefCell<dyn FnMut()>>,
    /// Delete a conversation/agent card from the list.
    pub on_agent_delete: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: return to the previous view (chat).
    pub on_settings_back: Rc<RefCell<dyn FnMut()>>,
    /// Settings: switch the active settings page.
    pub on_settings_navigate: Rc<RefCell<dyn FnMut(SettingsPage)>>,
    /// Settings: toggle dark mode.
    pub on_toggle_dark_mode: Rc<RefCell<dyn FnMut(bool)>>,
    /// Settings: save the profile (name, email).
    pub on_save_profile: Rc<RefCell<dyn FnMut(String, String)>>,
    /// Settings: save the LLM config (provider, model, api_key, base_url, temperature).
    pub on_save_llm: Rc<RefCell<dyn FnMut(String, String, String, String, String)>>,
    /// Settings: register a worker (name, url).
    pub on_add_worker: Rc<RefCell<dyn FnMut(String, String)>>,
    /// Settings: remove a worker by id.
    pub on_remove_worker: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: unlock the vault with a passphrase.
    pub on_vault_unlock: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: create a cluster identity (name, passphrase).
    pub on_create_cluster: Rc<RefCell<dyn FnMut(String, String)>>,
    /// Settings: unlock the cluster identity (passphrase).
    pub on_unlock_cluster: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: add an authorized key (name, pem, fingerprint).
    pub on_add_authorized_key: Rc<RefCell<dyn FnMut(String, String, String)>>,
    /// Settings: remove an authorized key by id.
    pub on_remove_authorized_key: Rc<RefCell<dyn FnMut(String)>>,
}

/// Callbacks supplied by the host app for the AI domain (vault + connectors).
pub struct AiActions {
    pub on_open_connectors: Rc<RefCell<dyn FnMut()>>,
    pub on_close_connectors: Rc<RefCell<dyn FnMut()>>,
    pub on_open_vault: Rc<RefCell<dyn FnMut()>>,
    pub on_close_vault: Rc<RefCell<dyn FnMut()>>,
    pub on_vault_unlock_draft_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_vault_unlock: Rc<RefCell<dyn FnMut()>>,
    pub on_vault_new_key_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_vault_new_value_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_vault_secret_add: Rc<RefCell<dyn FnMut()>>,
    pub on_vault_secret_delete: Rc<RefCell<dyn FnMut(String)>>,
    pub on_connector_search_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_open: Rc<RefCell<dyn FnMut()>>,
    pub on_install_edit: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_pick: Rc<RefCell<dyn FnMut(String, String, String)>>,
    pub on_install_close: Rc<RefCell<dyn FnMut()>>,
    pub on_install_name_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_source_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_source_value_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_search_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_secret_toggle: Rc<RefCell<dyn FnMut(String, bool)>>,
    pub on_install_submit: Rc<RefCell<dyn FnMut()>>,
    pub on_connector_delete: Rc<RefCell<dyn FnMut(String)>>,
    pub on_connector_discover: Rc<RefCell<dyn FnMut(String)>>,
    pub on_connector_toggle: Rc<RefCell<dyn FnMut(String, bool)>>,
}

/// Build the complete app UI: topbar, sidebar, main content, and the
/// auxiliary sheets (crons, connectors, vault) stacked on top.
///
/// `#[no_mangle]` exposes the symbol so `app/src/hot_ui.rs` can generate a
/// hot-reloadable wrapper for it; without it the app would never reload this
/// function and UI edits would not show up live.
#[allow(unsafe_code)]
#[no_mangle]
pub fn build_ui(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    ai: &AiSnapshot,
    ai_actions: &AiActions,
) -> Box<dyn Element> {
    let topbar = shell::build_topbar(app, state, actions);
    let sidebar = sidebar::build_sidebar(app, state, actions, ai_actions);
    let main = shell::build_main(app, state, actions);
    let on_drag_start = actions.on_sidebar_drag_start.clone();
    let on_drag_move = actions.on_sidebar_drag_move.clone();
    let on_drag_end = actions.on_sidebar_drag_end.clone();
    let body = shell::SidebarLayout::new(sidebar, main, state.sidebar_width)
        .with_dragging(state.sidebar_dragging)
        .with_on_drag_start(move |x| (on_drag_start.borrow_mut())(x))
        .with_on_drag_move(move |x| (on_drag_move.borrow_mut())(x))
        .with_on_drag_end(move || (on_drag_end.borrow_mut())());

    let mut shell_col = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    shell_col = shell_col.with_child(topbar);
    // Separator below the topbar, like the sidebar separators. The topbar is
    // a fixed height and is not resizable, so this is purely a visual rule.
    shell_col = shell_col.with_child(Divider::horizontal().finish());
    // The body must consume only the *remaining* height below the topbar, so
    // wrap it in `Expanded`; otherwise the chat composer would be pushed past
    // the bottom edge of the window.
    shell_col = shell_col.with_child(Expanded::new(body.finish()).finish());

    let on_close_crons = actions.on_close_crons.clone();
    let crons_sheet = Sheet::new(crons::build_crons_drawer(app, state, actions))
        .with_expanded(state.crons_open)
        .with_width(SHEET_DEFAULT_WIDTH)
        .with_on_close(move || (on_close_crons.borrow_mut())())
        .finish();

    let on_close_connectors = ai_actions.on_close_connectors.clone();
    let connectors_sheet = Sheet::new(connectors::build_connectors_sheet(app, ai, ai_actions))
        .with_expanded(ai.connectors_open)
        .with_width(CONNECTORS_WIDTH)
        .with_on_close(move || (on_close_connectors.borrow_mut())())
        .finish();

    let on_close_vault = ai_actions.on_close_vault.clone();
    let vault_sheet = Sheet::new(vault::build_vault_sheet(app, ai, ai_actions))
        .with_expanded(ai.vault_open)
        .with_width(SHEET_DEFAULT_WIDTH)
        .with_on_close(move || (on_close_vault.borrow_mut())())
        .finish();

    let stack = Stack::new().with_children(vec![
        shell_col.finish(),
        crons_sheet,
        connectors_sheet,
        vault_sheet,
    ]);

    Container::new(stack.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .finish()
}
