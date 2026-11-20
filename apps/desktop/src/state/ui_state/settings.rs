//! The settings tab's state loads: the file projection, the theme it drives and
//! the remote configuration the rest of the app reads.
//!
//! The settings files are the source of truth. [`UiState::refresh_settings`] is
//! the one place that reads them for the tab: it asks the desktop service for the
//! redacted `ConfigView` of each file (Lane B2's `settings_document`), copies the
//! paths and revisions into [`SettingsUiState`], and reloads the theme overrides
//! the Appearance page shows. A write replaces the view with the fresh one the
//! service returns; a reload is this function plus a notice.

use goble_core::config::{ConfigFile, RemoteMode, SettingsPage as SchemaPage};
use goble_core::ssh_hosts::SshHosts;
use goble_ui::theme::ThemePreset;

use super::*;
use crate::ui::settings::{SettingsAction, SettingsRegion, SettingsUiState as SettingsState};

impl UiState {
    pub fn refresh_llm_models(&mut self, desktop: &DesktopState) {
        self.models = desktop.available_models(&self.settings_llm_provider);
        if self.selected_model.trim().is_empty() {
            self.selected_model = desktop.default_model(&self.settings_llm_provider);
        }
        self.model_groups = desktop.model_groups(&self.settings_llm_provider);
        // The harness's own answer to "what can this draft list": the commands
        // and skills behind `/` and `$`, and the models behind the tray.
        self.composer_catalog = desktop.composer_catalog(&self.settings_llm_provider);
        let conversations: Vec<(u64, String)> = self
            .pane_sessions
            .iter()
            .map(|(pane_id, session)| (*pane_id, session.conversation_id.clone()))
            .collect();
        for (pane_id, chat_id) in conversations {
            if chat_id.trim().is_empty() {
                continue;
            }
            let level = if desktop.get_chat_reasoning(&chat_id) {
                "on"
            } else {
                "off"
            };
            self.pane_controls_mut(pane_id).thinking = level.to_string();
        }
    }

    pub fn refresh_agent_name(&mut self, desktop: &DesktopState) {
        if let Some(name) = desktop.list_agents().first().map(|a| a.name.clone()) {
            self.agent_name = name;
        }
    }

    pub fn prime_llm_form(&self) {
        *self.llm_dialog_provider.borrow_mut() = self.settings_llm_provider.clone();
        *self.llm_dialog_model.borrow_mut() = self.settings_llm_model.clone();
        *self.llm_dialog_api_key.borrow_mut() = self.settings_llm_api_key.clone();
        *self.llm_dialog_base_url.borrow_mut() = self.settings_llm_base_url.clone();
        *self.llm_dialog_temperature.borrow_mut() = self.settings_llm_temperature.clone();
        *self.llm_dialog_focus.borrow_mut() = None;
    }

    /// Fill the Hosts page's read-only `~/.ssh` view once, when it is shown.
    pub fn ensure_ssh_hosts(&mut self) {
        if self.settings.ssh_hosts.is_some() {
            return;
        }
        if let Some(home) = self.settings.ssh_home() {
            self.settings.ssh_hosts = Some(goble_core::ssh_hosts::read_ssh_hosts(&home));
        }
    }

    pub fn ssh_hosts_for_command(&self) -> Option<SshHosts> {
        self.ssh_home()
            .map(|home| goble_core::ssh_hosts::read_ssh_hosts(&home))
    }

    pub fn attach_update_service(&mut self, service: std::sync::Arc<crate::update::UpdateService>) {
        self.update_view = service.snapshot();
        self.update = Some(service);
    }

    pub fn refresh_update_view(&mut self) {
        if let Some(service) = &self.update {
            self.update_view = service.snapshot();
        }
    }

    pub fn check_for_updates(&mut self) {
        if let Some(service) = &self.update {
            service.check_now();
            self.update_view = service.snapshot();
        }
    }

    pub fn install_available_update(&mut self) {
        if let Some(service) = &self.update {
            service.install_available();
            self.update_view = service.snapshot();
        }
    }

    fn ssh_home(&self) -> Option<std::path::PathBuf> {
        self.settings
            .ssh_home_override
            .clone()
            .or_else(|| crate::state::paths::home_directory().map(std::path::PathBuf::from))
    }

    /// Reload the crons drawer's rows: the registered workflows, each with its
    /// schedule and the newest run the ledger holds.
    pub fn refresh_crons(&mut self, desktop: &DesktopState) {
        self.refresh_workflow_runs(desktop);
        self.crons = desktop
            .list_workflows()
            .into_iter()
            .map(|wf| {
                let schedule = match wf.trigger {
                    goble_core::agent::Trigger::Cron { expression } => expression,
                    _ => "manual".to_string(),
                };
                let last_run = self
                    .latest_run(&wf.id)
                    .map(|run| format!("{} ({})", run.started_at, run.status))
                    .unwrap_or_else(|| "never".to_string());
                CronEntry::new(wf.id, wf.name, schedule, last_run).with_enabled(wf.enabled)
            })
            .collect();
    }

    /// Reload everything the Settings tab renders, from the files.
    ///
    /// `desktop` is the service handle the app owns; without it (a state built
    /// for a test that does not exercise settings) the page shows what it has and
    /// says nothing was loaded.
    pub fn refresh_settings(&mut self, desktop: &DesktopState) {
        self.refresh_settings_theme(desktop);
        self.refresh_settings_files(desktop);
        self.send_immediately =
            crate::ui::settings::fields::app_bool(&self.settings, "send_immediately", false);
        // The Providers page's Accounts and Agent CLIs sections read the
        // catalog's PATH detection (a stat/resolve pass, no --version probe) so
        // a page opened from a restored layout is already populated. Only on
        // the first load: a page write (and the colour wheel drags through
        // them) must not re-scan 39 harness commands every event; the Refresh
        // action and the harness writes re-scan explicitly.
        if self.settings.harnesses.is_empty() {
            self.settings.harnesses = desktop.list_harnesses();
        }
        self.settings_workers = desktop
            .list_workers()
            .into_iter()
            .map(|w| (w.id.clone(), w.name.clone(), w.url.clone(), w.paired))
            .collect();
        self.settings.vault.unlocked = desktop.is_vault_unlocked();
        if self.settings.vault.unlocked {
            self.settings.vault.secrets = desktop
                .list_vault_secrets()
                .into_iter()
                .map(|secret| (secret.key, secret.updated_at))
                .collect();
        }
        self.settings.update_view = self.update_view.clone();
        // The pages that read ~/.ssh (Hosts) share the one read; the page asks
        // for it when it is first shown, not while it draws.
        if self.settings.ssh_hosts.is_none() {
            if let Some(home) = self.settings.ssh_home() {
                self.settings.ssh_hosts = Some(goble_core::ssh_hosts::read_ssh_hosts(&home));
            }
        }
        self.vim_mode = desktop.get_vim_mode();
        self.auto_save_on_close = desktop.get_auto_save_on_close();
        self.refresh_llm_models(desktop);
        self.refresh_remote_agents(Some(desktop));
        self.settings.page = self.settings_page();
        self.settings.nav_index = SettingsState::nav_index_of(self.settings.page);
        // A reload can shorten a page; keep the focused row inside it.
        let count = crate::ui::settings::pages::page_model(self.settings.page, &self.settings)
            .control_count();
        if count > 0 {
            self.settings.row_index = self.settings.row_index.min(count - 1);
        }
    }

    /// Read the three settings files into the tab's state. The service's typed
    /// config is reloaded too, so the rest of the app (the remote choosers, the
    /// theme overrides) reads the same file the page just did — an edit made by
    /// hand is picked up by a reload, not only by a restart.
    pub fn refresh_settings_files(&mut self, desktop: &DesktopState) {
        let config = desktop
            .reload_settings_file(ConfigFile::Config)
            .unwrap_or_else(|_| desktop.settings_document());
        let environment = desktop
            .reload_settings_file(ConfigFile::Environment)
            .unwrap_or_else(|_| desktop.settings_document_for(ConfigFile::Environment));
        let hosts = desktop
            .reload_settings_file(ConfigFile::Hosts)
            .unwrap_or_else(|_| desktop.settings_document_for(ConfigFile::Hosts));
        self.settings.paths.home = goble_core::app_home::GobleHome::locate()
            .ok()
            .map(|home| home.root().to_path_buf());
        self.settings.paths.config = desktop
            .settings_path(ConfigFile::Config)
            .or_else(|| Some(std::path::PathBuf::from(&config.path)));
        self.settings.paths.environment = desktop
            .settings_path(ConfigFile::Environment)
            .or_else(|| Some(std::path::PathBuf::from(&environment.path)));
        self.settings.paths.hosts = desktop
            .settings_path(ConfigFile::Hosts)
            .or_else(|| Some(std::path::PathBuf::from(&hosts.path)));
        self.settings.adopt(config);
        self.settings.adopt(environment);
        self.settings.adopt(hosts);
    }

    /// Reload just one file's projection after a write or an external change.
    pub fn adopt_settings_view(&mut self, view: goble_core::config::ConfigView) {
        self.settings.adopt_with_problems(view);
        let count = crate::ui::settings::pages::page_model(self.settings.page, &self.settings)
            .control_count();
        if count > 0 {
            self.settings.row_index = self.settings.row_index.min(count - 1);
        }
    }

    /// The theme overrides the Appearance page writes, read back from the
    /// service's typed config so the app's live theme matches the file.
    fn refresh_settings_theme(&mut self, desktop: &DesktopState) {
        let theme = desktop.config().theme.clone();
        self.settings_dark_mode = theme.dark;
        self.theme_primary = theme.primary;
        self.theme_secondary = theme.secondary;
        self.theme_accent = theme.accent;
        self.theme_bg = theme.bg;
        self.theme_surface = theme.surface;
        self.theme_preset = theme.preset;
    }

    /// The `[remote]` / `[remote_agents.*]` view the rest of the app reads (the
    /// remote choosers, the deploy report). The Settings page reads the same
    /// tables through its own projection; this keeps the app-wide copy in step.
    pub fn refresh_remote_agents(&mut self, desktop: Option<&DesktopState>) {
        let Some(desktop) = desktop else {
            return;
        };
        let config = desktop.config();
        self.remote_agents = config
            .remote_agents
            .iter()
            .map(|(name, agent)| RemoteAgentEntry::from_config(name, agent))
            .collect();
        self.remote_mode = config.remote.mode;
        self.remote_gateway_url = config
            .remote
            .gateway_url()
            .unwrap_or_default()
            .to_string();
        self.remote_gateway_token = config
            .remote
            .worker_token()
            .unwrap_or_default()
            .to_string();
        self.remote_config_path = desktop
            .config_path()
            .or_else(|| {
                goble_core::app_home::GobleHome::locate()
                    .ok()
                    .map(|home| home.config_path())
            })
            .map(|path| path.display().to_string())
            .unwrap_or_default();
    }

    /// Whether Remote has somewhere to run: a gateway that is named and
    /// tokened, a ready `[remote_agents.<name>]` table, or a paired worker.
    pub fn remote_configured(&self) -> bool {
        if self.remote_mode == RemoteMode::Gateway
            && !self.remote_gateway_url.trim().is_empty()
            && !self.remote_gateway_token.trim().is_empty()
        {
            return true;
        }
        if self
            .remote_agents
            .iter()
            .any(|agent| agent.enabled && agent.missing.is_empty())
        {
            return true;
        }
        self.settings_workers.iter().any(|worker| worker.3)
    }

    /// What the settings page says about Remote under its heading: the
    /// configuration's own answer, unless a deploy started from the cloud-agent
    /// action is in flight or has reported.
    pub fn remote_status_line(&self) -> String {
        match &self.remote_deploy {
            RemoteDeploy::Idle => self.remote_config_line(),
            RemoteDeploy::Deploying { agent, provider } => {
                format!("Deploying {agent} on {provider} · this takes a moment")
            }
            RemoteDeploy::Ready { sandbox } => format!("Sandbox ready · {sandbox}"),
            RemoteDeploy::Failed { message } => format!("Deploy failed · {message}"),
        }
    }

    fn remote_config_line(&self) -> String {
        if self.remote_mode == RemoteMode::Gateway {
            return if self.remote_gateway_url.trim().is_empty() {
                "Gateway server · not configured".to_string()
            } else if self.remote_gateway_token.trim().is_empty() {
                format!(
                    "Gateway server · {} · missing worker token",
                    self.remote_gateway_url
                )
            } else {
                format!("Gateway server · {}", self.remote_gateway_url)
            };
        }
        if self.remote_agents.is_empty() {
            return "Turns run on this machine. Add a [remote] gateway or a \
                    [remote_agents.<name>] table to config.toml to run them elsewhere."
                .to_string();
        }
        self.remote_agents
            .iter()
            .map(|agent| format!("{} · {}", agent.name, agent.status))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn begin_remote_deploy(&mut self, agent: &RemoteAgentEntry) {
        self.remote_deploy = RemoteDeploy::Deploying {
            agent: agent.name.clone(),
            provider: agent.provider.clone(),
        };
    }

    /// Apply a `remote:deploy` report.
    pub fn apply_remote_deploy(&mut self, payload: &serde_json::Value) {
        let field = |name: &str| {
            payload
                .get(name)
                .and_then(|value| value.as_str())
                .unwrap_or_default()
        };
        match field("status") {
            "ready" => {
                let (provider, sandbox_name) = (field("provider"), field("sandbox_name"));
                if !provider.is_empty() && !sandbox_name.is_empty() {
                    self.remote_deploy = RemoteDeploy::Ready {
                        sandbox: format!("{provider} · {sandbox_name}"),
                    };
                }
            }
            "failed" => {
                let message = match field("message") {
                    "" => "the deploy stopped before the sandbox existed".to_string(),
                    message => message.to_string(),
                };
                self.remote_deploy = RemoteDeploy::Failed { message };
            }
            _ => {}
        }
    }

    /// Open Settings on Agent, where the Remote configuration and the file that
    /// holds it are named. Every chooser that names Remote before anything is
    /// configured lands here.
    pub fn open_remote_settings_tab(&mut self, desktop: Option<&DesktopState>) {
        self.open_settings_tab(desktop);
        self.settings_set_page(crate::ui::SettingsCategory::Agent);
        if let Some(index) = crate::ui::settings::pages::page_model(
            crate::ui::SettingsCategory::Agent,
            &self.settings,
        )
        .interactive_rows()
        .iter()
        .position(|row| row.id == "remote.mode")
        {
            self.settings.region = SettingsRegion::Page;
            self.settings.row_index = index;
            self.settings.focused_field = None;
        }
        if let Some(desktop) = desktop {
            self.save_panes(desktop);
        }
    }

    /// The page the settings tab should dispatch to the schema: the settings
    /// module's page list and the core schema's list are the same IA, so this is
    /// a straight map. It exists so a page that names a schema field can be
    /// checked against the page the schema puts it on.
    pub fn settings_schema_page(&self) -> SchemaPage {
        crate::ui::settings::schema_page(self.settings.page)
            .unwrap_or(SchemaPage::General)
    }

    /// Navigate the settings tab from the rail's search: the query selects the
    /// first page whose label or schema rows match, and the search stays up.
    pub fn settings_search_navigate(&mut self) {
        let query = self.settings.search.trim().to_lowercase();
        if query.is_empty() {
            return;
        }
        let Some(page) = crate::ui::SettingsCategory::ALL
            .iter()
            .copied()
            .find(|page| crate::ui::settings::page_matches(*page, &query))
        else {
            return;
        };
        self.settings_set_page(page);
        self.settings.search_focused = false;
    }
}

impl SettingsState {
    /// The home directory `~/.ssh` is read under, when there is one. Tests point
    /// the settings at a fixture by setting this; the running app uses the real
    /// home.
    pub fn ssh_home(&self) -> Option<std::path::PathBuf> {
        self.ssh_home_override
            .clone()
            .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
    }
}

/// The actions a page needs from the app's own update checker: surfaced here so
/// the settings dispatch can call the existing flow rather than reimplement it.
pub fn update_actions(state: &mut UiState, action: &SettingsAction) -> bool {
    match action {
        SettingsAction::CheckUpdates => {
            state.check_for_updates();
            true
        }
        SettingsAction::InstallUpdate => {
            state.install_available_update();
            true
        }
        _ => false,
    }
}

/// Keep the theme preset import referenced: the Appearance page writes the key
/// this enum's `key()` owns.
#[allow(dead_code)]
fn _preset(_: ThemePreset) {}
