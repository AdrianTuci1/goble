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
//!
//! `[remote_agents.<name>]` is goble's own section: it declares the sandbox
//! providers a harness can be shipped to. This build models and reports the
//! declaration; it does not provision or call a provider.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

mod migrate;
#[cfg(test)]
mod tests;

pub use migrate::{LegacyModelConfig, LlmConfig, ProviderConfig};

/// The commented block `config.toml` opens with, documenting the file's own
/// shape. It is written by [`GobleConfig::to_toml`] — the one writer for the
/// file, seeding and saving alike — so a save cannot drop it the way a rewrite
/// of the body alone did.
pub const CONFIG_HEADER: &str = "\
# goble configuration, in grok's own layout: one [model.<slug>] table per model,
# the model new sessions start on under [models] default, and MCP servers under
# [mcp_servers.<name>]. Every section and every key is optional, so a file that
# sets one thing parses, and a file written for grok reads here.
#
# A model. The slug is its name in this file, `model` is the id sent to the API
# (the slug itself when it is left out), `base_url` picks the endpoint, and
# `api_key` is the credential, written inline. `hidden = true` keeps an entry
# out of the picker while it stays usable by naming it.
#
#   [model.deepseek]
#   model = \"deepseek-flash\"
#   name = \"Deepseek-V4-Flash\"
#   base_url = \"https://api.deepseek.com\"
#   api_key = \"sk-...\"
#   max_completion_tokens = 16384
#   context_window = 256000
#
# Which model the app starts on, by the slug of a table above:
#
#   [models]
#   default = \"deepseek\"
#
# An MCP server is an HTTP endpoint (`url`) or a stdio command (`command`, with
# `args`); `enabled` is true unless it says otherwise.
#
#   [mcp_servers.penpot]
#   url = \"http://localhost:4400/mcp\"
#
# [theme] is goble's own section and grok has no equivalent: `dark`, `accent`,
# and the optional `primary` / `secondary`.
#
# A remote agent declares a sandbox provider the harness can be shipped to.
# The table's name is the agent's. `provider` is one of modal, e2b, daytona,
# box, docker or ssh; `enabled` is true unless it says otherwise. Modal reads
# `token_id` and `token_secret`, the hosted providers read `api_key`, and
# docker and ssh need no credentials. `api_url`, `image`, `region` and
# `environment` are optional. Nothing is provisioned in this build: Settings
# -> Agent & Approval reports what each table declares and what it is missing.
#
#   [remote_agents.builder]
#   provider = \"modal\"
#   token_id = \"...\"
#   token_secret = \"...\"
#   image = \"ubuntu:24.04\"
#
#   [remote_agents.reviewer]
#   provider = \"e2b\"
#   api_key = \"...\"
#   region = \"us-east-1\"
#
# Sections this app does not model (grok's [cli], [ui], [privacy], the
# marketplace) are kept exactly as they were written. Editing this file by hand
# is expected: the app reads it when it starts, and writes it back — this header
# included — when a setting that lives here changes.

";

/// The top-level sections this schema models. Everything else a file carries
/// (grok's `[cli]`, `[ui]`, `[marketplace]`, `[privacy]`, …) is kept verbatim in
/// [`GobleConfig::extra`] so a save does not drop another tool's settings.
const SECTIONS: [&str; 6] = [
    "model",
    "models",
    "mcp_servers",
    "theme",
    "llm",
    "remote_agents",
];

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
    /// `[remote_agents.<name>]` — the sandbox providers a harness can be
    /// shipped to, keyed by the name the user gave the agent.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub remote_agents: BTreeMap<String, RemoteAgentConfig>,
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

/// A sandbox provider a remote agent can run on. The serialized names are the
/// ones written in `config.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemoteAgentProvider {
    Modal,
    E2b,
    Daytona,
    Box,
    Docker,
    Ssh,
}

impl RemoteAgentProvider {
    /// The provider's name as the config file writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            RemoteAgentProvider::Modal => "modal",
            RemoteAgentProvider::E2b => "e2b",
            RemoteAgentProvider::Daytona => "daytona",
            RemoteAgentProvider::Box => "box",
            RemoteAgentProvider::Docker => "docker",
            RemoteAgentProvider::Ssh => "ssh",
        }
    }

    /// The credential keys this provider needs, in the order a missing-credential
    /// line names them. Docker and SSH need none.
    fn credential_keys(self) -> &'static [&'static str] {
        match self {
            RemoteAgentProvider::Modal => &["MODAL_TOKEN_ID", "MODAL_TOKEN_SECRET"],
            RemoteAgentProvider::E2b => &["E2B_API_KEY"],
            RemoteAgentProvider::Daytona => &["DAYTONA_API_KEY"],
            RemoteAgentProvider::Box => &["BOX_API_KEY"],
            RemoteAgentProvider::Docker | RemoteAgentProvider::Ssh => &[],
        }
    }
}

impl std::fmt::Display for RemoteAgentProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One `[remote_agents.<name>]` table: a sandbox provider the harness can be
/// shipped to. Phase one models the declaration only — nothing here provisions
/// a sandbox or reaches a provider's API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteAgentConfig {
    /// The sandbox provider. Required: a table without one is reported on load
    /// rather than guessed at, so there is no `#[serde(default)]` here.
    pub provider: RemoteAgentProvider,
    /// Whether the agent is offered at all.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// The provider's API key (E2B, Daytona, Box).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Modal's token id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_id: Option<String>,
    /// Modal's token secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_secret: Option<String>,
    /// A provider endpoint override. No provider requires one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
    /// The sandbox image the harness is shipped in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// The provider region.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// The sandbox environment the harness runs in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
}

impl RemoteAgentConfig {
    /// The credential keys this table has not set, named as the environment
    /// variables a user would supply them as. Empty means the table is ready.
    pub fn missing_credentials(&self) -> Vec<&'static str> {
        let filled = |value: &Option<String>| {
            value
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
        };
        self.provider
            .credential_keys()
            .iter()
            .copied()
            .filter(|key| match *key {
                "MODAL_TOKEN_ID" => !filled(&self.token_id),
                "MODAL_TOKEN_SECRET" => !filled(&self.token_secret),
                _ => !filled(&self.api_key),
            })
            .collect()
    }

    /// What the Settings row says about this table: `ready`, `disabled`, or the
    /// credential keys it is missing.
    pub fn status_label(&self) -> String {
        if !self.enabled {
            return "disabled".to_string();
        }
        let missing = self.missing_credentials();
        if missing.is_empty() {
            "ready".to_string()
        } else {
            format!("missing {}", missing.join(", "))
        }
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

fn is_true(value: &bool) -> bool {
    *value
}

fn default_accent() -> String {
    "#14b8a6".to_string()
}

impl GobleConfig {
    /// Write the file: [`CONFIG_HEADER`] first, then the new layout. Any `extra`
    /// section is appended as it was read, so a section goble does not model
    /// survives the round-trip. This is the one writer for `config.toml`, so
    /// every path — the seeded file and every save — carries the header.
    pub fn to_toml(&self) -> anyhow::Result<String> {
        let body = toml::to_string(self).map_err(anyhow::Error::from)?;
        Ok(format!("{CONFIG_HEADER}{body}"))
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

        // The remote agents are read one table at a time: a provider name this
        // build does not know costs that agent, not the others and not the
        // file, so a newer config still loads its models and theme.
        if let Some(value) = table.get("remote_agents") {
            match value.as_table() {
                Some(entries) => {
                    for (name, entry) in entries {
                        match entry.clone().try_into::<RemoteAgentConfig>() {
                            Ok(parsed) => {
                                config.remote_agents.insert(name.clone(), parsed);
                            }
                            Err(e) => problems.push(format!("[remote_agents.{name}] ignored: {e}")),
                        }
                    }
                }
                None => problems.push("[remote_agents] ignored: not a table".to_string()),
            }
        }

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
