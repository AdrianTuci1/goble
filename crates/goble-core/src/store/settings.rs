use anyhow::{Context, Result};
use rusqlite::params;

use super::Store;

impl Store {
    pub fn set_llm_setting(
        &self,
        provider: &str,
        api_key: &str,
        base_url: Option<&str>,
        model: &str,
        temperature: Option<f32>,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO llm_settings (provider, api_key, base_url, model, temperature)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(provider) DO UPDATE SET api_key=excluded.api_key, base_url=excluded.base_url,
                                                model=excluded.model, temperature=excluded.temperature",
            params![provider, api_key, base_url, model, temperature],
        )?;
        Ok(())
    }

    pub fn get_llm_setting(
        &self,
        provider: &str,
    ) -> Result<Option<(String, Option<String>, String, Option<f32>)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT api_key, base_url, model, temperature FROM llm_settings WHERE provider = ?1",
        )?;
        let mut rows = stmt.query(params![provider])?;
        if let Some(row) = rows.next()? {
            Ok(Some((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<f32>>(3)?,
            )))
        } else {
            Ok(None)
        }
    }
}

impl Store {
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        Ok(rows.next()?.map(|r| r.get(0)).transpose()?)
    }

    /// Upsert a named credential (API key, token, secret). The value is stored
    /// in plaintext, consistent with `llm_settings` and `settings`; the harness
    /// exposes only the *name* to the model and substitutes the value at
    /// execution time so it never appears in the transcript.
    pub fn set_credential(&self, name: &str, value: &str) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO credentials (name, value) VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET value = excluded.value",
            params![name, value],
        )?;
        Ok(())
    }

    pub fn get_credential(&self, name: &str) -> Result<Option<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT value FROM credentials WHERE name = ?1")?;
        let mut rows = stmt.query(params![name])?;
        Ok(rows.next()?.map(|r| r.get(0)).transpose()?)
    }

    pub fn list_credential_names(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT name FROM credentials ORDER BY name")?;
        let mut rows = stmt.query([])?;
        let mut names = Vec::new();
        while let Some(row) = rows.next()? {
            names.push(row.get::<_, String>(0)?);
        }
        Ok(names)
    }

    /// Store the encrypted cluster wallet (IdentityWallet) under a dedicated
    /// settings key so it does not collide with the legacy ClusterIdentitySnapshot.
    pub fn set_cluster_wallet(
        &self,
        wallet: &crate::encrypted_wallet::EncryptedWallet,
    ) -> Result<()> {
        let value = serde_json::to_string(wallet).context("failed to serialize cluster wallet")?;
        self.set_setting("cluster_wallet", &value)
    }

    pub fn get_cluster_wallet(&self) -> Result<Option<crate::encrypted_wallet::EncryptedWallet>> {
        match self.get_setting("cluster_wallet")? {
            Some(value) => {
                let wallet =
                    serde_json::from_str(&value).context("failed to deserialize cluster wallet")?;
                Ok(Some(wallet))
            }
            None => Ok(None),
        }
    }

    pub fn set_cluster_identity(
        &self,
        snapshot: &crate::cluster_key::ClusterIdentitySnapshot,
    ) -> Result<()> {
        let value =
            serde_json::to_string(snapshot).context("failed to serialize cluster identity")?;
        self.set_setting("cluster_identity", &value)
    }

    pub fn get_cluster_identity(
        &self,
    ) -> Result<Option<crate::cluster_key::ClusterIdentitySnapshot>> {
        match self.get_setting("cluster_identity")? {
            Some(value) => {
                let snapshot = serde_json::from_str(&value)
                    .context("failed to deserialize cluster identity")?;
                Ok(Some(snapshot))
            }
            None => Ok(None),
        }
    }
}
