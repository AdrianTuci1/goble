//! What a setup conversation needs: the state of Remote and the hub, and the
//! brief an agent starts from.
//!
//! `goble-cli setup` and the app's "Set up with the agent" row both read this
//! module, so the two surfaces ask for the same values and report the same
//! answer. Nothing here writes a file: the caller owns the config it passes in,
//! and the brief only tells the agent which command to run.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{GobleConfig, HubConfig};

/// Credentials a provider can read from the environment instead of its own
/// table. Status reports presence only: a value never leaves the machine.
pub const CREDENTIAL_ENV_KEYS: [&str; 9] = [
    "MODAL_TOKEN_ID",
    "MODAL_TOKEN_SECRET",
    "E2B_API_KEY",
    "DAYTONA_API_KEY",
    "BOX_API_KEY",
    "GOOGLE_CLOUD_PROJECT",
    "GOOGLE_CLOUD_REGION",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "GOOGLE_CLOUD_QUOTA_PROJECT",
];

/// Which setup a brief is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SetupTarget {
    Remote,
    Hub,
}

impl SetupTarget {
    /// The word the setup commands and the brief use.
    pub fn as_str(self) -> &'static str {
        match self {
            SetupTarget::Remote => "remote",
            SetupTarget::Hub => "hub",
        }
    }
}

/// One `[remote_agents.<name>]` table as setup reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupAgent {
    pub name: String,
    pub provider: String,
    pub status: String,
    pub missing: Vec<String>,
}

/// The hub's settings as setup reports them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupHub {
    pub database: Option<String>,
    pub relay_url: Option<String>,
    pub token: bool,
    pub workflows: Option<String>,
    pub status: String,
    pub missing: Vec<String>,
}

/// Everything about Remote and the hub that a setup conversation asks about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupStatus {
    pub config_path: String,
    pub hub_path: String,
    /// `platform` or `gateway`.
    pub remote_mode: String,
    pub gateway_url: Option<String>,
    pub gateway_token: bool,
    pub agents: Vec<SetupAgent>,
    /// `None` when `hub.toml` does not exist yet.
    pub hub: Option<SetupHub>,
    /// One entry per [`CREDENTIAL_ENV_KEYS`] name: whether this machine's
    /// environment sets it.
    pub env: BTreeMap<String, bool>,
}

impl SetupStatus {
    /// Read the state out of a loaded config, a hub file and an environment.
    pub fn from_config(
        config: &GobleConfig,
        config_path: &Path,
        hub_path: &Path,
        hub: Option<&HubConfig>,
        env: &BTreeMap<String, String>,
    ) -> Self {
        let agents = config
            .remote_agents
            .iter()
            .map(|(name, agent)| SetupAgent {
                name: name.clone(),
                provider: agent.provider.as_str().to_string(),
                status: agent.status_label(),
                missing: agent
                    .missing_credentials()
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            })
            .collect();
        let hub = hub.map(|hub| SetupHub {
            database: hub.database.clone(),
            relay_url: hub.relay_url.clone(),
            token: hub.has_token(),
            workflows: hub.workflows.clone(),
            status: hub.status_label(),
            missing: hub.missing().into_iter().map(str::to_string).collect(),
        });
        Self {
            config_path: config_path.display().to_string(),
            hub_path: hub_path.display().to_string(),
            remote_mode: match config.remote.mode {
                super::RemoteMode::Gateway => "gateway".to_string(),
                super::RemoteMode::Platform => "platform".to_string(),
            },
            gateway_url: config.remote.gateway_url().map(str::to_string),
            gateway_token: config.remote.worker_token().is_some(),
            agents,
            hub,
            env: CREDENTIAL_ENV_KEYS
                .iter()
                .map(|key| {
                    let set = env
                        .get(*key)
                        .is_some_and(|value| !value.trim().is_empty());
                    ((*key).to_string(), set)
                })
                .collect(),
        }
    }

    /// Whether Remote already has somewhere to run.
    pub fn remote_configured(&self) -> bool {
        (self.remote_mode == "gateway" && self.gateway_url.is_some() && self.gateway_token)
            || self.agents.iter().any(|agent| agent.missing.is_empty())
    }

    /// The short report the setup commands and the brief both use.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.remote_mode == "gateway" {
            let url = self.gateway_url.as_deref().unwrap_or("no URL");
            let token = if self.gateway_token {
                "token set"
            } else {
                "no worker token"
            };
            lines.push(format!("remote: gateway {url} · {token}"));
        } else {
            lines.push(format!("remote: platform · {} agent(s)", self.agents.len()));
        }
        for agent in &self.agents {
            lines.push(format!(
                "agent {}: {} · {}",
                agent.name, agent.provider, agent.status
            ));
        }
        match &self.hub {
            Some(hub) => lines.push(format!("hub: {} · {}", self.hub_path, hub.status)),
            None => lines.push(format!("hub: {} · not configured", self.hub_path)),
        }
        let set: Vec<&str> = self
            .env
            .iter()
            .filter(|(_, set)| **set)
            .map(|(key, _)| key.as_str())
            .collect();
        if set.is_empty() {
            lines.push("credentials in this environment: none of the provider keys".to_string());
        } else {
            lines.push(format!("credentials in this environment: {}", set.join(", ")));
        }
        lines
    }

    /// The brief an agent starts a setup conversation from. `cli` is the
    /// absolute path (or the name) of the `goble-cli` binary that owns the
    /// writes, so the agent never has to guess how to reach it.
    pub fn brief(&self, target: SetupTarget, cli: &str) -> String {
        match target {
            SetupTarget::Remote => self.remote_brief(cli),
            SetupTarget::Hub => self.hub_brief(cli),
        }
    }

    fn state_block(&self) -> String {
        self.lines()
            .into_iter()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn remote_brief(&self, cli: &str) -> String {
        format!(
            "Set up Remote for Goble with me, one question at a time.\n\
             \n\
             Reply in the language I write in. Ask a single question, wait for my answer, and \
             write nothing until the question it belongs to has one. Never invent a credential, \
             a project id, an image reference or a URL: when I do not know a value, name the \
             page or command that shows it and wait.\n\
             \n\
             What Goble has now (config: {config}):\n\
             {state}\n\
             \n\
             Read that file before writing. Every write goes through `{cli} setup …`, which keeps \
             the file's header and the sections it does not touch; do not rewrite it by hand.\n\
             \n\
             Ask which shape I want:\n\
             \n\
             1. A gateway server: workers connect out to it, so nothing needs an inbound port \
             here. Ask for its public URL (wss://…) and the token goble-gateway was started \
             with, then run:\n\
               {cli} setup gateway --url <url> --token <token>\n\
             \n\
             2. A sandbox platform: Goble ships a worker image into a provider. Ask which \
             provider, then collect only what that one reads:\n\
               modal      token id and token secret (modal.com → Settings → Tokens)\n\
               e2b        an E2B API key (the e2b.dev dashboard)\n\
               daytona    a Daytona API key\n\
               box        a Boat API key\n\
               cloud_run  the Google Cloud project id, the Cloud Run region (for example \
             europe-west1), a worker image in Artifact Registry, and either a bearer token or a \
             gcloud session on this machine (`gcloud auth login`)\n\
               docker     nothing; the image is built locally\n\
               ssh        the destination as user@host, and a login that works\n\
             Every platform also needs `image`: the image that carries goblin-worker. Then run:\n\
               {cli} setup remote --name <name> --provider <provider> [--api-key …] \
             [--token-id …] [--token-secret …] [--image …] [--region …] [--environment …] \
             [--api-url …]\n\
             Name the table after what I will use it for (builder, reviewer, …).\n\
             \n\
             After a write, run `{cli} setup status` and read the result back to me in one line. \
             If a table still reports missing keys, ask for exactly those and nothing else. Then \
             ask whether I want to try it: choosing Remote in the app deploys the worker through \
             this table, and Settings → Agent & Approval shows the same status.",
            config = self.config_path,
            state = self.state_block(),
            cli = cli,
        )
    }

    fn hub_brief(&self, cli: &str) -> String {
        format!(
            "Set up the hub for Goble with me, one question at a time.\n\
             \n\
             Reply in the language I write in. Ask one question, wait for my answer, and write \
             only what I gave you.\n\
             \n\
             What Goble has now:\n\
             {state}\n\
             \n\
             The hub's settings live in {hub}. Write them through `{cli} setup hub …`.\n\
             \n\
             Ask, in this order:\n\
             1. Should the hub dial a relay (the wss:// URL goble-gateway serves) so runs reach \
             workers on other machines, or stay local while I try it?\n\
             2. Which bearer token should its API accept? Offer to generate one with \
             `openssl rand -hex 32`; a token I already have works too.\n\
             3. Where should its SQLite file live? The default is ~/.goble/hub.db.\n\
             4. Where are the workflows it deploys from? The default is .goble/workflows in the \
             project.\n\
             \n\
             Then run:\n\
               {cli} setup hub --relay-url <url> --token <token> --database <path> \
             --workflows <dir>\n\
             and `{cli} setup status`. Say what is ready and what the hub still needs before it \
             can start. Goble does not ship the hub binary yet: record the settings, say they are \
             ready for it, and do not claim the hub is running.",
            state = self.state_block(),
            hub = self.hub_path,
            cli = cli,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GobleConfig;

    fn config() -> GobleConfig {
        GobleConfig::from_toml(
            r#"
[remote]
mode = "platform"

[remote_agents.builder]
provider = "e2b"
image = "ghcr.io/goble/worker:latest"

[remote_agents.local]
provider = "docker"
"#,
        )
        .expect("parse")
    }

    fn status() -> SetupStatus {
        let env = BTreeMap::from([
            ("E2B_API_KEY".to_string(), "sk-e2b".to_string()),
            ("GOOGLE_CLOUD_REGION".to_string(), "  ".to_string()),
        ]);
        SetupStatus::from_config(
            &config(),
            Path::new("/home/me/.goble/config.toml"),
            Path::new("/home/me/.goble/hub.toml"),
            None,
            &env,
        )
    }

    #[test]
    fn status_names_the_agent_that_still_needs_a_key() {
        let status = status();
        assert_eq!(status.remote_mode, "platform");
        assert!(status.remote_configured(), "the docker table needs no key");
        assert_eq!(status.agents.len(), 2);
        assert_eq!(status.agents[0].name, "builder");
        assert_eq!(status.agents[0].provider, "e2b");
        assert_eq!(status.agents[0].missing, vec!["E2B_API_KEY"]);
        assert_eq!(status.agents[1].status, "ready");

        // Presence only, and a blank value is not presence.
        assert_eq!(status.env.get("E2B_API_KEY"), Some(&true));
        assert_eq!(status.env.get("GOOGLE_CLOUD_REGION"), Some(&false));
        assert_eq!(status.env.get("MODAL_TOKEN_ID"), Some(&false));

        let lines = status.lines().join("\n");
        assert!(lines.contains("agent builder: e2b · missing E2B_API_KEY"), "{lines}");
        assert!(lines.contains("hub: /home/me/.goble/hub.toml · not configured"), "{lines}");
        assert!(lines.contains("credentials in this environment: E2B_API_KEY"), "{lines}");
    }

    #[test]
    fn the_remote_brief_names_the_state_the_command_and_the_path() {
        let brief = status().brief(SetupTarget::Remote, "/opt/goble/bin/goble-cli");
        assert!(brief.contains("/opt/goble/bin/goble-cli setup gateway"), "{brief}");
        assert!(brief.contains("setup remote --name"), "{brief}");
        assert!(brief.contains("/home/me/.goble/config.toml"), "{brief}");
        assert!(brief.contains("missing E2B_API_KEY"), "{brief}");
        for provider in ["modal", "e2b", "daytona", "box", "cloud_run", "docker", "ssh"] {
            assert!(brief.contains(provider), "the brief names {provider}");
        }
        assert!(brief.contains("one question at a time"), "{brief}");
    }

    #[test]
    fn the_hub_brief_records_that_the_hub_binary_is_not_wired_yet() {
        let brief = status().brief(SetupTarget::Hub, "goble-cli");
        assert!(brief.contains("goble-cli setup hub"), "{brief}");
        assert!(brief.contains("/home/me/.goble/hub.toml"), "{brief}");
        assert!(brief.contains("does not ship the hub binary yet"), "{brief}");
    }

    #[test]
    fn a_gateway_that_is_named_and_tokened_is_configured() {
        let config = GobleConfig::from_toml(
            "[remote]\nmode = \"gateway\"\ngateway_url = \"https://gateway.example.com\"\nworker_token = \"wt-1\"\n",
        )
        .expect("parse");
        let status = SetupStatus::from_config(
            &config,
            Path::new("config.toml"),
            Path::new("hub.toml"),
            Some(&HubConfig {
                relay_url: Some("wss://relay.example.com".to_string()),
                ..HubConfig::default()
            }),
            &BTreeMap::new(),
        );
        assert!(status.remote_configured());
        assert_eq!(status.remote_mode, "gateway");
        assert_eq!(
            status.gateway_url.as_deref(),
            Some("https://gateway.example.com")
        );
        assert!(status.gateway_token);
        let hub = status.hub.as_ref().expect("the hub file was given");
        assert_eq!(hub.status, "missing database, token, workflows");
        assert_eq!(
            status.hub.as_ref().and_then(|hub| hub.relay_url.as_deref()),
            Some("wss://relay.example.com")
        );
    }
}
