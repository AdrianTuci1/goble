use anyhow::Result;
use rusqlite::params;

use super::Store;

impl Store {
    pub fn insert_vault_secret(
        &self,
        key: &str,
        encrypted_value: &[u8],
        metadata: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO vault_secrets (key, encrypted_value, metadata, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO UPDATE SET encrypted_value=excluded.encrypted_value, metadata=excluded.metadata, updated_at=excluded.updated_at",
            params![key, encrypted_value, metadata, updated_at],
        )?;
        Ok(())
    }

    pub fn list_vault_secrets(&self) -> Result<Vec<(String, Vec<u8>, String, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT key, encrypted_value, metadata, updated_at FROM vault_secrets ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
