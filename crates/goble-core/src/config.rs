use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GobleConfig {
    pub version: u32,
    pub llm: LlmConfig,
    pub theme: ThemeConfig,
    #[serde(default)]
    pub workspace: WorkspaceConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkspaceConfig {
    #[serde(default)]
    pub default_target: WorkspaceTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum WorkspaceTarget {
    #[default]
    Local,
    Remote {
        #[serde(default)]
        worker_id: Option<String>,
    },
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
            workspace: WorkspaceConfig::default(),
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
    fn config_roundtrip_includes_workspace_default_target() {
        let mut config = GobleConfig::default();
        config.workspace.default_target = WorkspaceTarget::Remote {
            worker_id: Some("vps-1".to_string()),
        };
        let toml = config.to_toml().unwrap();
        let parsed = GobleConfig::from_toml(&toml).unwrap();
        assert_eq!(parsed.workspace.default_target, config.workspace.default_target);
    }

    #[test]
    fn config_without_workspace_section_defaults_to_local() {
        let toml = r##"
version = 1

[llm]
default_provider = "openai"
providers = []

[theme]
dark = true
accent = "#14b8a6"
"##;
        let parsed = GobleConfig::from_toml(toml).unwrap();
        assert_eq!(parsed.workspace.default_target, WorkspaceTarget::Local);
    }
}
