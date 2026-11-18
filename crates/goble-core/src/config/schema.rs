//! The settings schema: one table of field specs, the path-op applicator, the
//! validation rules and the redacted view the UI reads.
//!
//! The same table drives two things that must never disagree: which fields the
//! settings row renders and what a write of that field is allowed to contain.
//! There is no schema engine here — Goble's settings paths are fixed and known,
//! so the table is a static list and validation is explicit Rust.
//!
//! Path ops ([`ConfigOp`]) are applied to the [`toml_edit::DocumentMut`] and not
//! to a typed section, so a row never rewrites a table it only saw a redacted
//! view of: a masked API key a form did not type is never deleted by that form's
//! save. That is the whole reason writes are path ops.

use toml_edit::{DocumentMut, Item, Table, Value};

use crate::harness::HARNESSES;

use super::document::{
    group_views, host_views, ApiKeySource, AppView, ConfigDocument, ConfigError, ConfigFile,
    ConfigOp, ConfigProblem, ConfigRevision, ConfigSections, ConfigValue, ConfigView, FieldError,
    HarnessView, McpView, ModelView, RemoteAgentView, RemoteView, SecretState, ThemeView,
};
use super::GobleConfig;

/// Which settings page a field belongs to (the Lane A IA, §4.2 of the design).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingsPage {
    General,
    Appearance,
    Providers,
    Connectors,
    Agent,
    Environments,
    Hosts,
    Secrets,
    Shortcuts,
    Updates,
    Advanced,
    About,
}

impl SettingsPage {
    /// Every page the settings pane can open.
    pub const ALL: [SettingsPage; 12] = [
        SettingsPage::General,
        SettingsPage::Appearance,
        SettingsPage::Providers,
        SettingsPage::Connectors,
        SettingsPage::Agent,
        SettingsPage::Environments,
        SettingsPage::Hosts,
        SettingsPage::Secrets,
        SettingsPage::Shortcuts,
        SettingsPage::Updates,
        SettingsPage::Advanced,
        SettingsPage::About,
    ];

    /// The page's label in the rail and the breadcrumb.
    pub fn label(self) -> &'static str {
        match self {
            SettingsPage::General => "General",
            SettingsPage::Appearance => "Appearance",
            SettingsPage::Providers => "Providers",
            SettingsPage::Connectors => "Connectors",
            SettingsPage::Agent => "Agent",
            SettingsPage::Environments => "Environments",
            SettingsPage::Hosts => "Hosts",
            SettingsPage::Secrets => "Secrets",
            SettingsPage::Shortcuts => "Shortcuts",
            SettingsPage::Updates => "Updates",
            SettingsPage::Advanced => "Advanced",
            SettingsPage::About => "About",
        }
    }
}

/// The control a field is rendered with, and the rule a write must satisfy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FieldKind {
    Text,
    /// A text field whose value never leaves the core: the view carries only
    /// [`SecretState`].
    Secret,
    Integer {
        min: i64,
        max: i64,
    },
    Float {
        min: f64,
        max: f64,
    },
    Bool,
    Enum(&'static [&'static str]),
    /// `#rrggbb`, the form the theme writer stores.
    Colour,
    /// An `http://` or `https://` URL.
    Url,
    /// A URL that may also be a WebSocket endpoint (`ws://` / `wss://`): the
    /// gateway URL is what a worker dials, and goble's is a WebSocket URL.
    GatewayUrl,
    /// A filesystem path; any non-empty string is accepted.
    Path,
    /// A TOML table written key-by-key.
    Table,
    /// An array of strings.
    StringArray,
}

impl FieldKind {
    /// Check one TOML item against the kind. `None` means it fits.
    pub fn check(&self, item: &Item) -> Option<String> {
        match self {
            FieldKind::Text | FieldKind::Path => match item.as_str() {
                Some(_) => None,
                None => Some("expected a string".to_string()),
            },
            FieldKind::Secret => match item.as_str() {
                Some(_) => None,
                None => Some("expected a string".to_string()),
            },
            FieldKind::Bool => match item.as_bool() {
                Some(_) => None,
                None => Some("expected true or false".to_string()),
            },
            FieldKind::Integer { min, max } => match item.as_integer() {
                Some(value) if value >= *min && value <= *max => None,
                Some(_) => Some(format!("expected an integer in {min}..={max}")),
                None => Some("expected an integer".to_string()),
            },
            FieldKind::Float { min, max } => {
                let value = item
                    .as_float()
                    .or_else(|| item.as_integer().map(|v| v as f64));
                match value {
                    Some(value) if value >= *min && value <= *max => None,
                    Some(_) => Some(format!("expected a number in {min}..={max}")),
                    None => Some("expected a number".to_string()),
                }
            }
            FieldKind::Enum(options) => match item.as_str() {
                Some(value) if options.contains(&value) => None,
                Some(value) => Some(format!("{value:?} is not one of: {}", options.join(", "))),
                None => Some("expected a string".to_string()),
            },
            FieldKind::Colour => match item.as_str() {
                Some(value) if is_hex_colour(value) => None,
                Some(_) => Some("expected a colour as #rrggbb".to_string()),
                None => Some("expected a colour string".to_string()),
            },
            FieldKind::Url => match item.as_str() {
                Some(value) if is_http_url(value) => None,
                Some(_) => Some("expected an http:// or https:// URL".to_string()),
                None => Some("expected a URL string".to_string()),
            },
            FieldKind::GatewayUrl => match item.as_str() {
                Some(value) if is_gateway_url(value) => None,
                Some(_) => Some("expected an http://, https://, ws:// or wss:// URL".to_string()),
                None => Some("expected a URL string".to_string()),
            },
            FieldKind::Table => match item.as_table_like() {
                Some(_) => None,
                None => Some("expected a table".to_string()),
            },
            FieldKind::StringArray => check_string_array(item),
        }
    }
}

/// One field the schema knows: its path, its row text, its control and its
/// validation rule. `path` segments with `*` mark a map key (a model slug, a
/// server name), so one spec covers every entry of that map.
#[derive(Clone, Copy, Debug)]
pub struct ConfigFieldSpec {
    /// The table path, e.g. `["app", "scroll_speed"]`.
    pub path: &'static [&'static str],
    pub label: &'static str,
    pub description: &'static str,
    pub kind: FieldKind,
    /// The display default. `None` means the resolver's own default (a model's
    /// id is its slug, a remote mode is `platform`). Unsetting a key restores it.
    pub default: Option<&'static str>,
    pub page: SettingsPage,
    /// Whether the value is stripped from [`ConfigView`].
    pub secret: bool,
}

/// The approval modes `[app] approval_mode` accepts; they are the labels the
/// composer's mode control writes (`goble_ui::ApprovalMode::label`).
pub const APPROVAL_MODES: &[&str] = &["always-approve", "manual", "plan", "auto-review"];

/// The static field table. One source of truth for the UI rows and for what a
/// write of that path may contain.
pub const FIELDS: &[ConfigFieldSpec] = &[
    // [app] — General
    ConfigFieldSpec {
        path: &["app", "friendly_name"],
        label: "Friendly name",
        description: "How Goble addresses you.",
        kind: FieldKind::Text,
        default: None,
        page: SettingsPage::General,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["app", "usage"],
        label: "What you use Goble for",
        description: "The answer from onboarding.",
        kind: FieldKind::Text,
        default: None,
        page: SettingsPage::General,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["app", "telemetry"],
        label: "Share anonymous usage data",
        description: "Off unless you turn it on.",
        kind: FieldKind::Bool,
        default: Some("false"),
        page: SettingsPage::General,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["app", "in_app_notifications"],
        label: "In-app announcements",
        description:
            "The turn-finished banner. System notifications are not available in this build.",
        kind: FieldKind::Bool,
        default: Some("true"),
        page: SettingsPage::General,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["app", "invert_scroll"],
        label: "Invert scrolling",
        description: "Natural scrolling for the wheel.",
        kind: FieldKind::Bool,
        default: Some("false"),
        page: SettingsPage::General,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["app", "scroll_speed"],
        label: "Scroll speed",
        description: "1 (slowest) to 100 (fastest).",
        kind: FieldKind::Integer { min: 1, max: 100 },
        default: Some("50"),
        page: SettingsPage::General,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["app", "font_size"],
        label: "Font size",
        description: "The interface text scale.",
        kind: FieldKind::Float { min: 0.6, max: 2.0 },
        default: Some("1.0"),
        page: SettingsPage::Appearance,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["app", "auto_save_on_close"],
        label: "Save on close",
        description: "Save a file pane's edits instead of asking.",
        kind: FieldKind::Bool,
        default: Some("false"),
        page: SettingsPage::Appearance,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["app", "approval_mode"],
        label: "Approval mode",
        description: "How a proposed command reaches the shell.",
        kind: FieldKind::Enum(APPROVAL_MODES),
        default: Some("manual"),
        page: SettingsPage::Agent,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["app", "auto_approve"],
        label: "Auto-approve questions",
        description: "Approve `ask_user` questions without waiting.",
        kind: FieldKind::Bool,
        default: Some("false"),
        page: SettingsPage::Agent,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["app", "vim_mode"],
        label: "Vim mode",
        description: "Modal editing in the rich input.",
        kind: FieldKind::Bool,
        default: Some("false"),
        page: SettingsPage::Agent,
        secret: false,
    },
    // [theme] — Appearance
    ConfigFieldSpec {
        path: &["theme", "dark"],
        label: "Dark base",
        description: "The built-in dark or light scheme.",
        kind: FieldKind::Bool,
        default: Some("true"),
        page: SettingsPage::Appearance,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["theme", "preset"],
        label: "Palette",
        description: "A named palette; unset is the Default base.",
        kind: FieldKind::Text,
        default: None,
        page: SettingsPage::Appearance,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["theme", "accent"],
        label: "Accent",
        description: "Overrides the accent channel.",
        kind: FieldKind::Colour,
        default: None,
        page: SettingsPage::Appearance,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["theme", "primary"],
        label: "Primary",
        description: "Overrides the primary channel.",
        kind: FieldKind::Colour,
        default: None,
        page: SettingsPage::Appearance,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["theme", "secondary"],
        label: "Secondary",
        description: "Overrides the secondary channel.",
        kind: FieldKind::Colour,
        default: None,
        page: SettingsPage::Appearance,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["theme", "bg"],
        label: "Background",
        description: "Overrides the page background.",
        kind: FieldKind::Colour,
        default: None,
        page: SettingsPage::Appearance,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["theme", "surface"],
        label: "Surface",
        description: "Overrides the pane surface.",
        kind: FieldKind::Colour,
        default: None,
        page: SettingsPage::Appearance,
        secret: false,
    },
    // [model.*] — Providers
    ConfigFieldSpec {
        path: &["model", "*", "model"],
        label: "Model id",
        description: "The id sent to the API.",
        kind: FieldKind::Text,
        default: None,
        page: SettingsPage::Providers,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["model", "*", "name"],
        label: "Display name",
        description: "The label the picker shows.",
        kind: FieldKind::Text,
        default: None,
        page: SettingsPage::Providers,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["model", "*", "base_url"],
        label: "Base URL",
        description: "The provider endpoint.",
        kind: FieldKind::Url,
        default: None,
        page: SettingsPage::Providers,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["model", "*", "api_key"],
        label: "API key",
        description: "Written inline in this 0600 file.",
        kind: FieldKind::Secret,
        default: None,
        page: SettingsPage::Providers,
        secret: true,
    },
    ConfigFieldSpec {
        path: &["model", "*", "api_key_env"],
        label: "API key environment variable",
        description: "Read the key from this variable at resolve time instead.",
        kind: FieldKind::Text,
        default: None,
        page: SettingsPage::Providers,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["model", "*", "max_completion_tokens"],
        label: "Max completion tokens",
        description: "The response cap.",
        kind: FieldKind::Integer {
            min: 1,
            max: i64::MAX,
        },
        default: None,
        page: SettingsPage::Providers,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["model", "*", "context_window"],
        label: "Context window",
        description: "The total context size in tokens.",
        kind: FieldKind::Integer {
            min: 1,
            max: i64::MAX,
        },
        default: None,
        page: SettingsPage::Providers,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["model", "*", "hidden"],
        label: "Hidden",
        description: "Keep the entry out of the picker.",
        kind: FieldKind::Bool,
        default: Some("false"),
        page: SettingsPage::Providers,
        secret: false,
    },
    // [models]
    ConfigFieldSpec {
        path: &["models", "default"],
        label: "Default model",
        description: "The model new sessions start on.",
        kind: FieldKind::Text,
        default: None,
        page: SettingsPage::Providers,
        secret: false,
    },
    // [mcp_servers.*] — Connectors
    ConfigFieldSpec {
        path: &["mcp_servers", "*", "url"],
        label: "URL",
        description: "The streamable-HTTP endpoint.",
        kind: FieldKind::Url,
        default: None,
        page: SettingsPage::Connectors,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["mcp_servers", "*", "command"],
        label: "Command",
        description: "The stdio program to run.",
        kind: FieldKind::Text,
        default: None,
        page: SettingsPage::Connectors,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["mcp_servers", "*", "args"],
        label: "Arguments",
        description: "Arguments for the stdio command.",
        kind: FieldKind::StringArray,
        default: None,
        page: SettingsPage::Connectors,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["mcp_servers", "*", "env"],
        label: "Environment",
        description: "Extra variables for the stdio command.",
        kind: FieldKind::Table,
        default: None,
        page: SettingsPage::Connectors,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["mcp_servers", "*", "enabled"],
        label: "Enabled",
        description: "Whether the server is synced into the runtime registry.",
        kind: FieldKind::Bool,
        default: Some("true"),
        page: SettingsPage::Connectors,
        secret: false,
    },
    // [harness.*] — Providers / Agent CLIs
    ConfigFieldSpec {
        path: &["harness", "*", "enabled"],
        label: "Enabled",
        description: "Whether the harness is offered in the picker.",
        kind: FieldKind::Bool,
        default: Some("true"),
        page: SettingsPage::Providers,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["harness", "*", "command"],
        label: "Command",
        description: "Overrides the catalog's command.",
        kind: FieldKind::Text,
        default: None,
        page: SettingsPage::Providers,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["harness", "*", "args"],
        label: "Arguments",
        description: "Extra arguments passed at launch.",
        kind: FieldKind::StringArray,
        default: None,
        page: SettingsPage::Providers,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["harness", "*", "env"],
        label: "Environment",
        description: "Extra variables set at launch.",
        kind: FieldKind::Table,
        default: None,
        page: SettingsPage::Providers,
        secret: false,
    },
    // [remote] — Agent / Remote
    ConfigFieldSpec {
        path: &["remote", "mode"],
        label: "Remote mode",
        description: "`platform` (sandbox tables) or `gateway`.",
        kind: FieldKind::Enum(&["platform", "gateway"]),
        default: Some("platform"),
        page: SettingsPage::Agent,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["remote", "gateway_url"],
        label: "Gateway URL",
        description: "The gateway workers connect to.",
        kind: FieldKind::GatewayUrl,
        default: None,
        page: SettingsPage::Agent,
        secret: false,
    },
    ConfigFieldSpec {
        path: &["remote", "worker_token"],
        label: "Worker token",
        description: "The token the gateway was started with.",
        kind: FieldKind::Secret,
        default: None,
        page: SettingsPage::Agent,
        secret: true,
    },
];

/// The spec for a path, matching map segments (`model.deepseek.api_key`) against
/// the `*` wildcard.
pub fn field_spec(path: &[String]) -> Option<&'static ConfigFieldSpec> {
    FIELDS.iter().find(|spec| {
        spec.path.len() == path.len()
            && spec
                .path
                .iter()
                .zip(path)
                .all(|(expected, actual)| *expected == "*" || *expected == actual.as_str())
    })
}

/// The display default of a field, if it has one.
pub fn default_value(spec: &ConfigFieldSpec) -> Option<&'static str> {
    spec.default
}

/// Whether the document's value at the spec's path is the display default (or
/// absent, which reads as the default). Drives the row's reset affordance.
pub fn is_default(doc: &DocumentMut, spec: &ConfigFieldSpec) -> bool {
    let Some(item) = item_at(doc, spec.path) else {
        return true;
    };
    match spec.default {
        None => false,
        Some(default) => item_matches_default(item, spec.kind, default),
    }
}

/// Read a field's value from the document as a display string. Secrets are
/// returned as the empty string: the schema never hands a secret around.
pub fn display_value(doc: &DocumentMut, spec: &ConfigFieldSpec) -> Option<String> {
    if spec.secret {
        return None;
    }
    let item = item_at(doc, spec.path)?;
    Some(match item {
        Item::Value(Value::String(value)) => value.value().clone(),
        other => other.to_string().trim().to_string(),
    })
}

fn item_at<'a>(doc: &'a DocumentMut, path: &[&str]) -> Option<&'a Item> {
    let (first, rest) = path.split_first()?;
    let mut item = doc.get(first)?;
    for segment in rest {
        if *segment == "*" {
            // A wildcard spec addresses every map entry; the caller that needs a
            // concrete entry resolves it first.
            return None;
        }
        item = item.as_table_like()?.get(segment)?;
    }
    Some(item)
}

fn item_matches_default(item: &Item, kind: FieldKind, default: &str) -> bool {
    match kind {
        FieldKind::Bool => item
            .as_bool()
            .is_some_and(|value| value == (default == "true")),
        FieldKind::Integer { .. } => item
            .as_integer()
            .is_some_and(|value| default.parse::<i64>() == Ok(value)),
        FieldKind::Float { .. } => {
            let value = item
                .as_float()
                .or_else(|| item.as_integer().map(|v| v as f64));
            value.is_some_and(|value| default.parse::<f64>() == Ok(value))
        }
        _ => item.as_str().is_some_and(|value| value == default),
    }
}

/// Apply a batch of path ops to the document. Every op is applied in order; a
/// failure (an empty path, a segment that is not a table) stops the batch, and
/// the editor writes nothing when it fails.
pub fn apply_ops(doc: &mut DocumentMut, ops: &[ConfigOp]) -> Result<(), ConfigError> {
    for op in ops {
        apply_op(doc, op)?;
    }
    Ok(())
}

/// Apply one path op.
pub fn apply_op(doc: &mut DocumentMut, op: &ConfigOp) -> Result<(), ConfigError> {
    let path = op.path();
    if path.is_empty() || path.iter().any(String::is_empty) {
        return Err(invalid_path(path, "a setting path cannot be empty"));
    }
    match op {
        ConfigOp::Set { path, value } => set_path(doc, path, value),
        ConfigOp::Unset { path } => {
            unset_path(doc, path);
            Ok(())
        }
    }
}

fn invalid_path(path: &[String], message: &str) -> ConfigError {
    ConfigError::Invalid(vec![FieldError {
        path: path.to_vec(),
        message: message.to_string(),
    }])
}

fn set_path(
    doc: &mut DocumentMut,
    path: &[String],
    value: &ConfigValue,
) -> Result<(), ConfigError> {
    let (leaf, parents) = path
        .split_last()
        .ok_or_else(|| invalid_path(path, "a setting path cannot be empty"))?;
    let mut table: &mut Table = doc.as_table_mut();
    for segment in parents {
        if table.get(segment).is_none() {
            let mut created = Table::new();
            // An implicit intermediate table keeps the file from gaining an empty
            // `[model]` header beside `[model.deepseek]`.
            created.set_implicit(true);
            table.insert(segment, Item::Table(created));
        }
        table = table
            .get_mut(segment)
            .and_then(|item| item.as_table_mut())
            .ok_or_else(|| {
                ConfigError::Invalid(vec![FieldError {
                    path: path.to_vec(),
                    message: format!("{segment} is not a table"),
                }])
            })?;
    }
    // Replace a scalar's value in place: the `Item` (and so the comment above
    // the key and the key's own decor) stays, while only the value changes. A
    // save changes the setting and nothing else — the comment above a key is
    // part of the file's documentation.
    if let Some(Item::Value(old)) = table.get_mut(leaf) {
        if !matches!(value, ConfigValue::Table(_)) {
            *old = value.to_toml_value();
            return Ok(());
        }
    }
    let mut item = match value {
        ConfigValue::Table(entries) => Item::Table(table_value(entries)),
        other => Item::Value(other.to_toml_value()),
    };
    if let Some(existing) = table.get(leaf) {
        match (&existing, &mut item) {
            (Item::Table(old), Item::Table(new)) => {
                new.decor_mut().clone_from(old.decor());
            }
            (Item::Value(old), Item::Table(new)) => {
                new.decor_mut().clone_from(old.decor());
            }
            (Item::Table(old), Item::Value(new)) => {
                new.decor_mut().clone_from(old.decor());
            }
            _ => {}
        }
    }
    table.insert(leaf, item);
    Ok(())
}

/// A table item built from a path-op value, so `[mcp_servers.name]` writes as a
/// table header and nested fields render as their own tables.
fn table_value(entries: &[(String, ConfigValue)]) -> Table {
    let mut table = Table::new();
    for (key, value) in entries {
        let item = match value {
            ConfigValue::Table(nested) => Item::Table(table_value(nested)),
            other => Item::Value(other.to_toml_value()),
        };
        table.insert(key, item);
    }
    table
}

/// Remove the leaf of `path`. A path that is already absent is a no-op, so
/// "reset to default" is idempotent.
fn unset_path(doc: &mut DocumentMut, path: &[String]) {
    let Some((leaf, parents)) = path.split_last() else {
        return;
    };
    let Some(item) = get_item(doc, parents) else {
        return;
    };
    let Some(table) = item.as_table_like_mut() else {
        return;
    };
    table.remove(leaf);
}

fn get_item<'a>(doc: &'a mut DocumentMut, path: &[String]) -> Option<&'a mut Item> {
    let (first, rest) = path.split_first()?;
    let mut item = doc.get_mut(first)?;
    for segment in rest {
        item = item.as_table_like_mut()?.get_mut(segment)?;
    }
    Some(item)
}

/// Validate a document for its file kind. The editor refuses to write when this
/// returns any error, so a bad edit leaves the file byte-identical.
pub fn validate(file: ConfigFile, doc: &DocumentMut) -> Vec<FieldError> {
    match file {
        ConfigFile::Config => validate_config(doc),
        ConfigFile::Environment => validate_environment(doc),
        ConfigFile::Hosts => validate_hosts(doc),
    }
}

fn validate_config(doc: &DocumentMut) -> Vec<FieldError> {
    let mut errors = Vec::new();

    // Table-shaped sections first: a wrong shape costs the section, and the
    // per-field checks below can then walk it without panicking.
    for section in ["app", "theme", "models", "remote"] {
        if let Some(item) = doc.get(section) {
            if item.as_table_like().is_none() {
                errors.push(FieldError::new(&[section], "expected a table"));
            }
        }
    }
    if let Some(item) = doc.get("model") {
        if item.as_table_like().is_none() {
            errors.push(FieldError::new(&["model"], "expected a table of models"));
        }
    }
    if let Some(item) = doc.get("mcp_servers") {
        if item.as_table_like().is_none() {
            errors.push(FieldError::new(
                &["mcp_servers"],
                "expected a table of servers",
            ));
        }
    }
    if let Some(item) = doc.get("harness") {
        if item.as_table_like().is_none() {
            errors.push(FieldError::new(
                &["harness"],
                "expected a table of harnesses",
            ));
        }
    }
    if let Some(item) = doc.get("remote_agents") {
        if item.as_table_like().is_none() {
            errors.push(FieldError::new(
                &["remote_agents"],
                "expected a table of remote agents",
            ));
        }
    }

    validate_app(doc, &mut errors);
    validate_theme(doc, &mut errors);
    validate_models(doc, &mut errors);
    validate_mcp(doc, &mut errors);
    validate_harness(doc, &mut errors);
    validate_remote(doc, &mut errors);
    validate_remote_agents(doc, &mut errors);
    errors
}

/// Check an `[app]` or `[theme]` table against the field table: every known key
/// is checked, every unknown key is preserved untouched.
fn validate_table_fields(doc: &DocumentMut, section: &str, errors: &mut Vec<FieldError>) {
    let Some(item) = doc.get(section) else {
        return;
    };
    let Some(table) = item.as_table_like() else {
        return;
    };
    for spec in FIELDS {
        let Some(leaf) = spec.path.last() else {
            continue;
        };
        if spec.path.first().copied() != Some(section) || spec.path.len() != 2 {
            continue;
        }
        if let Some(item) = table.get(leaf) {
            if let Some(message) = spec.kind.check(item) {
                errors.push(FieldError {
                    path: spec.path.iter().map(|s| s.to_string()).collect(),
                    message,
                });
            }
        }
    }
}

fn validate_app(doc: &DocumentMut, errors: &mut Vec<FieldError>) {
    validate_table_fields(doc, "app", errors);
}

fn validate_theme(doc: &DocumentMut, errors: &mut Vec<FieldError>) {
    validate_table_fields(doc, "theme", errors);
}

fn validate_models(doc: &DocumentMut, errors: &mut Vec<FieldError>) {
    if let Some(default) = doc
        .get("models")
        .and_then(|item| item.as_table_like())
        .and_then(|table| table.get("default"))
    {
        if default.as_str().is_none() {
            errors.push(FieldError::new(
                &["models", "default"],
                "expected a model slug",
            ));
        }
    }
    let Some(models) = doc.get("model").and_then(|item| item.as_table_like()) else {
        return;
    };
    for (slug, item) in models.iter() {
        if !is_valid_map_key(slug) {
            errors.push(FieldError::new(
                &["model", slug],
                "a model slug may only use letters, digits, '-' and '_'",
            ));
        }
        let Some(table) = item.as_table_like() else {
            errors.push(FieldError::new(
                &["model", slug],
                "expected a table of model settings",
            ));
            continue;
        };
        for spec in FIELDS {
            if spec.path.first().copied() != Some("model") || spec.path.len() != 3 {
                continue;
            }
            let Some(leaf) = spec.path.last() else {
                continue;
            };
            let Some(value) = table.get(leaf) else {
                continue;
            };
            if let Some(message) = spec.kind.check(value) {
                errors.push(FieldError {
                    path: vec!["model".to_string(), slug.to_string(), leaf.to_string()],
                    message,
                });
            }
        }
        // A model that names no id at all is legal (the slug is the id), but an
        // explicitly blank one is a mistake worth naming.
        if let Some(model) = table.get("model").and_then(|item| item.as_str()) {
            if model.trim().is_empty() {
                errors.push(FieldError::new(
                    &["model", slug, "model"],
                    "a model id cannot be blank (leave the key out to use the slug)",
                ));
            }
        }
        if let Some(env) = table.get("api_key_env").and_then(|item| item.as_str()) {
            if !is_valid_env_name(env) {
                errors.push(FieldError::new(
                    &["model", slug, "api_key_env"],
                    "an environment variable name is letters, digits and '_'",
                ));
            }
        }
    }
}

fn validate_mcp(doc: &DocumentMut, errors: &mut Vec<FieldError>) {
    let Some(servers) = doc.get("mcp_servers").and_then(|item| item.as_table_like()) else {
        return;
    };
    for (name, item) in servers.iter() {
        if name.trim().is_empty() {
            errors.push(FieldError::new(
                &["mcp_servers"],
                "an MCP server needs a name",
            ));
            continue;
        }
        let Some(table) = item.as_table_like() else {
            errors.push(FieldError::new(
                &["mcp_servers", name],
                "expected a table of server settings",
            ));
            continue;
        };
        for (key, kind) in [
            ("url", FieldKind::Url),
            ("command", FieldKind::Text),
            ("enabled", FieldKind::Bool),
            ("env", FieldKind::Table),
        ] {
            if let Some(value) = table.get(key) {
                if let Some(message) = kind.check(value) {
                    errors.push(FieldError::new(&["mcp_servers", name, key], message));
                }
            }
        }
        if let Some(args) = table.get("args") {
            if let Some(message) = check_string_array(args) {
                errors.push(FieldError::new(&["mcp_servers", name, "args"], message));
            }
        }
        if table.get("url").is_none() && table.get("command").is_none() {
            errors.push(FieldError::new(
                &["mcp_servers", name],
                "an MCP server needs either a url or a command",
            ));
        }
    }
}

fn validate_harness(doc: &DocumentMut, errors: &mut Vec<FieldError>) {
    let Some(harnesses) = doc.get("harness").and_then(|item| item.as_table_like()) else {
        return;
    };
    for (id, item) in harnesses.iter() {
        if !is_valid_map_key(id) {
            errors.push(FieldError::new(
                &["harness", id],
                "a harness id may only use letters, digits, '-' and '_'",
            ));
            continue;
        }
        let Some(table) = item.as_table_like() else {
            errors.push(FieldError::new(
                &["harness", id],
                "expected a table of harness settings",
            ));
            continue;
        };
        if let Some(enabled) = table.get("enabled") {
            if let Some(message) = FieldKind::Bool.check(enabled) {
                errors.push(FieldError::new(&["harness", id, "enabled"], message));
            }
        }
        if let Some(command) = table.get("command") {
            if let Some(message) = FieldKind::Text.check(command) {
                errors.push(FieldError::new(&["harness", id, "command"], message));
            } else if command
                .as_str()
                .is_some_and(|value| value.trim().is_empty())
            {
                errors.push(FieldError::new(
                    &["harness", id, "command"],
                    "a command cannot be blank",
                ));
            }
        }
        if let Some(args) = table.get("args") {
            if let Some(message) = check_string_array(args) {
                errors.push(FieldError::new(&["harness", id, "args"], message));
            }
        }
        if let Some(env) = table.get("env") {
            if let Some(message) = check_string_table(env) {
                errors.push(FieldError::new(&["harness", id, "env"], message));
            }
        }
    }
}

fn validate_remote(doc: &DocumentMut, errors: &mut Vec<FieldError>) {
    let Some(table) = doc.get("remote").and_then(|item| item.as_table_like()) else {
        return;
    };
    for spec in FIELDS {
        if spec.path.first().copied() != Some("remote") || spec.path.len() != 2 {
            continue;
        }
        let Some(leaf) = spec.path.last() else {
            continue;
        };
        if let Some(value) = table.get(leaf) {
            if let Some(message) = spec.kind.check(value) {
                errors.push(FieldError::new(&["remote", leaf], message));
            }
        }
    }
}

fn validate_remote_agents(doc: &DocumentMut, errors: &mut Vec<FieldError>) {
    let Some(agents) = doc
        .get("remote_agents")
        .and_then(|item| item.as_table_like())
    else {
        return;
    };
    for (name, item) in agents.iter() {
        let Some(table) = item.as_table_like() else {
            errors.push(FieldError::new(
                &["remote_agents", name],
                "expected a table of agent settings",
            ));
            continue;
        };
        let provider = table.get("provider").and_then(|item| item.as_str());
        match provider {
            Some(provider)
                if crate::config::RemoteAgentProvider::ALL
                    .iter()
                    .any(|known| known.as_str() == provider) => {}
            Some(provider) => errors.push(FieldError::new(
                &["remote_agents", name, "provider"],
                format!("{provider:?} is not a provider this build knows"),
            )),
            None => errors.push(FieldError::new(
                &["remote_agents", name, "provider"],
                "an agent table needs a provider",
            )),
        }
        for key in ["enabled"] {
            if let Some(value) = table.get(key) {
                if let Some(message) = FieldKind::Bool.check(value) {
                    errors.push(FieldError::new(&["remote_agents", name, key], message));
                }
            }
        }
        for key in [
            "api_key",
            "token_id",
            "token_secret",
            "image",
            "region",
            "environment",
        ] {
            if let Some(value) = table.get(key) {
                if let Some(message) = FieldKind::Text.check(value) {
                    errors.push(FieldError::new(&["remote_agents", name, key], message));
                }
            }
        }
        if let Some(value) = table.get("api_url") {
            if let Some(message) = FieldKind::Url.check(value) {
                errors.push(FieldError::new(
                    &["remote_agents", name, "api_url"],
                    message,
                ));
            }
        }
    }
}

fn validate_environment(doc: &DocumentMut) -> Vec<FieldError> {
    let mut errors = Vec::new();
    let Some(groups) = doc.get("groups").and_then(|item| item.as_table_like()) else {
        return errors;
    };
    for (group, item) in groups.iter() {
        let Some(table) = item.as_table_like() else {
            errors.push(FieldError::new(&["groups", group], "expected a table"));
            continue;
        };
        for (name, value) in table.iter() {
            let ok = value.as_str().is_some()
                || value.as_table_like().is_some_and(|table| {
                    table.len() == 1 && table.get("vault").and_then(|item| item.as_str()).is_some()
                });
            if !ok {
                errors.push(FieldError::new(
                    &["groups", group, name],
                    "expected a string or { vault = \"key\" }",
                ));
            }
        }
    }
    errors
}

fn validate_hosts(doc: &DocumentMut) -> Vec<FieldError> {
    let mut errors = Vec::new();
    let Some(hosts) = doc.get("hosts") else {
        return errors;
    };
    let Some(entries) = hosts.as_array_of_tables() else {
        errors.push(FieldError::new(&["hosts"], "expected an array of tables"));
        return errors;
    };
    for (index, entry) in entries.iter().enumerate() {
        let index = index.to_string();
        match entry.get("name").and_then(|item| item.as_str()) {
            Some(value) if !value.trim().is_empty() => {}
            _ => errors.push(FieldError::new(
                &["hosts", index.as_str()],
                "a saved host needs a name",
            )),
        }
        match entry.get("host").and_then(|item| item.as_str()) {
            Some(value) if !value.trim().is_empty() => {}
            _ => errors.push(FieldError::new(
                &["hosts", index.as_str()],
                "a saved host needs an address",
            )),
        }
        if let Some(port) = entry.get("port") {
            let valid = port
                .as_integer()
                .is_some_and(|value| (1..=65535).contains(&value));
            if !valid {
                errors.push(FieldError::new(
                    &["hosts", index.as_str(), "port"],
                    "a port is an integer in 1..=65535",
                ));
            }
        }
        for key in ["user", "identity"] {
            if let Some(value) = entry.get(key) {
                if value.as_str().is_none() {
                    errors.push(FieldError::new(
                        &["hosts", index.as_str(), key],
                        "expected a string",
                    ));
                }
            }
        }
    }
    errors
}

fn check_string_array(item: &Item) -> Option<String> {
    let Some(array) = item.as_array() else {
        return Some("expected an array of strings".to_string());
    };
    for value in array.iter() {
        if value.as_str().is_none() {
            return Some("expected an array of strings".to_string());
        }
    }
    None
}

fn check_string_table(item: &Item) -> Option<String> {
    let table = item.as_table_like()?;
    for (key, value) in table.iter() {
        if value.as_str().is_none() {
            return Some(format!("{key} must be a string"));
        }
    }
    None
}

/// Whether a TOML key is safe as a bare table key: letters, digits, `-`, `_`.
pub fn is_valid_map_key(key: &str) -> bool {
    !key.trim().is_empty()
        && key.len() <= 128
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn is_valid_env_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && !name.starts_with(|ch: char| ch.is_ascii_digit())
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_hex_colour(value: &str) -> bool {
    let value = value.trim();
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    hex.len() == 6 && hex.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn is_http_url(value: &str) -> bool {
    let value = value.trim();
    value.starts_with("http://") || value.starts_with("https://")
}

/// The gateway URL is dialled by the worker, so a WebSocket URL is the normal
/// form; a plain HTTP(S) URL is accepted for a gateway behind a proxy.
fn is_gateway_url(value: &str) -> bool {
    let value = value.trim();
    is_http_url(value) || value.starts_with("ws://") || value.starts_with("wss://")
}

/// Build the redacted, typed view of the three settings files. `config` is the
/// primary document (it carries the revision and the path); `environment` and
/// `hosts` supply their own sections. An unreadable secondary file contributes
/// its problems and an empty section, never an error: the UI can still show the
/// config and the Advanced page explains the rest.
pub fn redacted_view(
    config: &ConfigDocument,
    environment: Option<&ConfigDocument>,
    hosts: Option<&ConfigDocument>,
) -> ConfigView {
    let typed = config.typed_config().unwrap_or_default();
    let mut problems = config.problems().to_vec();
    let environment_section = match environment {
        Some(document) => match document.typed_environment() {
            Ok(groups) => group_views(&groups),
            Err(error) => {
                problems.push(ConfigProblem::new("environment.toml", error.to_string()));
                Vec::new()
            }
        },
        None => Vec::new(),
    };
    if let Some(document) = environment {
        for problem in document.problems() {
            problems.push(ConfigProblem::new(
                format!("environment.toml.{}", problem.section),
                problem.message.clone(),
            ));
        }
    }
    let hosts_section = match hosts {
        Some(document) => {
            for problem in document.problems() {
                problems.push(ConfigProblem::new(
                    format!("hosts.toml.{}", problem.section),
                    problem.message.clone(),
                ));
            }
            match document.typed_hosts() {
                Ok(hosts) => host_views(&hosts),
                Err(_) => Vec::new(),
            }
        }
        None => Vec::new(),
    };

    let sections = ConfigSections {
        app: app_view(&typed.app),
        theme: theme_view(&typed.theme),
        models: model_views(&typed),
        default_model: typed.models.default.clone(),
        harnesses: harness_views(&typed),
        mcp: mcp_views(&typed),
        remote: RemoteView {
            mode: match typed.remote.mode {
                crate::config::RemoteMode::Platform => "platform".to_string(),
                crate::config::RemoteMode::Gateway => "gateway".to_string(),
            },
            gateway_url: typed.remote.gateway_url.clone(),
            worker_token: SecretState::set(typed.remote.worker_token().is_some()),
        },
        remote_agents: remote_agent_views(&typed),
        hosts: hosts_section,
        environment: environment_section,
    };

    ConfigView {
        file: ConfigFile::Config,
        path: config.path().display().to_string(),
        revision: config.revision().clone(),
        mode: config.mode(),
        sections,
        problems,
    }
}

/// The view of a document on its own: the config view for `Config`, a view whose
/// relevant section is filled for the other two files.
pub fn view_for(file: ConfigFile, document: &ConfigDocument) -> ConfigView {
    match file {
        ConfigFile::Config => redacted_view(document, None, None),
        _ => {
            let mut view = redacted_view(document, None, None);
            view.file = file;
            view.path = document.path().display().to_string();
            view.revision = document.revision().clone();
            view.mode = document.mode();
            view.sections = ConfigSections::default();
            view.problems = document.problems().to_vec();
            match file {
                ConfigFile::Environment => {
                    if let Ok(groups) = document.typed_environment() {
                        view.sections.environment = group_views(&groups);
                    }
                }
                ConfigFile::Hosts => {
                    if let Ok(hosts) = document.typed_hosts() {
                        view.sections.hosts = host_views(&hosts);
                    }
                }
                ConfigFile::Config => {}
            }
            view
        }
    }
}

fn app_view(app: &crate::config::AppConfig) -> AppView {
    AppView {
        friendly_name: app.friendly_name.clone(),
        usage: app.usage.clone(),
        telemetry: app.telemetry,
        in_app_notifications: app.in_app_notifications,
        invert_scroll: app.invert_scroll,
        scroll_speed: app.scroll_speed,
        font_size: app.font_size,
        auto_save_on_close: app.auto_save_on_close,
        approval_mode: app.approval_mode.clone(),
        auto_approve: app.auto_approve,
        vim_mode: app.vim_mode,
    }
}

fn theme_view(theme: &crate::config::ThemeConfig) -> ThemeView {
    ThemeView {
        dark: Some(theme.dark),
        preset: theme.preset.clone(),
        accent: theme.accent.clone(),
        primary: theme.primary.clone(),
        secondary: theme.secondary.clone(),
        bg: theme.bg.clone(),
        surface: theme.surface.clone(),
    }
}

fn model_views(config: &GobleConfig) -> Vec<ModelView> {
    config
        .model
        .iter()
        .map(|(slug, model)| {
            let (api_key_source, api_key_env) = match model.resolved_key() {
                Some(_) => (ApiKeySource::Inline, None),
                None => match model
                    .api_key_env
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    Some(name) => (ApiKeySource::Env, Some(name.to_string())),
                    None => (ApiKeySource::Missing, None),
                },
            };
            ModelView {
                slug: slug.clone(),
                name: model.name.clone(),
                model_id: model.model_id(slug),
                base_url: model.base_url.clone(),
                api_key: SecretState::set(model.resolved_key().is_some()),
                api_key_env,
                api_key_source,
                max_completion_tokens: model.max_completion_tokens,
                context_window: model.context_window,
                hidden: model.hidden.unwrap_or(false),
                is_default: config.models.default.as_deref() == Some(slug),
            }
        })
        .collect()
}

fn harness_views(config: &GobleConfig) -> Vec<HarnessView> {
    HARNESSES
        .iter()
        .map(|spec| {
            let configured = config.harness.get(spec.id);
            HarnessView {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                configured: configured.is_some(),
                enabled: configured.map(|entry| entry.enabled).unwrap_or(true),
                command: configured.and_then(|entry| entry.command.clone()),
                args: configured
                    .map(|entry| entry.args.clone())
                    .unwrap_or_default(),
                env: configured
                    .map(|entry| {
                        entry
                            .env
                            .iter()
                            .map(|(key, value)| (key.clone(), value.clone()))
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        })
        .collect()
}

fn mcp_views(config: &GobleConfig) -> Vec<McpView> {
    config
        .mcp_servers
        .iter()
        .map(|(name, server)| McpView {
            name: name.clone(),
            url: server.url.clone(),
            command: server.command.clone(),
            args: server.args.clone(),
            env: server
                .env
                .iter()
                .flatten()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            enabled: server.enabled,
        })
        .collect()
}

fn remote_agent_views(config: &GobleConfig) -> Vec<RemoteAgentView> {
    config
        .remote_agents
        .iter()
        .map(|(name, agent)| RemoteAgentView {
            name: name.clone(),
            provider: agent.provider.as_str().to_string(),
            enabled: agent.enabled,
            api_key: SecretState::set(
                agent
                    .api_key
                    .as_deref()
                    .is_some_and(|v| !v.trim().is_empty()),
            ),
            token_id: SecretState::set(
                agent
                    .token_id
                    .as_deref()
                    .is_some_and(|v| !v.trim().is_empty()),
            ),
            token_secret: SecretState::set(
                agent
                    .token_secret
                    .as_deref()
                    .is_some_and(|v| !v.trim().is_empty()),
            ),
            api_url: agent.api_url.clone(),
            image: agent.image.clone(),
            region: agent.region.clone(),
            environment: agent.environment.clone(),
            missing_credentials: agent
                .missing_credentials()
                .iter()
                .map(|key| key.to_string())
                .collect(),
        })
        .collect()
}

/// An empty view for a fresh install: what `settings_document` returns before a
/// file has been read.
pub fn empty_view(file: ConfigFile, path: &std::path::Path) -> ConfigView {
    ConfigView::empty(file, path, ConfigRevision::missing())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn document(file: ConfigFile, text: &str) -> ConfigDocument {
        ConfigDocument::from_text(
            file,
            Path::new("/nonexistent/goble-settings-test"),
            text.to_string(),
            0,
        )
        .expect("valid TOML")
    }

    fn errors(text: &str) -> Vec<String> {
        let doc = document(ConfigFile::Config, text).document().clone();
        validate(ConfigFile::Config, &doc)
            .into_iter()
            .map(|error| format!("{}: {}", error.dotted(), error.message))
            .collect()
    }

    #[test]
    fn a_well_formed_config_validates_clean() {
        let text = r##"
[app]
friendly_name = "Ada"
telemetry = false
in_app_notifications = true
invert_scroll = false
scroll_speed = 50
font_size = 1.0
auto_save_on_close = false
approval_mode = "manual"
auto_approve = false
vim_mode = true

[theme]
dark = true
preset = "goblenight"
accent = "#14b8a6"
primary = "#e8e8e8"
secondary = "#8e8e8e"
bg = "#0e0e0e"
surface = "#101010"

[model.deepseek]
model = "deepseek-flash"
base_url = "https://api.deepseek.com"
api_key = "sk-live"
api_key_env = "DEEPSEEK_API_KEY"
max_completion_tokens = 16384
context_window = 256000
hidden = false

[models]
default = "deepseek"

[mcp_servers.penpot]
url = "http://localhost:4400/mcp"
enabled = true

[harness.codex]
enabled = true
command = "/opt/homebrew/bin/codex"
args = ["--full-auto"]
env = { RUST_LOG = "info" }

[remote]
mode = "gateway"
gateway_url = "https://gateway.example.com"
worker_token = "wt-1"

[remote_agents.builder]
provider = "e2b"
api_key = "e2b-key"
"##;
        assert!(errors(text).is_empty(), "{:?}", errors(text));
    }

    #[test]
    fn invalid_app_theme_and_model_values_name_their_paths() {
        let text = r##"
[app]
scroll_speed = 500
font_size = 4.0
approval_mode = "yolo"
telemetry = "yes"

[theme]
accent = "teal"
dark = "no"

[model."bad.slug"]
model = ""
base_url = "not-a-url"
max_completion_tokens = 0
api_key_env = "1BAD"

[models]
default = 7
"##;
        let found = errors(text);
        for expected in [
            "app.scroll_speed",
            "app.font_size",
            "app.approval_mode",
            "app.telemetry",
            "theme.accent",
            "theme.dark",
            "models.default",
            "model.bad.slug",
            "model.bad.slug.model",
            "model.bad.slug.base_url",
            "model.bad.slug.max_completion_tokens",
            "model.bad.slug.api_key_env",
        ] {
            assert!(
                found.iter().any(|error| error.starts_with(expected)),
                "{expected} missing from {found:?}"
            );
        }
    }

    #[test]
    fn mcp_harness_and_remote_tables_are_checked() {
        let text = r##"
[mcp_servers.empty]

[mcp_servers.weird]
url = "ftp://example.test"
args = "not-an-array"

[harness.codex]
enabled = "yes"
command = "   "
args = [1, 2]
env = { A = 1 }

[remote]
mode = "wireguard"
gateway_url = "gateway.example.com"

[remote_agents.builder]
provider = "teleport"
enabled = "no"
api_url = "example"
"##;
        let found = errors(text);
        for expected in [
            "mcp_servers.empty",
            "mcp_servers.weird.url",
            "mcp_servers.weird.args",
            "harness.codex.enabled",
            "harness.codex.command",
            "harness.codex.args",
            "harness.codex.env",
            "remote.mode",
            "remote.gateway_url",
            "remote_agents.builder.provider",
            "remote_agents.builder.enabled",
            "remote_agents.builder.api_url",
        ] {
            assert!(
                found.iter().any(|error| error.starts_with(expected)),
                "{expected} missing from {found:?}"
            );
        }
    }

    #[test]
    fn the_environment_and_hosts_files_validate() {
        let environment = document(
            ConfigFile::Environment,
            "[groups.alpha]\nGOOD = \"value\"\nVAULTED = { vault = \"key\" }\n[groups.broken]\nNUMBER = 3\n",
        );
        let found = validate(ConfigFile::Environment, environment.document());
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].dotted(), "groups.broken.NUMBER");

        let hosts = document(
            ConfigFile::Hosts,
            "[[hosts]]\nname = \"prod\"\nhost = \"prod.example.com\"\nport = 70000\n[[hosts]]\nhost = \"missing-name\"\n",
        );
        let found = validate(ConfigFile::Hosts, hosts.document());
        assert!(
            found.iter().any(|error| error.dotted().contains("port")),
            "{found:?}"
        );
        assert!(
            found.iter().any(|error| error.message.contains("name")),
            "{found:?}"
        );
    }

    #[test]
    fn the_redacted_view_never_carries_a_secret() {
        let config = document(
            ConfigFile::Config,
            r##"
[app]
friendly_name = "Ada"

[model.deepseek]
model = "deepseek-flash"
base_url = "https://api.deepseek.com"
api_key = "sk-secret-inline"
max_completion_tokens = 16384

[models]
default = "deepseek"

[model.envmodel]
model = "kimi-k2"
api_key_env = "KIMI_API_KEY"

[mcp_servers.penpot]
url = "http://localhost:4400/mcp"

[remote]
mode = "gateway"
gateway_url = "https://gateway.example.com"
worker_token = "worker-token-secret"

[remote_agents.builder]
provider = "e2b"
api_key = "e2b-api-key-secret"
"##,
        );
        let environment = document(
            ConfigFile::Environment,
            "[groups.alpha]\nAPI_TOKEN = \"env-value-secret\"\nVAULTED = { vault = \"deploy\" }\n",
        );
        let hosts = document(
            ConfigFile::Hosts,
            "[[hosts]]\nname = \"prod\"\nhost = \"prod.example.com\"\n",
        );
        let view = redacted_view(&config, Some(&environment), Some(&hosts));
        let json = serde_json::to_string(&view).expect("serialize");
        for secret in [
            "sk-secret-inline",
            "worker-token-secret",
            "e2b-api-key-secret",
            "env-value-secret",
        ] {
            assert!(!json.contains(secret), "{secret} leaked into {json}");
        }
        // The view still says a credential is configured, and where it comes from.
        let model = view
            .sections
            .models
            .iter()
            .find(|model| model.slug == "deepseek")
            .expect("model");
        assert!(model.api_key.set);
        assert_eq!(model.api_key_source, ApiKeySource::Inline);
        assert!(model.is_default);
        let env_model = view
            .sections
            .models
            .iter()
            .find(|model| model.slug == "envmodel")
            .expect("env model");
        assert_eq!(env_model.api_key_source, ApiKeySource::Env);
        assert_eq!(env_model.api_key_env.as_deref(), Some("KIMI_API_KEY"));
        assert!(view.sections.remote.worker_token.set);
        assert!(view.sections.remote_agents[0].api_key.set);
        // The vault reference is a key name, not a secret, and is kept.
        let group = &view.sections.environment[0];
        assert!(matches!(
            &group.variables[1].value,
            super::super::document::EnvValueView::Vault { vault } if vault == "deploy"
        ));
        // Hosts are part of the view.
        assert_eq!(view.sections.hosts[0].name, "prod");
        assert!(view.problems.is_empty(), "{:?}", view.problems);
    }

    #[test]
    fn the_field_table_drives_defaults_and_reset() {
        let doc = document(
            ConfigFile::Config,
            "[app]\nscroll_speed = 80\ntelemetry = true\n",
        );
        let document = doc.document();
        let spec = field_spec(&["app".to_string(), "scroll_speed".to_string()]).expect("spec");
        assert_eq!(default_value(spec), Some("50"));
        assert!(!is_default(document, spec));
        assert_eq!(display_value(document, spec).as_deref(), Some("80"));

        let spec = field_spec(&["app".to_string(), "vim_mode".to_string()]).expect("spec");
        // Absent reads as the default, which is what drives the reset affordance.
        assert!(is_default(document, spec));
        assert_eq!(display_value(document, spec), None);
    }

    #[test]
    fn a_secret_field_never_returns_its_value() {
        let doc = document(ConfigFile::Config, "[model.x]\napi_key = \"sk-live\"\n");
        let document = doc.document();
        let spec = field_spec(&["model".to_string(), "x".to_string(), "api_key".to_string()])
            .expect("spec");
        assert!(spec.secret);
        assert_eq!(display_value(document, spec), None);
    }

    #[test]
    fn a_websocket_gateway_url_is_accepted() {
        // The worker dials the gateway over WebSocket, so `wss://` is the
        // normal config value; validation must not call it invalid.
        let text = r#"
[remote]
mode = "gateway"
gateway_url = "wss://gateway.example/ws/worker"
worker_token = "wt-1"
"#;
        assert!(errors(text).is_empty(), "{:?}", errors(text));

        let plain = text.replace(
            "wss://gateway.example/ws/worker",
            "ws://127.0.0.1:8788/ws/worker",
        );
        assert!(errors(&plain).is_empty(), "{:?}", errors(&plain));

        // Any other scheme is still refused.
        let other = text.replace("wss://gateway.example/ws/worker", "ftp://gateway/ws");
        assert!(!errors(&other).is_empty(), "ftp is not a gateway URL");
    }
}
