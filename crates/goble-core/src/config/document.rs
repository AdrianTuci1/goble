//! The three settings documents and the types the editor and its callers pass
//! around.
//!
//! `~/.goble/config.toml`, `~/.goble/environment.toml` and `~/.goble/hosts.toml`
//! are the source of truth for settings: they are read and written directly, in
//! place, with comments and unknown sections preserved. This module holds the
//! document handle, the content-hash revision, the path-op vocabulary and the
//! redacted view the UI reads. The edit protocol itself (locks, atomic writes,
//! conflict fencing) lives in [`super::editor`]; the field table and validation
//! rules live in [`super::schema`].
//!
//! Nothing here writes a file. [`ConfigDocument::load`] reads one and parses it
//! into a [`toml_edit::DocumentMut`], which round-trips every byte it was not
//! asked to change.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use toml_edit::DocumentMut;

use crate::environment::{self, EnvValue, EnvironmentFile};
use crate::hosts::{HostEntry, HostsFile};

use super::GobleConfig;

/// Which settings file a document addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigFile {
    /// `~/.goble/config.toml`.
    Config,
    /// `~/.goble/environment.toml`.
    Environment,
    /// `~/.goble/hosts.toml`.
    Hosts,
}

impl ConfigFile {
    /// Every settings file, in the order the app opens them.
    pub const ALL: [ConfigFile; 3] = [
        ConfigFile::Config,
        ConfigFile::Environment,
        ConfigFile::Hosts,
    ];

    /// The file name under the Goble home.
    pub fn file_name(self) -> &'static str {
        match self {
            ConfigFile::Config => "config.toml",
            ConfigFile::Environment => "environment.toml",
            ConfigFile::Hosts => crate::hosts::HOSTS_FILE_NAME,
        }
    }

    /// The name the UI, logs and errors use for the file.
    pub fn label(self) -> &'static str {
        self.file_name()
    }

    /// Where this file lives under `root` (`~/.goble` in the running app).
    pub fn path_in(self, root: &Path) -> PathBuf {
        root.join(self.file_name())
    }
}

impl std::fmt::Display for ConfigFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.file_name())
    }
}

/// The content revision the caller rendered from.
///
/// `hash` is the SHA-256 of the exact bytes on disk; `generation` counts how
/// many times this process has loaded or written the document. Every write
/// carries the revision it read, so a write over a file that changed in between
/// is refused with [`ConfigError::Conflict`] instead of silently clobbering the
/// other edit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigRevision {
    /// SHA-256 of the file's bytes, lowercase hex.
    pub hash: String,
    /// How many loads/writes this process has made of the file; 0 for a file
    /// this process has not observed yet.
    pub generation: u64,
}

impl ConfigRevision {
    /// The revision of `bytes` at `generation`.
    pub fn of(bytes: &[u8], generation: u64) -> Self {
        Self {
            hash: sha256_hex(bytes),
            generation,
        }
    }

    /// The revision a missing file has: the hash of no bytes.
    pub fn missing() -> Self {
        Self::of(b"", 0)
    }

    /// Whether this is the revision of a file that does not exist yet.
    pub fn is_missing(&self) -> bool {
        self.hash == Self::missing().hash
    }

    /// The short hash the UI and Advanced page show.
    pub fn short(&self) -> &str {
        match self.hash.get(..12) {
            Some(short) => short,
            None => self.hash.as_str(),
        }
    }
}

impl std::fmt::Display for ConfigRevision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.short(), self.generation)
    }
}

/// SHA-256 of `bytes`, lowercase hex. The one hash rule the revision and the
/// watcher share.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex::encode(digest)
}

/// One value a [`ConfigOp`] writes. The small vocabulary a settings row needs;
/// tables are written key-by-key so an unknown key inside a user's table (or the
/// comments beside it) survives.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigValue {
    Str(String),
    Bool(bool),
    Int(i64),
    Float(f64),
    StrArray(Vec<String>),
    Table(Vec<(String, ConfigValue)>),
}

impl ConfigValue {
    /// The value of a plain TOML value, if it maps onto the vocabulary.
    pub fn from_toml(value: &toml::Value) -> Option<Self> {
        Some(match value {
            toml::Value::String(value) => ConfigValue::Str(value.clone()),
            toml::Value::Boolean(value) => ConfigValue::Bool(*value),
            toml::Value::Integer(value) => ConfigValue::Int(*value),
            toml::Value::Float(value) => ConfigValue::Float(*value),
            toml::Value::Array(values) => {
                let mut items = Vec::with_capacity(values.len());
                for value in values {
                    match value {
                        toml::Value::String(value) => items.push(value.clone()),
                        toml::Value::Integer(value) => items.push(value.to_string()),
                        _ => return None,
                    }
                }
                ConfigValue::StrArray(items)
            }
            toml::Value::Table(table) => {
                let mut entries = Vec::with_capacity(table.len());
                for (key, value) in table {
                    entries.push((key.clone(), ConfigValue::from_toml(value)?));
                }
                ConfigValue::Table(entries)
            }
            toml::Value::Datetime(_) => return None,
        })
    }

    /// The TOML item this value becomes. Strings, booleans and numbers become
    /// values; a table becomes an inline table, so a nested row set writes as one
    /// path value (`[model.x]` is written as a full table by the editor's
    /// recursive insert, not inline).
    pub fn to_toml_value(&self) -> toml_edit::Value {
        match self {
            ConfigValue::Str(value) => toml_edit::Value::from(value.clone()),
            ConfigValue::Bool(value) => toml_edit::Value::from(*value),
            ConfigValue::Int(value) => toml_edit::Value::from(*value),
            ConfigValue::Float(value) => toml_edit::Value::from(*value),
            ConfigValue::StrArray(values) => {
                let mut array = toml_edit::Array::new();
                for value in values {
                    array.push(value.clone());
                }
                toml_edit::Value::Array(array)
            }
            ConfigValue::Table(entries) => {
                let mut table = toml_edit::InlineTable::new();
                for (key, value) in entries {
                    table.insert(key, value.to_toml_value());
                }
                toml_edit::Value::InlineTable(table)
            }
        }
    }
}

/// One path operation a settings form applies. `Set` writes one path; `Unset`
/// removes it, which is the file-backed "back to the default" (an absent key
/// reads as the default, so the reset never writes the default value).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigOp {
    Set {
        /// The table path, e.g. `["app", "scroll_speed"]` or
        /// `["model", "deepseek", "api_key"]`. Last segment is the leaf key.
        path: Vec<String>,
        value: ConfigValue,
    },
    Unset {
        path: Vec<String>,
    },
}

impl ConfigOp {
    /// A `Set` op.
    pub fn set(path: &[&str], value: ConfigValue) -> Self {
        ConfigOp::Set {
            path: path.iter().map(|segment| segment.to_string()).collect(),
            value,
        }
    }

    /// An `Unset` op.
    pub fn unset(path: &[&str]) -> Self {
        ConfigOp::Unset {
            path: path.iter().map(|segment| segment.to_string()).collect(),
        }
    }

    /// The path this op addresses.
    pub fn path(&self) -> &[String] {
        match self {
            ConfigOp::Set { path, .. } | ConfigOp::Unset { path } => path,
        }
    }
}

/// One validation failure: which path is wrong and why, phrased for a row's
/// inline error line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldError {
    /// The table path the failure is about.
    pub path: Vec<String>,
    /// The message a settings row shows.
    pub message: String,
}

impl FieldError {
    /// A path and message pair.
    pub fn new(path: &[&str], message: impl Into<String>) -> Self {
        Self {
            path: path.iter().map(|segment| segment.to_string()).collect(),
            message: message.into(),
        }
    }

    /// The dotted form of the path, for logs and the Advanced page.
    pub fn dotted(&self) -> String {
        self.path.join(".")
    }
}

/// One section a load could not read. A file that holds one bad section still
/// loads every other section, and the Advanced page reports this list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigProblem {
    /// The section name (`model`, `remote_agents.builder`, …) or `""`.
    pub section: String,
    /// What went wrong.
    pub message: String,
}

impl ConfigProblem {
    /// A section and message pair.
    pub fn new(section: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            section: section.into(),
            message: message.into(),
        }
    }

    /// Split a loader message (`"[model.x] ignored: …"`) into its section and
    /// body. Messages that do not name a section keep the whole text.
    pub fn from_message(message: &str) -> Self {
        if let Some(rest) = message.strip_prefix('[') {
            if let Some((section, body)) = rest.split_once("] ignored: ") {
                return Self::new(section, body.to_string());
            }
        }
        Self::new("", message.to_string())
    }

    /// The one-line form a log line and the Advanced page print.
    pub fn line(&self) -> String {
        if self.section.is_empty() {
            self.message.clone()
        } else {
            format!("[{}]: {}", self.section, self.message)
        }
    }
}

/// Why a read or write was refused. Each variant is actionable: a conflict
/// reloads, an invalid edit fixes a field, an unparseable file opens in a pane.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum ConfigError {
    /// The file changed between the read and the write; nothing was written.
    #[error("the settings file changed on disk since it was read")]
    Conflict {
        file: ConfigFile,
        expected: ConfigRevision,
        actual: ConfigRevision,
    },
    /// Validation failed; nothing was written.
    #[error("the edit is invalid")]
    Invalid(Vec<FieldError>),
    /// The file is not TOML; it is never overwritten.
    #[error("the settings file cannot be parsed, so nothing was written: {0}")]
    RefusedUnparseable(String, ConfigFile),
    /// The filesystem refused the read or write.
    #[error("settings file I/O failed: {1}")]
    Io(ConfigFile, String),
    /// A typed read was asked for the wrong file kind.
    #[error("the document is not the file this read expects")]
    WrongFile(ConfigFile),
}

impl ConfigError {
    /// The file the error is about.
    pub fn file(&self) -> Option<ConfigFile> {
        match self {
            ConfigError::Conflict { file, .. }
            | ConfigError::RefusedUnparseable(_, file)
            | ConfigError::Io(file, _)
            | ConfigError::WrongFile(file) => Some(*file),
            ConfigError::Invalid(_) => None,
        }
    }

    /// Whether retrying after a reload can succeed: only a revision conflict.
    pub fn is_conflict(&self) -> bool {
        matches!(self, ConfigError::Conflict { .. })
    }

    /// The short message a settings notice shows.
    pub fn notice(&self) -> String {
        match self {
            ConfigError::Conflict { file, .. } => {
                format!(
                    "{} changed on disk — reload it and try again",
                    file.file_name()
                )
            }
            ConfigError::Invalid(errors) => errors
                .first()
                .map(|error| error.message.clone())
                .unwrap_or_else(|| "the edit is invalid".to_string()),
            other => other.to_string(),
        }
    }
}

/// The typed read of a document: the existing forgiving structs, not a second
/// parser. A section that does not parse costs itself, and the messages land in
/// [`ConfigDocument::problems`].
#[derive(Clone, Debug)]
pub enum TypedDocument {
    Config(Box<GobleConfig>),
    Environment(EnvironmentFile),
    Hosts(HostsFile),
}

impl TypedDocument {
    /// The config projection, when this document is `config.toml`.
    pub fn config(&self) -> Option<&GobleConfig> {
        match self {
            TypedDocument::Config(config) => Some(config),
            _ => None,
        }
    }

    /// The environment projection, when this document is `environment.toml`.
    pub fn environment(&self) -> Option<&EnvironmentFile> {
        match self {
            TypedDocument::Environment(groups) => Some(groups),
            _ => None,
        }
    }

    /// The hosts projection, when this document is `hosts.toml`.
    pub fn hosts(&self) -> Option<&HostsFile> {
        match self {
            TypedDocument::Hosts(hosts) => Some(hosts),
            _ => None,
        }
    }
}

/// One settings file, parsed as an editable document.
///
/// The document keeps the whole file — header comments, hand-written comments,
/// ordering and sections this build does not model — and writes back everything
/// it was not asked to change. [`ConfigDocument::typed`] is the typed read the
/// rest of the app already uses; it never replaces the document for writing.
#[derive(Clone, Debug)]
pub struct ConfigDocument {
    file: ConfigFile,
    path: PathBuf,
    revision: ConfigRevision,
    raw: String,
    doc: DocumentMut,
    problems: Vec<ConfigProblem>,
}

impl ConfigDocument {
    /// Read `path` as `file`. A missing file is an empty document (so a first
    /// write can create it); a file that is not TOML is refused with
    /// [`ConfigError::RefusedUnparseable`] — a bad edit must never be
    /// overwritten.
    pub fn load(file: ConfigFile, path: &Path) -> Result<Self, ConfigError> {
        let raw = match std::fs::read(path) {
            Ok(bytes) => String::from_utf8(bytes).map_err(|error| {
                ConfigError::RefusedUnparseable(format!("not UTF-8: {error}"), file)
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(ConfigError::Io(
                    file,
                    format!("read {}: {error}", path.display()),
                ))
            }
        };
        Self::from_text(file, path, raw, 0)
    }

    /// Parse a document from text that was already read. `generation` is the
    /// per-process counter the editor owns; a direct caller uses 0.
    pub fn from_text(
        file: ConfigFile,
        path: &Path,
        raw: String,
        generation: u64,
    ) -> Result<Self, ConfigError> {
        let doc = raw
            .parse::<DocumentMut>()
            .map_err(|error| ConfigError::RefusedUnparseable(error.to_string(), file))?;
        let problems = problems_for(file, &raw);
        Ok(Self {
            file,
            path: path.to_path_buf(),
            revision: ConfigRevision::of(raw.as_bytes(), generation),
            raw,
            doc,
            problems,
        })
    }

    /// Which file this document is.
    pub fn file(&self) -> ConfigFile {
        self.file
    }

    /// Where it lives.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The revision of the exact bytes last read.
    pub fn revision(&self) -> &ConfigRevision {
        &self.revision
    }

    /// The exact text last read.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// The bytes last read.
    pub fn bytes(&self) -> &[u8] {
        self.raw.as_bytes()
    }

    /// The parsed document. Mutating it is how an edit applies its path ops.
    pub fn document(&self) -> &DocumentMut {
        &self.doc
    }

    /// Mutable access, for the editor only: a caller repairs the document in
    /// place and then hands it back through [`ConfigEditor::edit`](super::ConfigEditor::edit).
    pub fn document_mut(&mut self) -> &mut DocumentMut {
        &mut self.doc
    }

    /// The sections the last load could not read. A file with problems still
    /// loads every section around them and is only refused for a write when the
    /// edit would add a new problem.
    pub fn problems(&self) -> &[ConfigProblem] {
        &self.problems
    }

    /// The text the document would write: the full file, unchanged except for
    /// what was edited.
    pub fn serialized(&self) -> String {
        self.doc.to_string()
    }

    /// The file's mode as `0644`, or `missing` when there is no file yet.
    pub fn mode(&self) -> String {
        match std::fs::metadata(&self.path) {
            Ok(metadata) => mode_string(&metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => "missing".to_string(),
            Err(_) => "unknown".to_string(),
        }
    }

    /// The top-level table names the file carries, in file order.
    pub fn sections(&self) -> Vec<String> {
        self.doc.iter().map(|(key, _)| key.to_string()).collect()
    }

    /// Whether the document holds `path`.
    pub fn contains(&self, path: &[String]) -> bool {
        get_path(&self.doc, path).is_some()
    }

    /// The document parsed through the app's existing forgiving structs.
    pub fn typed(&self) -> Result<TypedDocument, ConfigError> {
        Ok(match self.file {
            ConfigFile::Config => TypedDocument::Config(Box::new(self.typed_config()?)),
            ConfigFile::Environment => TypedDocument::Environment(self.typed_environment()?),
            ConfigFile::Hosts => TypedDocument::Hosts(self.typed_hosts()?),
        })
    }

    /// The typed `config.toml` read: the same forgiving loader the app already
    /// uses, so there is no second parser to drift.
    pub fn typed_config(&self) -> Result<GobleConfig, ConfigError> {
        if self.file != ConfigFile::Config {
            return Err(ConfigError::WrongFile(self.file));
        }
        // `load_toml` only fails when the text is not TOML at all, which the
        // document parse already ruled out.
        let (config, _problems) = GobleConfig::load_toml(&self.raw)
            .map_err(|error| ConfigError::RefusedUnparseable(error.to_string(), self.file))?;
        Ok(config)
    }

    /// The typed `environment.toml` read.
    pub fn typed_environment(&self) -> Result<EnvironmentFile, ConfigError> {
        if self.file != ConfigFile::Environment {
            return Err(ConfigError::WrongFile(self.file));
        }
        let (groups, _problems) = environment::load_toml_with_problems(&self.raw)
            .map_err(|error| ConfigError::RefusedUnparseable(error.to_string(), self.file))?;
        Ok(groups)
    }

    /// The typed `hosts.toml` read. A row that does not parse is reported in
    /// [`ConfigDocument::problems`] and skipped here, so one bad row does not
    /// hide the rest.
    pub fn typed_hosts(&self) -> Result<HostsFile, ConfigError> {
        if self.file != ConfigFile::Hosts {
            return Err(ConfigError::WrongFile(self.file));
        }
        let mut hosts = HostsFile::default();
        let table = self.doc.as_table();
        let Some(entries) = table.get("hosts") else {
            return Ok(hosts);
        };
        let Some(entries) = entries.as_array_of_tables() else {
            return Ok(hosts);
        };
        for entry in entries.iter() {
            match host_entry_from_edit_table(entry) {
                Ok(host) => hosts.hosts.push(host),
                Err(_) => continue,
            }
        }
        Ok(hosts)
    }
}

/// Build a [`HostEntry`] from a `[[hosts]]` table.
fn host_entry_from_edit_table(table: &toml_edit::Table) -> Result<HostEntry, String> {
    let string = |key: &str| -> Result<Option<String>, String> {
        match table.get(key) {
            None => Ok(None),
            Some(item) => item
                .as_str()
                .map(|value| Some(value.to_string()))
                .ok_or_else(|| format!("{key} must be a string")),
        }
    };
    let name = string("name")?.ok_or_else(|| "the name is missing".to_string())?;
    let host = string("host")?.ok_or_else(|| "the host is missing".to_string())?;
    let user = string("user")?;
    let identity = string("identity")?;
    let port = match table.get("port") {
        None => None,
        Some(item) => Some(
            item.as_integer()
                .and_then(|value| u16::try_from(value).ok())
                .ok_or_else(|| "port must be an integer in 1..=65535".to_string())?,
        ),
    };
    Ok(HostEntry {
        name,
        host,
        user,
        port,
        identity,
    })
}

/// The sections a load reports as problems for `text` of `file`. Pure, so the
/// editor can compare "problems before" with "problems after" an edit and refuse
/// one that adds a new problem.
pub fn problems_for(file: ConfigFile, text: &str) -> Vec<ConfigProblem> {
    match file {
        ConfigFile::Config => match GobleConfig::load_toml(text) {
            Ok((_config, problems)) => problems
                .iter()
                .map(|problem| ConfigProblem::from_message(problem))
                .collect(),
            Err(error) => vec![ConfigProblem::new("", error.to_string())],
        },
        ConfigFile::Environment => match environment::load_toml_with_problems(text) {
            Ok((_groups, problems)) => problems
                .iter()
                .map(|problem| ConfigProblem::from_message(problem))
                .collect(),
            Err(error) => vec![ConfigProblem::new("", error.to_string())],
        },
        ConfigFile::Hosts => {
            let Ok(table) = toml::from_str::<toml::Table>(text) else {
                return vec![ConfigProblem::new("", "not valid TOML")];
            };
            let Some(entries) = table.get("hosts") else {
                return Vec::new();
            };
            let Some(entries) = entries.as_array() else {
                return vec![ConfigProblem::new("hosts", "expected an array of tables")];
            };
            let mut problems = Vec::new();
            for (index, entry) in entries.iter().enumerate() {
                let parsed: Result<HostsWireEntry, _> = entry.clone().try_into();
                if let Err(error) = parsed {
                    problems.push(ConfigProblem::new(
                        format!("hosts[{index}]"),
                        format!("a saved host needs a name and a host address: {error}"),
                    ));
                }
            }
            problems
        }
    }
}

/// The serde shape `hosts.toml` rows parse through, mirroring
/// [`HostEntry`]'s required fields. The fields are read by serde, not by code.
#[derive(Deserialize)]
#[allow(dead_code)]
struct HostsWireEntry {
    name: String,
    host: String,
}

/// The octal mode of a file (`0600`), for the Advanced page and the permission
/// migration report.
pub fn mode_string(metadata: &std::fs::Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        format!("{:04o}", metadata.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        "unknown".to_string()
    }
}

/// Resolve a table path inside a document (`["model", "deepseek", "api_key"]`).
pub fn get_path<'a>(doc: &'a DocumentMut, path: &[String]) -> Option<&'a toml_edit::Item> {
    let (first, rest) = path.split_first()?;
    let mut item = doc.get(first)?;
    for segment in rest {
        item = item.as_table_like()?.get(segment)?;
    }
    Some(item)
}

/// A redacted secret slot: whether a value is configured, never the value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretState {
    pub set: bool,
}

impl SecretState {
    pub fn set(set: bool) -> Self {
        Self { set }
    }
}

impl From<bool> for SecretState {
    fn from(set: bool) -> Self {
        Self { set }
    }
}

/// Where a model's API key comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeySource {
    /// Written inline in `config.toml`.
    Inline,
    /// Read from the environment variable named by `api_key_env`.
    Env,
    /// Declared, but no key is configured.
    Missing,
}

/// `[app]` as the UI reads it: every field optional, because an absent key means
/// the default (and reset is an unset, never a written default).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AppView {
    pub friendly_name: Option<String>,
    pub usage: Option<String>,
    pub telemetry: Option<bool>,
    pub in_app_notifications: Option<bool>,
    pub invert_scroll: Option<bool>,
    pub scroll_speed: Option<i64>,
    pub font_size: Option<f64>,
    pub auto_save_on_close: Option<bool>,
    pub approval_mode: Option<String>,
    pub auto_approve: Option<bool>,
    pub vim_mode: Option<bool>,
    pub send_immediately: Option<bool>,
}

/// `[theme]` as the UI reads it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ThemeView {
    pub dark: Option<bool>,
    pub preset: Option<String>,
    pub accent: Option<String>,
    pub primary: Option<String>,
    pub secondary: Option<String>,
    pub bg: Option<String>,
    pub surface: Option<String>,
}

/// One `[model.<slug>]` row, with the key redacted to whether it is configured.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelView {
    pub slug: String,
    pub name: Option<String>,
    pub model_id: String,
    pub base_url: Option<String>,
    pub api_key: SecretState,
    pub api_key_env: Option<String>,
    pub api_key_source: ApiKeySource,
    pub max_completion_tokens: Option<u32>,
    pub context_window: Option<u64>,
    pub hidden: bool,
    pub is_default: bool,
}

/// One `[harness.<id>]` row. The configured fields are the override; the catalog
/// row provides the name and default command.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HarnessView {
    pub id: String,
    pub name: String,
    pub configured: bool,
    pub enabled: bool,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// One `[mcp_servers.<name>]` row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct McpView {
    pub name: String,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub enabled: bool,
}

impl McpView {
    /// The transport the row shows: `http` for a URL, `stdio` for a command.
    pub fn transport(&self) -> &'static str {
        match (&self.url, &self.command) {
            (Some(_), _) => "http",
            (None, Some(_)) => "stdio",
            (None, None) => "none",
        }
    }
}

/// `[remote]` as the UI reads it, with the worker token redacted.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RemoteView {
    pub mode: String,
    pub gateway_url: Option<String>,
    pub worker_token: SecretState,
}

/// One `[remote_agents.<name>]` row, with credentials redacted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteAgentView {
    pub name: String,
    pub provider: String,
    pub enabled: bool,
    pub api_key: SecretState,
    pub token_id: SecretState,
    pub token_secret: SecretState,
    pub api_url: Option<String>,
    pub image: Option<String>,
    pub region: Option<String>,
    pub environment: Option<String>,
    pub missing_credentials: Vec<String>,
}

/// One row of `hosts.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HostView {
    pub name: String,
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity: Option<String>,
}

impl HostView {
    /// The target line the Hosts row draws: `user@host:port` with ssh's
    /// defaults (the local account, port 22) when the file leaves them out.
    pub fn target_line(&self) -> String {
        let user = self
            .user
            .as_deref()
            .map(str::trim)
            .filter(|user| !user.is_empty());
        let port = self.port.unwrap_or(22);
        match user {
            Some(user) => format!("{user}@{}:{port}", self.host),
            None => format!("{}:{port}", self.host),
        }
    }
}

/// How one environment variable is stored: the value itself or a vault key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvValueView {
    /// A literal value is configured. The value itself is never returned.
    Value(SecretState),
    /// The value lives in the encrypted vault under `vault`.
    Vault { vault: String },
}

/// One variable of an environment group, redacted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnvVarView {
    pub name: String,
    pub value: EnvValueView,
}

/// One `[groups.<name>]` table, redacted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupView {
    pub name: String,
    pub variables: Vec<EnvVarView>,
}

/// The redacted, typed projection of all three files — what the settings UI
/// reads. It never carries an inline key, a worker token, a remote agent
/// credential or an environment value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfigSections {
    pub app: AppView,
    pub theme: ThemeView,
    pub models: Vec<ModelView>,
    pub default_model: Option<String>,
    pub harnesses: Vec<HarnessView>,
    pub mcp: Vec<McpView>,
    pub remote: RemoteView,
    pub remote_agents: Vec<RemoteAgentView>,
    pub hosts: Vec<HostView>,
    pub environment: Vec<GroupView>,
}

/// The full settings projection for a file: which file, where it is, the
/// revision the UI holds, its mode, the redacted sections and the load problems.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConfigView {
    pub file: ConfigFile,
    pub path: String,
    pub revision: ConfigRevision,
    pub mode: String,
    pub sections: ConfigSections,
    pub problems: Vec<ConfigProblem>,
}

impl ConfigView {
    /// The empty view a fresh install has: no sections, no revision beyond the
    /// missing-file hash.
    pub fn empty(file: ConfigFile, path: &Path, revision: ConfigRevision) -> Self {
        Self {
            file,
            path: path.display().to_string(),
            revision,
            mode: "missing".to_string(),
            sections: ConfigSections::default(),
            problems: Vec::new(),
        }
    }

    /// The `[app]` section of this view.
    pub fn app(&self) -> &AppView {
        &self.sections.app
    }
}

/// Turn the typed environment groups into the redacted view. A literal value
/// becomes "configured" with no bytes; a vault reference keeps the key name,
/// which is not a secret.
pub fn group_views(groups: &EnvironmentFile) -> Vec<GroupView> {
    groups
        .iter()
        .map(|(name, variables)| GroupView {
            name: name.clone(),
            variables: variables
                .iter()
                .map(|(name, value)| EnvVarView {
                    name: name.clone(),
                    value: match value {
                        EnvValue::Literal(_) => EnvValueView::Value(SecretState::set(true)),
                        EnvValue::Vault { vault } => EnvValueView::Vault {
                            vault: vault.clone(),
                        },
                    },
                })
                .collect(),
        })
        .collect()
}

/// Turn the typed hosts into the view.
pub fn host_views(hosts: &HostsFile) -> Vec<HostView> {
    hosts
        .hosts
        .iter()
        .filter(|host| host.is_valid())
        .map(|host| HostView {
            name: host.name.clone(),
            host: host.host.clone(),
            user: host.user.clone(),
            port: host.port,
            identity: host.identity.clone(),
        })
        .collect()
}
