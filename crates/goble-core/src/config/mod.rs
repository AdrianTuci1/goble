//! `~/.goble/config.toml`.
//!
//! The layout is grok's (`~/.grok/config.toml`): one `[model.<slug>]` table per
//! model, the default selection under `[models] default`, and MCP servers under
//! `[mcp_servers.<name>]`. Every section and field is optional, so a file that
//! sets a single key parses, and a file written for grok reads here.
//!
//! `[theme]` is goble's own section: grok has no equivalent, and goble keeps the
//! setting rather than dropping it.
//!
//! A file in the pre-grok shape (`version` + `[llm]` + `[theme]`) still loads.
//! [`GobleConfig::load_toml`] folds `[llm]` into `[model.*]` / `[models]` and
//! clears it, so the next [`GobleConfig::to_toml`] writes the new layout — the
//! migration a save performs, with no separate migration step to run.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

mod migrate;
#[cfg(test)]
mod tests;

pub use migrate::{LegacyModelConfig, LlmConfig, ProviderConfig};

/// The top-level sections this schema models. Everything else a file carries
/// (grok's `[cli]`, `[ui]`, `[marketplace]`, `[privacy]`, …) is kept verbatim in
/// [`GobleConfig::extra`] so a save does not drop another tool's settings.
const SECTIONS: [&str; 5] = ["model", "models", "mcp_servers", "theme", "llm"];

/// `~/.goble/config.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GobleConfig {
    /// `[model.<slug>]` — the model catalog, keyed by slug.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub model: BTreeMap<String, ModelConfig>,
    /// `[models]` — which model new sessions start with.
    #[serde(skip_serializing_if = "ModelsConfig::is_unset")]
    pub models: ModelsConfig,
    /// `[mcp_servers.<name>]` — MCP servers declared in the config file.
    /// goble's live MCP registry is the store; this section is the file's copy.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub mcp_servers: BTreeMap<String, McpServerConfig>,
    /// `[theme]` — goble's own appearance settings.
    pub theme: ThemeConfig,
    /// The pre-grok `[llm]` section, read only. [`GobleConfig::absorb_legacy`]
    /// moves it into `[model.*]` / `[models]` and clears it, so an old file
    /// migrates on load and is rewritten in the new layout on the next save.
    #[serde(skip_serializing_if = "LlmConfig::is_empty")]
    pub llm: LlmConfig,
    /// Sections this schema does not model, kept as they were parsed so saving
    /// does not drop them.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

/// `[models]` — the default selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ModelsConfig {
    /// The `[model.<slug>]` key new sessions start with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

impl ModelsConfig {
    fn is_unset(&self) -> bool {
        self.default.is_none()
    }
}

/// One `[model.<slug>]` table: a model goble can talk to, with the endpoint and
/// the key that reach it. The field names and their optionality mirror grok's
/// `[model.<id>]`, so the same table is readable by both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ModelConfig {
    /// `model` — the id sent to the API. Empty means the slug is the id.
    pub model: String,
    /// `name` — the label shown in the picker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The provider endpoint base URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// The literal API key, inline, as grok writes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Maximum tokens per response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,
    /// Total context window in tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// Declared but not offered in the picker (grok's `hidden`), still usable by
    /// naming it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

impl ModelConfig {
    /// The id sent to the API: the `model` field, or the slug when it is empty.
    pub fn model_id(&self, slug: &str) -> String {
        if self.model.trim().is_empty() {
            slug.to_string()
        } else {
            self.model.clone()
        }
    }

    /// The inline key, when it is set and not blank.
    pub fn resolved_key(&self) -> Option<&str> {
        self.api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
    }
}

/// One `[mcp_servers.<name>]` table. Either an HTTP endpoint (`url`) or a stdio
/// command (`command` + `args`), as grok declares them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct McpServerConfig {
    /// Streamable-HTTP endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// stdio server: the program to run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            url: None,
            command: None,
            args: Vec::new(),
            env: None,
            enabled: true,
        }
    }
}

/// `[theme]` — goble's own appearance settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    #[serde(default = "default_true")]
    pub dark: bool,
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary: Option<String>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            dark: true,
            accent: default_accent(),
            primary: None,
            secondary: None,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_accent() -> String {
    "#14b8a6".to_string()
}

impl GobleConfig {
    /// Write the new layout. Any `extra` section is appended as it was read, so
    /// a section goble does not model survives the round-trip.
    pub fn to_toml(&self) -> anyhow::Result<String> {
        toml::to_string(self).map_err(anyhow::Error::from)
    }

    /// Strict parse: a field whose value has the wrong type fails the whole
    /// file. [`GobleConfig::load_toml`] is the forgiving path a load uses.
    pub fn from_toml(s: &str) -> anyhow::Result<Self> {
        let mut config: Self = toml::from_str(s).map_err(anyhow::Error::from)?;
        // A top-level scalar (the old `version = 1`) cannot sit in `extra`: TOML
        // requires values before tables, and serializing it back would emit it
        // after them.
        config.extra.retain(|_, value| value.is_table());
        Ok(config)
    }

    /// Parse a config file for loading: keep every section that parses, migrate
    /// a legacy `[llm]` section, and report one message per section dropped.
    /// `Err` only when the text is not TOML at all — then nothing in it can be
    /// trusted and the caller keeps the config it already has.
    pub fn load_toml(s: &str) -> anyhow::Result<(Self, Vec<String>)> {
        let table: toml::Table = toml::from_str(s).map_err(anyhow::Error::from)?;
        let mut problems = Vec::new();
        let mut config = Self::default();

        macro_rules! section {
            ($key:literal, $field:expr) => {
                if let Some(value) = table.get($key) {
                    match value.clone().try_into() {
                        Ok(parsed) => $field = parsed,
                        Err(e) => problems.push(format!("[{0}] ignored: {e}", $key)),
                    }
                }
            };
        }

        section!("model", config.model);
        section!("models", config.models);
        section!("mcp_servers", config.mcp_servers);
        section!("theme", config.theme);
        section!("llm", config.llm);

        for (key, value) in table {
            if SECTIONS.contains(&key.as_str()) || !value.is_table() {
                continue;
            }
            config.extra.insert(key, value);
        }

        config.absorb_legacy();
        Ok((config, problems))
    }

    /// The `[model.<slug>]` entry `name` refers to — by slug, or by the id its
    /// `model` field holds.
    pub fn model_for(&self, name: &str) -> Option<(&str, &ModelConfig)> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        if let Some((slug, entry)) = self.model.get_key_value(name) {
            return Some((slug.as_str(), entry));
        }
        self.model
            .iter()
            .find(|(_, entry)| entry.model == name)
            .map(|(slug, entry)| (slug.as_str(), entry))
    }

    /// Every model id the config declares, in slug order, skipping `hidden`
    /// entries.
    pub fn model_ids(&self) -> Vec<String> {
        self.model
            .iter()
            .filter(|(_, entry)| entry.hidden != Some(true))
            .map(|(slug, entry)| entry.model_id(slug))
            .collect()
    }

    /// The id `[models] default` names: the `model` field of that slug's table,
    /// or the slug itself when it has no table.
    pub fn default_model_id(&self) -> Option<String> {
        let slug = self.models.default.as_deref()?.trim();
        if slug.is_empty() {
            return None;
        }
        Some(match self.model.get(slug) {
            Some(entry) => entry.model_id(slug),
            None => slug.to_string(),
        })
    }
}
