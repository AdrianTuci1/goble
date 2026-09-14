//! Environment secret groups: the named groups of secrets a Settings →
//! Environment pane builds, which a remote session is meant to start with.
//!
//! Two tables (`secret_groups`, `secret_group_entries`) rather than a `credentials`
//! row per secret under a prefix, because a group is a thing with a name, an
//! order and a lifecycle of its own. Values are stored the way the `credentials`
//! table stores them — plaintext — so nothing here is encrypted; the vault
//! (`vault_secrets`, `CredentialVault`) is the encrypted store and is untouched.

use anyhow::Result;
use rusqlite::params;

use super::Store;

/// One named group of secrets, with its entries in insertion order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretGroup {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub entries: Vec<SecretEntry>,
}

/// One name/value pair inside a [`SecretGroup`]. The value is plaintext.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretEntry {
    pub id: String,
    pub group_id: String,
    pub name: String,
    pub value: String,
    pub updated_at: String,
}

impl Store {
    /// Create a group. The caller supplies the id and the timestamp, the way
    /// the other insert methods in this store do.
    pub fn create_secret_group(&self, id: &str, name: &str, now: &str) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO secret_groups (id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            params![id, name, now],
        )?;
        Ok(())
    }

    /// Every group with its entries, ordered by name. A group with no entries
    /// comes back with an empty `entries`, not absent.
    pub fn list_secret_groups(&self) -> Result<Vec<SecretGroup>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, created_at, updated_at FROM secret_groups ORDER BY name",
        )?;
        let mut rows = stmt.query([])?;
        let mut groups = Vec::new();
        while let Some(row) = rows.next()? {
            groups.push(SecretGroup {
                id: row.get(0)?,
                name: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                entries: Vec::new(),
            });
        }
        let mut entries = conn.prepare(
            "SELECT id, group_id, name, value, updated_at FROM secret_group_entries
             ORDER BY name",
        )?;
        let mut rows = entries.query([])?;
        while let Some(row) = rows.next()? {
            let entry = SecretEntry {
                id: row.get(0)?,
                group_id: row.get(1)?,
                name: row.get(2)?,
                value: row.get(3)?,
                updated_at: row.get(4)?,
            };
            if let Some(group) = groups.iter_mut().find(|g| g.id == entry.group_id) {
                group.entries.push(entry);
            }
        }
        Ok(groups)
    }

    /// One group with its entries, or `None` when no such group exists.
    pub fn get_secret_group(&self, id: &str) -> Result<Option<SecretGroup>> {
        Ok(self.list_secret_groups()?.into_iter().find(|g| g.id == id))
    }

    /// Delete a group and every entry in it; returns whether a group was there.
    /// Entries are removed explicitly rather than by cascade, so the two tables
    /// never disagree about which group an entry belongs to.
    pub fn delete_secret_group(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM secret_group_entries WHERE group_id = ?1", params![id])?;
        let removed = conn.execute("DELETE FROM secret_groups WHERE id = ?1", params![id])?;
        Ok(removed > 0)
    }

    /// Insert a secret into a group, or update the value of the entry that
    /// already carries its name — the (group, name) pair is unique, so adding a
    /// name twice edits the entry rather than duplicating it.
    pub fn upsert_secret_entry(
        &self,
        id: &str,
        group_id: &str,
        name: &str,
        value: &str,
        now: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO secret_group_entries (id, group_id, name, value, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(group_id, name) DO UPDATE SET value = excluded.value,
                                                       updated_at = excluded.updated_at",
            params![id, group_id, name, value, now],
        )?;
        Ok(())
    }

    /// Remove one entry by id; returns whether a row was removed.
    pub fn delete_secret_entry(&self, id: &str) -> Result<bool> {
        let removed = self
            .conn
            .lock()
            .execute("DELETE FROM secret_group_entries WHERE id = ?1", params![id])?;
        Ok(removed > 0)
    }

    /// Remove one entry by the name it carries inside `group_id`; returns
    /// whether a row was removed. Used when an edit renames an entry.
    pub fn delete_secret_entry_named(&self, group_id: &str, name: &str) -> Result<bool> {
        let removed = self.conn.lock().execute(
            "DELETE FROM secret_group_entries WHERE group_id = ?1 AND name = ?2",
            params![group_id, name],
        )?;
        Ok(removed > 0)
    }
}
