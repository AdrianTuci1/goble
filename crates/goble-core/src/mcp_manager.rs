use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::agent::{McpManifest, McpRuntime, McpServer, McpSource};
use crate::llm::ToolDefinition;
use crate::mcp_client::{McpClient, McpSseClient, McpTool};
use crate::mcp_installer::{InstalledMcp, McpInstaller};
use crate::mcp_registry::{McpRegistry, McpSearchResult};
use crate::secret::Secret;
use crate::store::Store;

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
        credentials: Vec<Secret>,
        manifest: Option<McpManifest>,
    ) -> Result<String> {
        let server =
            build_server_from_user_input(id, name, source, source_value, credentials, manifest)?;

        // Persist in store before any network activity so the UI sees it immediately.
        let now = chrono::Utc::now().to_rfc3339();
        let manifest_json = serde_json::to_string(&server.manifest)?;
        let source_value_str = source_value.map(|s| s.to_string());
        let credentials_key = server.credentials_key.clone();
        let secret_ids_json = "[]";
        let enabled_tools_json = "[]";
        store.insert_mcp_server(
            &server.id,
            &server.name,
            source,
            source_value_str.as_deref(),
            &manifest_json,
            credentials_key.as_deref(),
            secret_ids_json,
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
        credentials: Option<Vec<Secret>>,
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
        let current_secret_ids_json = row.6;
        let current_enabled_tools_json = row.7;

        let mut server: McpServer = build_server_from_user_input(
            id,
            name.unwrap_or(&current_name),
            &source,
            source_value.or(current_value.as_deref()),
            credentials.unwrap_or_default(),
            manifest.or_else(|| serde_json::from_str(&current_manifest_json).ok()),
        )?;
        server.updated_at = chrono::Utc::now();

        let manifest_json = serde_json::to_string(&server.manifest)?;
        let credentials_key = server.credentials_key.clone();
        store.insert_mcp_server(
            id,
            &server.name,
            &source,
            source_value
                .or(current_value.as_deref())
                .map(|s| s.to_string())
                .as_deref(),
            &manifest_json,
            credentials_key.as_deref(),
            &current_secret_ids_json,
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

fn build_server_from_user_input(
    id: &str,
    name: &str,
    source: &str,
    source_value: Option<&str>,
    credentials: Vec<Secret>,
    manifest: Option<McpManifest>,
) -> Result<McpServer> {
    let mcp_source = match source {
        "npm" => McpSource::Npm {
            package: source_value
                .context("npm source requires package name")?
                .to_string(),
            version: "latest".to_string(),
        },
        "github" => {
            let parts = source_value.context("github source requires owner/repo")?;
            let (repo, rev) = parts
                .split_once('#')
                .map(|(r, v)| (r, v))
                .unwrap_or((parts, "main"));
            McpSource::Github {
                repo: repo.to_string(),
                rev: rev.to_string(),
            }
        }
        "local" => McpSource::Local {
            path: source_value
                .context("local source requires path")?
                .to_string(),
        },
        "url" => McpSource::Url {
            url: source_value.context("url source requires url")?.to_string(),
        },
        "stdio" => McpSource::Npm {
            package: source_value.unwrap_or(id).to_string(),
            version: "latest".to_string(),
        },
        _ => anyhow::bail!("unknown source {source}"),
    };

    let manifest = manifest.unwrap_or_else(|| McpManifest {
        schema_version: "1".to_string(),
        entrypoint: "dist/index.js".to_string(),
        runtime: McpRuntime::Binary {
            command: "npx".to_string(),
            args: vec!["-y".to_string(), source_value.unwrap_or(id).to_string()],
        },
        auth_schema: vec![],
        capabilities: vec!["tools".to_string()],
        config_schema: serde_json::json!({}),
    });

    let credentials_key = if credentials.is_empty() {
        None
    } else {
        Some(uuid::Uuid::new_v4().to_string())
    };

    let now = chrono::Utc::now();
    Ok(McpServer {
        id: id.to_string(),
        name: name.to_string(),
        source: mcp_source,
        manifest,
        credentials_key,
        installed_at: now,
        updated_at: now,
    })
}

fn tool_definition_from_mcp_tool(
    server_id: &str,
    full_name: &str,
    tool: &McpTool,
) -> ToolDefinition {
    let description = tool.description.clone().unwrap_or_else(|| {
        format!(
            "MCP tool {tool_name} from server {server_id}",
            tool_name = tool.name
        )
    });
    let parameters = match &tool.input_schema {
        crate::mcp_client::McpToolInputSchema::Object {
            properties,
            required,
            ..
        } => {
            let mut schema = serde_json::json!({
                "type": "object",
                "properties": properties.clone().unwrap_or_default(),
            });
            if let Some(req) = required {
                schema["required"] = serde_json::json!(req.clone());
            }
            schema
        }
        crate::mcp_client::McpToolInputSchema::Other(v) => v.clone(),
    };
    ToolDefinition {
        name: full_name.to_string(),
        description,
        parameters,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_manager_update_meta_and_enabled_tools_filter() {
        use crate::agent::{McpManifest, McpRuntime, McpServer, McpSource};

        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("index.js"),
            include_str!("../tests/mcp_mock_server.js"),
        )
        .unwrap();

        let server = McpServer {
            id: "mock-echo".to_string(),
            name: "Mock Echo".to_string(),
            source: McpSource::Local {
                path: src.to_string_lossy().to_string(),
            },
            manifest: McpManifest {
                schema_version: "1".to_string(),
                entrypoint: "index.js".to_string(),
                runtime: McpRuntime::V8Isolate,
                auth_schema: vec![],
                capabilities: vec!["tools".to_string()],
                config_schema: serde_json::json!({}),
            },
            credentials_key: None,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let store = Store::open_in_memory().unwrap();
        let manager = McpManager::new().with_installer(McpInstaller::new(tmp.path().join("cache")));
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(manager.install_mcp_server(
            &store,
            &server.id,
            &server.name,
            "local",
            Some(&src.to_string_lossy()),
            vec![],
            Some(server.manifest.clone()),
        ))
        .unwrap();

        let listed = manager.list_mcp_servers(&store).unwrap();
        assert_eq!(listed[0].enabled_tools.len(), 0);

        // Discover defaults all tools as enabled.
        manager
            .discover_and_enable_all(&store, "mock-echo")
            .unwrap();
        let listed = manager.list_mcp_servers(&store).unwrap();
        assert_eq!(listed[0].enabled_tools, vec!["echo"]);

        // Disable the only tool.
        manager
            .update_mcp_server_meta(&store, "mock-echo", &[], &[])
            .unwrap();
        let tools = manager.refresh_from_store(&store).unwrap();
        let concrete: Vec<_> = tools
            .into_iter()
            .filter(|t| t.name == "mcp_mock_echo_echo")
            .collect();
        assert!(concrete.is_empty(), "disabled tool should not be exposed");

        // Re-enable and verify exposed again.
        manager
            .update_mcp_server_meta(&store, "mock-echo", &[], &["echo".to_string()])
            .unwrap();
        let tools = manager.refresh_from_store(&store).unwrap();
        assert!(
            tools.iter().any(|t| t.name == "mcp_mock_echo_echo"),
            "enabled tool should be exposed"
        );
    }

    #[test]
    fn test_mcp_manager_secret_ids_persisted() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = Store::open_in_memory().unwrap();
        let manager = McpManager::new().with_installer(McpInstaller::new(tmp.path().join("cache")));
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(manager.install_mcp_server(
            &store,
            "mcp-sequential-thinking",
            "Sequential Thinking",
            "npm",
            Some("@modelcontextprotocol/server-sequential-thinking"),
            vec![],
            None,
        ))
        .unwrap();

        manager
            .update_mcp_server_meta(
                &store,
                "mcp-sequential-thinking",
                &["openai_api_key".to_string()],
                &[],
            )
            .unwrap();

        let listed = manager.list_mcp_servers(&store).unwrap();
        assert_eq!(listed[0].secret_ids, vec!["openai_api_key"]);
    }

    #[test]
    fn test_mcp_manager_generic_tool() {
        let tool = McpManager::generic_mcp_call_tool();
        assert_eq!(tool.name, "mcp_call");
    }

    #[test]
    fn test_build_server_from_user_input() {
        let server = build_server_from_user_input(
            "mcp-sequential-thinking",
            "Sequential Thinking",
            "npm",
            Some("@modelcontextprotocol/server-sequential-thinking"),
            vec![],
            None,
        )
        .unwrap();
        assert_eq!(
            server.source,
            McpSource::Npm {
                package: "@modelcontextprotocol/server-sequential-thinking".to_string(),
                version: "latest".to_string(),
            }
        );
    }

    #[test]
    fn test_mcp_manager_install_and_discover_local_mock() {
        use crate::agent::{McpManifest, McpRuntime, McpServer, McpSource};
        use chrono::Utc;

        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let server_path = src.join("index.js");
        let script = include_str!("../tests/mcp_mock_server.js");
        std::fs::write(&server_path, script).unwrap();

        let server = McpServer {
            id: "mock-echo".to_string(),
            name: "Mock Echo".to_string(),
            source: McpSource::Local {
                path: src.to_string_lossy().to_string(),
            },
            manifest: McpManifest {
                schema_version: "1".to_string(),
                entrypoint: "index.js".to_string(),
                runtime: McpRuntime::V8Isolate,
                auth_schema: vec![],
                capabilities: vec!["tools".to_string()],
                config_schema: serde_json::json!({}),
            },
            credentials_key: None,
            installed_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let store = Store::open_in_memory().unwrap();
        let manager = McpManager::new().with_installer(McpInstaller::new(tmp.path().join("cache")));
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(manager.install_mcp_server(
            &store,
            &server.id,
            &server.name,
            "local",
            Some(&src.to_string_lossy()),
            vec![],
            Some(server.manifest.clone()),
        ))
        .unwrap();

        let listed = manager.list_mcp_servers(&store).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "mock-echo");

        manager
            .discover_and_enable_all(&store, "mock-echo")
            .unwrap();

        let tools = manager.refresh_from_store(&store).unwrap();
        let concrete = tools
            .iter()
            .find(|t| t.name == "mcp_mock_echo_echo")
            .cloned();
        assert!(
            concrete.is_some(),
            "concrete mcp tool should be exposed: got {:?}",
            tools.iter().map(|t| &t.name).collect::<Vec<_>>()
        );

        let result = manager
            .call_tool(
                "mcp_mock_echo_echo",
                serde_json::json!({ "message": "hello" }),
            )
            .unwrap();
        let text = result
            .get("content")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|o| o.get("text"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert_eq!(text, "echo: hello");

        manager.delete_mcp_server(&store, "mock-echo").unwrap();
        assert!(manager.list_mcp_servers(&store).unwrap().is_empty());
    }
}
