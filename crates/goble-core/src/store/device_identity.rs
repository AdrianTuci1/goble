use anyhow::Result;
use rusqlite::params;

use super::Store;

impl Store {
    pub fn insert_device_identity(
        &self,
        id: &str,
        cluster_name: &str,
        cert_pem: &str,
        key_pem: &str,
        ca_cert_pem: &str,
        role: &str,
        is_owner: bool,
        deployment_mode: &str,
        deployment_config: &str,
        created_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO device_identities (id, cluster_name, cert_pem, key_pem, ca_cert_pem, role, is_owner, deployment_mode, deployment_config, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET cluster_name=excluded.cluster_name, cert_pem=excluded.cert_pem, key_pem=excluded.key_pem,
                                           ca_cert_pem=excluded.ca_cert_pem, role=excluded.role, is_owner=excluded.is_owner,
                                           deployment_mode=excluded.deployment_mode, deployment_config=excluded.deployment_config",
            params![id, cluster_name, cert_pem, key_pem, ca_cert_pem, role, is_owner as i32, deployment_mode, deployment_config, created_at],
        )?;
        Ok(())
    }

    pub fn list_device_identities(
        &self,
    ) -> Result<Vec<(String, String, String, String, String, String, bool, String, String, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, cluster_name, cert_pem, key_pem, ca_cert_pem, role, is_owner, deployment_mode, deployment_config, created_at FROM device_identities ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, bool>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_device_identity(&self, id: &str) -> Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM device_identities WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn insert_cluster_invite(
        &self,
        id: &str,
        cluster_name: &str,
        code: &str,
        pem_bundle: &str,
        revoked: bool,
        created_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO cluster_invites (id, cluster_name, code, pem_bundle, revoked, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET cluster_name=excluded.cluster_name, code=excluded.code,
                                           pem_bundle=excluded.pem_bundle, revoked=excluded.revoked",
            params![id, cluster_name, code, pem_bundle, revoked as i32, created_at],
        )?;
        Ok(())
    }

    pub fn list_cluster_invites(
        &self,
        cluster_name: Option<&str>,
    ) -> Result<Vec<(String, String, String, String, bool, String)>> {
        let conn = self.conn.lock();
        if let Some(name) = cluster_name {
            let mut stmt = conn.prepare(
                "SELECT id, cluster_name, code, pem_bundle, revoked, created_at FROM cluster_invites WHERE cluster_name = ?1 ORDER BY created_at DESC",
            )?;
            let rows = stmt.query_map(params![name], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, bool>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })?;
            return rows.collect::<Result<Vec<_>, _>>().map_err(Into::into);
        }
        let mut stmt = conn.prepare(
            "SELECT id, cluster_name, code, pem_bundle, revoked, created_at FROM cluster_invites ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, bool>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_cluster_invite_by_code(&self, code: &str) -> Result<Option<(String, String, String, String, bool, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, cluster_name, code, pem_bundle, revoked, created_at FROM cluster_invites WHERE code = ?1",
        )?;
        let mut rows = stmt.query(params![code])?;
        if let Some(row) = rows.next()? {
            Ok(Some((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, bool>(4)?,
                row.get::<_, String>(5)?,
            )))
        } else {
            Ok(None)
        }
    }

    pub fn revoke_cluster_invite(&self, id: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE cluster_invites SET revoked = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn delete_cluster_invite(&self, id: &str) -> Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM cluster_invites WHERE id = ?1", params![id])?;
        Ok(())
    }
}
