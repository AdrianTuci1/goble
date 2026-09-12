//! Internal SQLite store for agents, chats, teams, execution traces, MCP
//! registry cache and settings.
//!
//! One module per surface the database carries: the schema itself
//! ([`schema`]), settings, credentials and the cluster wallet ([`settings`]),
//! device identities and cluster invites ([`device_identity`]), the audit log
//! ([`audit`]), agents and their memory ([`agents`]), workers ([`workers`]),
//! chats and their messages ([`chats`]), MCP servers, principals and grants
//! ([`mcp`]), teams ([`teams`]), vault secrets ([`vault`]), workflows and
//! executions ([`workflows`]), missions with their reasoning steps and pending
//! asks/commands ([`missions`]) and the snapshot export/import
//! ([`snapshot`]). Every surface is a block of methods on the single [`Store`]
//! handle, which is the state they all share.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::Connection;

mod agents;
mod audit;
mod chats;
mod device_identity;
mod mcp;
mod missions;
mod schema;
mod settings;
mod snapshot;
mod teams;
mod vault;
mod workers;
mod workflows;

#[cfg(test)]
mod tests;

/// Internal SQLite store for agents, chats, teams, execution traces, MCP registry cache and settings.
pub struct Store {
    conn: Arc<Mutex<Connection>>,
    path: Option<PathBuf>,
}

impl Store {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path.as_ref()).context("failed to open sqlite store")?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            path: Some(path.as_ref().to_path_buf()),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("failed to open in-memory sqlite store")?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            path: None,
        };
        store.migrate()?;
        Ok(store)
    }

    /// Path to the underlying SQLite database, if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

impl Clone for Store {
    fn clone(&self) -> Self {
        Self {
            conn: Arc::clone(&self.conn),
            path: self.path.clone(),
        }
    }
}
