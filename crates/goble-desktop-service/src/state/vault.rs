use chrono::Utc;
use goble_core::vault::CredentialVault;

use super::{DesktopState, VaultSecretInfo};

impl DesktopState {
    pub fn set_vault_passphrase(&self, passphrase: String) {
        *self.vault_passphrase.lock() = passphrase.into_bytes();
    }

    pub fn is_vault_unlocked(&self) -> bool {
        !self.vault_passphrase.lock().is_empty()
    }

    pub fn set_vault_secret(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let passphrase = self.vault_passphrase.lock().clone();
        if passphrase.is_empty() {
            anyhow::bail!("vault passphrase not set");
        }
        let value_bytes = value.as_bytes();
        self.vault.lock().set(key, value_bytes, &passphrase)?;
        let encrypted = self.vault.lock().to_bytes()?;
        // Persist encrypted vault as a single JSON blob under vault_blob setting
        self.store
            .lock()
            .set_setting("vault_blob", &String::from_utf8_lossy(&encrypted))?;
        self.emit("vault:updated", ());
        Ok(())
    }

    /// Remove a secret from the vault and persist the updated blob.
    pub fn delete_vault_secret(&self, key: &str) -> anyhow::Result<()> {
        let passphrase = self.vault_passphrase.lock().clone();
        if passphrase.is_empty() {
            anyhow::bail!("vault passphrase not set");
        }
        self.vault.lock().remove(key)?;
        let encrypted = self.vault.lock().to_bytes()?;
        self.store
            .lock()
            .set_setting("vault_blob", &String::from_utf8_lossy(&encrypted))?;
        self.emit("vault:updated", ());
        Ok(())
    }

    pub fn unlock_vault(&self, passphrase: String) -> anyhow::Result<Vec<String>> {
        let bytes = self
            .store
            .lock()
            .get_setting("vault_blob")?
            .unwrap_or_default();
        if !bytes.is_empty() {
            let vault = CredentialVault::from_bytes(bytes.as_bytes())?;
            // Verify by trying to get a random key? Actually we can just set and unlock.
            let keys = vault.keys();
            self.vault_passphrase.lock().clear();
            self.vault_passphrase.lock().extend(passphrase.as_bytes());
            // Try to decrypt all entries to verify passphrase
            for key in &keys {
                if self.vault.lock().get(key, passphrase.as_bytes()).is_err() {
                    anyhow::bail!("wrong passphrase");
                }
            }
            *self.vault.lock() = vault;
        } else {
            self.vault_passphrase.lock().clear();
            self.vault_passphrase.lock().extend(passphrase.as_bytes());
        }
        self.emit("vault:updated", ());
        Ok(self.vault.lock().keys())
    }

    pub fn list_vault_secrets(&self) -> Vec<VaultSecretInfo> {
        self.vault
            .lock()
            .keys()
            .into_iter()
            .map(|key| VaultSecretInfo {
                key,
                updated_at: Utc::now().to_rfc3339(),
            })
            .collect()
    }

    /// Store a named credential. The value is persisted in plaintext, matching
    /// how LLM keys are stored; only the name is exposed to the agent.
    pub fn set_credential(&self, name: &str, value: &str) -> anyhow::Result<()> {
        self.store.lock().set_credential(name, value)
    }

    pub fn get_credential(&self, name: &str) -> anyhow::Result<Option<String>> {
        self.store.lock().get_credential(name)
    }

    pub fn list_credential_names(&self) -> anyhow::Result<Vec<String>> {
        self.store.lock().list_credential_names()
    }

    /// Record that a principal may perform `grant` over `scope`.
    pub fn grant_access(&self, principal_id: &str, grant: &str, scope: &str) -> anyhow::Result<()> {
        self.store.lock().grant_access(principal_id, grant, scope)
    }

    /// The grants a principal holds as `(grant, scope, created_at)`.
    pub fn list_principal_access(
        &self,
        principal_id: &str,
    ) -> anyhow::Result<Vec<(String, String, String)>> {
        self.store.lock().list_access(principal_id)
    }

    /// Remove a matching grant; returns whether a row was removed.
    pub fn revoke_access(&self, principal_id: &str, grant: &str, scope: &str) -> anyhow::Result<bool> {
        self.store.lock().revoke_access(principal_id, grant, scope)
    }

    /// Ensure `~/.goble/principals/<id>/` exists so a principal's credentials and
    /// context have a home alongside the workspace state.
    pub fn ensure_principal_dir(&self, principal_id: &str) -> anyhow::Result<()> {
        let home = goble_core::app_home::GobleHome::locate()?;
        let dir = home.principals_dir().join(principal_id);
        std::fs::create_dir_all(&dir)?;
        Ok(())
    }
}
