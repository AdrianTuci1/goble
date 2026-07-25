use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::llm::ToolDefinition;
use crate::mcp_client::{McpClient, McpSseClient, McpTool};
use crate::store::Store;

/// Manages live MCP stdio and SSE clients and exposes their tools to the harness.
#[derive(Clone, Default)]
pub struct McpManager {
    stdio_clients: Arc<Mutex<HashMap<String, Arc<McpClient>>>>,
    sse_clients: Arc<Mutex<HashMap<String, Arc<McpSseClient>>>>,
    tool_index: Arc<Mutex<HashMap<String, McpToolMapping>>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct McpToolMapping {
    server_id: String,
    tool_name: String,
}

impl McpManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load all MCP servers from the store and register their tools lazily.
    pub fn refresh_from_store(&self, store: &Store) -> Result<Vec<ToolDefinition>> {
        let rows = store.list_mcp_servers()?;
        let mut tool_defs = Vec::new();
        let mut index = self.tool_index.lock();
        index.clear();
        for (id, _name, source, source_value, _manifest, _credentials, _installed, _updated) in rows {
            let prefix = format!("mcp_{}_", id.replace('-', "_"));
            match source.as_str() {
                "stdio" | "npm" | "local" => {
                    // stdio servers will be lazily spawned on first use
                    let server_id = id.clone();
                    let full_name = format!("{prefix}call");
                    index.insert(
                        full_name.clone(),
                        McpToolMapping {
                            server_id: server_id.clone(),
                            tool_name: "__any__".to_string(),
                        },
                    );
                    tool_defs.push(ToolDefinition {
                        name: full_name,
                        description: format!(
                            "MCP proxy for server {id}. Call any tool by name with its arguments."
                        ),
                        parameters: serde_json::json!({
                            "type": "object",
                            "properties": {
                                "tool": { "type": "string", "description": "The MCP tool name to call" },
                                "arguments": { "type": "object" }
                            },
                            "required": ["tool", "arguments"]
                        }),
                    });
                }
                "sse" | "url" => {
                    if let Some(url) = source_value.as_ref() {
                        let client = Arc::new(McpSseClient::new(url));
                        self.sse_clients.lock().insert(id.clone(), client);
                    }
                }
                _ => {}
            }
        }
        Ok(tool_defs)
    }

    /// Resolve a harness tool name to an MCP server call.
    pub fn is_mcp_tool(&self, name: &str) -> bool {
        self.tool_index.lock().contains_key(name)
    }

    /// Discover concrete tools from a stdio MCP server. Spawns the server if not already running.
    pub fn discover_stdio_tools(
        &self,
        server_id: &str,
        command: &str,
        args: &[String],
        env: HashMap<String, String>,
    ) -> Result<Vec<McpTool>> {
        let mut clients = self.stdio_clients.lock();
        let client = if let Some(c) = clients.get(server_id) {
            c.clone()
        } else {
            let c = Arc::new(McpClient::spawn_owned(command, args, env)?);
            c.initialize()?;
            clients.insert(server_id.to_string(), c.clone());
            c
        };
        drop(clients);
        let tools = client.list_tools()?;
        let mut index = self.tool_index.lock();
        // Build concrete tool index
        for tool in &tools {
            let full_name = format!("mcp_{}_{}", server_id.replace('-', "_"), tool.name.replace('-', "_"));
            index.insert(
                full_name.clone(),
                McpToolMapping {
                    server_id: server_id.to_string(),
                    tool_name: tool.name.clone(),
                },
            );
        }
        Ok(tools)
    }

    /// Call an MCP tool by its harness name (e.g. `mcp_filesystem_read_file`).
    pub fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_manager_generic_tool() {
        let tool = McpManager::generic_mcp_call_tool();
        assert_eq!(tool.name, "mcp_call");
    }
}
