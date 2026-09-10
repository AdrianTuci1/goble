use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GobleConfig {
    pub version: u32,
    pub llm: LlmConfig,
    pub theme: ThemeConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmConfig {
    #[serde(default)]
    pub default_provider: String,
    #[serde(default)]
    pub default_model: String,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
}

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

/// A selectable LLM model, listed in the composer's model tray / slash picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    pub provider: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub api_key_secret_id: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeConfig {
    pub dark: bool,
    pub accent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<String>,
}

impl Default for GobleConfig {
    fn default() -> Self {
        Self {
            version: 1,
            llm: LlmConfig {
                default_provider: "openai".to_string(),
                default_model: String::new(),
                providers: Vec::new(),
                models: Vec::new(),
            },
            theme: ThemeConfig {
                dark: true,
                accent: "#14b8a6".to_string(),
                primary: None,
                secondary: None,
            },
        }
    }
}

impl GobleConfig {
    pub fn to_toml(&self) -> anyhow::Result<String> {
        toml::to_string(self).map_err(anyhow::Error::from)
    }

    pub fn from_toml(s: &str) -> anyhow::Result<Self> {
        toml::from_str(s).map_err(anyhow::Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_with_models() {
        let config = GobleConfig {
            version: 1,
            llm: LlmConfig {
                default_provider: "openai".into(),
                default_model: "gpt-4o".into(),
                providers: vec![ProviderConfig {
                    name: "openai".into(),
                    api_key_secret_id: "openai-key".into(),
                    base_url: Some("https://api.openai.com/v1".into()),
                    model: "gpt-4o".into(),
                }],
                models: vec![ModelConfig {
                    id: "gpt-4o".into(),
                    provider: "openai".into(),
                    label: "GPT-4o".into(),
                    api_key_secret_id: "openai-key".into(),
                    base_url: Some("https://api.openai.com/v1".into()),
                    enabled: true,
                }],
            },
            theme: ThemeConfig {
                dark: true,
                accent: "#14b8a6".into(),
                primary: None,
                secondary: None,
            },
        };
        let toml = config.to_toml().expect("serialize");
        let parsed = GobleConfig::from_toml(&toml).expect("deserialize");
        assert_eq!(parsed, config);
        assert_eq!(parsed.llm.default_model, "gpt-4o");
        assert_eq!(parsed.llm.models.len(), 1);
        assert!(parsed.llm.models[0].enabled);
    }

    #[test]
    fn config_parses_back_compat_without_models() {
        // A config file written before the `models`/`default_model` fields must
        // still parse (the new fields are serde-default).
        let toml = r##"
version = 1
[llm]
default_provider = "openai"
[[llm.providers]]
name = "openai"
api_key_secret_id = "openai-key"
base_url = "https://api.openai.com/v1"
model = "gpt-4o"
[theme]
dark = true
accent = "#14b8a6"
"##;
        let parsed = GobleConfig::from_toml(toml).expect("parse");
        assert!(parsed.llm.models.is_empty());
        assert!(parsed.llm.default_model.is_empty());
    }
}
