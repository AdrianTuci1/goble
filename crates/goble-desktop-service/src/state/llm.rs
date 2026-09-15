use std::sync::Arc;

use goble_core::llm::{self, CompletionRequest, LlmProvider};

use super::{DesktopState, Intent, LlmSetting};

fn fallback_mock() -> (Arc<dyn LlmProvider>, String) {
    (
        Arc::new(llm::MockProvider::new(
            "mock",
            llm::CompletionResponse::new(
                "No LLM provider configured or API key missing. Add one in Settings.",
                Vec::new(),
            ),
        )),
        "mock".to_string(),
    )
}

impl DesktopState {
    pub fn set_llm_setting(
        &self,
        provider: &str,
        api_key: &str,
        base_url: Option<&str>,
        model: &str,
        temperature: Option<f32>,
    ) -> anyhow::Result<()> {
        self.store
            .lock()
            .set_llm_setting(provider, api_key, base_url, model, temperature)?;
        Ok(())
    }

    pub fn resolve_llm_provider(
        &self,
        provider_name: &str,
        model_override: &str,
    ) -> (Arc<dyn LlmProvider>, String) {
        let provider_name = if provider_name.is_empty() {
            "openai"
        } else {
            provider_name
        };
        // A `[model.<slug>]` entry carries its own endpoint and key, so a model
        // the config declares is reachable without the store's per-provider
        // setting. It wins for the model it names; anything else falls through
        // to the store exactly as before.
        if let Some(resolved) = self.config_model_provider(provider_name, model_override) {
            return resolved;
        }
        match provider_name.to_lowercase().as_str() {
            "openai" | "openrouter" => {
                let setting = self.get_llm_setting(provider_name);
                if let Some(s) = setting {
                    if !s.api_key.is_empty() {
                        let base = s.base_url.unwrap_or_else(|| {
                            if provider_name == "openai" {
                                "https://api.openai.com/v1".to_string()
                            } else {
                                "https://openrouter.ai/api/v1".to_string()
                            }
                        });
                        let provider: Arc<dyn LlmProvider> = if provider_name == "openai" {
                            Arc::new(llm::OpenAiProvider::new("openai", s.api_key, base))
                        } else {
                            Arc::new(llm::OpenRouterProvider::new(s.api_key))
                        };
                        let model = if model_override.is_empty() {
                            s.model
                        } else {
                            model_override.to_string()
                        };
                        return (provider, model);
                    }
                }
                fallback_mock()
            }
            "anthropic" => {
                let setting = self.get_llm_setting("anthropic");
                if let Some(s) = setting {
                    if !s.api_key.is_empty() {
                        return (
                            Arc::new(llm::AnthropicProvider::new(s.api_key)),
                            if model_override.is_empty() {
                                s.model
                            } else {
                                model_override.to_string()
                            },
                        );
                    }
                }
                fallback_mock()
            }
            "ollama" => {
                let setting = self.get_llm_setting("ollama");
                let base = setting
                    .as_ref()
                    .and_then(|s| s.base_url.clone())
                    .unwrap_or_else(|| "http://localhost:11434".to_string());
                (
                    Arc::new(llm::OllamaProvider::new(base)),
                    if model_override.is_empty() {
                        setting.map(|s| s.model).unwrap_or_default()
                    } else {
                        model_override.to_string()
                    },
                )
            }
            "deepseek" => {
                let setting = self.get_llm_setting("deepseek");
                if let Some(s) = setting {
                    if !s.api_key.is_empty() {
                        let base = s
                            .base_url
                            .unwrap_or_else(|| "https://api.deepseek.com/v1".to_string());
                        return (
                            Arc::new(llm::OpenAiProvider::new("deepseek", s.api_key, base)),
                            if model_override.is_empty() {
                                s.model
                            } else {
                                model_override.to_string()
                            },
                        );
                    }
                }
                fallback_mock()
            }
            _ => fallback_mock(),
        }
    }

    pub async fn classify_intent(
        &self,
        provider: &str,
        model: &str,
        text: &str,
    ) -> anyhow::Result<Intent> {
        let (llm, model_name) = self.resolve_llm_provider(provider, model);
        let system = "You are an intent classifier for a desktop AI agent app. The user can ask you to do the following in natural language. Return ONLY a JSON object with no markdown, no explanation.\n\nAvailable intents:\n- chat: general conversation\n- create_agent: user wants to create an agent (extract name, prompt, optional tools)\n- install_mcp: user wants to install an MCP connector (extract source, value)\n- search_mcp: user wants to find an MCP connector (extract query)\n- schedule_agent: user wants to schedule an agent to run repeatedly (extract agent name/id, cron expression)\n- create_workflow: user wants to create a workflow of agents (extract name, cron expression, list of agents by name or id)\n- run_agent: user wants to run an existing agent with a prompt (extract agent name/id, prompt)\n\nReturn JSON shape: {\"intent\": \"...\", \"params\": {\"name\":\"...\", \"prompt\":\"...\", \"tools\":[], \"source\":\"...\", \"value\":\"...\", \"query\":\"...\", \"agent\":\"...\", \"expression\":\"...\", \"agents\":[], \"message\":\"...\"}}".to_string();
        let req = CompletionRequest::new(provider, model_name)
            .with_system(system)
            .with_user(text);
        let res = llm
            .complete(req)
            .await
            .map_err(|e| anyhow::anyhow!("llm error: {e}"))?;
        let content = res
            .content
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        let intent: Intent = serde_json::from_str(content)
            .map_err(|e| anyhow::anyhow!("parse intent error: {e} from content: {content}"))?;
        Ok(intent)
    }

    pub fn get_llm_setting(&self, provider: &str) -> Option<LlmSetting> {
        self.store
            .lock()
            .get_llm_setting(provider)
            .ok()
            .flatten()
            .map(|(api_key, base_url, model, temperature)| LlmSetting {
                api_key,
                base_url,
                model,
                temperature,
            })
    }

    /// Enabled model ids declared in the global config (`~/.goble/config.toml`
    /// `[model.<slug>]` tables), in slug order. The adopted schema carries no
    /// per-model provider — each entry has its own `base_url` — so this is the
    /// whole configured catalog.
    pub fn config_models(&self) -> Vec<String> {
        self.config().model_ids()
    }

    /// The model id `[models] default` names, if any.
    pub fn config_default_model(&self) -> Option<String> {
        self.config().default_model_id()
    }

    /// The provider for a model the config declares: the entry's own `base_url`
    /// picks the backend and its inline `api_key` is the credential. `None` when
    /// the config does not declare the model, or declares it without a key.
    fn config_model_provider(
        &self,
        provider_name: &str,
        model: &str,
    ) -> Option<(Arc<dyn LlmProvider>, String)> {
        let config = self.config();
        let (slug, entry) = config.model_for(model)?;
        let key = entry.resolved_key()?.to_string();
        let base = entry
            .base_url
            .clone()
            .filter(|base| !base.trim().is_empty());
        let model_id = entry.model_id(slug);
        let provider: Arc<dyn LlmProvider> = match config_backend(provider_name, base.as_deref()) {
            "anthropic" => Arc::new(llm::AnthropicProvider::new(key)),
            "openrouter" => Arc::new(llm::OpenRouterProvider::new(key)),
            "ollama" => Arc::new(llm::OllamaProvider::new(
                base.unwrap_or_else(|| "http://localhost:11434".to_string()),
            )),
            _ => Arc::new(llm::OpenAiProvider::new(
                provider_name,
                key,
                base.unwrap_or_else(|| config_backend_base(provider_name)),
            )),
        };
        Some((provider, model_id))
    }

    /// Model ids offered in the composer's model dropdown for a provider. Models
    /// declared in the global config `[model.<slug>]` take precedence; otherwise
    /// the base catalog comes from [`goble_core::llm::provider_models`]. The
    /// configured model (if any) is promoted to the front.
    pub fn available_models(&self, provider: &str) -> Vec<String> {
        let provider = if provider.is_empty() { "openai" } else { provider };
        let mut models = self.config_models();
        if models.is_empty() {
            models = llm::provider_models(provider);
        }
        if let Some(s) = self.get_llm_setting(provider) {
            if !s.model.is_empty() {
                if let Some(pos) = models.iter().position(|m| m == &s.model) {
                    let m = models.remove(pos);
                    models.insert(0, m);
                } else {
                    models.insert(0, s.model);
                }
            }
        }
        if models.is_empty() {
            models.push(llm::default_model_for(provider).to_string());
        }
        models
    }

    /// The model to select by default for a provider: the global config default
    /// when set, otherwise the configured model, otherwise the provider default.
    pub fn default_model(&self, provider: &str) -> String {
        let provider = if provider.is_empty() { "openai" } else { provider };
        if let Some(default) = self.config_default_model() {
            return default;
        }
        if let Some(s) = self.get_llm_setting(provider) {
            if !s.model.is_empty() {
                return s.model;
            }
        }
        llm::default_model_for(provider).to_string()
    }
}

/// Which backend talks to a model the config declares. The caller's provider
/// name wins when it names a non-OpenAI API; otherwise the entry's `base_url`
/// decides, since a configured custom endpoint is OpenAI-compatible.
fn config_backend(provider_name: &str, base_url: Option<&str>) -> &'static str {
    match provider_name.trim().to_lowercase().as_str() {
        "anthropic" => return "anthropic",
        "openrouter" => return "openrouter",
        "ollama" => return "ollama",
        _ => {}
    }
    let base = base_url.unwrap_or_default().to_ascii_lowercase();
    if base.contains("anthropic") {
        "anthropic"
    } else if base.contains("openrouter") {
        "openrouter"
    } else if base.contains("ollama") || base.contains(":11434") {
        "ollama"
    } else {
        "openai"
    }
}

/// The endpoint an OpenAI-compatible configured model without a `base_url`
/// talks to: the same default the store path uses for that provider.
fn config_backend_base(provider_name: &str) -> String {
    match provider_name.trim().to_lowercase().as_str() {
        "deepseek" => "https://api.deepseek.com/v1".to_string(),
        _ => "https://api.openai.com/v1".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_core::config::GobleConfig;
    use goble_core::store::Store;

    /// A state whose config is the one a `[model.<slug>]` file describes. The
    /// home is never touched: the config is installed in memory.
    fn state_with(config: &str) -> Arc<DesktopState> {
        let dir = tempfile::tempdir().unwrap();
        let state = DesktopState::new(
            Store::open_in_memory().unwrap(),
            crate::thread_store::ThreadStore::new(dir.path()).unwrap(),
        );
        let (config, problems) = GobleConfig::load_toml(config).expect("valid TOML");
        assert!(problems.is_empty(), "{problems:?}");
        *state.config.lock() = config;
        state
    }

    const CONFIGURED: &str = r#"
[model.deepseek]
model = "deepseek-flash"
base_url = "https://api.deepseek.com"
name = "Deepseek-V4-Flash"
api_key = "sk-live"
max_completion_tokens = 16384
context_window = 256000

[model.kimi]
model = "kimi-k2.7"
base_url = "https://api.kimi.com/coding/v1"
api_key = "sk-kimi"

[models]
default = "deepseek"
"#;

    #[test]
    fn the_configured_catalog_is_the_model_ids() {
        let state = state_with(CONFIGURED);

        assert_eq!(
            state.config_models(),
            vec!["deepseek-flash".to_string(), "kimi-k2.7".to_string()]
        );
        assert_eq!(
            state.config_default_model().as_deref(),
            Some("deepseek-flash"),
            "the default slug resolves to the id sent to the API"
        );
        assert_eq!(state.default_model("openai"), "deepseek-flash");
        // The configured catalog replaces the provider's built-in list.
        assert_eq!(
            state.available_models("openai"),
            vec!["deepseek-flash".to_string(), "kimi-k2.7".to_string()]
        );
    }

    #[test]
    fn a_config_with_no_models_falls_back_to_the_provider_catalog() {
        let state = state_with("[theme]\ndark = false\n");

        assert!(state.config_models().is_empty());
        assert_eq!(state.config_default_model(), None);
        assert_eq!(
            state.available_models("openai"),
            vec![
                "gpt-4o".to_string(),
                "gpt-4o-mini".to_string(),
                "gpt-4.1".to_string(),
                "o3-mini".to_string()
            ]
        );
        assert_eq!(state.default_model("openai"), "gpt-4o");
    }

    #[test]
    fn a_configured_model_uses_its_own_entry_not_the_mock_fallback() {
        let state = state_with(CONFIGURED);

        // With no store setting and no config entry the provider falls back to
        // the mock; a `[model.<slug>]` entry with its own `api_key` must reach
        // the real provider instead, and be resolved by its API model id.
        let (_, model) = state.resolve_llm_provider("openai", "deepseek-flash");
        assert_eq!(model, "deepseek-flash");

        let (_, slugless) = state.resolve_llm_provider("openai", "not-configured");
        assert_eq!(slugless, "mock", "an undeclared model still falls through");
    }

    #[test]
    fn an_entry_without_a_key_does_not_shadow_the_store_path() {
        let state = state_with(
            r#"
[model.gpt-4o]
model = "gpt-4o"
"#,
        );

        let (_, model) = state.resolve_llm_provider("openai", "gpt-4o");
        assert_eq!(model, "mock", "no key in the entry, none in the store");
    }

    /// The write the model dialog's Save performs: the configured model becomes
    /// a `[model.<slug>]` entry carrying the key inline, and the default. Kept
    /// here so the entry shape and the slug lookup are exercised.
    #[test]
    fn saving_a_model_writes_a_model_entry_and_the_default() {
        let state = state_with("");
        let mut config = state.config();
        let id = "deepseek-flash".to_string();

        let slug = config
            .model_for(&id)
            .map(|(slug, _)| slug.to_string())
            .unwrap_or_else(|| id.clone());
        let entry = config.model.entry(slug.clone()).or_default();
        entry.model = id.clone();
        if entry.name.is_none() {
            entry.name = Some(id.clone());
        }
        entry.api_key = Some("sk-live".to_string());
        entry.base_url = Some("https://api.deepseek.com".to_string());
        config.models.default = Some(slug.clone());

        let toml = config.to_toml().expect("serialize");
        assert!(toml.contains("[model.deepseek-flash]"), "{toml}");
        assert!(toml.contains(r#"api_key = "sk-live""#), "{toml}");
        assert!(toml.contains(r#"default = "deepseek-flash""#), "{toml}");

        // A second save of a model an entry already declares updates that entry
        // instead of adding a slug that would list the model twice.
        let mut config = config.clone();
        let slug_again = config
            .model_for(&id)
            .map(|(slug, _)| slug.to_string())
            .unwrap_or_else(|| id.clone());
        config.model.entry(slug_again).or_default().api_key = Some("sk-rotated".to_string());

        assert_eq!(config.model.len(), 1);
        assert_eq!(config.model_ids(), vec!["deepseek-flash".to_string()]);
        assert_eq!(
            config.model["deepseek-flash"].resolved_key(),
            Some("sk-rotated")
        );
    }

    #[test]
    fn the_base_url_picks_the_backend() {
        assert_eq!(
            config_backend("openai", Some("https://api.kimi.com/v1")),
            "openai"
        );
        assert_eq!(
            config_backend("openai", Some("https://api.anthropic.com")),
            "anthropic"
        );
        assert_eq!(
            config_backend("openai", Some("https://openrouter.ai/api/v1")),
            "openrouter"
        );
        assert_eq!(
            config_backend("openai", Some("http://localhost:11434")),
            "ollama"
        );
        // The caller's name wins over the endpoint.
        assert_eq!(
            config_backend("anthropic", Some("https://api.deepseek.com")),
            "anthropic"
        );
    }
}
