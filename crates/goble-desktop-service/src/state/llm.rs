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

    /// Enabled model ids declared for `provider` in the global config
    /// (`~/.goble/config.toml` `[llm.models]`), in declaration order.
    pub fn config_models(&self, provider: &str) -> Vec<String> {
        self.config()
            .llm
            .models
            .iter()
            .filter(|m| m.provider == provider && m.enabled)
            .map(|m| m.id.clone())
            .collect()
    }

    /// The globally configured default model id, if any.
    pub fn config_default_model(&self) -> Option<String> {
        let default = self.config().llm.default_model.clone();
        if default.is_empty() {
            None
        } else {
            Some(default)
        }
    }

    /// Model ids offered in the composer's model dropdown for a provider. Models
    /// declared in the global config `[llm.models]` take precedence; otherwise
    /// the base catalog comes from [`goble_core::llm::provider_models`]. The
    /// configured model (if any) is promoted to the front.
    pub fn available_models(&self, provider: &str) -> Vec<String> {
        let provider = if provider.is_empty() { "openai" } else { provider };
        let mut models = self.config_models(provider);
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
