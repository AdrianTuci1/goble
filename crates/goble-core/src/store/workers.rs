use anyhow::Result;
use rusqlite::params;

use super::Store;

impl Store {
    pub fn delete_worker(&self, id: &str) -> Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM workers WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn insert_worker(
        &self,
        id: &str,
        name: &str,
        host: Option<&str>,
        status: &str,
        public_key: Option<&str>,
        config: &str,
        created_at: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO workers (id, name, host, pairing_status, public_key, config, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, host=excluded.host, pairing_status=excluded.pairing_status,
                                           public_key=excluded.public_key, config=excluded.config, updated_at=excluded.updated_at",
            params![id, name, host, status, public_key, config, created_at, updated_at],
        )?;
        Ok(())
    }

    pub fn get_worker(&self, id: &str) -> Result<Option<(String, Option<String>, String, String)>> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT name, host, pairing_status, config FROM workers WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            )))
        } else {
            Ok(None)
        }
    }

    pub fn list_workers(
        &self,
    ) -> Result<
        Vec<(
            String,
            String,
            Option<String>,
            String,
            Option<String>,
            String,
            String,
            String,
        )>,
    > {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, name, host, pairing_status, public_key, config, created_at, updated_at FROM workers ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
