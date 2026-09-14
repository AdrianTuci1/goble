use anyhow::Result;
use rusqlite::params;

use super::Store;

impl Store {
    pub fn insert_mcp_server(
        &self,
        id: &str,
        name: &str,
        source: &str,
        source_value: Option<&str>,
        manifest: &str,
        credentials_key: Option<&str>,
        secret_ids: &str,
        enabled_tools: &str,
        installed_at: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO mcp_servers (id, name, source, source_value, manifest, credentials_key, secret_ids, enabled_tools, installed_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, source=excluded.source, source_value=excluded.source_value, manifest=excluded.manifest,
                                           credentials_key=excluded.credentials_key, secret_ids=excluded.secret_ids, enabled_tools=excluded.enabled_tools,
                                           updated_at=excluded.updated_at",
            params![id, name, source, source_value, manifest, credentials_key, secret_ids, enabled_tools, installed_at, updated_at],
        )?;
        Ok(())
    }

    pub fn delete_mcp_server(&self, id: &str) -> Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM mcp_servers WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_mcp_servers(
        &self,
    ) -> Result<
        Vec<(
            String,
            String,
            String,
            Option<String>,
            String,
            Option<String>,
            String,
            String,
            String,
            String,
        )>,
    > {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, name, source, source_value, manifest, credentials_key, secret_ids, enabled_tools, installed_at, updated_at FROM mcp_servers ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn insert_principal(
        &self,
        id: &str,
        kind: &str,
        name: &str,
        created_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO principals (id, kind, name, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, name=excluded.name",
            params![id, kind, name, created_at],
        )?;
        Ok(())
    }

    pub fn list_principals(&self) -> Result<Vec<(String, String, String, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT id, kind, name, created_at FROM principals ORDER BY created_at ASC")?;
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

    pub fn get_principal(&self, id: &str) -> Result<Option<(String, String, String, String)>> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT id, kind, name, created_at FROM principals WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            )))
        } else {
            Ok(None)
        }
    }

    pub fn delete_principal(&self, id: &str) -> Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM principals WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Grant `grant` (e.g. "run", "read") over `scope` (e.g. "workspace",
    /// "mcp:search") to a principal. Grants record what every principal with
    /// access to this workspace may do.
    pub fn grant_access(&self, principal_id: &str, grant: &str, scope: &str) -> Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.lock().execute(
            "INSERT INTO access_grants (id, principal_id, grant, scope, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, principal_id, grant, scope, now],
        )?;
        Ok(())
    }

    /// List the grants for a principal as `(grant, scope, created_at)`.
    pub fn list_access(&self, principal_id: &str) -> Result<Vec<(String, String, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT grant, scope, created_at FROM access_grants WHERE principal_id = ?1 ORDER BY created_at ASC",
        )?;
        let mut rows = stmt.query(params![principal_id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, String>(2)?,
            ));
        }
        Ok(out)
    }

    /// Remove a matching grant (by principal + grant + scope). Returns whether a
    /// row was deleted.
    pub fn revoke_access(&self, principal_id: &str, grant: &str, scope: &str) -> Result<bool> {
        let removed = self.conn.lock().execute(
            "DELETE FROM access_grants WHERE principal_id = ?1 AND grant = ?2 AND (scope = ?3 OR scope IS NULL)",
            params![principal_id, grant, scope],
        )?;
        Ok(removed > 0)
    }

    pub fn insert_mcp_account(
        &self,
        id: &str,
        server_id: &str,
        principal_id: &str,
        name: &str,
        secret_ids: &str,
        created_at: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO mcp_accounts (id, server_id, principal_id, name, secret_ids, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET server_id=excluded.server_id, principal_id=excluded.principal_id,
                                           name=excluded.name, secret_ids=excluded.secret_ids, updated_at=excluded.updated_at",
            params![id, server_id, principal_id, name, secret_ids, created_at, updated_at],
        )?;
        Ok(())
    }

    pub fn list_mcp_accounts(
        &self,
        principal_id: Option<&str>,
    ) -> Result<Vec<(String, String, String, String, String, String, String)>> {
        let conn = self.conn.lock();
        if let Some(pid) = principal_id {
            let mut stmt = conn.prepare(
                "SELECT id, server_id, principal_id, name, secret_ids, created_at, updated_at FROM mcp_accounts WHERE principal_id = ?1 ORDER BY updated_at DESC",
            )?;
            let rows = stmt.query_map(params![pid], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })?;
            return rows.collect::<Result<Vec<_>, _>>().map_err(Into::into);
        }
        let mut stmt = conn.prepare(
            "SELECT id, server_id, principal_id, name, secret_ids, created_at, updated_at FROM mcp_accounts ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_mcp_account(
        &self,
        id: &str,
    ) -> Result<Option<(String, String, String, String, String, String, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, server_id, principal_id, name, secret_ids, created_at, updated_at FROM mcp_accounts WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            )))
        } else {
            Ok(None)
        }
    }

    pub fn delete_mcp_account(&self, id: &str) -> Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM mcp_accounts WHERE id = ?1", params![id])?;
        Ok(())
    }
}
