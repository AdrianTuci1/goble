//! AI domain state: vault + MCP connectors.
//!
//! Mirrors the plain [`goble_ui_hot::AiSnapshot`] but lives in the
//! executable so it survives hot library swaps and is owned by the app.

use goble_desktop_service::DesktopState;
use goble_ui_hot::{McpSearchEntry, McpServerEntry, VaultSecretEntry};

#[derive(Clone)]
pub struct AiState {
    pub connectors_open: bool,
    pub vault_open: bool,
    pub vault_unlocked: bool,
    pub vault_secrets: Vec<VaultSecretEntry>,
    pub vault_unlock_draft: String,
    pub vault_new_key: String,
    pub vault_new_value: String,
    pub vault_error: Option<String>,
    pub connector_search: String,
    pub connectors: Vec<McpServerEntry>,
    pub install_open: bool,
    pub install_editing_id: Option<String>,
    pub install_name: String,
    pub install_source: String,
    pub install_source_value: String,
    pub install_search_query: String,
    pub install_search_results: Vec<McpSearchEntry>,
    pub install_selected_secrets: Vec<String>,
    pub install_error: Option<String>,
    pub installing: bool,
}

impl AiState {
    pub fn from_desktop(desktop: &DesktopState) -> Self {
        let mut state = Self {
            connectors_open: false,
            vault_open: false,
            vault_unlocked: false,
            vault_secrets: Vec::new(),
            vault_unlock_draft: String::new(),
            vault_new_key: String::new(),
            vault_new_value: String::new(),
            vault_error: None,
            connector_search: String::new(),
            connectors: Vec::new(),
            install_open: false,
            install_editing_id: None,
            install_name: String::new(),
            install_source: "npm".to_string(),
            install_source_value: String::new(),
            install_search_query: String::new(),
            install_search_results: Vec::new(),
            install_selected_secrets: Vec::new(),
            install_error: None,
            installing: false,
        };
        state.refresh_vault(desktop);
        state.refresh_connectors(desktop);
        state
    }

    /// Reload vault status + secret keys from the backend.
    pub fn refresh_vault(&mut self, desktop: &DesktopState) {
        self.vault_unlocked = desktop.is_vault_unlocked();
        self.vault_secrets = desktop
            .list_vault_secrets()
            .into_iter()
            .map(|s| VaultSecretEntry {
                key: s.key,
                updated_at: s.updated_at,
            })
            .collect();
    }

    /// Reload installed MCP servers from the backend.
    pub fn refresh_connectors(&mut self, desktop: &DesktopState) {
        match desktop.list_mcp_servers() {
            Ok(servers) => {
                self.connectors = servers
                    .into_iter()
                    .map(|s| McpServerEntry {
                        id: s.id,
                        name: s.name,
                        source: s.source,
                        source_value: s.source_value,
                        capabilities: s.capabilities,
                        auth_required: s.auth_required,
                        discovered_tools: s.discovered_tools,
                        secret_ids: s.secret_ids,
                        enabled_tools: s.enabled_tools,
                    })
                    .collect();
            }
            Err(e) => log::warn!("list_mcp_servers failed: {e}"),
        }
    }

    /// Refresh registry search results. Requires a tokio runtime entered on
    /// the calling thread (main.rs keeps one alive for the app lifetime).
    pub fn refresh_search(&mut self, desktop: &DesktopState) {
        self.install_search_results = desktop
            .search_mcp_servers(&self.install_search_query)
            .into_iter()
            .map(|r| McpSearchEntry {
                id: r.id,
                name: r.name,
                description: r.description,
                capabilities: r.capabilities,
                auth_required: r.auth_required,
                source_kind: r.source_kind,
            })
            .collect();
    }

    /// Mock data used when the backend store cannot be opened (dev fallback).
    pub fn mock() -> Self {
        Self {
            connectors_open: false,
            vault_open: false,
            vault_unlocked: false,
            vault_secrets: vec![
                VaultSecretEntry {
                    key: "openai_api_key".to_string(),
                    updated_at: "now".to_string(),
                },
                VaultSecretEntry {
                    key: "anthropic_api_key".to_string(),
                    updated_at: "now".to_string(),
                },
            ],
            vault_unlock_draft: String::new(),
            vault_new_key: String::new(),
            vault_new_value: String::new(),
            vault_error: None,
            connector_search: String::new(),
            connectors: vec![
                McpServerEntry {
                    id: "mcp-postgres".to_string(),
                    name: "PostgreSQL".to_string(),
                    source: "npm".to_string(),
                    source_value: Some("@modelcontextprotocol/server-postgres".to_string()),
                    capabilities: vec!["query".to_string(), "schema".to_string()],
                    auth_required: true,
                    discovered_tools: vec!["query".to_string(), "list_tables".to_string()],
                    secret_ids: vec!["openai_api_key".to_string()],
                    enabled_tools: vec!["query".to_string()],
                },
                McpServerEntry {
                    id: "mcp-filesystem".to_string(),
                    name: "Filesystem".to_string(),
                    source: "npm".to_string(),
                    source_value: Some("@modelcontextprotocol/server-filesystem".to_string()),
                    capabilities: vec!["read".to_string(), "write".to_string()],
                    auth_required: false,
                    discovered_tools: vec!["read_file".to_string(), "write_file".to_string()],
                    secret_ids: Vec::new(),
                    enabled_tools: Vec::new(),
                },
            ],
            install_open: false,
            install_editing_id: None,
            install_name: String::new(),
            install_source: "npm".to_string(),
            install_source_value: String::new(),
            install_search_query: String::new(),
            install_search_results: Vec::new(),
            install_selected_secrets: Vec::new(),
            install_error: None,
            installing: false,
        }
    }
}
