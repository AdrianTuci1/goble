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
        self.refresh_environment_groups(Some(desktop));
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

    // ---- The settings tab's two-region keyboard model ----------------------
    //
    // The rail and the content pane are the two regions; `settings_focus` says
    // which holds the keyboard, `settings_pane_focus` which pane control it is
    // on. `app/src/ui/settings.rs` documents the key map and draws the ring.
    // The page the pane shows is not a field of its own: it rides in the
    // settings leaf (see [`UiState::settings_page`]).

    /// The pane's focus order for the page the settings pane shows, from the
    /// same rules the pane draws its rows with.
    pub fn settings_pane_controls(&self) -> Vec<SettingsControl> {
        crate::ui::settings::pane_controls(
            self.settings_page(),
            &self.settings_environment_groups,
            self.settings_environment_open_group.as_deref(),
            self.settings_ssh_hosts.as_ref(),
        )
    }

    /// The pane control the keyboard is on, when the pane holds the keyboard.
    pub fn settings_focused_control(&self) -> Option<SettingsControl> {
        if self.settings_focus != SettingsFocus::Pane {
            return None;
        }
        self.settings_pane_controls()
            .get(self.settings_pane_focus)
            .cloned()
    }

    /// Select a category from the rail and reset the pane's focus to that
    /// pane's first control: the keys that got here belong to the rail. The
    /// page is written into the settings leaf, so it travels with the pane.
    pub fn settings_select_category(&mut self, category: SettingsCategory) {
        self.settings_set_page(category);
        self.settings_pane_focus = 0;
        self.settings_clear_field_edit();
    }

    /// Step the active category by `delta`, wrapping, and reset the pane's
    /// focus to that pane's first control.
    pub fn settings_category_step(&mut self, delta: i32) {
        let all = SettingsCategory::ALL;
        let count = all.len() as i32;
        if count == 0 {
            return;
        }
        let pos = all.iter().position(|c| *c == self.settings_page()).unwrap_or(0) as i32;
        self.settings_select_category(all[(pos + delta).rem_euclid(count) as usize]);
    }

    /// Step the keyboard's slot inside the region that holds it.
    pub fn settings_focus_step(&mut self, delta: i32) {
        match self.settings_focus {
            SettingsFocus::Rail => self.settings_category_step(delta),
            SettingsFocus::Pane => self.settings_pane_step(delta),
        }
    }

    /// Move the pane's focused control by `delta`, clamped at the ends. A caret
    /// in a field is let go: the keys are navigating again.
    pub fn settings_pane_step(&mut self, delta: i32) {
        let len = self.settings_pane_controls().len();
        if len == 0 {
            return;
        }
        self.settings_pane_focus =
            (self.settings_pane_focus as i32 + delta).clamp(0, len as i32 - 1) as usize;
        self.settings_clear_field_edit();
    }

    /// Move the keyboard into the pane, on its first control. A pane with
    /// nothing interactive (Advanced) does not take it.
    pub fn settings_focus_into_pane(&mut self) {
        if self.settings_pane_controls().is_empty() {
            return;
        }
        self.settings_focus = SettingsFocus::Pane;
        self.settings_pane_focus = 0;
        self.settings_clear_field_edit();
    }

    /// Move the keyboard back to the rail.
    pub fn settings_focus_out_of_pane(&mut self) {
        self.settings_focus = SettingsFocus::Rail;
        self.settings_clear_field_edit();
    }

    /// Leave the caret wherever it is, forgetting the edit it belonged to. What
    /// the user typed stays in the field — this is the path for a key that
    /// navigates away, not for `Escape`, which puts the field back.
    fn settings_clear_field_edit(&mut self) {
        self.settings_pane_field_active = false;
        self.settings_field_edit_start = None;
    }

    /// The text field a control names, if it names one.
    fn settings_field_mut(&mut self, control: &SettingsControl) -> Option<&mut String> {
        match control {
            SettingsControl::EnvironmentGroupName => Some(&mut self.settings_environment_group_draft),
            SettingsControl::EnvironmentSecretName => {
                Some(&mut self.settings_environment_secret_name)
            }
            SettingsControl::EnvironmentSecretValue => {
                Some(&mut self.settings_environment_secret_value)
            }
            _ => None,
        }
    }

    /// `Enter` on a text field: the first press puts the caret in it and
    /// remembers what it held, the second commits what was typed.
    pub fn settings_activate_field(&mut self) {
        let Some(control) = self.settings_focused_control() else {
            return;
        };
        if !control.is_text_field() {
            return;
        }
        if self.settings_pane_field_active {
            self.settings_commit_field();
            return;
        }
        self.settings_field_edit_start = self
            .settings_field_mut(&control)
            .map(|value| value.clone());
        self.settings_pane_field_active = true;
    }

    /// Commit the field's text: it stays, the caret goes.
    pub fn settings_commit_field(&mut self) {
        self.settings_clear_field_edit();
    }

    /// `Escape` in a field: put back what it held when the caret went in, so a
    /// cancelled edit leaves nothing behind, and keep the sheet open.
    pub fn settings_cancel_field(&mut self) {
        if let Some(start) = self.settings_field_edit_start.take() {
            if let Some(control) = self.settings_focused_control() {
                if let Some(value) = self.settings_field_mut(&control) {
                    *value = start;
                }
            }
        }
        self.settings_pane_field_active = false;
    }

    /// Activate the focused pane control. The controls whose behaviour already
    /// exists as an action (the switches, the routing choice, the model reload)
    /// are dispatched there by the action layer; this is the half that belongs
    /// to the state: the environment rows, the SSH connection rows and a text
    /// field taking the caret.
    pub fn settings_activate_control(&mut self, desktop: Option<&DesktopState>) {
        match self.settings_focused_control() {
            Some(control) if control.is_text_field() => self.settings_activate_field(),
            Some(SettingsControl::EnvironmentCreateGroup) => {
                self.environment_create_group(desktop);
            }
            Some(SettingsControl::EnvironmentGroup(id)) => self.environment_open_group(&id),
            Some(SettingsControl::EnvironmentDeleteGroup(id)) => {
                self.environment_delete_group(&id, desktop);
            }
            Some(SettingsControl::EnvironmentSaveSecret) => {
                self.environment_save_secret(desktop);
            }
            Some(SettingsControl::EnvironmentSecret(id)) => self.environment_edit_secret(&id),
            Some(SettingsControl::EnvironmentDeleteSecret(id)) => {
                self.environment_delete_secret(&id, desktop);
            }
            // Enter on a connection names it and stops there: this build opens
            // no session over SSH, so the row is the source of a target rather
            // than a thing that connects.
            Some(SettingsControl::SshHost(alias)) => self.ssh_select_host(&alias),
            Some(SettingsControl::ReloadSshHosts) => self.reload_ssh_hosts(),
            _ => {}
        }
    }

    /// Adjust the focused pane control by `delta` when it holds a discrete
    /// value. The steppers are dispatched by the action layer, which owns the
    /// shared zoom cell; the theme-channel column belongs here.
    pub fn settings_adjust_control(&mut self, delta: i32) {
        let Some(SettingsControl::ThemeChannel(_)) = self.settings_focused_control() else {
            return;
        };
        let order = crate::ui::color_picker::COLOR_TARGET_ORDER;
        let pos = order
            .iter()
            .position(|t| *t == self.theme_color_target)
            .unwrap_or(0) as i32;
        self.theme_color_target = order[(pos + delta).rem_euclid(order.len() as i32) as usize];
    }

    // ---- Settings -> Environment -------------------------------------------

    /// Reload the grouped secrets from the store. Every change the pane makes
    /// goes through the store first and then comes back through here, so what
    /// is drawn is what is persisted.
    pub fn refresh_environment_groups(&mut self, desktop: Option<&DesktopState>) {
        let Some(desktop) = desktop else {
            return;
        };
        match desktop.environment_groups() {
            Ok(groups) => {
                // A group that vanished elsewhere closes the pane's view of it.
                if let Some(open) = &self.settings_environment_open_group {
                    if !groups.iter().any(|g| &g.id == open) {
                        self.settings_environment_open_group = None;
                        self.settings_environment_editing = None;
                    }
                }
                self.settings_environment_groups = groups;
            }
            Err(e) => log::warn!("environment_groups failed: {e}"),
        }
        // Removing the row the keyboard was on shortens the pane's order: keep
        // the focused slot inside it instead of leaving it past the end.
        let len = self.settings_pane_controls().len();
        if len > 0 {
            self.settings_pane_focus = self.settings_pane_focus.min(len - 1);
        }
    }

    /// Create the group the name field holds and open it.
    pub fn environment_create_group(&mut self, desktop: Option<&DesktopState>) {
        let name = self.settings_environment_group_draft.trim().to_string();
        if name.is_empty() {
            return;
        }
        let Some(desktop) = desktop else {
            return;
        };
        match desktop.create_environment_group(&name) {
            Ok(group) => {
                self.settings_environment_group_draft.clear();
                self.settings_environment_open_group = Some(group.id);
                self.settings_environment_editing = None;
                self.refresh_environment_groups(Some(desktop));
            }
            Err(e) => log::warn!("create_environment_group failed: {e}"),
        }
    }

    /// Open a group (its secrets join the pane), or close the open one.
    pub fn environment_open_group(&mut self, id: &str) {
        if self.settings_environment_open_group.as_deref() == Some(id) {
            self.settings_environment_open_group = None;
        } else {
            self.settings_environment_open_group = Some(id.to_string());
        }
        self.settings_environment_editing = None;
        self.settings_environment_secret_name.clear();
        self.settings_environment_secret_value.clear();
    }

    /// Delete a group and everything in it.
    pub fn environment_delete_group(&mut self, id: &str, desktop: Option<&DesktopState>) {
        let Some(desktop) = desktop else {
            return;
        };
        match desktop.delete_environment_group(id) {
            Ok(_) => {
                if self.settings_environment_open_group.as_deref() == Some(id) {
                    self.settings_environment_open_group = None;
                }
                self.settings_environment_editing = None;
                self.refresh_environment_groups(Some(desktop));
            }
            Err(e) => log::warn!("delete_environment_group failed: {e}"),
        }
    }

    /// Load one entry back into the fields, so the next save edits it.
    pub fn environment_edit_secret(&mut self, id: &str) {
        let Some(entry) = self
            .settings_environment_open_group
            .as_deref()
            .and_then(|group| {
                self.settings_environment_groups
                    .iter()
                    .find(|g| g.id == group)
            })
            .and_then(|group| group.entries.iter().find(|e| e.id == id))
        else {
            return;
        };
        self.settings_environment_secret_name = entry.name.clone();
        self.settings_environment_secret_value = entry.value.clone();
        self.settings_environment_editing = Some(entry.name.clone());
    }

    /// Remove one secret from the open group.
    pub fn environment_delete_secret(&mut self, id: &str, desktop: Option<&DesktopState>) {
        let Some(desktop) = desktop else {
            return;
        };
        match desktop.delete_environment_secret(id) {
            Ok(_) => {
                self.settings_environment_editing = None;
                self.refresh_environment_groups(Some(desktop));
            }
            Err(e) => log::warn!("delete_environment_secret failed: {e}"),
        }
    }

    /// Store the secret the two fields hold into the open group. An entry the
    /// fields were loaded from is edited in place, and renamed too when its
    /// name changed; otherwise this adds a new one.
    pub fn environment_save_secret(&mut self, desktop: Option<&DesktopState>) {
        let Some(group_id) = self.settings_environment_open_group.clone() else {
            return;
        };
        let name = self.settings_environment_secret_name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let Some(desktop) = desktop else {
            return;
        };
        let renamed_from = self.settings_environment_editing.clone();
        match desktop.save_environment_secret(
            &group_id,
            renamed_from.as_deref(),
            &name,
            &self.settings_environment_secret_value,
        ) {
            Ok(_) => {
                self.settings_environment_secret_name.clear();
                self.settings_environment_secret_value.clear();
                self.settings_environment_editing = None;
                self.refresh_environment_groups(Some(desktop));
            }
            Err(e) => log::warn!("save_environment_secret failed: {e}"),
        }
    }

    // ---- Settings -> Connections -------------------------------------------

    /// Read `~/.ssh` if the Connections page is showing and the cache is still
    /// empty. Called once per frame from the rebuild, before the snapshot is
    /// taken, so the page never reads the directory while it draws and a frame
    /// that changes nothing reads nothing.
    pub fn ensure_ssh_hosts(&mut self) {
        if self.settings_page() != SettingsCategory::Connections || self.settings_ssh_read {
            return;
        }
        self.reload_ssh_hosts();
    }

    /// Re-read `~/.ssh` and replace the cache. The read is a plain function of
    /// the home directory, so the page and the action layer share this one path.
    pub fn reload_ssh_hosts(&mut self) {
        self.settings_ssh_read = true;
        self.settings_ssh_hosts = self
            .ssh_home()
            .map(|home| goble_core::ssh_hosts::read_ssh_hosts(&home));
        // A host that is no longer in the config cannot stay selected: the row
        // the keyboard would be on is gone.
        let selected = self.settings_ssh_selected.take();
        if let Some(alias) = selected {
            if self
                .settings_ssh_hosts
                .as_ref()
                .is_some_and(|ssh| ssh.hosts.iter().any(|h| h.alias == alias))
            {
                self.settings_ssh_selected = Some(alias);
            }
        }
        // Re-reading can shorten the pane's order: keep the focused slot inside
        // it instead of leaving it past the end.
        let len = self.settings_pane_controls().len();
        if len > 0 {
            self.settings_pane_focus = self.settings_pane_focus.min(len - 1);
        }
    }

    /// Select a connection by alias: `Enter` names it for whatever consumes the
    /// target next. Nothing connects — this build opens no session over SSH.
    pub fn ssh_select_host(&mut self, alias: &str) {
        let known = self
            .settings_ssh_hosts
            .as_ref()
            .is_some_and(|ssh| ssh.hosts.iter().any(|h| h.alias == alias));
        if known {
            self.settings_ssh_selected = Some(alias.to_string());
        }
    }

    /// The home directory `~/.ssh` is read under: the injected one when there is
    /// one, else the user's own.
    fn ssh_home(&self) -> Option<std::path::PathBuf> {
        self.settings_ssh_home
            .clone()
            .or_else(|| crate::state::paths::home_directory().map(std::path::PathBuf::from))
    }
}
