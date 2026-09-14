use chrono::Utc;
use goble_core::harness::WebSearchConfig;

use super::{DesktopState, LogEntry};

impl DesktopState {
    pub fn add_log(&self, message: impl Into<String>) {
        let entry = LogEntry {
            id: format!("{}", uuid::Uuid::new_v4()),
            timestamp: Utc::now().to_rfc3339(),
            message: message.into(),
        };
        self.logs.lock().push(entry);
        self.emit("logs:updated", ());
    }

    pub fn get_logs(&self) -> Vec<LogEntry> {
        self.logs.lock().clone()
    }

    /// Whether the agent auto-approves `ask_user` questions instead of
    /// suspending on them. Persisted under a dedicated settings key.
    pub fn get_auto_approve(&self) -> bool {
        self.store
            .lock()
            .get_setting("auto_approve")
            .ok()
            .flatten()
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    pub fn set_auto_approve(&self, enabled: bool) -> anyhow::Result<()> {
        self.store
            .lock()
            .set_setting("auto_approve", if enabled { "1" } else { "0" })?;
        Ok(())
    }

    /// Whether modal (vim) editing is on for the rich input. Off unless the
    /// user turned it on; the setting persists across restarts.
    pub fn get_vim_mode(&self) -> bool {
        self.store
            .lock()
            .get_setting("vim_mode")
            .ok()
            .flatten()
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    pub fn set_vim_mode(&self, enabled: bool) -> anyhow::Result<()> {
        self.store
            .lock()
            .set_setting("vim_mode", if enabled { "1" } else { "0" })?;
        Ok(())
    }

    /// The persisted native-UI pane layout (spaces + active space/pane) as a
    /// JSON blob, if a previous run saved one. `None` when never saved or on a
    /// fresh install.
    pub fn get_ui_panes(&self) -> Option<String> {
        self.store.lock().get_setting("ui.panes").ok().flatten()
    }

    /// Persist the native-UI pane layout as a JSON blob, replacing any prior one.
    pub fn set_ui_panes(&self, json: &str) -> anyhow::Result<()> {
        self.store.lock().set_setting("ui.panes", json)?;
        Ok(())
    }

    /// The user-added environment mediums (id, label) a previous run persisted,
    /// as a JSON blob. `None` when the user has never added a custom medium.
    pub fn get_ui_mediums(&self) -> Option<String> {
        self.store.lock().get_setting("ui.mediums").ok().flatten()
    }

    /// Persist the user-added environment mediums as a JSON blob, replacing any
    /// prior one. The built-in Local/Remote mediums are not persisted here; only
    /// the custom mediums a user added via the topbar "+" menu are.
    pub fn set_ui_mediums(&self, json: &str) -> anyhow::Result<()> {
        self.store.lock().set_setting("ui.mediums", json)?;
        Ok(())
    }

    /// Whether the user has completed (or dismissed) the first-run onboarding
    /// flow. A returning run skips the onboarding overlays and the
    /// getting-started tip when this is set.
    pub fn onboarding_done(&self) -> bool {
        self.store
            .lock()
            .get_setting("onboarding.done")
            .ok()
            .flatten()
            .as_deref()
            == Some("1")
    }

    /// Mark the first-run onboarding complete so a returning run skips it.
    pub fn set_onboarding_done(&self) -> anyhow::Result<()> {
        self.store.lock().set_setting("onboarding.done", "1")?;
        Ok(())
    }

    /// Web-search backend (hosted xAI-style endpoint + API key + optional model),
    /// persisted alongside the LLM/model settings. When neither key nor URL is
    /// set, the harness falls back to DuckDuckGo for the `web_search` tool.
    pub fn get_web_search_setting(&self) -> WebSearchConfig {
        let store = self.store.lock();
        let api_key = store
            .get_setting("web_search_api_key")
            .ok()
            .flatten()
            .unwrap_or_default();
        let base_url = store
            .get_setting("web_search_base_url")
            .ok()
            .flatten()
            .unwrap_or_default();
        let model = store
            .get_setting("web_search_model")
            .ok()
            .flatten()
            .unwrap_or_default();
        WebSearchConfig {
            api_key,
            base_url,
            model,
        }
    }

    pub fn set_web_search_setting(
        &self,
        api_key: &str,
        base_url: &str,
        model: &str,
    ) -> anyhow::Result<()> {
        let store = self.store.lock();
        store.set_setting("web_search_api_key", api_key)?;
        store.set_setting("web_search_base_url", base_url)?;
        store.set_setting("web_search_model", model)?;
        Ok(())
    }
}
