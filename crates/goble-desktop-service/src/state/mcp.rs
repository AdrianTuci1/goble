use anyhow::Context;
use goble_core::agent::AgentSpec;
use goble_core::mcp_client::McpTool;
use goble_core::mcp_manager::McpServerSummary;
use goble_core::mcp_registry::McpSearchResult;

use super::DesktopState;

fn parse_mcp_source(
    source: &str,
    source_value: Option<&str>,
) -> anyhow::Result<goble_core::agent::McpSource> {
    match source {
        "npm" => Ok(goble_core::agent::McpSource::Npm {
            package: source_value.unwrap_or("").to_string(),
            version: "latest".to_string(),
        }),
        "github" => {
            let parts: Vec<&str> = source_value.unwrap_or("").split('#').collect();
            Ok(goble_core::agent::McpSource::Github {
                repo: parts.first().unwrap_or(&"").to_string(),
                rev: parts.get(1).unwrap_or(&"main").to_string(),
            })
        }
        "local" => Ok(goble_core::agent::McpSource::Local {
            path: source_value.unwrap_or("").to_string(),
        }),
        "url" | "sse" => Ok(goble_core::agent::McpSource::Url {
            url: source_value.unwrap_or("").to_string(),
        }),
        _ => anyhow::bail!("unknown mcp source: {source}"),
    }
}

impl DesktopState {
    /// Resolve MCP servers referenced by an agent spec, substituting vault secrets into the server config.
    pub(super) fn resolve_mcp_servers_for_agent(
        &self,
        spec: &AgentSpec,
    ) -> anyhow::Result<Vec<goble_core::agent::McpServer>> {
        if spec.mcp_ids.is_empty() {
            return Ok(vec![]);
        }
        let summaries = self.list_mcp_servers()?;
        let passphrase = self.vault_passphrase.lock().clone();
        let mut resolved = Vec::new();
        for id in &spec.mcp_ids {
            let summary = match summaries.iter().find(|s| &s.id == id) {
                Some(s) => s,
                None => {
                    anyhow::bail!("mcp server {id} referenced by agent {} not found", spec.id);
                }
            };
            let server = self.resolve_mcp_server_with_secrets(summary, &passphrase)?;
            resolved.push(server);
        }
        Ok(resolved)
    }

    fn resolve_mcp_server_with_secrets(
        &self,
        summary: &McpServerSummary,
        passphrase: &[u8],
    ) -> anyhow::Result<goble_core::agent::McpServer> {
        let rows = self.store.lock().list_mcp_servers()?;
        let row = rows
            .into_iter()
            .find(|(i, _, _, _, _, _, _, _, _, _)| i == &summary.id)
            .context(format!("mcp server {} not found", summary.id))?;
        let manifest: goble_core::agent::McpManifest = serde_json::from_str(&row.4)?;
        let source = parse_mcp_source(&summary.source, summary.source_value.as_deref())?;
        let mut server = goble_core::agent::McpServer {
            id: summary.id.clone(),
            name: summary.name.clone(),
            source,
            manifest,
            credentials_key: None,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        if !summary.secret_ids.is_empty() {
            if passphrase.is_empty() {
                anyhow::bail!("vault is locked; cannot resolve secrets for {}", summary.id);
            }
            let mut env = std::collections::HashMap::new();
            for key in &summary.secret_ids {
                let value = self.vault.lock().get(key, passphrase)?.context(format!(
                    "vault secret {key} missing for mcp server {}",
                    summary.id
                ))?;
                env.insert(key.clone(), String::from_utf8_lossy(&value).to_string());
            }
            server.credentials_key = Some(serde_json::to_string(&env)?);
        }
        Ok(server)
    }

    pub fn list_mcp_servers(&self) -> Result<Vec<McpServerSummary>, anyhow::Error> {
        self.mcp_manager.list_mcp_servers(&self.store.lock())
    }

    pub fn search_mcp_servers(&self, query: &str) -> Vec<McpSearchResult> {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.block_on(self.mcp_manager.search_mcp_servers(query))
        } else {
            Vec::new()
        }
    }

    pub fn test_call_mcp_tool(
        &self,
        id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, anyhow::Error> {
        let store = self.store.lock();
        tokio::runtime::Handle::try_current()
            .map_err(|e| anyhow::anyhow!("no tokio runtime: {e}"))
            .and_then(|h| {
                h.block_on(
                    self.mcp_manager
                        .test_call_tool(&store, id, tool_name, arguments),
                )
                .map_err(|e| e)
            })
    }

    pub fn install_mcp_server(
        &self,
        id: &str,
        name: &str,
        source: &str,
        source_value: Option<&str>,
        secret_ids: Vec<String>,
        manifest: Option<goble_core::agent::McpManifest>,
    ) -> Result<String, anyhow::Error> {
        let store = self.store.lock().clone();
        let secrets: Vec<goble_core::secret::Secret> = secret_ids
            .iter()
            .map(|name| goble_core::secret::Secret::new(name, "mcp", vec![]))
            .collect();
        tokio::runtime::Handle::try_current()
            .map_err(|e| anyhow::anyhow!("no tokio runtime: {e}"))
            .and_then(|h| {
                h.block_on(self.mcp_manager.install_mcp_server(
                    &store,
                    id,
                    name,
                    source,
                    source_value,
                    &secrets,
                    manifest,
                ))
                .map_err(|e| e)
            })
    }

    pub fn update_mcp_server(
        &self,
        id: &str,
        name: Option<&str>,
        source_value: Option<&str>,
        secret_ids: Option<Vec<String>>,
        manifest: Option<goble_core::agent::McpManifest>,
    ) -> Result<String, anyhow::Error> {
        let store = self.store.lock().clone();
        let secrets = secret_ids.map(|ids| {
            ids.iter()
                .map(|name| goble_core::secret::Secret::new(name, "mcp", vec![]))
                .collect::<Vec<_>>()
        });
        tokio::runtime::Handle::try_current()
            .map_err(|e| anyhow::anyhow!("no tokio runtime: {e}"))
            .and_then(|h| {
                h.block_on(self.mcp_manager.update_mcp_server(
                    &store,
                    id,
                    name,
                    source_value,
                    secrets.as_deref(),
                    manifest,
                ))
                .map_err(|e| e)
            })
    }

    pub fn delete_mcp_server(&self, id: &str) -> Result<String, anyhow::Error> {
        let store = self.store.lock().clone();
        self.mcp_manager.delete_mcp_server(&store, id)
    }

    pub fn update_mcp_server_meta(
        &self,
        id: &str,
        secret_ids: Vec<String>,
        enabled_tools: Vec<String>,
    ) -> Result<String, anyhow::Error> {
        let store = self.store.lock().clone();
        self.mcp_manager
            .update_mcp_server_meta(&store, id, &secret_ids, &enabled_tools)
    }

    pub fn discover_mcp_tools(&self, id: &str) -> Result<Vec<McpTool>, anyhow::Error> {
        self.mcp_manager
            .discover_and_enable_all(&self.store.lock(), id)?;
        self.mcp_manager.discover_and_register(id)
    }
}
