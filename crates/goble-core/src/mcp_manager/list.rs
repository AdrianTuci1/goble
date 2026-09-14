use crate::agent::{McpManifest, McpRuntime};
use crate::store::Store;
use anyhow::Result;

use super::McpServerSummary;

use super::McpManager;

impl McpManager {
    /// List installed MCP servers from the store with discovered tools.
    pub fn list_mcp_servers(&self, store: &Store) -> Result<Vec<McpServerSummary>> {
        let rows = store.list_mcp_servers()?;
        let index = self.tool_index.lock();
        let mut summaries = Vec::new();
        for (
            id,
            name,
            source,
            source_value,
            manifest_json,
            _credentials_key,
            secret_ids_json,
            enabled_tools_json,
            _installed_at,
            _updated_at,
        ) in rows
        {
            let manifest: McpManifest =
                serde_json::from_str(&manifest_json).unwrap_or_else(|_| McpManifest {
                    schema_version: "1".to_string(),
                    entrypoint: "".to_string(),
                    runtime: McpRuntime::V8Isolate,
                    auth_schema: vec![],
                    capabilities: vec![],
                    config_schema: serde_json::json!({}),
                });
            let prefix = format!("mcp_{}_", id.replace('-', "_"));
            let discovered_tools: Vec<String> = index
                .keys()
                .filter(|k| k.starts_with(&prefix) && **k != format!("{prefix}call"))
                .cloned()
                .collect();
            let secret_ids: Vec<String> =
                serde_json::from_str(&secret_ids_json).unwrap_or_default();
            let enabled_tools: Vec<String> =
                serde_json::from_str(&enabled_tools_json).unwrap_or_default();
            summaries.push(McpServerSummary {
                id: id.clone(),
                name,
                source: source.clone(),
                source_value: source_value.clone(),
                capabilities: manifest.capabilities.clone(),
                auth_required: !manifest.auth_schema.is_empty(),
                discovered_tools,
                secret_ids,
                enabled_tools,
            });
        }
        Ok(summaries)
    }
}
