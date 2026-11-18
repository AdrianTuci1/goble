//! `[mcp_servers.*]` is the declaration of record; the store's MCP rows are the
//! runtime projection.
//!
//! McpManager serves tools from the SQLite `mcp_servers` table (tool mappings,
//! install cache). The settings file is where a server is declared. This module
//! keeps the two in step on every config load and on every config write:
//!
//! - a declared, enabled table becomes (or updates) a store row;
//! - a declared, disabled table is removed from the runtime registry;
//! - a store row with no declaration is removed (the file is authoritative);
//! - the one-time reverse import writes a table for a store row that predates
//!   the file, then marks `config.mcp_import_done` so it never runs twice.
//!
//! The declaration's `env` is kept in the store row's manifest
//! (`config_schema.env`) because the store schema has no env column and a schema
//! migration is out of scope for this lane; the runtime tool discovery honours
//! it once the service wiring lands (Lane B2). The test path below does pass it
//! to the spawned process today.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;

use crate::agent::{McpManifest, McpRuntime};
use crate::config::{
    ConfigEditor, ConfigError, ConfigFile, ConfigOp, ConfigValue, GobleConfig, McpServerConfig,
};
use crate::mcp_client::McpClient;
use crate::store::Store;

use super::McpManager;

/// The store key that records the one-time MCP declaration import ran.
pub const MCP_IMPORT_MARKER: &str = "config.mcp_import_done";

/// How long a declared MCP server gets to initialize and list its tools.
pub const MCP_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// What one sync changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct McpSyncOutcome {
    /// Declared servers added to the runtime registry.
    pub added: Vec<String>,
    /// Declared servers whose runtime row changed.
    pub updated: Vec<String>,
    /// Servers removed from the runtime registry (disabled or undeclared).
    pub removed: Vec<String>,
    /// Servers whose runtime row already matched the declaration.
    pub unchanged: Vec<String>,
}

impl McpSyncOutcome {
    /// Whether anything changed.
    pub fn changed(&self) -> bool {
        !self.added.is_empty() || !self.updated.is_empty() || !self.removed.is_empty()
    }

    /// One-line report for the log.
    pub fn line(&self) -> String {
        format!(
            "mcp: +{} ~{} -{} ={}",
            self.added.len(),
            self.updated.len(),
            self.removed.len(),
            self.unchanged.len()
        )
    }
}

/// What a store row would be if `name`/`server` were written now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub id: String,
    pub name: String,
    pub source: String,
    pub source_value: Option<String>,
    pub manifest_json: String,
}

/// The declaration for one `[mcp_servers.<name>]` table, or `None` when the
/// table names no transport (validation refuses that, so this only happens for a
/// file written around the app — it is skipped, not guessed at).
pub fn declaration_for(name: &str, server: &McpServerConfig) -> Option<Declaration> {
    let (source, source_value, runtime) = match (&server.url, &server.command) {
        (Some(url), _) if !url.trim().is_empty() => (
            "url".to_string(),
            Some(url.trim().to_string()),
            McpRuntime::V8Isolate,
        ),
        (_, Some(command)) if !command.trim().is_empty() => (
            "local".to_string(),
            Some(command.trim().to_string()),
            McpRuntime::Binary {
                command: command.trim().to_string(),
                args: server.args.clone(),
            },
        ),
        _ => return None,
    };
    let manifest = McpManifest {
        schema_version: "1".to_string(),
        entrypoint: String::new(),
        runtime,
        auth_schema: vec![],
        capabilities: vec!["tools".to_string()],
        config_schema: serde_json::json!({
            "env": server
                .env
                .as_ref()
                .map(|env| env.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<BTreeMap<_, _>>())
                .unwrap_or_default(),
        }),
    };
    Some(Declaration {
        id: name.to_string(),
        name: name.to_string(),
        source,
        source_value,
        manifest_json: serde_json::to_string(&manifest).ok()?,
    })
}

impl McpManager {
    /// Project the config file's declarations onto the store's runtime registry.
    ///
    /// The file is authoritative: what it declares is what the runtime has. This
    /// is deliberately not merged with store edits — a server edited only in the
    /// old UI (there is no such path in this build) would be removed on the next
    /// config load, which is the point of declaring the file the source of truth.
    pub fn sync_from_config(
        &self,
        store: &Store,
        config: &GobleConfig,
    ) -> Result<McpSyncOutcome> {
        let rows = store.list_mcp_servers()?;
        let existing: HashMap<String, (String, String, String)> = rows
            .into_iter()
            .map(|(id, _name, source, source_value, manifest, _, _, _, _, _)| {
                (id, (source, source_value.unwrap_or_default(), manifest))
            })
            .collect();

        let mut outcome = McpSyncOutcome::default();
        let mut declared: HashMap<String, Declaration> = HashMap::new();
        for (name, server) in &config.mcp_servers {
            // `enabled = false` keeps the declaration in the file but takes the
            // server out of the runtime registry: the file still says what it
            // is, the app just does not serve it.
            if !server.enabled {
                continue;
            }
            let Some(declaration) = declaration_for(name, server) else {
                log::warn!("[mcp_servers.{name}] declares no url or command; skipped");
                continue;
            };
            declared.insert(declaration.id.clone(), declaration);
        }

        let now = Utc::now().to_rfc3339();
        for (id, declaration) in &declared {
            let matches = existing.get(id).is_some_and(|(source, source_value, manifest)| {
                source == &declaration.source
                    && source_value == declaration.source_value.as_deref().unwrap_or_default()
                    && manifest == &declaration.manifest_json
            });
            if matches {
                outcome.unchanged.push(declaration.name.clone());
                continue;
            }
            let is_installed = !existing.contains_key(id);
            let installed_at = now.clone();
            store.insert_mcp_server(
                id,
                &declaration.name,
                &declaration.source,
                declaration.source_value.as_deref(),
                &declaration.manifest_json,
                None,
                "[]",
                "[]",
                &installed_at,
                &now,
            )?;
            if is_installed {
                outcome.added.push(declaration.name.clone());
            } else {
                outcome.updated.push(declaration.name.clone());
            }
        }

        for id in existing.keys() {
            if declared.contains_key(id) {
                continue;
            }
            // A row that is not declared by the file is not part of the runtime
            // registry: the file is the declaration of record. (A disabled
            // declaration is absent from `declared`, so it lands here too.)
            store.delete_mcp_server(id)?;
            outcome.removed.push(id.to_string());
        }

        if outcome.changed() {
            log::info!("{}", outcome.line());
        }
        Ok(outcome)
    }

    /// Really initialize a declared MCP server and list its tools. A URL server
    /// is reported honestly as not testable over this path yet (the SSE client is
    /// async and lives in the service); a stdio server is spawned, initialized
    /// and asked for `tools/list` under [`MCP_PROBE_TIMEOUT`].
    pub fn probe_declaration(
        &self,
        server: &McpServerConfig,
        timeout: Duration,
    ) -> Result<Vec<String>, String> {
        let Some(command) = server
            .command
            .as_deref()
            .map(str::trim)
            .filter(|command| !command.is_empty())
        else {
            if let Some(url) = server.url.as_deref().filter(|url| !url.trim().is_empty()) {
                return Err(format!(
                    "the HTTP transport ({url}) is tested through the runtime registry; \
                     no stdio process to initialize"
                ));
            }
            return Err("the server declares no command or url".to_string());
        };
        let args = server.args.clone();
        let env: HashMap<String, String> = server
            .env
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect();
        probe_stdio(command, &args, env, timeout)
    }
}

/// Spawn a stdio MCP server, `initialize` it and list its tools, with a
/// wall-clock timeout. The spawn is blocking (`McpClient` is synchronous), so it
/// runs on its own thread and the caller's timeout is enforced by a channel.
pub fn probe_stdio(
    command: &str,
    args: &[String],
    env: HashMap<String, String>,
    timeout: Duration,
) -> Result<Vec<String>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let command = command.to_string();
    let timeout_message = command.clone();
    let args = args.to_vec();
    std::thread::Builder::new()
        .name("goble-mcp-probe".to_string())
        .spawn(move || {
            let result = (|| -> Result<Vec<String>, String> {
                let client = McpClient::spawn_owned(&command, &args, env)
                    .map_err(|error| format!("spawn {command}: {error}"))?;
                client
                    .initialize()
                    .map_err(|error| format!("initialize: {error}"))?;
                let tools = client
                    .list_tools()
                    .map_err(|error| format!("tools/list: {error}"))?;
                Ok(tools.into_iter().map(|tool| tool.name).collect())
            })();
            let _ = tx.send(result);
        })
        .map_err(|error| format!("spawn probe thread: {error}"))?;
    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(_) => Err(format!(
            "{timeout_message} did not answer within {}s",
            timeout.as_secs()
        )),
    }
}

/// What a one-time MCP import did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum McpImportOutcome {
    Imported { names: Vec<String> },
    AlreadyImported,
    FileHasServers,
    NothingToImport,
}

impl McpImportOutcome {
    /// The log line.
    pub fn line(&self) -> String {
        match self {
            McpImportOutcome::Imported { names } => {
                format!("imported {} MCP server(s) into config.toml", names.len())
            }
            McpImportOutcome::AlreadyImported => "mcp declarations already imported".to_string(),
            McpImportOutcome::FileHasServers => {
                "config.toml already declares [mcp_servers]; the store rows were left alone"
                    .to_string()
            }
            McpImportOutcome::NothingToImport => "no MCP store rows to import".to_string(),
        }
    }
}

/// Whether the one-time MCP import has run.
pub fn mcp_import_done(store: &Store) -> bool {
    store
        .get_setting(MCP_IMPORT_MARKER)
        .ok()
        .flatten()
        .as_deref()
        == Some("1")
}

/// Write a `[mcp_servers.<name>]` table into `config.toml` for each store row
/// that predates the file, once. The rows are not deleted: a rollback to a build
/// that reads them still finds them.
pub fn import_store_mcp_once(
    editor: &ConfigEditor,
    store: &Store,
) -> Result<McpImportOutcome, ConfigError> {
    if mcp_import_done(store) {
        return Ok(McpImportOutcome::AlreadyImported);
    }
    let document = editor.load(ConfigFile::Config)?;
    if document.contains(&["mcp_servers".to_string()]) {
        store
            .set_setting(MCP_IMPORT_MARKER, "1")
            .map_err(|error| ConfigError::Io(ConfigFile::Config, format!("store: {error}")))?;
        return Ok(McpImportOutcome::FileHasServers);
    }
    let rows = store
        .list_mcp_servers()
        .map_err(|error| ConfigError::Io(ConfigFile::Config, format!("store: {error}")))?;
    let mut ops = Vec::new();
    let mut names = Vec::new();
    for (id, _name, source, source_value, _manifest, _credentials, _secrets, _tools, _installed, _updated) in rows {
        let Some(source_value) = source_value.filter(|value| !value.trim().is_empty()) else {
            continue;
        };
        let entries = match source.as_str() {
            "url" | "sse" => vec![("url".to_string(), ConfigValue::Str(source_value))],
            "npm" | "stdio" => vec![
                ("command".to_string(), ConfigValue::Str("npx".to_string())),
                (
                    "args".to_string(),
                    ConfigValue::StrArray(vec!["-y".to_string(), source_value]),
                ),
            ],
            "local" => vec![("command".to_string(), ConfigValue::Str(source_value))],
            _ => continue,
        };
        names.push(id.clone());
        ops.push(ConfigOp::set(
            &["mcp_servers", id.as_str()],
            ConfigValue::Table(entries),
        ));
    }
    if ops.is_empty() {
        return Ok(McpImportOutcome::NothingToImport);
    }
    let expected = document.revision().clone();
    editor.edit(ConfigFile::Config, Some(&expected), |doc| {
        crate::config::schema::apply_ops(doc, &ops)
    })?;
    store
        .set_setting(MCP_IMPORT_MARKER, "1")
        .map_err(|error| ConfigError::Io(ConfigFile::Config, format!("store: {error}")))?;
    log::info!("{}", McpImportOutcome::Imported { names: names.clone() }.line());
    Ok(McpImportOutcome::Imported { names })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GobleConfig;

    fn config(text: &str) -> GobleConfig {
        let (config, problems) = GobleConfig::load_toml(text).expect("valid TOML");
        assert!(problems.is_empty(), "{problems:?}");
        config
    }

    fn store_ids(store: &Store) -> Vec<String> {
        let mut ids: Vec<String> = store
            .list_mcp_servers()
            .unwrap()
            .into_iter()
            .map(|row| row.0)
            .collect();
        ids.sort();
        ids
    }

    const DECLARED: &str = r#"
[mcp_servers.penpot]
url = "http://localhost:4400/mcp"

[mcp_servers.files]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
enabled = true
"#;

    #[test]
    fn a_declaration_syncs_into_the_runtime_registry() {
        let store = Store::open_in_memory().unwrap();
        let manager = McpManager::new();
        let outcome = manager
            .sync_from_config(&store, &config(DECLARED))
            .expect("sync");
        assert_eq!(outcome.added.len(), 2, "{outcome:?}");
        assert!(outcome.added.contains(&"penpot".to_string()));
        assert!(outcome.added.contains(&"files".to_string()));
        assert_eq!(store_ids(&store), ["files", "penpot"]);

        let rows = store.list_mcp_servers().unwrap();
        let penpot = rows.iter().find(|row| row.0 == "penpot").unwrap();
        assert_eq!(penpot.2, "url");
        assert_eq!(penpot.3.as_deref(), Some("http://localhost:4400/mcp"));
        let files = rows.iter().find(|row| row.0 == "files").unwrap();
        assert_eq!(files.2, "local");
        let manifest: McpManifest = serde_json::from_str(&files.4).unwrap();
        assert_eq!(
            manifest.runtime,
            McpRuntime::Binary {
                command: "npx".to_string(),
                args: vec![
                    "-y".to_string(),
                    "@modelcontextprotocol/server-filesystem".to_string(),
                    "/tmp".to_string()
                ],
            }
        );

        // A second sync of the same file is a no-op.
        let again = manager
            .sync_from_config(&store, &config(DECLARED))
            .expect("second sync");
        assert!(!again.changed(), "{again:?}");
        assert_eq!(again.unchanged.len(), 2, "{again:?}");
    }

    #[test]
    fn disabling_a_declaration_removes_it_from_the_runtime_registry() {
        let store = Store::open_in_memory().unwrap();
        let manager = McpManager::new();
        manager
            .sync_from_config(&store, &config(DECLARED))
            .expect("sync");
        assert_eq!(store_ids(&store), ["files", "penpot"]);

        let disabled = config(
            r#"
[mcp_servers.penpot]
url = "http://localhost:4400/mcp"
enabled = false

[mcp_servers.files]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
"#,
        );
        let outcome = manager.sync_from_config(&store, &disabled).expect("sync");
        assert!(outcome.removed.contains(&"penpot".to_string()), "{outcome:?}");
        assert_eq!(store_ids(&store), ["files"]);
    }

    #[test]
    fn editing_a_declaration_updates_the_runtime_row() {
        let store = Store::open_in_memory().unwrap();
        let manager = McpManager::new();
        manager
            .sync_from_config(&store, &config(DECLARED))
            .expect("sync");

        let edited = config(
            r#"
[mcp_servers.penpot]
url = "http://localhost:5500/mcp"
"#,
        );
        let outcome = manager.sync_from_config(&store, &edited).expect("sync");
        assert!(outcome.updated.contains(&"penpot".to_string()), "{outcome:?}");
        assert!(outcome.removed.contains(&"files".to_string()), "{outcome:?}");
        let rows = store.list_mcp_servers().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].3.as_deref(), Some("http://localhost:5500/mcp"));
    }

    #[test]
    fn a_store_row_with_no_declaration_is_removed() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_mcp_server(
                "old",
                "old",
                "url",
                Some("http://old.example/mcp"),
                "{}",
                None,
                "[]",
                "[]",
                "now",
                "now",
            )
            .unwrap();
        let manager = McpManager::new();
        let outcome = manager
            .sync_from_config(&store, &config(""))
            .expect("sync");
        assert!(outcome.removed.contains(&"old".to_string()), "{outcome:?}");
        assert!(store_ids(&store).is_empty());
    }

    #[test]
    fn the_reverse_import_writes_tables_once() {
        let dir = std::env::temp_dir().join(format!(
            "goble-mcp-import-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), "[theme]\ndark = true\n").unwrap();
        let editor = ConfigEditor::new(dir.clone());
        let store = Store::open_in_memory().unwrap();
        store
            .insert_mcp_server(
                "penpot",
                "penpot",
                "url",
                Some("http://localhost:4400/mcp"),
                "{}",
                None,
                "[]",
                "[]",
                "now",
                "now",
            )
            .unwrap();

        let outcome = import_store_mcp_once(&editor, &store).expect("import");
        assert_eq!(
            outcome,
            McpImportOutcome::Imported {
                names: vec!["penpot".to_string()]
            }
        );
        let text = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(text.contains("[mcp_servers.penpot]"), "{text}");
        assert!(text.contains(r#"url = "http://localhost:4400/mcp""#), "{text}");
        assert!(mcp_import_done(&store));

        let second = import_store_mcp_once(&editor, &store).expect("second");
        assert_eq!(second, McpImportOutcome::AlreadyImported);
        assert_eq!(
            std::fs::read_to_string(dir.join("config.toml")).unwrap(),
            text
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_declared_stdio_server_really_answers_initialize_and_tools_list() {
        use std::os::unix::fs::PermissionsExt;

        let script = r#"#!/bin/sh
read -r line
id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"mock","version":"1"}}}\n' "$id"
read -r line
id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"echo","description":"echo","inputSchema":{"type":"object","properties":{}}}]}}\n' "$id"
"#;
        let dir = std::env::temp_dir().join(format!(
            "goble-mcp-probe-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let server = dir.join("mock-mcp-server");
        std::fs::write(&server, script).unwrap();
        std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o755)).unwrap();

        let manager = McpManager::new();
        let declaration = McpServerConfig {
            command: Some(server.display().to_string()),
            ..McpServerConfig::default()
        };
        let tools = manager
            .probe_declaration(&declaration, Duration::from_secs(10))
            .expect("initialize + tools/list");
        assert_eq!(tools, ["echo"]);
    }
}
