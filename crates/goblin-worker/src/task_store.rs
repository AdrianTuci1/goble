use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use goble_core::agent::{AgentId, Trigger};
use rusqlite::{params, Connection};
use uuid::Uuid;

/// Stored scheduled task (a routine).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledTask {
    pub id: String,
    pub agent_id: AgentId,
    pub trigger: Trigger,
    pub created_at: DateTime<Utc>,
    pub enabled: bool,
    /// RFC3339 timestamp of the most recent run, if any.
    pub last_run_at: Option<DateTime<Utc>>,
    /// Outcome of the most recent run (e.g. "running", "success", "error").
    pub last_status: Option<String>,
}

impl ScheduledTask {
    pub fn new(agent_id: AgentId, trigger: Trigger) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            agent_id,
            trigger,
            created_at: Utc::now(),
            enabled: true,
            last_run_at: None,
            last_status: None,
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
                enabled INTEGER NOT NULL DEFAULT 1,
                last_run_at TEXT,
                last_status TEXT
            )",
            [],
        )?;
        // Migrate older databases that predate the run-state columns.
        let columns: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(scheduled_tasks)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            let mut cols = Vec::new();
            for row in rows {
                cols.push(row?);
            }
            cols
        };
        for (column, decl) in [("last_run_at", "TEXT"), ("last_status", "TEXT")] {
            if !columns.iter().any(|c| c == column) {
                conn.execute(
                    &format!("ALTER TABLE scheduled_tasks ADD COLUMN {column} {decl}"),
                    [],
                )?;
            }
        }
        Ok(Self { conn })
    }

    pub fn insert(&self, task: &ScheduledTask) -> Result<()> {
        let trigger_json = serde_json::to_string(&task.trigger)?;
        self.conn.execute(
            "INSERT INTO scheduled_tasks (id, agent_id, trigger_json, created_at, enabled, last_run_at, last_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                 trigger_json = excluded.trigger_json,
                 enabled = excluded.enabled,
                 last_run_at = excluded.last_run_at,
                 last_status = excluded.last_status",
            params![
                task.id,
                task.agent_id.0.clone(),
                trigger_json,
                task.created_at.to_rfc3339(),
                task.enabled as i32,
                task.last_run_at.map(|t| t.to_rfc3339()),
                task.last_status.clone()
            ],
        )?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<ScheduledTask>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, agent_id, trigger_json, created_at, enabled, last_run_at, last_status FROM scheduled_tasks",
        )?;
        let rows = stmt.query_map([], |row: &rusqlite::Row| {
            let id: String = row.get(0)?;
            let agent_id: String = row.get(1)?;
            let trigger_json: String = row.get(2)?;
            let created_at: String = row.get(3)?;
            let enabled: i32 = row.get(4)?;
            let last_run_at: Option<String> = row.get(5)?;
            let last_status: Option<String> = row.get(6)?;
            let trigger: Trigger = serde_json::from_str(&trigger_json).unwrap_or(Trigger::Manual);
            Ok(ScheduledTask {
                id,
                agent_id: AgentId(agent_id),
                trigger,
                created_at: DateTime::parse_from_rfc3339(&created_at)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                enabled: enabled != 0,
                last_run_at: last_run_at
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc)),
                last_status,
            })
        })?;

        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row.map_err(|e| anyhow::anyhow!(e))?);
        }
        Ok(tasks)
    }

    /// Record that a routine fired (or finished) with the given outcome.
    pub fn mark_run(&self, id: &str, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE scheduled_tasks SET last_run_at = ?1, last_status = ?2 WHERE id = ?3",
            params![Utc::now().to_rfc3339(), status, id],
        )?;
        Ok(())
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

    #[test]
    fn test_mark_run_records_last_status() {
        let tmp = TempDir::new().unwrap();
        let store = TaskStore::open(tmp.path().join("tasks.db")).unwrap();
        let task = ScheduledTask::new(AgentId::generate(), Trigger::Manual);
        store.insert(&task).unwrap();
        assert!(store.list().unwrap()[0].last_run_at.is_none());

        store.mark_run(&task.id, "running").unwrap();
        store.mark_run(&task.id, "success").unwrap();
        let tasks = store.list().unwrap();
        assert_eq!(tasks[0].last_status.as_deref(), Some("success"));
        assert!(tasks[0].last_run_at.is_some());
    }
}
