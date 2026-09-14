use anyhow::{Context, Result};
use rusqlite::params;

use super::Store;

impl Store {
    pub fn insert_agent(
        &self,
        id: &str,
        name: &str,
        spec: &str,
        created_at: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO agents (id, name, spec, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, spec=excluded.spec, updated_at=excluded.updated_at",
            params![id, name, spec, created_at, updated_at],
        )?;
        Ok(())
    }

    pub fn list_agents(&self) -> Result<Vec<(String, String, String, String, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, spec, created_at, updated_at FROM agents ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_agent(&self, id: &str) -> Result<Option<(String, String, String, String, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT id, name, spec, created_at, updated_at FROM agents WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            )))
        } else {
            Ok(None)
        }
    }

    pub fn update_agent(&self, id: &str, name: &str, spec: &str, updated_at: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE agents SET name = ?2, spec = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, name, spec, updated_at],
        )?;
        Ok(())
    }

    pub fn delete_agent(&self, id: &str) -> Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM agents WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn get_agent_memory(&self, agent_id: &str) -> Result<Option<crate::agent_memory::AgentMemory>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT memory FROM agent_memory WHERE agent_id = ?1")?;
        let mut rows = stmt.query(params![agent_id])?;
        if let Some(row) = rows.next()? {
            let json: String = row.get(0)?;
            let memory =
                serde_json::from_str(&json).context("failed to deserialize agent memory")?;
            Ok(Some(memory))
        } else {
            Ok(None)
        }
    }

    pub fn put_agent_memory(&self, memory: &crate::agent_memory::AgentMemory) -> Result<()> {
        let json =
            serde_json::to_string(memory).context("failed to serialize agent memory")?;
        self.conn.lock().execute(
            "INSERT INTO agent_memory (agent_id, version, memory, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(agent_id) DO UPDATE SET version=excluded.version, memory=excluded.memory,
                                                 updated_at=excluded.updated_at",
            params![memory.agent_id, memory.version, json, memory.updated_at.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn list_agent_memories(&self) -> Result<Vec<crate::agent_memory::AgentMemory>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT memory FROM agent_memory ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let json = row?;
            out.push(
                serde_json::from_str(&json).context("failed to deserialize agent memory")?,
            );
        }
        Ok(out)
    }
}
