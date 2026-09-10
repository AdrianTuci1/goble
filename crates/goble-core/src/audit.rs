use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Categories of auditable events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditCategory {
    Identity,
    Vault,
    Worker,
    Agent,
    Cluster,
    Settings,
}

/// A single entry in the cluster audit log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub timestamp: String,
    pub category: AuditCategory,
    pub actor: String,
    pub action: String,
    pub details: HashMap<String, String>,
}

impl AuditEntry {
    pub fn new(
        id: impl Into<String>,
        timestamp: impl Into<String>,
        category: AuditCategory,
        actor: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            timestamp: timestamp.into(),
            category,
            actor: actor.into(),
            action: action.into(),
            details: HashMap::new(),
        }
    }

    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }
}

/// In-memory audit log. Persistence is handled by the caller (e.g. `Store`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditLog {
    pub entries: Vec<AuditEntry>,
}

impl AuditLog {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn append(&mut self, entry: AuditEntry) {
        self.entries.push(entry);
    }

    pub fn list(&self, limit: Option<usize>) -> &[AuditEntry] {
        match limit {
            Some(n) => {
                let start = self.entries.len().saturating_sub(n);
                &self.entries[start..]
            }
            None => &self.entries,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).context("failed to serialize audit log")
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).context("failed to deserialize audit log")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_log_roundtrip() {
        let mut log = AuditLog::new();
        log.append(
            AuditEntry::new(
                "entry-1",
                "2026-08-14T00:00:00Z",
                AuditCategory::Identity,
                "device-1",
                "cluster_created",
            )
            .with_detail("cluster_name", "prod"),
        );
        log.append(
            AuditEntry::new(
                "entry-2",
                "2026-08-14T00:01:00Z",
                AuditCategory::Worker,
                "device-1",
                "worker_cert_issued",
            )
            .with_detail("worker_id", "worker-1"),
        );
        let bytes = log.to_bytes().unwrap();
        let loaded = AuditLog::from_bytes(&bytes).unwrap();
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(
            loaded.entries[0].details.get("cluster_name").unwrap(),
            "prod"
        );
    }

    #[test]
    fn test_audit_log_list_limit() {
        let mut log = AuditLog::new();
        for i in 0..5 {
            log.append(AuditEntry::new(
                format!("entry-{i}"),
                "2026-08-14T00:00:00Z",
                AuditCategory::Agent,
                "device",
                "run",
            ));
        }
        assert_eq!(log.list(Some(2)).len(), 2);
        assert_eq!(log.list(Some(2))[0].id, "entry-3");
        assert_eq!(log.list(None).len(), 5);
    }
}
