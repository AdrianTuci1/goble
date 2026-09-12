use crate::agent::{McpManifest, McpServer};
use crate::mcp_installer::McpInstaller;
use crate::secret::Secret;
use crate::store::Store;
use anyhow::{Context, Result};

use super::server::build_server_from_user_input;

use super::McpManager;

impl McpManager {
    /// Install or update an MCP server in the store. Optionally install the package locally.
    ///
    /// `source_value` is interpreted per source:
    /// - npm: package name (e.g. `@modelcontextprotocol/server-sequential-thinking`)
    /// - github: `owner/repo` optionally followed by `#rev`
    /// - local: directory path
    /// - url: URL string
    pub async fn install_mcp_server(
        &self,
        store: &Store,
        id: &str,
        name: &str,
        source: &str,
        source_value: Option<&str>,
        secret_ids: &[Secret],
        manifest: Option<McpManifest>,
    ) -> Result<String> {
        let server =
            build_server_from_user_input(id, name, source, source_value, vec![], manifest)?;

        // Persist in store before any network activity so the UI sees it immediately.
        let now = chrono::Utc::now().to_rfc3339();
        let manifest_json = serde_json::to_string(&server.manifest)?;
        let source_value_str = source_value.map(|s| s.to_string());
        let secret_ids_json = serde_json::to_string(
            &secret_ids
                .iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>(),
        )?;
        let enabled_tools_json = "[]";
        store.insert_mcp_server(
            &server.id,
            &server.name,
            source,
            source_value_str.as_deref(),
            &manifest_json,
            None,
            &secret_ids_json,
            enabled_tools_json,
            &now,
            &now,
        )?;

        // If a local installer is configured, download/cache the package.
        if let Some(cache_dir) = self.cache_dir() {
            let installer = McpInstaller::new(cache_dir);
            let installed = installer.install(&server).await?;
            let _ = installed;
        }

        Ok(format!("mcp server {} installed", id))
    }

    /// Update an installed MCP server. Replaces config/credentials and re-installs.
    pub async fn update_mcp_server(
        &self,
        store: &Store,
        id: &str,
        name: Option<&str>,
        source_value: Option<&str>,
        secret_ids: Option<&[Secret]>,
        manifest: Option<McpManifest>,
    ) -> Result<String> {
        let rows = store.list_mcp_servers()?;
        let row = rows
            .into_iter()
            .find(|(i, _, _, _, _, _, _, _, _, _)| i == id)
            .context(format!("mcp server {id} not found"))?;
        let source = row.2;
        let current_name = row.1;
        let current_value = row.3;
        let current_manifest_json = row.4;
        let _current_secret_ids_json = row.6;
        let current_enabled_tools_json = row.7;

        let mut server: McpServer = build_server_from_user_input(
            id,
            name.unwrap_or(&current_name),
            &source,
            source_value.or(current_value.as_deref()),
            vec![],
            manifest.or_else(|| serde_json::from_str(&current_manifest_json).ok()),
        )?;
        server.updated_at = chrono::Utc::now();

        let manifest_json = serde_json::to_string(&server.manifest)?;
        let secret_ids_json = serde_json::to_string(
            &secret_ids
                .map(|s| s.iter().map(|sec| sec.name.clone()).collect::<Vec<_>>())
                .unwrap_or_default(),
        )?;
        store.insert_mcp_server(
            id,
            &server.name,
            &source,
            source_value
                .or(current_value.as_deref())
                .map(|s| s.to_string())
                .as_deref(),
            &manifest_json,
            None,
            &secret_ids_json,
            &current_enabled_tools_json,
            &row.8,
            &server.updated_at.to_rfc3339(),
        )?;

        if let Some(cache_dir) = self.cache_dir() {
            let installer = McpInstaller::new(cache_dir);
            let installed = installer.install(&server).await?;
            let _ = installed;
        }

        Ok(format!("mcp server {id} updated"))
    }

    /// Update only the secret_ids and enabled_tools for an MCP server.
    pub fn update_mcp_server_meta(
        &self,
        store: &Store,
        id: &str,
        secret_ids: &[String],
        enabled_tools: &[String],
    ) -> Result<String> {
        let rows = store.list_mcp_servers()?;
        let row = rows
            .into_iter()
            .find(|(i, _, _, _, _, _, _, _, _, _)| i == id)
            .context(format!("mcp server {id} not found"))?;
        let secret_ids_json = serde_json::to_string(secret_ids)?;
        let enabled_tools_json = serde_json::to_string(enabled_tools)?;
        store.insert_mcp_server(
            id,
            &row.1,
            &row.2,
            row.3.as_deref(),
            &row.4,
            row.5.as_deref(),
            &secret_ids_json,
            &enabled_tools_json,
            &row.8,
            &chrono::Utc::now().to_rfc3339(),
        )?;
        Ok(format!("mcp server {id} meta updated"))
    }

    /// Update only the enabled_tools for an MCP server from discovered tool names.
    /// Used by discover_and_enable_all to default all discovered tools to enabled.
    pub fn update_mcp_server_enabled_tools(
        &self,
        store: &Store,
        id: &str,
        enabled_tools: &[Secret],
    ) -> Result<String> {
        let rows = store.list_mcp_servers()?;
        let row = rows
            .into_iter()
            .find(|(i, _, _, _, _, _, _, _, _, _)| i == id)
            .context(format!("mcp server {id} not found"))?;
        let enabled_tools_json = serde_json::to_string(
            &enabled_tools
                .iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>(),
        )?;
        store.insert_mcp_server(
            id,
            &row.1,
            &row.2,
            row.3.as_deref(),
            &row.4,
            row.5.as_deref(),
            &row.6,
            &enabled_tools_json,
            &row.8,
            &chrono::Utc::now().to_rfc3339(),
        )?;
        Ok(format!("mcp server {id} enabled tools updated"))
    }

    /// Delete an installed MCP server from the store and stop any running client.
    pub fn delete_mcp_server(&self, store: &Store, id: &str) -> Result<String> {
        store.list_mcp_servers()?;
        store.delete_mcp_server(id)?;
        self.stop_client(id);
        let mut index = self.tool_index.lock();
        let prefix = format!("mcp_{}_", id.replace('-', "_"));
        index.retain(|k, _| !k.starts_with(&prefix));
        Ok(format!("mcp server {id} deleted"))
    }

    fn stop_client(&self, id: &str) {
        self.stdio_clients.lock().remove(id);
        self.sse_clients.lock().remove(id);
    }
}
