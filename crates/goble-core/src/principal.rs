use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A logical actor that owns agents, secrets, and MCP accounts.
/// Even a single human user creates multiple principals over time
/// (e.g. personal Slack channels, teams, clients) so that credentials
/// and workspaces stay isolated.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrincipalId(pub String);

impl PrincipalId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Built-in principal created at account setup. Cannot be deleted.
    pub fn default_user() -> Self {
        Self("principal_default_user".to_string())
    }
}

impl fmt::Display for PrincipalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub id: PrincipalId,
    pub kind: PrincipalKind,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

impl Principal {
    pub fn new(id: PrincipalId, kind: PrincipalKind, name: impl Into<String>) -> Self {
        Self {
            id,
            kind,
            name: name.into(),
            created_at: Utc::now(),
        }
    }

    pub fn default_user() -> Self {
        Self::new(PrincipalId::default_user(), PrincipalKind::User, "Me")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum PrincipalKind {
    User,
    SlackChannel { channel_id: String },
    Team { team_id: String },
    Client { client_id: String },
}

/// Concrete connection to an MCP server on behalf of a principal.
/// One MCP server can have many accounts (one per principal) so that
/// agents serving different channels/clients never share credentials.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct McpAccountId(pub String);

impl McpAccountId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl fmt::Display for McpAccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpAccount {
    pub id: McpAccountId,
    pub server_id: String,
    pub principal_id: PrincipalId,
    pub name: String,
    pub secret_ids: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl McpAccount {
    pub fn new(
        server_id: impl Into<String>,
        principal_id: PrincipalId,
        name: impl Into<String>,
        secret_ids: Vec<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: McpAccountId::generate(),
            server_id: server_id.into(),
            principal_id,
            name: name.into(),
            secret_ids,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Reference attached to an AgentSpec to grant access to a specific MCP account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpAccountRef {
    pub account_id: McpAccountId,
    pub server_id: String,
}
