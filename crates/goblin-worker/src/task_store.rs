use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use goble_core::agent::{AgentId, Trigger};
use rusqlite::{params, Connection};
use uuid::Uuid;

/// Stored scheduled task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledTask {
    pub id: String,
    pub agent_id: AgentId,
    pub trigger: Trigger,
    pub created_at: DateTime<Utc>,
    pub enabled: bool,
}

impl ScheduledTask {
    pub fn new(agent_id: AgentId, trigger: Trigger) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            agent_id,
            trigger,
            created_at: Utc::now(),
            enabled: true,
        }
    }
}

/// Persistent SQLite store for scheduled tasks.
pub struct TaskStore {
    conn: Connection,
}

impl TaskStore {
    pub fn open(path: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")))?;
        let conn = Connection::open(&path)
            .with_context(|| format!("failed to open task store at {path:?}"))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS scheduled_tasks (
                id TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                trigger_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1
            )",
            [],
        )?;
        Ok(Self { conn })
    }

    pub fn insert(&self, task: &ScheduledTask) -> Result<()> {
        let trigger_json = serde_json::to_string(&task.trigger)?;
        self.conn.execute(
            "INSERT INTO scheduled_tasks (id, agent_id, trigger_json, created_at, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                 trigger_json = excluded.trigger_json,
                 enabled = excluded.enabled",
            params![
                task.id,
                task.agent_id.0.clone(),
                trigger_json,
                task.created_at.to_rfc3339(),
                task.enabled as i32
            ],
        )?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<ScheduledTask>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, agent_id, trigger_json, created_at, enabled FROM scheduled_tasks",
        )?;
        let rows = stmt.query_map([], |row: &rusqlite::Row| {
            let id: String = row.get(0)?;
            let agent_id: String = row.get(1)?;
            let trigger_json: String = row.get(2)?;
            let created_at: String = row.get(3)?;
            let enabled: i32 = row.get(4)?;
            let trigger: Trigger = serde_json::from_str(&trigger_json).unwrap_or(Trigger::Manual);
            Ok(ScheduledTask {
                id,
                agent_id: AgentId(agent_id),
                trigger,
                created_at: DateTime::parse_from_rfc3339(&created_at)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                enabled: enabled != 0,
            })
        })?;

        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row.map_err(|e| anyhow::anyhow!(e))?);
        }
        Ok(tasks)
    }

    pub fn delete(&self, id: &str) -> Result<bool> {
        let affected = self
            .conn
            .execute("DELETE FROM scheduled_tasks WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    pub fn enable(&self, id: &str, enabled: bool) -> Result<bool> {
        let affected = self.conn.execute(
            "UPDATE scheduled_tasks SET enabled = ?1 WHERE id = ?2",
            params![enabled as i32, id],
        )?;
        Ok(affected > 0)
    }

    pub fn close(self) -> Result<()> {
        self.conn.close().map_err(|(_, e)| anyhow::anyhow!(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_store_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let store = TaskStore::open(tmp.path().join("tasks.db")).unwrap();
        let task = ScheduledTask::new(
            AgentId::generate(),
            Trigger::Heartbeat {
                interval_seconds: 60,
            },
        );
        store.insert(&task).unwrap();
        let tasks = store.list().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].agent_id, task.agent_id);
        assert_eq!(tasks[0].trigger, task.trigger);
    }

    #[test]
    fn test_delete_and_enable() {
        let tmp = TempDir::new().unwrap();
        let store = TaskStore::open(tmp.path().join("tasks.db")).unwrap();
        let task = ScheduledTask::new(AgentId::generate(), Trigger::Manual);
        store.insert(&task).unwrap();
        assert!(store.delete(&task.id).unwrap());
        assert!(store.list().unwrap().is_empty());

        let task2 = ScheduledTask::new(AgentId::generate(), Trigger::Manual);
        store.insert(&task2).unwrap();
        assert!(store.enable(&task2.id, false).unwrap());
        assert!(!store.list().unwrap()[0].enabled);
    }
}
