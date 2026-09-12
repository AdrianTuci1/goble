use std::cell::RefCell;
use std::rc::Rc;

use super::{SettingsPage, SettingsView};

impl SettingsView {
    pub fn with_profile(mut self, name: impl Into<String>, email: impl Into<String>) -> Self {
        self.profile_name = name.into();
        self.profile_email = email.into();
        self
    }

    pub fn with_llm(
        mut self,
        provider: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        temperature: impl Into<String>,
    ) -> Self {
        self.llm_provider = provider.into();
        self.llm_model = model.into();
        self.llm_api_key = api_key.into();
        self.llm_base_url = base_url.into();
        self.llm_temperature = temperature.into();
        self
    }

    pub fn with_dark_mode(mut self, enabled: bool) -> Self {
        self.dark_mode = enabled;
        self
    }

    pub fn with_vault_state(mut self, unlocked: bool, secrets: Vec<String>) -> Self {
        self.vault_unlocked = unlocked;
        self.vault_secrets = secrets;
        self
    }

    pub fn with_cluster_state(mut self, name: impl Into<String>, configured: bool) -> Self {
        self.cluster_name = name.into();
        self.cluster_configured = configured;
        self
    }

    pub fn with_workers(mut self, workers: Vec<(String, String, String, bool)>) -> Self {
        self.workers = workers;
        self
    }

    pub fn with_authorized_keys(mut self, keys: Vec<(String, String, String)>) -> Self {
        self.authorized_keys = keys;
        self
    }

    pub fn with_on_navigate<F: FnMut(SettingsPage) + 'static>(mut self, callback: F) -> Self {
        self.on_navigate = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_save_profile<F: FnMut(String, String) + 'static>(mut self, callback: F) -> Self {
        self.on_save_profile = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_save_llm<F: FnMut(String, String, String, String, String) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_save_llm = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_toggle_dark_mode<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_dark_mode = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_unlock_vault<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_unlock_vault = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_add_vault_secret<F: FnMut(String, String) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_add_vault_secret = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_create_cluster<F: FnMut(String, String) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_create_cluster = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_unlock_cluster<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_unlock_cluster = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_add_worker<F: FnMut(String, String) + 'static>(mut self, callback: F) -> Self {
        self.on_add_worker = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_pair_worker<F: FnMut(String, String) + 'static>(mut self, callback: F) -> Self {
        self.on_pair_worker = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_remove_worker<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_remove_worker = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_add_authorized_key<F: FnMut(String, String, String) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_add_authorized_key = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_remove_authorized_key<F: FnMut(String) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_remove_authorized_key = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set a callback fired by the top-left Back button (returns to the previous
    /// view). Without a callback the button is not rendered.
    pub fn with_on_back<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_back = Some(Rc::new(RefCell::new(callback)));
        self
    }
}
