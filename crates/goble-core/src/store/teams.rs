use anyhow::Result;
use rusqlite::params;

use super::Store;

impl Store {
    pub fn list_teams(&self) -> Result<Vec<(String, String, String, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT id, name, metadata, created_at FROM teams ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn insert_team(
        &self,
        id: &str,
        name: &str,
        metadata: &str,
        created_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO teams (id, name, metadata, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, metadata=excluded.metadata",
            params![id, name, metadata, created_at],
        )?;
        Ok(())
    }

    pub fn delete_team(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM team_members WHERE team_id = ?1", params![id])?;
        conn.execute("DELETE FROM teams WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn insert_team_member(&self, team_id: &str, agent_id: &str) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO team_members (team_id, agent_id) VALUES (?1, ?2)
             ON CONFLICT DO NOTHING",
            params![team_id, agent_id],
        )?;
        Ok(())
    }

    pub fn list_team_members(&self, team_id: &str) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT team_id, agent_id FROM team_members WHERE team_id = ?1")?;
        let rows = stmt.query_map(params![team_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
