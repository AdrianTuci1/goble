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
    fn test_worker_config_mtls_url() {
        let bundle = crate::provision::WorkerBundle {
            worker_id: "w-1".to_string(),
            cert_pem: "CERT".to_string(),
            key_pem: "KEY".to_string(),
            ca_cert_pem: "CA".to_string(),
            cluster_name: "goble".to_string(),
        };
        let cfg = WorkerConfig::new("vps", "1.2.3.4", "root")
            .with_pairing_code("123456")
            .with_worker_bundle(bundle);
        assert_eq!(cfg.websocket_url(), "wss://1.2.3.4:7878/ws");
        assert!(cfg.worker_bundle.is_some());
    }

    #[test]
    fn test_worker_config_plain_url() {
        let cfg = WorkerConfig::new("vps", "1.2.3.4", "root").with_pairing_code("123456");
        assert_eq!(cfg.websocket_url(), "ws://1.2.3.4:7878/ws");
        assert!(cfg.worker_bundle.is_none());
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
