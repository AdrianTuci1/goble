use super::*;

impl UiState {
    pub fn refresh_crons(&mut self, desktop: &DesktopState) {
        self.crons = desktop
            .list_workflows()
            .into_iter()
            .map(|wf| {
                let schedule = match wf.trigger {
                    Trigger::Cron { expression } => expression,
                    _ => "manual".to_string(),
                };
                CronEntry::new(wf.id, wf.name, schedule, "unknown").with_enabled(wf.enabled)
            })
            .collect();
    }

    pub fn refresh_agent_name(&mut self, desktop: &DesktopState) {
        if let Some(name) = desktop.list_agents().first().map(|a| a.name.clone()) {
            self.agent_name = name;
        }
    }

    /// Reload settings data (workers, cluster, vault, LLM) from the backend.
    pub fn refresh_settings(&mut self, desktop: &DesktopState) {
        // Theme is persisted in `~/.goble/config.toml`.
        let theme = desktop.config().theme;
        self.settings_dark_mode = theme.dark;
        self.theme_primary = theme.primary;
        self.theme_secondary = theme.secondary;
        self.theme_accent = Some(theme.accent);
        self.settings_workers = desktop
            .list_workers()
            .into_iter()
            .map(|w| (w.id.clone(), w.name.clone(), w.url.clone(), w.paired))
            .collect();
        if let Some(identity) = desktop.get_cluster_identity() {
            self.settings_cluster_name = identity.cluster_name.clone();
            self.settings_cluster_configured = true;
        } else {
            self.settings_cluster_configured = false;
        }
        self.settings_vault_unlocked = desktop.is_vault_unlocked();
        self.vim_mode = desktop.get_vim_mode();
        if let Some(s) = desktop.get_llm_setting("openai") {
            self.settings_llm_provider = "openai".to_string();
            self.settings_llm_model = s.model;
            self.settings_llm_api_key = s.api_key;
            self.settings_llm_base_url = s.base_url.unwrap_or_default();
            if let Some(t) = s.temperature {
                self.settings_llm_temperature = t.to_string();
            }
        }
        self.refresh_llm_models(desktop);
    }

    /// Populate the composer's model dropdown from the configured provider and
    /// default the selected model. The selection is only set on first load, so
    /// a user's per-session model choice survives subsequent refreshes.
    pub fn refresh_llm_models(&mut self, desktop: &DesktopState) {
        self.models = desktop.available_models(&self.settings_llm_provider);
        if self.selected_model.trim().is_empty() {
            self.selected_model = desktop.default_model(&self.settings_llm_provider);
        }
    }

    /// Copy the current LLM settings into the dialog's editable fields, so the
    /// model-provider dialog opens pre-filled with what's configured (empty on
    /// first run) and with no field focused.
    pub fn prime_llm_form(&self) {
        *self.llm_dialog_provider.borrow_mut() = self.settings_llm_provider.clone();
        *self.llm_dialog_model.borrow_mut() = self.settings_llm_model.clone();
        *self.llm_dialog_api_key.borrow_mut() = self.settings_llm_api_key.clone();
        *self.llm_dialog_base_url.borrow_mut() = self.settings_llm_base_url.clone();
        *self.llm_dialog_temperature.borrow_mut() = self.settings_llm_temperature.clone();
        *self.llm_dialog_focus.borrow_mut() = None;
    }
}
