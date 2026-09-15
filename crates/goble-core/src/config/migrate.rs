//! Reading a `~/.goble/config.toml` written before the grok-shaped schema, and
//! folding it into the new one.
//!
//! The old file held `[llm]` (a provider list, a flat model list, a default
//! provider/model) and a required `version`. [`GobleConfig::absorb_legacy`] turns
//! each entry into a `[model.<slug>]` table and clears the section, so the file
//! is migrated in memory on load and rewritten in the new layout on the next
//! save. The section itself is never written back.

use serde::{Deserialize, Serialize};

use super::{GobleConfig, ModelConfig};

/// The pre-grok `[llm]` section, kept so a file written before the migration
/// still loads. Never written back once absorbed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LlmConfig {
    pub default_provider: String,
    pub default_model: String,
    pub providers: Vec<ProviderConfig>,
    pub models: Vec<LegacyModelConfig>,
}

impl LlmConfig {
    pub(crate) fn is_empty(&self) -> bool {
        self.default_provider.is_empty()
            && self.default_model.is_empty()
            && self.providers.is_empty()
            && self.models.is_empty()
    }
}

/// A pre-grok `[[llm.providers]]` entry. The store holds the live per-provider
/// credential; this is the config file's copy of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    #[serde(default)]
    pub api_key_secret_id: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: String,
}

/// A pre-grok `[[llm.models]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyModelConfig {
    pub id: String,
    pub provider: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub api_key_secret_id: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default = "super::default_true")]
    pub enabled: bool,
}

impl GobleConfig {
    /// Fold a legacy `[llm]` section into `[model.*]` / `[models]` and clear it.
    /// Idempotent: an already-new config is left untouched.
    pub fn absorb_legacy(&mut self) {
        if self.llm.is_empty() {
            return;
        }
        let legacy = std::mem::take(&mut self.llm);
        let mut default_slug = None;

        for model in &legacy.models {
            if model.id.trim().is_empty() {
                continue;
            }
            let slug = self.migrate_slug(&model.id, &model.provider);
            let entry = ModelConfig {
                model: model.id.clone(),
                name: Some(if model.label.trim().is_empty() {
                    model.id.clone()
                } else {
                    model.label.clone()
                }),
                base_url: model.base_url.clone(),
                // The old field was never resolved through the vault — no reader
                // looked it up — so the string it held was the key itself.
                api_key: Some(model.api_key_secret_id.clone()).filter(|key| !key.trim().is_empty()),
                max_completion_tokens: None,
                context_window: None,
                hidden: (!model.enabled).then_some(true),
            };
            self.model.insert(slug.clone(), entry);
            if legacy.default_model == model.id
                && (legacy.default_provider.is_empty() || legacy.default_provider == model.provider)
            {
                default_slug = Some(slug);
            }
        }

        // A provider with no `[[llm.models]]` entry holds the only record of its
        // endpoint and key, so it becomes a model entry of its own.
        for provider in &legacy.providers {
            if provider.name.trim().is_empty() || self.model.contains_key(&provider.name) {
                continue;
            }
            let id = if provider.model.trim().is_empty() {
                provider.name.clone()
            } else {
                provider.model.clone()
            };
            self.model.insert(
                provider.name.clone(),
                ModelConfig {
                    model: id.clone(),
                    name: Some(provider.name.clone()),
                    base_url: provider.base_url.clone(),
                    api_key: Some(provider.api_key_secret_id.clone())
                        .filter(|key| !key.trim().is_empty()),
                    ..Default::default()
                },
            );
        }

        if self.models.default.is_none() {
            // The old default named a model id: the slug of the entry declaring
            // it, or the id itself when no entry does (it may name a catalog
            // model), because `[models] default` holds a `[model.<slug>]` key.
            self.models.default = default_slug.or_else(|| {
                let id = legacy.default_model.trim();
                if id.is_empty() {
                    return None;
                }
                Some(
                    self.model_for(id)
                        .map(|(slug, _)| slug.to_string())
                        .unwrap_or_else(|| id.to_string()),
                )
            });
        }
    }

    /// A `[model.<slug>]` key for a migrated entry: the model id, falling back to
    /// `provider-id` and then a counter when the id is taken.
    fn migrate_slug(&self, id: &str, provider: &str) -> String {
        let base = id.trim();
        if !self.model.contains_key(base) {
            return base.to_string();
        }
        let combined = format!("{}-{}", provider.trim(), base);
        if !provider.trim().is_empty() && !self.model.contains_key(&combined) {
            return combined;
        }
        let mut n = 2;
        loop {
            let candidate = format!("{base}-{n}");
            if !self.model.contains_key(&candidate) {
                return candidate;
            }
            n += 1;
        }
    }
}
