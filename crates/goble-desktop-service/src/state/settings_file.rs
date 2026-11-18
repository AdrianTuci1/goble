//! The file-backed settings surface the Settings tab talks to (Lane B2).
//!
//! Settings are read and written **directly in the settings files**
//! (`~/.goble/config.toml`, `environment.toml`, `hosts.toml`); the file is the
//! source of truth and there is no settings database. The core mechanics —
//! comment/unknown-key-preserving document edits, atomic `0600` writes, an
//! advisory file lock, content-hash revisions and conflict fencing — live in
//! `goble_core::config` (Lane B1). This module is the service face of them: the
//! one place the app calls, holding a [`ConfigEditor`] built from the pinned
//! config path, refreshing the in-memory typed config, syncing `[mcp_servers.*]`
//! declarations into the runtime registry and emitting the change events.
//!
//! Every mutation takes the revision the caller rendered from
//! ([`ConfigRevision`]) and returns the fresh **redacted** [`ConfigView`], so the
//! UI never holds a secret and never writes a whole section from a redacted
//! copy. A stale revision is [`ConfigError::Conflict`] and nothing is written; a
//! file that is not TOML is [`ConfigError::RefusedUnparseable`] and is never
//! overwritten.
//!
//! Events emitted on the app's event bus:
//! - `settings:changed` — `SettingsChangedEvent { file, revision }`, after every
//!   write and after an external change is noticed;
//! - `settings:file-changed` — `goble_core::config::ConfigChange`, for an
//!   external edit the watcher saw;
//! - `models:updated`, `mcp:updated`, `harnesses:updated` — empty payloads, for
//!   the lists beside the settings tab.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use goble_core::config::{
    host_views, redacted_view, view_for, ConfigChange, ConfigDocument, ConfigEditor, ConfigError,
    ConfigFile, ConfigOp, ConfigProblem, ConfigRevision, ConfigValue, ConfigView, FieldError,
    HostView, McpView, ModelView,
};
use goble_core::harness::{
    harness_by_id, status, status_with_version, HarnessStatus, HARNESSES,
};
use goble_core::hosts::HostEntry;
use goble_core::llm::{probe_model, ProbeReport};
use goble_core::mcp_manager::{import_store_mcp_once, MCP_PROBE_TIMEOUT};

use super::DesktopState;

/// The event a write (or an external edit) publishes: which file and the new
/// revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsChangedEvent {
    pub file: ConfigFile,
    pub revision: ConfigRevision,
}

/// A model edit the Add/Edit provider form submits. `None` leaves the field as
/// it is; for string fields `Some("")` removes the key (the file's default);
/// the key fields are write-only — the view only ever says whether one is set.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelDraft {
    pub name: Option<String>,
    /// `[model.<slug>] model` — empty removes it so the slug is the id.
    pub model_id: Option<String>,
    pub base_url: Option<String>,
    /// `Some("")` clears the inline key; `Some(key)` replaces it; `None` keeps it.
    pub api_key: Option<String>,
    /// `Some("")` clears the env-var name; `Some(name)` sets it; `None` keeps it.
    pub api_key_env: Option<String>,
    pub max_completion_tokens: Option<u32>,
    pub context_window: Option<u64>,
    pub hidden: Option<bool>,
    /// Also write `[models] default = slug`.
    pub set_default: bool,
}

/// A model probe against data the user typed but has not saved yet (the Add
/// dialog's "Test connection").
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelProbeDraft {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub provider_hint: Option<String>,
}

/// A `[harness.<id>]` override edit. `None` keeps the current value; an empty
/// `command` removes the override.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HarnessDraft {
    pub enabled: Option<bool>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<BTreeMap<String, String>>,
}

/// A connector transport, as the Add/Edit connector form submits it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpTransportDraft {
    Http { url: String },
    Stdio {
        command: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
    },
}

/// A `[mcp_servers.<name>]` declaration edit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpDraft {
    pub transport: McpTransportDraft,
    pub enabled: bool,
}

/// A saved-host edit (`hosts.toml`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HostDraft {
    pub name: String,
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity: Option<String>,
}

/// The running external-edit watcher. Dropping it stops the consumer thread and
/// with it the `notify` watcher.
pub(super) struct SettingsWatch {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for SettingsWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Run a future on the app's entered tokio runtime, the same way the existing
/// MCP/vault calls do (the app's main thread enters a multi-thread runtime for
/// its lifetime, see `apps/desktop/src/main.rs`).
fn block_on<F: std::future::Future>(future: F) -> Result<F::Output, String> {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => Ok(handle.block_on(future)),
        Err(error) => Err(format!("no tokio runtime: {error}")),
    }
}

fn invalid(path: &[&str], message: impl Into<String>) -> ConfigError {
    ConfigError::Invalid(vec![FieldError::new(path, message)])
}

fn no_config_path(file: ConfigFile) -> ConfigError {
    ConfigError::Io(
        file,
        "this state has no config path: pin one with `set_config_path`".to_string(),
    )
}

fn clean(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

/// Push a `Set` for a present non-empty string, an `Unset` for an empty one, and
/// nothing when the draft leaves the field alone.
fn set_or_unset(ops: &mut Vec<ConfigOp>, path: &[&str], value: &Option<String>) {
    match value.as_deref().map(str::trim) {
        None => {}
        Some("") => ops.push(ConfigOp::unset(path)),
        Some(text) => ops.push(ConfigOp::set(path, ConfigValue::Str(text.to_string()))),
    }
}

fn string_table(env: &BTreeMap<String, String>) -> ConfigValue {
    ConfigValue::Table(
        env.iter()
            .map(|(key, value)| (key.clone(), ConfigValue::Str(value.clone())))
            .collect(),
    )
}

impl DesktopState {
    /// The editor for the pinned config path (and its siblings). Built lazily
    /// from `config_path`, rebuilt if the path is re-pinned.
    pub(super) fn settings_editor(&self) -> Option<Arc<ConfigEditor>> {
        let path = self.config_path()?;
        let mut slot = self.settings_editor.lock();
        if let Some(existing) = slot.as_ref() {
            if existing.path(ConfigFile::Config) == path {
                return Some(Arc::clone(existing));
            }
        }
        let root = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let editor = Arc::new(ConfigEditor::with_config_path(root, path));
        *slot = Some(Arc::clone(&editor));
        Some(editor)
    }

    fn require_editor(&self, file: ConfigFile) -> Result<Arc<ConfigEditor>, ConfigError> {
        self.settings_editor().ok_or_else(|| no_config_path(file))
    }

    /// The settings projection the Settings tab renders: `config.toml` with its
    /// environment and hosts sections. Reads are cheap and synchronous (three
    /// small files); a missing file gives `mode: "missing"` and the
    /// empty-content revision, and a file that does not parse gives a view with
    /// the parse problem in `problems` rather than an error.
    pub fn settings_document(&self) -> ConfigView {
        self.settings_document_for(ConfigFile::Config)
    }

    /// [`Self::settings_document`] for one of the three files.
    pub fn settings_document_for(&self, file: ConfigFile) -> ConfigView {
        let Some(editor) = self.settings_editor() else {
            return ConfigView::empty(file, Path::new(""), ConfigRevision::missing());
        };
        match editor.load(file) {
            Ok(document) => self
                .view_from_document(file, &document, Some(&editor))
                .unwrap_or_else(|_| {
                    ConfigView::empty(file, &editor.path(file), ConfigRevision::missing())
                }),
            Err(error) => {
                let path = editor.path(file);
                let mut view =
                    ConfigView::empty(file, &path, ConfigRevision::missing());
                view.mode = file_mode(&path);
                view.problems.push(ConfigProblem::new("", error.to_string()));
                view
            }
        }
    }

    /// The path Settings -> Advanced names for a file (also what "Open config
    /// file" opens in a pane).
    pub fn settings_path(&self, file: ConfigFile) -> Option<PathBuf> {
        self.settings_editor().map(|editor| editor.path(file))
    }

    /// [`Self::settings_path`], named for the Advanced page's action.
    pub fn open_settings_file(&self, file: ConfigFile) -> Option<PathBuf> {
        self.settings_path(file)
    }

    /// Re-read one file and refresh the caches built from it.
    pub fn reload_settings_file(&self, file: ConfigFile) -> Result<ConfigView, ConfigError> {
        let editor = self.require_editor(file)?;
        let document = editor.load(file)?;
        let view = self.view_from_document(file, &document, Some(&editor))?;
        self.after_read(file, &document, &view);
        Ok(view)
    }

    /// Apply a path-op batch in one atomic write. A stale `expected` revision is
    /// `ConfigError::Conflict` (nothing written); an edit that fails validation is
    /// `ConfigError::Invalid`; an unparseable file is refused. The returned view
    /// is the fresh, redacted one — hand its revision to the next op.
    pub fn apply_setting_ops(
        &self,
        file: ConfigFile,
        ops: &[ConfigOp],
        expected: Option<&ConfigRevision>,
    ) -> Result<ConfigView, ConfigError> {
        let editor = self.require_editor(file)?;
        let outcome = editor.apply_ops(file, ops, expected)?;
        let view = self.view_from_document(file, &outcome.document, Some(&editor))?;
        if outcome.written {
            self.after_write(file, &outcome.document, &view);
        }
        Ok(view)
    }

    // --- models -----------------------------------------------------------

    /// The configured `[model.<slug>]` rows, redacted.
    pub fn list_config_models(&self) -> Vec<ModelView> {
        self.settings_document().sections.models
    }

    /// Create or edit one `[model.<slug>]` table with a path-op batch (the
    /// masked key is never deleted by a save that did not type it).
    pub fn upsert_config_model(
        &self,
        slug: &str,
        draft: &ModelDraft,
        expected: Option<&ConfigRevision>,
    ) -> Result<ConfigView, ConfigError> {
        let slug = slug.trim();
        if slug.is_empty() {
            return Err(invalid(&["model"], "a model needs a slug"));
        }
        let mut ops = Vec::new();
        set_or_unset(&mut ops, &["model", slug, "model"], &draft.model_id);
        set_or_unset(&mut ops, &["model", slug, "name"], &draft.name);
        set_or_unset(&mut ops, &["model", slug, "base_url"], &draft.base_url);
        set_or_unset(&mut ops, &["model", slug, "api_key"], &draft.api_key);
        set_or_unset(&mut ops, &["model", slug, "api_key_env"], &draft.api_key_env);
        if let Some(tokens) = draft.max_completion_tokens {
            ops.push(ConfigOp::set(
                &["model", slug, "max_completion_tokens"],
                ConfigValue::Int(tokens as i64),
            ));
        }
        if let Some(window) = draft.context_window {
            ops.push(ConfigOp::set(
                &["model", slug, "context_window"],
                ConfigValue::Int(window as i64),
            ));
        }
        if let Some(hidden) = draft.hidden {
            ops.push(ConfigOp::set(
                &["model", slug, "hidden"],
                ConfigValue::Bool(hidden),
            ));
        }
        if draft.set_default {
            ops.push(ConfigOp::set(
                &["models", "default"],
                ConfigValue::Str(slug.to_string()),
            ));
        }
        self.apply_setting_ops(ConfigFile::Config, &ops, expected)
    }

    /// Delete a model table; when it was the default, `[models] default` is
    /// cleared with it so nothing names a table that is gone.
    pub fn delete_config_model(
        &self,
        slug: &str,
        expected: Option<&ConfigRevision>,
    ) -> Result<ConfigView, ConfigError> {
        let slug = slug.trim();
        let mut ops = vec![ConfigOp::unset(&["model", slug])];
        if self.config().models.default.as_deref() == Some(slug) {
            ops.push(ConfigOp::unset(&["models", "default"]));
        }
        self.apply_setting_ops(ConfigFile::Config, &ops, expected)
    }

    /// Point `[models] default` at `slug`.
    pub fn set_default_model_config(
        &self,
        slug: &str,
        expected: Option<&ConfigRevision>,
    ) -> Result<ConfigView, ConfigError> {
        let slug = slug.trim();
        if slug.is_empty() {
            return Err(invalid(&["models", "default"], "the default model cannot be blank"));
        }
        self.apply_setting_ops(
            ConfigFile::Config,
            &[ConfigOp::set(
                &["models", "default"],
                ConfigValue::Str(slug.to_string()),
            )],
            expected,
        )
    }

    /// Really probe a configured model's endpoint (GET models per backend, 10 s).
    /// A rejected key/URL is a report with `ok: false`, status and body snippet;
    /// an unreachable endpoint is an error string.
    pub fn test_model_connection(&self, slug: &str) -> Result<ProbeReport, String> {
        let config = self.config();
        let Some(entry) = config.model.get(slug) else {
            return Err(format!("there is no [model.{slug}] entry in config.toml"));
        };
        let hint = provider_hint_for(slug);
        block_on(probe_model(entry, hint))?.map_err(|error| error.to_string())
    }

    /// Probe a not-yet-saved model (the Add dialog's Test connection).
    pub fn probe_model_draft(&self, draft: &ModelProbeDraft) -> Result<ProbeReport, String> {
        let entry = goble_core::config::ModelConfig {
            model: String::new(),
            name: None,
            base_url: clean(&draft.base_url),
            api_key: clean(&draft.api_key),
            api_key_env: clean(&draft.api_key_env),
            max_completion_tokens: None,
            context_window: None,
            hidden: None,
        };
        block_on(probe_model(&entry, draft.provider_hint.as_deref()))?
            .map_err(|error| error.to_string())
    }

    // --- harnesses --------------------------------------------------------

    /// Every catalog harness with the file's overrides applied (no `--version`
    /// probe: that is explicit, via [`Self::test_harness`]).
    pub fn list_harnesses(&self) -> Vec<HarnessStatus> {
        let config = self.config();
        HARNESSES
            .iter()
            .map(|spec| status(spec, config.harness.get(spec.id)))
            .collect()
    }

    /// Run `<command> --version` (5 s) for one harness.
    pub fn test_harness(&self, id: &str) -> Result<HarnessStatus, String> {
        let Some(spec) = harness_by_id(id) else {
            return Err(format!("{id} is not a harness in the catalog"));
        };
        let config = self.config();
        block_on(status_with_version(spec, config.harness.get(id)))
    }

    /// Write one `[harness.<id>]` override table.
    pub fn upsert_harness(
        &self,
        id: &str,
        draft: &HarnessDraft,
        expected: Option<&ConfigRevision>,
    ) -> Result<ConfigView, ConfigError> {
        let id = id.trim();
        if id.is_empty() {
            return Err(invalid(&["harness"], "a harness needs an id"));
        }
        let mut ops = Vec::new();
        if let Some(enabled) = draft.enabled {
            ops.push(ConfigOp::set(
                &["harness", id, "enabled"],
                ConfigValue::Bool(enabled),
            ));
        }
        set_or_unset(&mut ops, &["harness", id, "command"], &draft.command);
        if let Some(args) = &draft.args {
            ops.push(ConfigOp::set(
                &["harness", id, "args"],
                ConfigValue::StrArray(args.clone()),
            ));
        }
        if let Some(env) = &draft.env {
            ops.push(ConfigOp::set(&["harness", id, "env"], string_table(env)));
        }
        let view = self.apply_setting_ops(ConfigFile::Config, &ops, expected)?;
        self.emit("harnesses:updated", serde_json::json!({}));
        Ok(view)
    }

    // --- MCP connectors ---------------------------------------------------

    /// The `[mcp_servers.<name>]` declarations of record.
    pub fn list_mcp_config(&self) -> Vec<McpView> {
        self.settings_document().sections.mcp
    }

    /// Write one declaration and sync it into the runtime registry (the store's
    /// rows are derived from the file).
    pub fn upsert_mcp_config(
        &self,
        name: &str,
        draft: &McpDraft,
        expected: Option<&ConfigRevision>,
    ) -> Result<ConfigView, ConfigError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(invalid(&["mcp_servers"], "an MCP server needs a name"));
        }
        let mut ops = Vec::new();
        match &draft.transport {
            McpTransportDraft::Http { url } => {
                let url = url.trim();
                if url.is_empty() {
                    return Err(invalid(
                        &["mcp_servers", name, "url"],
                        "an HTTP connector needs a URL",
                    ));
                }
                ops.push(ConfigOp::unset(&["mcp_servers", name, "command"]));
                ops.push(ConfigOp::unset(&["mcp_servers", name, "args"]));
                ops.push(ConfigOp::unset(&["mcp_servers", name, "env"]));
                ops.push(ConfigOp::set(
                    &["mcp_servers", name, "url"],
                    ConfigValue::Str(url.to_string()),
                ));
            }
            McpTransportDraft::Stdio {
                command,
                args,
                env,
            } => {
                let command = command.trim();
                if command.is_empty() {
                    return Err(invalid(
                        &["mcp_servers", name, "command"],
                        "a stdio connector needs a command",
                    ));
                }
                ops.push(ConfigOp::unset(&["mcp_servers", name, "url"]));
                ops.push(ConfigOp::set(
                    &["mcp_servers", name, "command"],
                    ConfigValue::Str(command.to_string()),
                ));
                ops.push(ConfigOp::set(
                    &["mcp_servers", name, "args"],
                    ConfigValue::StrArray(args.clone()),
                ));
                if env.is_empty() {
                    ops.push(ConfigOp::unset(&["mcp_servers", name, "env"]));
                } else {
                    ops.push(ConfigOp::set(
                        &["mcp_servers", name, "env"],
                        string_table(env),
                    ));
                }
            }
        }
        ops.push(ConfigOp::set(
            &["mcp_servers", name, "enabled"],
            ConfigValue::Bool(draft.enabled),
        ));
        self.apply_setting_ops(ConfigFile::Config, &ops, expected)
    }

    /// Remove a declaration (and with it the runtime registry row).
    pub fn delete_mcp_config(
        &self,
        name: &str,
        expected: Option<&ConfigRevision>,
    ) -> Result<ConfigView, ConfigError> {
        let view = self.apply_setting_ops(
            ConfigFile::Config,
            &[ConfigOp::unset(&["mcp_servers", name])],
            expected,
        )?;
        self.emit("mcp:updated", serde_json::json!({}));
        Ok(view)
    }

    /// Really initialize a declared stdio connector and list its tools (10 s).
    pub fn test_mcp_config(&self, name: &str) -> Result<Vec<String>, String> {
        let config = self.config();
        let Some(server) = config.mcp_servers.get(name) else {
            return Err(format!("there is no [mcp_servers.{name}] declaration"));
        };
        self.mcp_manager.probe_declaration(server, MCP_PROBE_TIMEOUT)
    }

    // --- hosts ------------------------------------------------------------

    /// The saved hosts of `hosts.toml`.
    pub fn list_saved_hosts(&self) -> Vec<HostView> {
        let Some(editor) = self.settings_editor() else {
            return Vec::new();
        };
        match editor.load(ConfigFile::Hosts) {
            Ok(document) => document
                .typed_hosts()
                .map(|hosts| host_views(&hosts))
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    /// Create (`original = None`) or edit one saved host through the editor (the
    /// lock, revision fence, header and `0600` mode are the shared ones).
    pub fn upsert_host(
        &self,
        original: Option<&str>,
        draft: &HostDraft,
        expected: Option<&ConfigRevision>,
    ) -> Result<ConfigView, ConfigError> {
        let editor = self.require_editor(ConfigFile::Hosts)?;
        let document = editor.load(ConfigFile::Hosts)?;
        let revision = checked_revision(expected, &document)?;
        let mut hosts = document.typed_hosts()?;
        let name = draft.name.trim().to_string();
        let host = draft.host.trim().to_string();
        if name.is_empty() {
            return Err(invalid(&["hosts"], "a saved host needs a name"));
        }
        if host.is_empty() {
            return Err(invalid(&["hosts", &name], "a saved host needs an address"));
        }
        let entry = HostEntry {
            name: name.clone(),
            host,
            user: clean(&draft.user),
            port: draft.port,
            identity: clean(&draft.identity),
        };
        if let Some(original) = original {
            if let Some(slot) = hosts.hosts.iter_mut().find(|host| host.name == original) {
                *slot = entry;
            } else {
                hosts.hosts.push(entry);
            }
        } else {
            if hosts.hosts.iter().any(|host| host.name == name) {
                return Err(invalid(
                    &["hosts", &name],
                    "a saved host already has this name",
                ));
            }
            hosts.hosts.push(entry);
        }
        editor.save_hosts(&hosts, Some(&revision))?;
        let view = self.settings_document_for(ConfigFile::Hosts);
        self.emit(
            "settings:changed",
            SettingsChangedEvent {
                file: ConfigFile::Hosts,
                revision: view.revision.clone(),
            },
        );
        Ok(view)
    }

    /// Delete one saved host.
    pub fn delete_host(
        &self,
        name: &str,
        expected: Option<&ConfigRevision>,
    ) -> Result<ConfigView, ConfigError> {
        let editor = self.require_editor(ConfigFile::Hosts)?;
        let document = editor.load(ConfigFile::Hosts)?;
        let revision = checked_revision(expected, &document)?;
        let mut hosts = document.typed_hosts()?;
        hosts.hosts.retain(|host| host.name != name);
        editor.save_hosts(&hosts, Some(&revision))?;
        let view = self.settings_document_for(ConfigFile::Hosts);
        self.emit(
            "settings:changed",
            SettingsChangedEvent {
                file: ConfigFile::Hosts,
                revision: view.revision.clone(),
            },
        );
        Ok(view)
    }

    /// `ssh -o BatchMode=yes -o ConnectTimeout=5 <user@host> true` (8 s), so the
    /// row reports reachability or ssh's own error line.
    pub fn test_host(&self, name: &str) -> Result<String, String> {
        let Some(editor) = self.settings_editor() else {
            return Err("this state has no config path".to_string());
        };
        let document = editor
            .load(ConfigFile::Hosts)
            .map_err(|error| error.to_string())?;
        let hosts = document.typed_hosts().map_err(|error| error.to_string())?;
        let Some(host) = hosts.valid_hosts().into_iter().find(|host| host.name == name) else {
            return Err(format!("there is no saved host named {name}"));
        };
        let mut command = tokio::process::Command::new("ssh");
        command
            .arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("ConnectTimeout=5")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if let Some(port) = host.port {
            command.arg("-p").arg(port.to_string());
        }
        if let Some(identity) = host
            .identity
            .as_deref()
            .map(str::trim)
            .filter(|identity| !identity.is_empty())
        {
            command.arg("-i").arg(expand_tilde(identity));
        }
        if let Some(user) = host
            .user
            .as_deref()
            .map(str::trim)
            .filter(|user| !user.is_empty())
        {
            command.arg("-l").arg(user);
        }
        command.arg(&host.host).arg("true");
        let output = block_on(async move {
            tokio::time::timeout(Duration::from_secs(8), command.output())
                .await
                .map_err(|_| format!("ssh {} timed out after 8s", host.host))?
                .map_err(|error| format!("run ssh: {error}"))
        })??;
        if output.status.success() {
            Ok("reachable".to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let line = stderr
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("ssh exited non-zero");
            Err(line.to_string())
        }
    }

    // --- bootstrap / watcher ---------------------------------------------

    /// Startup work for the settings files, called by `open_default`: tighten
    /// the files to `0600`, run the one-time store imports, and sync the
    /// declarations into the runtime registry.
    pub fn bootstrap_settings_files(&self) {
        let Some(editor) = self.settings_editor() else {
            return;
        };
        #[cfg(unix)]
        for file in ConfigFile::ALL {
            match editor.tighten_permissions(file) {
                Ok(true) => log::info!("{}: tightened to 0600", editor.path(file).display()),
                Ok(false) => {}
                Err(error) => log::warn!(
                    "{}: tighten permissions failed: {error}",
                    editor.path(file).display()
                ),
            }
        }
        {
            let store = self.store.lock();
            match goble_core::config::import_store_prefs_once(&editor, &store) {
                Ok(goble_core::config::ImportOutcome::Imported { .. })
                | Ok(goble_core::config::ImportOutcome::AlreadyImported)
                | Ok(goble_core::config::ImportOutcome::FileHasApp)
                | Ok(goble_core::config::ImportOutcome::NothingToImport) => {}
                Err(error) => {
                    log::warn!("app-settings import skipped: {error}");
                    self.emit("settings:error", serde_json::json!({ "message": error.to_string() }));
                }
            }
            match import_store_mcp_once(&editor, &store) {
                Ok(_) => {}
                Err(error) => {
                    log::warn!("mcp declaration import skipped: {error}");
                    self.emit("settings:error", serde_json::json!({ "message": error.to_string() }));
                }
            }
        }
        // The imports may have written `[app]` and `[mcp_servers.*]`; fold the
        // file back into the in-memory config before the sync reads it.
        let _ = self.reload_config(&editor.path(ConfigFile::Config));
        self.sync_mcp_declarations(true);
        self.emit("models:updated", serde_json::json!({}));
        self.emit("harnesses:updated", serde_json::json!({}));
    }

    /// Start the external-edit watcher, which reloads the typed config and emits
    /// `settings:file-changed` when another editor writes a settings file. The
    /// watcher lives with the state; dropping the state stops it.
    pub fn start_settings_watcher(self: &Arc<Self>) {
        let Some(editor) = self.settings_editor() else {
            return;
        };
        if self.settings_watch.lock().is_some() {
            return;
        }
        let watcher = match goble_core::config::ConfigWatcher::start(Arc::clone(&editor)) {
            Ok(watcher) => watcher,
            Err(error) => {
                log::warn!("settings file watcher unavailable: {error}");
                return;
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let weak = Arc::downgrade(self);
        let thread = std::thread::Builder::new()
            .name("goble-settings-events".to_string())
            .spawn(move || {
                while !thread_stop.load(Ordering::SeqCst) {
                    let Some(change) = watcher.recv_timeout(Duration::from_millis(250)) else {
                        continue;
                    };
                    let Some(state) = weak.upgrade() else {
                        break;
                    };
                    state.on_external_settings_change(change);
                }
            })
            .ok();
        *self.settings_watch.lock() = Some(SettingsWatch { stop, thread });
    }

    fn on_external_settings_change(&self, change: ConfigChange) {
        let file = change.file;
        match file {
            ConfigFile::Config => {
                if let Some(editor) = self.settings_editor() {
                    match editor.load(ConfigFile::Config) {
                        Ok(document) => match document.typed_config() {
                            Ok(config) => {
                                *self.config.lock() = config;
                                self.sync_mcp_declarations(false);
                                self.emit("models:updated", serde_json::json!({}));
                                self.emit("mcp:updated", serde_json::json!({}));
                            }
                            Err(error) => log::warn!("reload config: {error}"),
                        },
                        Err(error) => log::warn!("reload config: {error}"),
                    }
                }
            }
            ConfigFile::Environment => {
                let _ = self.environment_groups();
            }
            ConfigFile::Hosts => {}
        }
        self.emit("settings:file-changed", change.clone());
        self.emit(
            "settings:changed",
            SettingsChangedEvent {
                file,
                revision: change.revision.clone(),
            },
        );
    }

    fn after_read(&self, file: ConfigFile, document: &ConfigDocument, _view: &ConfigView) {
        match file {
            ConfigFile::Config => {
                if let Ok(config) = document.typed_config() {
                    *self.config.lock() = config;
                }
                // Silent: this is a read. The read path announces nothing.
                self.sync_mcp_declarations(false);
            }
            ConfigFile::Environment => {
                let _ = self.environment_groups();
            }
            ConfigFile::Hosts => {}
        }
        // A plain read MUST NOT report itself as a change: the UI reloads on
        // `settings:changed`, so emitting here would make every read queue
        // another read (x3 files) and the next frame would drain an
        // ever-growing batch until the main thread never reaches a frame —
        // the 100% CPU / 1.2 GB event-storm regression. Only writes
        // (`after_write`) and external edits (`on_external_settings_change`)
        // announce a new revision; a read just refreshes the typed cache.
    }

    fn after_write(&self, file: ConfigFile, document: &ConfigDocument, view: &ConfigView) {
        match file {
            ConfigFile::Config => {
                if let Ok(config) = document.typed_config() {
                    *self.config.lock() = config;
                    self.sync_mcp_declarations(false);
                    self.emit("models:updated", serde_json::json!({}));
                    self.emit("mcp:updated", serde_json::json!({}));
                    self.emit("harnesses:updated", serde_json::json!({}));
                }
            }
            ConfigFile::Environment => {
                let _ = self.environment_groups();
            }
            ConfigFile::Hosts => {}
        }
        self.emit(
            "settings:changed",
            SettingsChangedEvent {
                file,
                revision: view.revision.clone(),
            },
        );
    }

    /// Project the file's declarations onto the runtime registry. `announce`
    /// decides whether a drive-by sync (a read, or a write that announces its
    /// own `mcp:updated`) is allowed to publish another event: a plain read must
    /// stay event-silent, or the UI's reload-on-change feeds itself.
    fn sync_mcp_declarations(&self, announce: bool) {
        let config = self.config();
        let outcome = {
            let store = self.store.lock();
            self.mcp_manager.sync_from_config(&store, &config)
        };
        match outcome {
            Ok(outcome) => {
                if outcome.changed() {
                    log::info!("{}", outcome.line());
                    if announce {
                        self.emit("mcp:updated", serde_json::json!({}));
                    }
                }
            }
            Err(error) => log::warn!("mcp declaration sync failed: {error}"),
        }
    }

    fn view_from_document(
        &self,
        file: ConfigFile,
        document: &ConfigDocument,
        editor: Option<&Arc<ConfigEditor>>,
    ) -> Result<ConfigView, ConfigError> {
        match file {
            ConfigFile::Config => {
                let environment = editor.and_then(|editor| editor.load(ConfigFile::Environment).ok());
                let hosts = editor.and_then(|editor| editor.load(ConfigFile::Hosts).ok());
                Ok(redacted_view(
                    document,
                    environment.as_ref(),
                    hosts.as_ref(),
                ))
            }
            other => Ok(view_for(other, document)),
        }
    }
}

/// The revision a typed whole-file save should require: the caller's, fenced
/// against the bytes just read, or the revision just read when the caller did
/// not render from one.
fn checked_revision(
    expected: Option<&ConfigRevision>,
    document: &ConfigDocument,
) -> Result<ConfigRevision, ConfigError> {
    match expected {
        Some(expected) if expected.hash != document.revision().hash => Err(ConfigError::Conflict {
            file: document.file(),
            expected: expected.clone(),
            actual: document.revision().clone(),
        }),
        Some(expected) => Ok(expected.clone()),
        None => Ok(document.revision().clone()),
    }
}

fn provider_hint_for(slug: &str) -> Option<&'static str> {
    let slug = slug.to_ascii_lowercase();
    for hint in ["anthropic", "openai", "deepseek", "openrouter", "ollama"] {
        if slug.contains(hint) {
            return Some(hint);
        }
    }
    None
}

fn expand_tilde(value: &str) -> String {
    let Some(rest) = value.strip_prefix("~/") else {
        return value.to_string();
    };
    match dirs::home_dir() {
        Some(home) => home.join(rest).display().to_string(),
        None => value.to_string(),
    }
}

/// The octal mode of a settings file, `missing` when there is no file, or
/// `unknown` when it cannot be read.
fn file_mode(path: &Path) -> String {
    match std::fs::metadata(path) {
        Ok(metadata) => goble_core::config::document::mode_string(&metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "missing".to_string(),
        Err(_) => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_core::config::{ConfigOp, ConfigValue};
    use goble_core::store::Store;

    /// A state over a throwaway directory with all three files pinned there, so
    /// no test can read or write the real `~/.goble`.
    fn state_at(name: &str) -> (tempfile::TempDir, Arc<DesktopState>) {
        let dir = tempfile::tempdir().expect("temp dir");
        let _ = name;
        let state = DesktopState::new(
            Store::open_in_memory().expect("store"),
            crate::thread_store::ThreadStore::new(dir.path()).expect("threads"),
        );
        state.set_config_path(dir.path().join("config.toml"));
        state.set_environment_path(dir.path().join("environment.toml"));
        (dir, state)
    }

    fn read(dir: &tempfile::TempDir, name: &str) -> String {
        std::fs::read_to_string(dir.path().join(name)).unwrap_or_default()
    }

    #[test]
    fn the_settings_document_honours_a_pinned_temp_path() {
        let (dir, state) = state_at("missing");
        let view = state.settings_document();
        assert_eq!(view.file, ConfigFile::Config);
        assert_eq!(view.mode, "missing");
        assert!(view.revision.is_missing());
        assert_eq!(
            view.path,
            dir.path().join("config.toml").display().to_string()
        );
        assert!(view.problems.is_empty());
        assert!(view.sections.models.is_empty());
    }

    #[test]
    fn applying_a_field_op_writes_the_file_and_returns_a_fresh_revision() {
        let (dir, state) = state_at("apply");
        std::fs::write(
            dir.path().join("config.toml"),
            "# my config\n\n[theme]\ndark = true\n",
        )
        .expect("seed");
        let before = state.settings_document();
        let after = state
            .apply_setting_ops(
                ConfigFile::Config,
                &[ConfigOp::set(
                    &["app", "vim_mode"],
                    ConfigValue::Bool(true),
                )],
                Some(&before.revision),
            )
            .expect("write");
        assert_eq!(after.sections.app.vim_mode, Some(true));
        assert_ne!(after.revision.hash, before.revision.hash);
        let text = read(&dir, "config.toml");
        assert!(text.contains("# my config"), "{text}");
        assert!(text.contains("vim_mode = true"), "{text}");

        // The fresh revision the write returned is the one the next op uses.
        let again = state
            .apply_setting_ops(
                ConfigFile::Config,
                &[ConfigOp::set(
                    &["app", "scroll_speed"],
                    ConfigValue::Int(60),
                )],
                Some(&after.revision),
            )
            .expect("second write");
        assert_eq!(again.sections.app.scroll_speed, Some(60));
        // The typed config the service serves is refreshed without a restart.
        assert_eq!(state.config().app.scroll_speed, Some(60));
    }

    #[test]
    fn a_stale_revision_conflicts_and_nothing_is_written() {
        let (dir, state) = state_at("conflict");
        std::fs::write(dir.path().join("config.toml"), "[theme]\ndark = true\n").expect("seed");
        let view = state.settings_document();
        let stale = view.revision.clone();
        let mut external = read(&dir, "config.toml");
        external.push_str("# edited while the page was open\n");
        std::fs::write(dir.path().join("config.toml"), &external).expect("external");

        let error = state
            .apply_setting_ops(
                ConfigFile::Config,
                &[ConfigOp::set(&["app", "vim_mode"], ConfigValue::Bool(true))],
                Some(&stale),
            )
            .expect_err("stale revision");
        assert!(error.is_conflict(), "{error:?}");
        assert_eq!(read(&dir, "config.toml"), external);
    }

    #[test]
    fn models_round_trip_through_the_service() {
        let (_dir, state) = state_at("models");
        let view = state
            .upsert_config_model(
                "kimi",
                &ModelDraft {
                    name: Some("Kimi K2".to_string()),
                    model_id: Some("kimi-k2".to_string()),
                    base_url: Some("https://api.kimi.com/coding/v1".to_string()),
                    api_key: Some("sk-kimi".to_string()),
                    max_completion_tokens: Some(8192),
                    set_default: true,
                    ..ModelDraft::default()
                },
                None,
            )
            .expect("write model");
        let model = view
            .sections
            .models
            .iter()
            .find(|model| model.slug == "kimi")
            .expect("listed");
        assert!(model.api_key.set, "the view says a key is configured");
        assert_eq!(model.api_key_source, goble_core::config::ApiKeySource::Inline);
        assert_eq!(view.sections.default_model.as_deref(), Some("kimi"));

        // An edit that does not touch the masked key keeps it.
        let view = state
            .upsert_config_model(
                "kimi",
                &ModelDraft {
                    name: Some("Kimi K2.5".to_string()),
                    ..ModelDraft::default()
                },
                Some(&view.revision),
            )
            .expect("edit");
        let model = &view.sections.models[0];
        assert_eq!(model.name.as_deref(), Some("Kimi K2.5"));
        assert!(model.api_key.set, "an untyped key survives an edit");

        // Deleting the model also clears the default that named it.
        let view = state
            .delete_config_model("kimi", Some(&view.revision))
            .expect("delete");
        assert!(view.sections.models.is_empty());
        assert_eq!(view.sections.default_model, None);
    }

    #[test]
    fn a_mcp_declaration_syncs_into_the_runtime_registry() {
        let (_dir, state) = state_at("mcp");
        let view = state
            .upsert_mcp_config(
                "penpot",
                &McpDraft {
                    transport: McpTransportDraft::Http {
                        url: "http://localhost:4400/mcp".to_string(),
                    },
                    enabled: true,
                },
                None,
            )
            .expect("declare");
        assert_eq!(view.sections.mcp.len(), 1);
        assert_eq!(view.sections.mcp[0].name, "penpot");
        assert_eq!(view.sections.mcp[0].transport(), "http");
        let rows = state.store.lock().list_mcp_servers().expect("rows");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].0, "penpot");

        // Disabling keeps the declaration but drops the runtime row.
        let view = state
            .upsert_mcp_config(
                "penpot",
                &McpDraft {
                    transport: McpTransportDraft::Http {
                        url: "http://localhost:4400/mcp".to_string(),
                    },
                    enabled: false,
                },
                Some(&view.revision),
            )
            .expect("disable");
        assert!(!view.sections.mcp[0].enabled);
        assert!(state.store.lock().list_mcp_servers().expect("rows").is_empty());

        state
            .delete_mcp_config("penpot", Some(&view.revision))
            .expect("delete");
        assert!(state.list_mcp_config().is_empty());
        assert!(state.test_mcp_config("penpot").is_err());
    }

    #[test]
    fn a_harness_override_is_written_and_listed() {
        let (dir, state) = state_at("harness");
        let view = state
            .upsert_harness(
                "codex",
                &HarnessDraft {
                    enabled: Some(false),
                    command: Some("/usr/local/bin/codex".to_string()),
                    args: Some(vec!["--full-auto".to_string()]),
                    env: None,
                },
                None,
            )
            .expect("write harness");
        assert_eq!(view.revision.hash.len(), 64);
        let text = read(&dir, "config.toml");
        assert!(text.contains("[harness.codex]"), "{text}");
        assert!(text.contains("command = \"/usr/local/bin/codex\""), "{text}");
        let codex = state
            .list_harnesses()
            .into_iter()
            .find(|harness| harness.id == "codex")
            .expect("catalog has codex");
        assert!(codex.configured);
        assert!(!codex.enabled);
        assert_eq!(codex.command, "/usr/local/bin/codex");
    }

    #[test]
    fn hosts_round_trip_through_the_editor() {
        let (dir, state) = state_at("hosts");
        let view = state
            .upsert_host(
                None,
                &HostDraft {
                    name: "prod".to_string(),
                    host: "prod.example.com".to_string(),
                    user: Some("deploy".to_string()),
                    port: Some(2222),
                    identity: Some("~/.ssh/id_prod".to_string()),
                },
                None,
            )
            .expect("add host");
        assert_eq!(view.sections.hosts.len(), 1);
        assert_eq!(view.sections.hosts[0].target_line(), "deploy@prod.example.com:2222");
        let text = read(&dir, "hosts.toml");
        assert!(text.starts_with("# Saved SSH hosts"), "{text}");
        assert!(text.contains("[[hosts]]"), "{text}");

        // Editing the existing entry (by its original name) replaces it.
        let view = state
            .upsert_host(
                Some("prod"),
                &HostDraft {
                    name: "prod".to_string(),
                    host: "prod2.example.com".to_string(),
                    port: Some(22),
                    ..HostDraft::default()
                },
                Some(&view.revision),
            )
            .expect("edit host");
        assert_eq!(view.sections.hosts[0].host, "prod2.example.com");

        let view = state
            .delete_host("prod", Some(&view.revision))
            .expect("delete host");
        assert!(view.sections.hosts.is_empty());
    }

    #[test]
    fn the_legacy_save_config_writes_through_the_editor() {
        let (dir, state) = state_at("save");
        let mut config = goble_core::config::GobleConfig::default();
        config.theme.dark = false;
        config.model.insert(
            "deepseek".to_string(),
            goble_core::config::ModelConfig {
                model: "deepseek-flash".to_string(),
                base_url: Some("https://api.deepseek.com".to_string()),
                api_key: Some("sk-test".to_string()),
                ..goble_core::config::ModelConfig::default()
            },
        );
        state.save_config(&config).expect("save");
        let text = read(&dir, "config.toml");
        assert!(text.contains("[model.deepseek]"), "{text}");
        assert!(text.contains("dark = false"), "{text}");
        state
            .reload_config(&dir.path().join("config.toml"))
            .expect("reload");
        assert_eq!(state.config(), config);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("config.toml"))
                .expect("stat")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "the settings file is private");
        }
    }

    #[test]
    fn the_hosts_view_has_a_target_label() {
        // The view type is what the Hosts page draws; keep its one derived line
        // checked here so the page and the file agree.
        let view = goble_core::config::HostView {
            name: "db".to_string(),
            host: "db.internal".to_string(),
            user: None,
            port: None,
            identity: None,
        };
        assert_eq!(view.target_line(), "db.internal:22");
    }
}

#[cfg(test)]
mod event_storm_tests {
    //! Regression: a plain settings READ must never emit a settings change
    //! event. The app reloads on `settings:changed` by re-reading all three
    //! files, so a read that announced itself fed a read → event → reload loop
    //! that pinned the main thread in `drain_events → refresh_settings →
    //! ConfigEditor::load` (100% CPU, >1 GB RSS, no first frame).

    use super::*;
    use crate::event_bus::CollectingEventBus;
    use goble_core::config::{ConfigOp, ConfigValue};
    use goble_core::store::Store;

    /// A state over a throwaway directory, all three settings files pinned
    /// there, with a capturing event bus.
    fn state_with_bus(name: &str) -> (tempfile::TempDir, Arc<DesktopState>, CollectingEventBus) {
        let dir = tempfile::tempdir().expect("temp dir");
        let _ = name;
        let state = DesktopState::new(
            Store::open_in_memory().expect("store"),
            crate::thread_store::ThreadStore::new(dir.path()).expect("threads"),
        );
        state.set_config_path(dir.path().join("config.toml"));
        state.set_environment_path(dir.path().join("environment.toml"));
        let bus = CollectingEventBus::new();
        state.set_event_bus(Arc::new(bus.clone()));
        (dir, state, bus)
    }

    /// The three files a configured Goble home has, with comments and a model.
    fn seed_three_files(dir: &tempfile::TempDir) {
        std::fs::write(
            dir.path().join("config.toml"),
            "# a hand-written comment
[app]
vim_mode = true

[theme]
dark = true
",
        )
        .expect("seed config");
        std::fs::write(
            dir.path().join("environment.toml"),
            "[groups.alpha]
LOG_LEVEL = \"info\"
",
        )
        .expect("seed environment");
        std::fs::write(
            dir.path().join("hosts.toml"),
            "[[hosts]]\nname = \"prod\"\nhost = \"prod.example.com\"\n",
        )
        .expect("seed hosts");
    }

    fn settings_events(events: &[(String, serde_json::Value)]) -> Vec<String> {
        events
            .iter()
            .filter(|(name, _)| name.starts_with("settings:"))
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Every read path the Settings tab drives, in one pass.
    fn read_everything(state: &DesktopState) {
        let _ = state.settings_document();
        for file in ConfigFile::ALL {
            let _ = state.settings_document_for(file);
            let _ = state.reload_settings_file(file);
            let _ = state.settings_path(file);
            let _ = state.open_settings_file(file);
        }
        let _ = state.list_config_models();
        let _ = state.list_harnesses();
        let _ = state.list_mcp_config();
        let _ = state.list_saved_hosts();
    }

    #[test]
    fn reads_emit_no_settings_events_and_do_not_grow_the_queue() {
        let (dir, state, bus) = state_with_bus("read-no-events");
        // The config declares an MCP server, so the first read's declaration
        // sync genuinely changes the runtime registry — and still must not
        // publish an event from a read path.
        seed_three_files(&dir);

        // The exact shape of the old storm: several reload rounds back to back.
        for round in 0..5 {
            read_everything(&state);
            let events = bus.events();
            assert!(
                settings_events(&events).is_empty(),
                "round {round}: a read emitted a settings event: {events:?}"
            );
            assert!(events.is_empty(), "round {round}: a read emitted any event: {events:?}");
        }
        assert!(bus.take_events().is_empty(), "the queue grew across reads");
    }

    #[test]
    fn a_write_emits_exactly_one_settings_changed_with_the_new_revision() {
        let (dir, state, bus) = state_with_bus("write-events");
        seed_three_files(&dir);
        let before = state.settings_document();
        bus.take_events();

        let after = state
            .apply_setting_ops(
                ConfigFile::Config,
                &[ConfigOp::set(
                    &["app", "scroll_speed"],
                    ConfigValue::Int(60),
                )],
                Some(&before.revision),
            )
            .expect("write");
        let events = bus.take_events();
        let changed: Vec<&(String, serde_json::Value)> = events
            .iter()
            .filter(|(name, _)| name == "settings:changed")
            .collect();
        assert_eq!(changed.len(), 1, "exactly one change event: {events:?}");
        assert_eq!(changed[0].1["file"], serde_json::json!("config"), "{events:?}");
        assert_eq!(
            changed[0].1["revision"]["hash"],
            serde_json::json!(after.revision.hash),
            "the event carries the revision the view now holds: {events:?}"
        );
        assert!(
            !events.iter().any(|(name, _)| name == "settings:file-changed"),
            "our own write is not an external change: {events:?}"
        );

        // The read the UI does in response must stay silent (no feedback loop).
        bus.take_events();
        state.reload_settings_file(ConfigFile::Config).expect("read after write");
        assert!(settings_events(&bus.events()).is_empty());
    }

    #[test]
    fn an_environment_write_announces_exactly_one_change() {
        let (dir, state, bus) = state_with_bus("env-write-events");
        seed_three_files(&dir);
        let before = state.settings_document_for(ConfigFile::Environment);
        bus.take_events();
        state
            .apply_setting_ops(
                ConfigFile::Environment,
                &[ConfigOp::set(
                    &["groups", "alpha", "LOG_LEVEL"],
                    ConfigValue::Str("debug".to_string()),
                )],
                Some(&before.revision),
            )
            .expect("write environment");
        let events = bus.take_events();
        assert_eq!(
            events.iter().filter(|(name, _)| name == "settings:changed").count(),
            1,
            "{events:?}"
        );
        // The refresh the write triggers reads the file back; it is silent.
        bus.take_events();
        state.reload_settings_file(ConfigFile::Environment).expect("read");
        assert!(settings_events(&bus.events()).is_empty());
    }

    #[test]
    fn an_external_edit_emits_file_changed_and_changed_once() {
        let (dir, state, bus) = state_with_bus("external-events");
        seed_three_files(&dir);
        bus.take_events();
        let change = ConfigChange {
            file: ConfigFile::Config,
            path: state
                .settings_path(ConfigFile::Config)
                .expect("a pinned path")
                .display()
                .to_string(),
            revision: ConfigRevision::of(b"[app]\nvim_mode = false\n", 7),
            previous: None,
            external: true,
        };
        state.on_external_settings_change(change);
        let events = bus.take_events();
        let file_changed: Vec<&(String, serde_json::Value)> = events
            .iter()
            .filter(|(name, _)| name == "settings:file-changed")
            .collect();
        assert_eq!(file_changed.len(), 1, "one external notice: {events:?}");
        assert_eq!(file_changed[0].1["file"], serde_json::json!("config"));
        assert_eq!(file_changed[0].1["external"], serde_json::json!(true));
        assert_eq!(
            events.iter().filter(|(name, _)| name == "settings:changed").count(),
            1,
            "one change notice: {events:?}"
        );

        // The UI's reload on the notice is silent.
        bus.take_events();
        read_everything(&state);
        assert!(settings_events(&bus.events()).is_empty());
    }
}
