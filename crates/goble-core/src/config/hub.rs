//! `~/.goble/hub.toml` — the hub's own settings.
//!
//! The hub is the automation plane: it decides what runs where and records what
//! happened. Its settings live in their own file because the hub runs as its own
//! process, with no window and no model key: the relay it dials, the bearer
//! token its API accepts, the SQLite file it records runs in, and the directory
//! of `.goble/workflows/*.toml`.
//!
//! This file is written by `goble-cli setup hub` and by the agent-led setup,
//! and read by the hub when it starts.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The header every written `hub.toml` opens with: the file documents itself,
/// so a user who opens it by hand knows what each key is for.
pub const HUB_HEADER: &str = "\
# The hub's own settings: the relay it dials, the token its API accepts, the
# SQLite file it records runs in, and the workflow directory it deploys from.
# Every key is optional; a key that is absent falls back to the hub's default.
#
#   [hub]
#   database = \"~/.goble/hub.db\"
#   relay_url = \"wss://relay.example.com\"
#   token = \"the bearer token the API accepts\"
#   workflows = \".goble/workflows\"
#
# `goble-cli setup hub` writes this file; Settings -> Agent & Approval reads the
# same paths beside the remote-agent tables.

";

/// `[hub]` — how the hub reaches workers and where it keeps its record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct HubConfig {
    /// The SQLite file the run ledger and the audit chain live in. Absent means
    /// `~/.goble/hub.db`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// The relay the hub dials to reach workers. The hub needs no inbound port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay_url: Option<String>,
    /// The bearer token the hub's API and MCP mirror accept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// The directory holding `.goble/workflows/*.toml`. Absent means the
    /// project's own `.goble/workflows`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflows: Option<String>,
}

fn non_empty(value: &Option<String>) -> bool {
    value
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

impl HubConfig {
    /// Parse a `hub.toml`. An unknown key is an error rather than a silent
    /// drop: this file is small and hand-editable, so a typo should say so.
    pub fn from_toml(text: &str) -> anyhow::Result<Self> {
        let table: toml::Table = toml::from_str(text).map_err(anyhow::Error::from)?;
        // The written shape wraps the keys in `[hub]`; a flat file that skips
        // the table reads the same way, so a hand-written short file works.
        let value = match table.get("hub") {
            Some(value) => value.clone(),
            None => toml::Value::Table(table),
        };
        let config: Self = value.try_into().map_err(anyhow::Error::from)?;
        Ok(config)
    }

    /// Emit the file: the header first, then the `[hub]` table.
    pub fn to_toml(&self) -> anyhow::Result<String> {
        #[derive(Serialize)]
        struct File<'a> {
            hub: &'a HubConfig,
        }
        let body = toml::to_string(&File { hub: self }).map_err(anyhow::Error::from)?;
        Ok(format!("{HUB_HEADER}{body}"))
    }

    /// Read `path`. A missing file is `Ok(None)`, so a caller can report "not
    /// configured" without treating it as an error.
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Option<Self>> {
        match fs::read_to_string(path.as_ref()) {
            Ok(text) => Ok(Some(Self::from_toml(&text)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(anyhow::Error::from(error)),
        }
    }

    /// Write `path`, creating its directory.
    pub fn save(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.to_toml()?)?;
        Ok(())
    }

    /// The keys that carry no value yet, in the order the file lists them.
    pub fn missing(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if !non_empty(&self.database) {
            missing.push("database");
        }
        if !non_empty(&self.relay_url) {
            missing.push("relay_url");
        }
        if !non_empty(&self.token) {
            missing.push("token");
        }
        if !non_empty(&self.workflows) {
            missing.push("workflows");
        }
        missing
    }

    /// What the setup output says: `ready`, or the keys still to fill in.
    pub fn status_label(&self) -> String {
        let missing = self.missing();
        if missing.is_empty() {
            "ready".to_string()
        } else {
            format!("missing {}", missing.join(", "))
        }
    }

    /// The relay URL when one is set.
    pub fn relay_url(&self) -> Option<&str> {
        self.relay_url.as_deref().map(str::trim).filter(|url| !url.is_empty())
    }

    /// Whether the hub has a token its API can accept.
    pub fn has_token(&self) -> bool {
        non_empty(&self.token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_file_round_trips_through_its_header() {
        let config = HubConfig {
            database: Some("~/.goble/hub.db".to_string()),
            relay_url: Some("wss://relay.example.com".to_string()),
            token: Some("hub-token".to_string()),
            workflows: None,
        };
        let toml = config.to_toml().expect("serialize");
        assert!(toml.starts_with(HUB_HEADER), "{toml}");
        assert!(toml.contains("[hub]"), "{toml}");
        assert_eq!(HubConfig::from_toml(&toml).expect("parse"), config);
        assert_eq!(config.missing(), vec!["workflows"]);
        assert_eq!(config.status_label(), "missing workflows");
    }

    #[test]
    fn an_empty_file_is_a_default_config() {
        assert_eq!(HubConfig::from_toml("").expect("parse"), HubConfig::default());
        assert_eq!(
            HubConfig::default().missing(),
            vec!["database", "relay_url", "token", "workflows"]
        );
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let error = HubConfig::from_toml("[hub]\nrelay_uri = \"wss://x\"\n")
            .expect_err("a typo should not pass");
        assert!(error.to_string().contains("relay_uri"), "{error}");
    }

    #[test]
    fn a_missing_file_reads_as_not_configured() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("hub.toml");
        assert_eq!(HubConfig::load(&path).expect("load"), None);
        let config = HubConfig {
            relay_url: Some("wss://relay.example.com".to_string()),
            token: Some("token".to_string()),
            ..HubConfig::default()
        };
        config.save(&path).expect("save");
        assert_eq!(HubConfig::load(&path).expect("load"), Some(config));
    }
}
