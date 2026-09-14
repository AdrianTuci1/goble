//! Environment secret groups: the named groups of secrets the Settings →
//! Environment pane builds, for a remote session to start with.
//!
//! The groups are persisted in the local store (plaintext, like `credentials`)
//! and handed to whoever asks for them. **No server side exists yet:** nothing
//! in this tree runs a remote session — [`DesktopState::run_chat_turn`] runs
//! the embedded daemon, and the app's remote route (`DaemonModel::run_turn`
//! with `WorkspaceRouting::Remote`) needs a `DaemonClient` that nothing ever
//! attaches — so these groups are read by the settings pane and stop there.
//! The accessor below is where a remote session would read them.

use chrono::Utc;
use goble_core::store::{SecretEntry, SecretGroup};

use super::DesktopState;

impl DesktopState {
    /// Every environment group with its secrets, ordered by group name.
    ///
    /// This is the read path a remote session would use. Nothing transmits the
    /// groups today: there is no remote transport in the tree, so a value read
    /// here has not left the machine.
    pub fn environment_groups(&self) -> anyhow::Result<Vec<SecretGroup>> {
        self.store.lock().list_secret_groups()
    }

    /// One group with its secrets, or `None` when no such group exists.
    pub fn environment_group(&self, id: &str) -> anyhow::Result<Option<SecretGroup>> {
        self.store.lock().get_secret_group(id)
    }

    /// Create an empty group under `name` and return it. The id and timestamps
    /// are minted here, the way the other service-level creators do.
    pub fn create_environment_group(&self, name: &str) -> anyhow::Result<SecretGroup> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "a group needs a name");
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.store.lock().create_secret_group(&id, name, &now)?;
        Ok(SecretGroup {
            id,
            name: name.to_string(),
            created_at: now.clone(),
            updated_at: now,
            entries: Vec::new(),
        })
    }

    /// Delete a group and every secret in it; returns whether a group was there.
    pub fn delete_environment_group(&self, id: &str) -> anyhow::Result<bool> {
        self.store.lock().delete_secret_group(id)
    }

    /// Add a secret to a group, or edit the one already carrying that name.
    /// When `renamed_from` names the entry's previous name it is removed first,
    /// so an edit that renames leaves no duplicate behind.
    pub fn save_environment_secret(
        &self,
        group_id: &str,
        renamed_from: Option<&str>,
        name: &str,
        value: &str,
    ) -> anyhow::Result<SecretEntry> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "a secret needs a name");
        let store = self.store.lock();
        anyhow::ensure!(
            store.get_secret_group(group_id)?.is_some(),
            "group {group_id} does not exist"
        );
        if let Some(previous) = renamed_from {
            if previous != name {
                store.delete_secret_entry_named(group_id, previous)?;
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        store.upsert_secret_entry(&id, group_id, name, value, &now)?;
        store
            .get_secret_group(group_id)?
            .and_then(|g| g.entries.into_iter().find(|e| e.name == name))
            .ok_or_else(|| anyhow::anyhow!("the secret was not stored"))
    }

    /// Remove one secret by id; returns whether a row was removed.
    pub fn delete_environment_secret(&self, id: &str) -> anyhow::Result<bool> {
        self.store.lock().delete_secret_entry(id)
    }
}
