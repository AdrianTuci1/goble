use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GobleConfig {
    pub version: u32,
    pub llm: LlmConfig,
    pub theme: ThemeConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmConfig {
    pub default_provider: String,
    pub providers: Vec<ProviderConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub api_key_secret_id: String,
    pub base_url: Option<String>,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeConfig {
    pub dark: bool,
    pub accent: String,
}

impl Default for GobleConfig {
    fn default() -> Self {
        Self {
            version: 1,
            llm: LlmConfig {
                default_provider: "openai".to_string(),
                providers: Vec::new(),
            },
            theme: ThemeConfig {
                dark: true,
                accent: "#14b8a6".to_string(),
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
