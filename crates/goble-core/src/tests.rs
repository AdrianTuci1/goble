#[cfg(test)]
mod tests {
    use crate::agent::{AgentSpec, AgentState, Trigger};
    use crate::config::{GobleConfig, ProviderConfig};
    use crate::worker::WorkerConfig;

    #[test]
    fn test_agent_spec_builder() {
        let agent = AgentSpec::new("demo", "do nothing")
            .with_description("test")
            .with_tools(vec!["tool".to_string()])
            .with_trigger(Trigger::Cron {
                expression: "* * * * *".to_string(),
            });
        assert_eq!(agent.name, "demo");
        assert_eq!(agent.tools.len(), 1);
        assert_eq!(agent.triggers.len(), 2);
    }

    #[test]
    fn test_worker_config() {
        let w = WorkerConfig::new("vps", "1.2.3.4", "root").with_pairing_code("123456");
        assert_eq!(w.host, "1.2.3.4");
        assert_eq!(w.pairing_code, "123456");
        assert_eq!(w.port, 7878);
    }

    #[test]
    fn test_config_toml_roundtrip() {
        let mut config = GobleConfig::default();
        config.llm.providers.push(ProviderConfig {
            name: "openai".to_string(),
            api_key_secret_id: "sk-123".to_string(),
            base_url: None,
            model: "gpt-4".to_string(),
        });
        let toml = config.to_toml().unwrap();
        let parsed = GobleConfig::from_toml(&toml).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn test_agent_state_serialization() {
        let state = AgentState::Error("boom".to_string());
        let s = serde_json::to_string(&state).unwrap();
        let d: AgentState = serde_json::from_str(&s).unwrap();
        assert_eq!(d, state);
    }
}
