//! The MCP manager: live MCP clients, their tools, and the installed-server
//! rows behind them.
//!
//! One module per surface of managing a server: listing what is installed
//! ([`list`]), installing, updating and deleting it ([`lifecycle`]), spawning
//! it and discovering the tools it exposes ([`discovery`]), calling those tools
//! ([`call`]) and describing a server from what the user typed ([`server`]).
//! Every surface is a block of methods on the single [`McpManager`] handle,
//! which is the state they all share: this module holds the handle, its
//! registry search and the installed-server summary.

mod call;
mod discovery;
mod lifecycle;
mod list;
mod server;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::mcp_client::{McpClient, McpSseClient};
use crate::mcp_installer::McpInstaller;
use crate::mcp_registry::{McpRegistry, McpSearchResult};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Manages live MCP stdio and SSE clients, exposes their tools to the harness,
/// and persists installed servers in the store.
#[derive(Clone, Default)]
pub struct McpManager {
    stdio_clients: Arc<Mutex<HashMap<String, Arc<McpClient>>>>,
    sse_clients: Arc<Mutex<HashMap<String, Arc<McpSseClient>>>>,
    tool_index: Arc<Mutex<HashMap<String, McpToolMapping>>>,
    #[allow(dead_code)]
    installer: Arc<Mutex<Option<McpInstaller>>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct McpToolMapping {
    server_id: String,
    tool_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerSummary {
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

impl McpManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_installer(self, installer: McpInstaller) -> Self {
        *self.installer.lock() = Some(installer);
        self
    }

    fn cache_dir(&self) -> Option<PathBuf> {
        self.installer.lock().as_ref().map(|i| i.cache_dir.clone())
    }

    /// Search both the local registry and (optionally) the web for MCP servers.
    pub async fn search_mcp_servers(&self, query: &str) -> Vec<McpSearchResult> {
        McpRegistry::builtin().search_mcp_servers(query).await
    }
}
