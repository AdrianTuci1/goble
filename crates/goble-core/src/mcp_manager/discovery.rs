use std::collections::HashMap;
use std::sync::Arc;

use crate::agent::{McpManifest, McpRuntime};
use crate::llm::ToolDefinition;
use crate::mcp_client::{McpClient, McpSseClient, McpTool};
use crate::mcp_installer::{InstalledMcp, McpInstaller};
use crate::store::Store;
use anyhow::{Context, Result};

use super::McpToolMapping;

use super::server::tool_definition_from_mcp_tool;

use super::McpManager;

impl McpManager {
    /// Load all MCP servers from the store and discover their concrete tools.
    /// For stdio servers without required auth, the server is spawned and tools/list called.
    /// For SSE/URL servers, a client is connected.
    pub fn refresh_from_store(&self, store: &Store) -> Result<Vec<ToolDefinition>> {
        let rows = store.list_mcp_servers()?;
        let mut tool_defs = Vec::new();
        let mut index = self.tool_index.lock();
        index.clear();
        for (
            id,
            _name,
            source,
            source_value,
            manifest_json,
            _credentials_key,
            _secret_ids_json,
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
            let needs_auth = !manifest.auth_schema.is_empty();
            let prefix = format!("mcp_{}_", id.replace('-', "_"));
            let enabled_tools: Vec<String> =
                serde_json::from_str(&enabled_tools_json).unwrap_or_default();
            match source.as_str() {
                "stdio" | "npm" | "local" => {
                    // Always register a generic proxy fallback for this server.
                    let full_name = format!("{prefix}call");
                    index.insert(
                        full_name.clone(),
                        McpToolMapping {
                            server_id: id.clone(),
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

                    // Auto-discover concrete tools when no auth is required and the server is installed.
                    if !needs_auth {
                        if let Ok(tools) = self.discover_stdio_tools_internal(
                            &id,
                            &manifest,
                            source_value.as_deref(),
                        ) {
                            for tool in &tools {
                                let full_name = format!("{prefix}{}", tool.name.replace('-', "_"));
                                if !enabled_tools.contains(&tool.name)
                                    && !enabled_tools.contains(&full_name)
                                {
                                    continue;
                                }
                                index.insert(
                                    full_name.clone(),
                                    McpToolMapping {
                                        server_id: id.clone(),
                                        tool_name: tool.name.clone(),
                                    },
                                );
                                tool_defs
                                    .push(tool_definition_from_mcp_tool(&id, &full_name, tool));
                            }
                        }
                    }
                }
                "sse" | "url" => {
                    if let Some(url) = source_value.as_ref() {
                        let client = Arc::new(McpSseClient::new(url));
                        self.sse_clients.lock().insert(id.clone(), client);
                        // SSE concrete tool discovery can be done in an async task; here we just expose the proxy.
                        let full_name = format!("{prefix}call");
                        index.insert(
                            full_name.clone(),
                            McpToolMapping {
                                server_id: id.clone(),
                                tool_name: "__any__".to_string(),
                            },
                        );
                        tool_defs.push(ToolDefinition {
                            name: full_name,
                            description: format!("MCP proxy for SSE server {id}."),
                            parameters: serde_json::json!({
                                "type": "object",
                                "properties": {
                                    "tool": { "type": "string" },
                                    "arguments": { "type": "object" }
                                },
                                "required": ["tool", "arguments"]
                            }),
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(tool_defs)
    }

    /// Discover concrete tools from a stdio MCP server and register them in the tool index.
    /// If the server is not already running, it is spawned using the local cache or the manifest.
    /// Discovered tools are registered in the index and returned; the caller is responsible
    /// for persisting enabled_tools to the store if desired.
    pub fn discover_and_register(&self, id: &str) -> Result<Vec<McpTool>> {
        let client = {
            let clients = self.stdio_clients.lock();
            if let Some(c) = clients.get(id) {
                c.clone()
            } else if let Some(cache_dir) = self.cache_dir() {
                drop(clients);
                let installer = McpInstaller::new(cache_dir);
                let installed = InstalledMcp {
                    id: id.to_string(),
                    path: installer.install_path(id),
                    manifest: self.guess_manifest(id)?,
                };
                let (command, args) = installed.runtime_command();
                let c = Arc::new(McpClient::spawn_owned(&command, &args, HashMap::new())?);
                c.initialize()?;
                self.stdio_clients.lock().insert(id.to_string(), c.clone());
                c
            } else {
                drop(clients);
                anyhow::bail!("no installer configured and server {id} is not running");
            }
        };
        let tools = client.list_tools()?;
        let prefix = format!("mcp_{}_", id.replace('-', "_"));
        let mut index = self.tool_index.lock();
        for tool in &tools {
            let full_name = format!("{prefix}{}", tool.name.replace('-', "_"));
            index.insert(
                full_name,
                McpToolMapping {
                    server_id: id.to_string(),
                    tool_name: tool.name.clone(),
                },
            );
        }
        Ok(tools)
    }

    /// Default all discovered tools as enabled for a server in the store.
    /// Returns the discovered tool names so the UI can show them.
    pub fn discover_and_enable_all(&self, store: &Store, id: &str) -> Result<Vec<String>> {
        let rows = store.list_mcp_servers()?;
        let (
            _id,
            _name,
            _source,
            source_value,
            manifest_json,
            _credentials_key,
            _secret_ids_json,
            _enabled_tools_json,
            _installed_at,
            _updated_at,
        ) = rows
            .into_iter()
            .find(|(i, _, _, _, _, _, _, _, _, _)| i == id)
            .context(format!("mcp server {id} not found"))?;
        let manifest: McpManifest =
            serde_json::from_str(&manifest_json).unwrap_or_else(|_| McpManifest {
                schema_version: "1".to_string(),
                entrypoint: "".to_string(),
                runtime: McpRuntime::V8Isolate,
                auth_schema: vec![],
                capabilities: vec![],
                config_schema: serde_json::json!({}),
            });
        let tools = self.discover_stdio_tools_internal(id, &manifest, source_value.as_deref())?;
        let prefix = format!("mcp_{}_", id.replace('-', "_"));
        let mut index = self.tool_index.lock();
        for tool in &tools {
            let full_name = format!("{prefix}{}", tool.name.replace('-', "_"));
            index.insert(
                full_name,
                McpToolMapping {
                    server_id: id.to_string(),
                    tool_name: tool.name.clone(),
                },
            );
        }
        drop(index);
        let names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
        if !names.is_empty() {
            let _ = self.update_mcp_server_meta(store, id, &[], &names);
        }
        Ok(names)
    }

    fn discover_stdio_tools_internal(
        &self,
        id: &str,
        manifest: &McpManifest,
        source_value: Option<&str>,
    ) -> Result<Vec<McpTool>> {
        if let Some(cache_dir) = self.cache_dir() {
            let installer = McpInstaller::new(cache_dir);
            let installed = InstalledMcp {
                id: id.to_string(),
                path: installer.install_path(id),
                manifest: manifest.clone(),
            };
            if installer.is_installed(id) {
                let (command, args) = installed.runtime_command();
                return self.discover_stdio_tools(id, &command, &args, HashMap::new());
            }
        }
        // If no cached install, try to run directly from the manifest (e.g. npx package).
        match &manifest.runtime {
            McpRuntime::Binary { command, args } => {
                let mut resolved_args = args.clone();
                if command == "npx" && source_value.is_some() {
                    resolved_args.insert(0, "-y".to_string());
                    resolved_args.insert(1, source_value.unwrap().to_string());
                }
                self.discover_stdio_tools(id, command, &resolved_args, HashMap::new())
            }
            McpRuntime::V8Isolate => {
                if let Some(path) = source_value {
                    let entrypoint = if manifest.entrypoint.is_empty() {
                        "index.js"
                    } else {
                        &manifest.entrypoint
                    };
                    let script_path = std::path::Path::new(path).join(entrypoint);
                    self.discover_stdio_tools(
                        id,
                        "node",
                        &[script_path.to_string_lossy().to_string()],
                        HashMap::new(),
                    )
                } else {
                    anyhow::bail!("cannot discover V8 isolate MCP without source path");
                }
            }
        }
    }

    fn guess_manifest(&self, id: &str) -> Result<McpManifest> {
        // Best-effort guess: assume npx binary for common servers.
        Ok(McpManifest {
            schema_version: "1".to_string(),
            entrypoint: "dist/index.js".to_string(),
            runtime: McpRuntime::Binary {
                command: "npx".to_string(),
                args: vec!["-y".to_string(), id.to_string()],
            },
            auth_schema: vec![],
            capabilities: vec!["tools".to_string()],
            config_schema: serde_json::json!({}),
        })
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
        client.list_tools()
    }
}
