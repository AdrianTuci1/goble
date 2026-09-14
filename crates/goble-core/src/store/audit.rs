use anyhow::{Context, Result};
use rusqlite::params;

use super::Store;

impl Store {
    pub fn append_audit_log(&self, entry: &crate::audit::AuditEntry) -> Result<()> {
        let details =
            serde_json::to_string(&entry.details).context("failed to serialize audit details")?;
        self.conn.lock().execute(
            "INSERT INTO audit_log (id, timestamp, category, actor, action, details)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO NOTHING",
            params![
                entry.id,
                entry.timestamp,
                format!("{:?}", entry.category).to_lowercase(),
                entry.actor,
                entry.action,
                details,
            ],
        )?;
        Ok(())
    }

    pub fn list_audit_logs(&self, limit: Option<usize>) -> Result<Vec<crate::audit::AuditEntry>> {
        let conn = self.conn.lock();
        let sql = "SELECT id, timestamp, category, actor, action, details FROM audit_log ORDER BY timestamp DESC";
        let mut stmt = conn.prepare(sql)?;
        let rows: Vec<(String, String, String, String, String, String)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let rows = if let Some(n) = limit {
            rows.into_iter().take(n).collect::<Vec<_>>()
        } else {
            rows
        };
        rows.into_iter()
            .map(|(id, timestamp, category, actor, action, details_json)| {
                let category = match category.as_str() {
                    "identity" => crate::audit::AuditCategory::Identity,
                    "vault" => crate::audit::AuditCategory::Vault,
                    "worker" => crate::audit::AuditCategory::Worker,
                    "agent" => crate::audit::AuditCategory::Agent,
                    "cluster" => crate::audit::AuditCategory::Cluster,
                    "credentials" => crate::audit::AuditCategory::Settings,
                    "settings" => crate::audit::AuditCategory::Settings,
                    _ => crate::audit::AuditCategory::Settings,
                };
                let details = serde_json::from_str(&details_json).unwrap_or_default();
                Ok(crate::audit::AuditEntry {
                    id,
                    timestamp,
                    category,
                    actor,
                    action,
                    details,
                })
            })
            .collect::<Result<Vec<_>>>()
    }
}
