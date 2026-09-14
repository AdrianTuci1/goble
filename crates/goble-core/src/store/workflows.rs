use anyhow::Result;
use rusqlite::params;

use super::Store;

impl Store {
    pub fn insert_workflow(
        &self,
        id: &str,
        name: &str,
        description: &str,
        spec: &str,
        trigger: &str,
        enabled: bool,
        created_at: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO workflows (id, name, description, spec, trigger, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, description=excluded.description, spec=excluded.spec,
                                           trigger=excluded.trigger, enabled=excluded.enabled, updated_at=excluded.updated_at",
            params![id, name, description, spec, trigger, enabled as i32, created_at, updated_at],
        )?;
        Ok(())
    }

    pub fn list_workflows(
        &self,
    ) -> Result<Vec<(String, String, String, String, String, bool, String, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, name, description, spec, trigger, enabled, created_at, updated_at FROM workflows ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, bool>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_workflow(&self, id: &str) -> Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM workflows WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn insert_execution(
        &self,
        id: &str,
        agent_id: Option<&str>,
        worker_id: Option<&str>,
        status: &str,
        trace: &str,
        started_at: &str,
        finished_at: Option<&str>,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO executions (id, agent_id, worker_id, status, trace, started_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET status=excluded.status, trace=excluded.trace, finished_at=excluded.finished_at",
            params![id, agent_id, worker_id, status, trace, started_at, finished_at],
        )?;
        Ok(())
    }

    pub fn list_executions(
        &self,
    ) -> Result<
        Vec<(
            String,
            Option<String>,
            Option<String>,
            String,
            String,
            String,
            Option<String>,
        )>,
    > {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, agent_id, worker_id, status, trace, started_at, finished_at FROM executions ORDER BY started_at DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
