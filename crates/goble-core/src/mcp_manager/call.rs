use crate::agent::{McpManifest, McpRuntime, McpServer, McpSource};
use crate::llm::ToolDefinition;
use crate::mcp_client::McpClient;
use crate::mcp_installer::McpInstaller;
use crate::store::Store;
use anyhow::{Context, Result};

use super::McpManager;

impl McpManager {
    /// Call an MCP tool by its harness name (e.g. `mcp_filesystem_read_file`).
    pub fn call_tool(&self, name: &str, arguments: serde_json::Value) -> Result<serde_json::Value> {
        let mapping = self
            .tool_index
            .lock()
            .get(name)
            .cloned()
            .context(format!("mcp tool {name} not found"))?;

        let server_id = mapping.server_id;
        let mut tool_name = mapping.tool_name.clone();
        let mut call_arguments = arguments.clone();
        if tool_name == "__any__" {
            tool_name = arguments
                .get("tool")
                .and_then(|v| v.as_str())
                .context("mcp_call requires a `tool` argument")?
                .to_string();
            call_arguments = arguments
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
        }

        // SSE client
        if let Some(client) = self.sse_clients.lock().get(&server_id).cloned() {
            let rt = tokio::runtime::Handle::try_current().context("no async runtime")?;
            return rt.block_on(client.call_tool(&tool_name, call_arguments));
        }

        // Stdio client
        let client = self
            .stdio_clients
            .lock()
            .get(&server_id)
            .cloned()
            .context(format!("mcp server {server_id} not connected"))?;
        client.call_tool(&tool_name, call_arguments)
    }

    /// Call a tool by server id and tool name. The server must already be started
    /// (e.g. via `refresh_from_store` or `discover_and_register`).
    /// Test a single tool call on an MCP server by spawning a temporary stdio client.
    /// Does not require the server to be already running in the manager.
    pub async fn test_call_tool(
        &self,
        store: &Store,
        id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let rows = store.list_mcp_servers()?;
        let row = rows
            .into_iter()
            .find(|(i, _, _, _, _, _, _, _, _, _)| i == id)
            .context(format!("mcp server {id} not found"))?;
        let source = row.2;
        let source_value = row.3;
        let manifest_json = row.4;
        let manifest: McpManifest =
            serde_json::from_str(&manifest_json).unwrap_or_else(|_| McpManifest {
                schema_version: "1".to_string(),
                entrypoint: "".to_string(),
                runtime: McpRuntime::V8Isolate,
                auth_schema: vec![],
                capabilities: vec![],
                config_schema: serde_json::json!({}),
            });

        let cache_dir = self.cache_dir().context("no installer configured")?;
        let installer = McpInstaller::new(cache_dir);
        let source = match source.as_str() {
            "npm" => McpSource::Npm {
                package: source_value.clone().unwrap_or_default(),
                version: "latest".to_string(),
            },
            "github" => McpSource::Github {
                repo: source_value.clone().unwrap_or_default(),
                rev: "main".to_string(),
            },
            "local" => McpSource::Local {
                path: source_value.clone().unwrap_or_default(),
            },
            "url" => McpSource::Url {
                url: source_value.clone().unwrap_or_default(),
            },
            _ => McpSource::Local {
                path: source_value.clone().unwrap_or_default(),
            },
        };
        let server = McpServer {
            id: id.to_string(),
            name: row.1,
            source,
            manifest,
            credentials_key: row.5,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let installed = installer.install(&server).await?;
        let (command, args) = installed.runtime_command();
        let env = server
            .credentials_key
            .as_ref()
            .and_then(|s| serde_json::from_str::<std::collections::HashMap<String, String>>(s).ok())
            .unwrap_or_default();
        let client = McpClient::spawn_owned(&command, &args, env)?;
        client.initialize()?;
        client.call_tool(tool_name, arguments)
    }

    pub fn call_tool_on_server(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value> {
        // SSE client
        if let Some(client) = self.sse_clients.lock().get(server_id).cloned() {
            let rt = tokio::runtime::Handle::try_current().context("no async runtime")?;
            return rt.block_on(client.call_tool(tool_name, arguments));
        }

        let client = self
            .stdio_clients
            .lock()
            .get(server_id)
            .cloned()
            .context(format!("mcp server {server_id} not connected"))?;
        client.call_tool(tool_name, arguments)
    }

    /// Resolve a harness tool name to an MCP server call.
    pub fn is_mcp_tool(&self, name: &str) -> bool {
        self.tool_index.lock().contains_key(name)
    }

    /// Return a generic `mcp_call` tool definition that lets the LLM pick server and tool.
    pub fn generic_mcp_call_tool() -> ToolDefinition {
        ToolDefinition {
            name: "mcp_call".to_string(),
            description: "Call a tool on a configured MCP server. Use this when the user asks about a capability provided by an MCP server.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "server_id": { "type": "string" },
                    "tool": { "type": "string" },
                    "arguments": { "type": "object" }
                },
                "required": ["server_id", "tool", "arguments"]
            }),
        }
    }
}
