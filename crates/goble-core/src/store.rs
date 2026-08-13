use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use base64::Engine;
use parking_lot::Mutex;
use rusqlite::{params, Connection};

/// Internal SQLite store for agents, chats, teams, execution traces, MCP registry cache and settings.
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path).context("failed to open sqlite store")?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("failed to open in-memory sqlite store")?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                spec TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS workers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                host TEXT,
                pairing_status TEXT NOT NULL,
                public_key TEXT,
                config TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS teams (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                metadata TEXT NOT NULL,
                created_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS team_members (
                team_id TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                PRIMARY KEY (team_id, agent_id)
            ) STRICT;

            CREATE TABLE IF NOT EXISTS chats (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                provider TEXT,
                model TEXT,
                agent_id TEXT,
                worker_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS chat_messages (
                id TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                tool_calls TEXT,
                created_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS executions (
                id TEXT PRIMARY KEY,
                agent_id TEXT,
                worker_id TEXT,
                status TEXT NOT NULL,
                trace TEXT NOT NULL,
                started_at TEXT NOT NULL,
                finished_at TEXT
            ) STRICT;

            CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                source TEXT NOT NULL,
                source_value TEXT,
                manifest TEXT NOT NULL,
                credentials_key TEXT,
                secret_ids TEXT NOT NULL DEFAULT '[]',
                enabled_tools TEXT NOT NULL DEFAULT '[]',
                installed_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS vault_secrets (
                key TEXT PRIMARY KEY,
                encrypted_value BLOB NOT NULL,
                metadata TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS principals (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                created_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS mcp_accounts (
                id TEXT PRIMARY KEY,
                server_id TEXT NOT NULL,
                principal_id TEXT NOT NULL,
                name TEXT NOT NULL,
                secret_ids TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS workflows (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                spec TEXT NOT NULL,
                trigger TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE INDEX IF NOT EXISTS idx_chat_messages_chat_id ON chat_messages(chat_id);
            CREATE INDEX IF NOT EXISTS idx_executions_agent_id ON executions(agent_id);
            CREATE INDEX IF NOT EXISTS idx_team_members_team_id ON team_members(team_id);
            CREATE INDEX IF NOT EXISTS idx_workflows_updated_at ON workflows(updated_at);
            CREATE INDEX IF NOT EXISTS idx_mcp_accounts_principal ON mcp_accounts(principal_id);

            CREATE TABLE IF NOT EXISTS missions (
                id TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                goal TEXT NOT NULL,
                status TEXT NOT NULL,
                plan TEXT,
                workflow_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE INDEX IF NOT EXISTS idx_missions_chat_id ON missions(chat_id);
            CREATE INDEX IF NOT EXISTS idx_missions_status ON missions(status);

            CREATE TABLE IF NOT EXISTS reasoning_steps (
                id TEXT PRIMARY KEY,
                mission_id TEXT NOT NULL,
                step_index INTEGER NOT NULL,
                mode TEXT NOT NULL,
                content TEXT NOT NULL,
                decision TEXT,
                tool_calls TEXT,
                created_at TEXT NOT NULL
            ) STRICT;

            CREATE INDEX IF NOT EXISTS idx_reasoning_mission ON reasoning_steps(mission_id, step_index);

            CREATE TABLE IF NOT EXISTS pending_asks (
                id TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                mission_id TEXT,
                question TEXT NOT NULL,
                quick_replies TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE INDEX IF NOT EXISTS idx_pending_asks_chat ON pending_asks(chat_id, status);

            CREATE TABLE IF NOT EXISTS llm_settings (
                provider TEXT PRIMARY KEY,
                api_key TEXT NOT NULL,
                base_url TEXT,
                model TEXT NOT NULL,
                temperature REAL
            ) STRICT;
            "#,
        )
        .context("failed to run migrations")?;
        Ok(())
    }

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

    pub fn delete_agent(&self, id: &str) -> Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM agents WHERE id = ?1", params![id])?;
        Ok(())
    }

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

    pub fn insert_chat(
        &self,
        id: &str,
        title: &str,
        provider: Option<&str>,
        model: Option<&str>,
        created_at: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO chats (id, title, provider, model, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, title, provider, model, created_at, updated_at],
        )?;
        Ok(())
    }

    pub fn set_chat_model(&self, id: &str, provider: &str, model: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE chats SET provider = ?1, model = ?2 WHERE id = ?3",
            params![provider, model, id],
        )?;
        Ok(())
    }

    pub fn get_chat_model(&self, id: &str) -> Result<Option<(Option<String>, Option<String>)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT provider, model FROM chats WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            )))
        } else {
            Ok(None)
        }
    }

    pub fn list_chats(
        &self,
    ) -> Result<
        Vec<(
            String,
            String,
            Option<String>,
            Option<String>,
            String,
            String,
        )>,
    > {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, title, provider, model, created_at, updated_at FROM chats ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn insert_chat_message(
        &self,
        id: &str,
        chat_id: &str,
        role: &str,
        content: &str,
        tool_calls: Option<&str>,
        created_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO chat_messages (id, chat_id, role, content, tool_calls, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, chat_id, role, content, tool_calls, created_at],
        )?;
        self.conn.lock().execute(
            "UPDATE chats SET updated_at = ?1 WHERE id = ?2",
            params![created_at, chat_id],
        )?;
        Ok(())
    }

    pub fn list_chat_messages(
        &self,
        chat_id: &str,
    ) -> Result<Vec<(String, String, String, Option<String>, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, role, content, tool_calls, created_at FROM chat_messages WHERE chat_id = ?1 ORDER BY created_at ASC")?;
        let rows = stmt.query_map(params![chat_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

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

    pub fn insert_mission(
        &self,
        id: &str,
        chat_id: &str,
        goal: &str,
        status: &str,
        plan: Option<&str>,
        workflow_id: Option<&str>,
        created_at: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO missions (id, chat_id, goal, status, plan, workflow_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET chat_id=excluded.chat_id, goal=excluded.goal, status=excluded.status,
                                           plan=excluded.plan, workflow_id=excluded.workflow_id, updated_at=excluded.updated_at",
            params![id, chat_id, goal, status, plan, workflow_id, created_at, updated_at],
        )?;
        Ok(())
    }

    pub fn list_missions(
        &self,
    ) -> Result<
        Vec<(
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            String,
            String,
        )>,
    > {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, chat_id, goal, status, plan, workflow_id, created_at, updated_at FROM missions ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_mission(
        &self,
        id: &str,
    ) -> Result<
        Option<(
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            String,
            String,
        )>,
    > {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, chat_id, goal, status, plan, workflow_id, created_at, updated_at FROM missions WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            )))
        } else {
            Ok(None)
        }
    }

    pub fn insert_reasoning_step(
        &self,
        id: &str,
        mission_id: &str,
        step_index: i32,
        mode: &str,
        content: &str,
        decision: Option<&str>,
        tool_calls: Option<&str>,
        created_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO reasoning_steps (id, mission_id, step_index, mode, content, decision, tool_calls, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET mission_id=excluded.mission_id, step_index=excluded.step_index,
                                           mode=excluded.mode, content=excluded.content, decision=excluded.decision,
                                           tool_calls=excluded.tool_calls, created_at=excluded.created_at",
            params![id, mission_id, step_index, mode, content, decision, tool_calls, created_at],
        )?;
        Ok(())
    }

    pub fn list_reasoning_steps(
        &self,
        mission_id: &str,
    ) -> Result<
        Vec<(
            String,
            i32,
            String,
            String,
            Option<String>,
            Option<String>,
            String,
        )>,
    > {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, step_index, mode, content, decision, tool_calls, created_at FROM reasoning_steps WHERE mission_id = ?1 ORDER BY step_index ASC",
        )?;
        let rows = stmt.query_map(params![mission_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i32>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn insert_pending_ask(
        &self,
        id: &str,
        chat_id: &str,
        mission_id: Option<&str>,
        question: &str,
        quick_replies: &str,
        status: &str,
        created_at: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO pending_asks (id, chat_id, mission_id, question, quick_replies, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET chat_id=excluded.chat_id, mission_id=excluded.mission_id,
                                           question=excluded.question, quick_replies=excluded.quick_replies,
                                           status=excluded.status, updated_at=excluded.updated_at",
            params![id, chat_id, mission_id, question, quick_replies, status, created_at, updated_at],
        )?;
        Ok(())
    }

    pub fn get_pending_ask(
        &self,
        chat_id: &str,
    ) -> Result<Option<(String, String, Option<String>, String, String, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, mission_id, question, quick_replies, status FROM pending_asks WHERE chat_id = ?1 AND status = 'pending' ORDER BY created_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![chat_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            )))
        } else {
            Ok(None)
        }
    }

    pub fn resolve_pending_ask(&self, id: &str, status: &str, updated_at: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE pending_asks SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, updated_at, id],
        )?;
        Ok(())
    }

    pub fn export_snapshot_payload(&self) -> Result<crate::snapshot::SnapshotPayload> {
        let mut tables = std::collections::HashMap::new();
        for table in SNAPSHOT_TABLES {
            tables.insert(table.to_string(), self.dump_table(table)?);
        }
        Ok(crate::snapshot::SnapshotPayload {
            version: crate::snapshot::SNAPSHOT_VERSION,
            tables,
        })
    }

    pub fn import_snapshot_payload(&self, payload: crate::snapshot::SnapshotPayload) -> Result<()> {
        if payload.version != crate::snapshot::SNAPSHOT_VERSION {
            anyhow::bail!("unsupported snapshot payload version {}", payload.version);
        }
        let conn = self.conn.lock();
        conn.execute("BEGIN IMMEDIATE", [])?;
        let result: Result<()> = (|| {
            for table in SNAPSHOT_TABLES {
                conn.execute(&format!("DELETE FROM {}", table), [])
                    .with_context(|| format!("failed to clear table {}", table))?;
                if let Some(rows) = payload.tables.get(*table) {
                    self.restore_table(&conn, table, rows)?;
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => conn.execute("COMMIT", [])?,
            Err(e) => {
                conn.execute("ROLLBACK", [])?;
                return Err(e);
            }
        };
        Ok(())
    }

    fn dump_table(&self, table: &str) -> Result<Vec<serde_json::Map<String, serde_json::Value>>> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(&format!("SELECT * FROM {}", table))
            .with_context(|| format!("failed to dump table {}", table))?;
        let column_count = stmt.column_count();
        let mut column_names = Vec::with_capacity(column_count);
        for idx in 0..column_count {
            column_names.push(stmt.column_name(idx)?.to_string());
        }
        let mut rows = Vec::new();
        let mut cursor = stmt.query([])?;
        while let Some(row) = cursor.next()? {
            let mut obj = serde_json::Map::new();
            for (idx, name) in column_names.iter().enumerate() {
                let value = match row.get_ref(idx)? {
                    rusqlite::types::ValueRef::Null => serde_json::Value::Null,
                    rusqlite::types::ValueRef::Integer(i) => {
                        serde_json::Value::Number(serde_json::Number::from(i))
                    }
                    rusqlite::types::ValueRef::Real(f) => serde_json::Number::from_f64(f)
                        .map_or(serde_json::Value::Null, serde_json::Value::Number),
                    rusqlite::types::ValueRef::Text(s) => serde_json::Value::String(
                        std::str::from_utf8(s).unwrap_or_default().to_string(),
                    ),
                    rusqlite::types::ValueRef::Blob(b) => serde_json::Value::String(
                        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b),
                    ),
                };
                obj.insert(name.clone(), value);
            }
            rows.push(obj);
        }
        Ok(rows)
    }

    fn restore_table(
        &self,
        conn: &rusqlite::Connection,
        table: &str,
        rows: &[serde_json::Map<String, serde_json::Value>],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let columns: Vec<&str> = rows[0].keys().map(|k| k.as_str()).collect();
        let placeholders = (1..=columns.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            table,
            columns.join(", "),
            placeholders
        );
        let mut stmt = conn
            .prepare(&sql)
            .with_context(|| format!("failed to prepare insert for table {}", table))?;
        for row in rows {
            let mut params: Vec<rusqlite::types::Value> = Vec::new();
            for col in &columns {
                params.push(json_to_sqlite_value(
                    row.get(*col).unwrap_or(&serde_json::Value::Null),
                ));
            }
            stmt.execute(rusqlite::params_from_iter(params.iter()))?;
        }
        Ok(())
    }
}

impl Clone for Store {
    fn clone(&self) -> Self {
        Self {
            conn: Arc::clone(&self.conn),
        }
    }
}

fn json_to_sqlite_value(v: &serde_json::Value) -> rusqlite::types::Value {
    match v {
        serde_json::Value::Null => rusqlite::types::Value::Null,
        serde_json::Value::Bool(b) => rusqlite::types::Value::Integer(*b as i64),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                rusqlite::types::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                rusqlite::types::Value::Real(f)
            } else {
                rusqlite::types::Value::Null
            }
        }
        serde_json::Value::String(s) => {
            if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s) {
                if base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes) == *s {
                    return rusqlite::types::Value::Blob(bytes);
                }
            }
            rusqlite::types::Value::Text(s.clone())
        }
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            rusqlite::types::Value::Text(serde_json::to_string(v).unwrap_or_default())
        }
    }
}

const SNAPSHOT_TABLES: &[&str] = &[
    "settings",
    "agents",
    "workers",
    "teams",
    "team_members",
    "chats",
    "chat_messages",
    "mcp_servers",
    "mcp_accounts",
    "principals",
    "vault_secrets",
    "workflows",
    "executions",
    "llm_settings",
    "missions",
    "reasoning_steps",
    "pending_asks",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_store() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("theme", "dark").unwrap();
        assert_eq!(
            store.get_setting("theme").unwrap(),
            Some("dark".to_string())
        );
        store.set_setting("theme", "light").unwrap();
        assert_eq!(
            store.get_setting("theme").unwrap(),
            Some("light".to_string())
        );
    }

    #[test]
    fn test_agent_crud() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_agent(
                "a1",
                "test-agent",
                "{}",
                "2024-01-01T00:00:00Z",
                "2024-01-01T00:00:00Z",
            )
            .unwrap();
        let agents = store.list_agents().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].1, "test-agent");
        assert!(store.get_agent("a1").unwrap().is_some());
        store.delete_agent("a1").unwrap();
        assert!(store.get_agent("a1").unwrap().is_none());
    }

    #[test]
    fn test_worker_crud() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_worker(
                "w1",
                "vps",
                Some("localhost:8787"),
                "unpaired",
                None,
                "{}",
                "2024-01-01T00:00:00Z",
                "2024-01-01T00:00:00Z",
            )
            .unwrap();
        let workers = store.list_workers().unwrap();
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].1, "vps");
        store.delete_worker("w1").unwrap();
        assert!(store.list_workers().unwrap().is_empty());
    }

    #[test]
    fn test_chat_messages() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_chat(
                "c1",
                "Test",
                None,
                None,
                "2024-01-01T00:00:00Z",
                "2024-01-01T00:00:00Z",
            )
            .unwrap();
        store
            .insert_chat_message("m1", "c1", "user", "hello", None, "2024-01-01T00:00:01Z")
            .unwrap();
        let msgs = store.list_chat_messages("c1").unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].2, "hello");
    }

    #[test]
    fn test_mcp_server_crud() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_mcp_server(
                "mcp1",
                "files",
                "npm",
                Some("@modelcontextprotocol/server-files"),
                "{}",
                None,
                "[]",
                "[]",
                "2024-01-01T00:00:00Z",
                "2024-01-01T00:00:00Z",
            )
            .unwrap();
        let servers = store.list_mcp_servers().unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].1, "files");
    }

    #[test]
    fn test_team_crud() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_team("t1", "Platform", "{}", "2024-01-01T00:00:00Z")
            .unwrap();
        let teams = store.list_teams().unwrap();
        assert_eq!(teams.len(), 1);
        assert_eq!(teams[0].1, "Platform");
        store.insert_team_member("t1", "a1").unwrap();
        let members = store.list_team_members("t1").unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].1, "a1");
    }

    #[test]
    fn test_vault_secret_crud() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_vault_secret("api_key", b"secret", "{}", "2024-01-01T00:00:00Z")
            .unwrap();
        let secrets = store.list_vault_secrets().unwrap();
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[0].0, "api_key");
        assert_eq!(secrets[0].1, b"secret");
    }

    #[test]
    fn test_workflow_crud() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_workflow(
                "wf1",
                "Deploy",
                "Deploy app",
                "{}",
                "manual",
                true,
                "2024-01-01T00:00:00Z",
                "2024-01-01T00:00:00Z",
            )
            .unwrap();
        let workflows = store.list_workflows().unwrap();
        assert_eq!(workflows.len(), 1);
        assert_eq!(workflows[0].1, "Deploy");
        assert!(workflows[0].5);
        store.delete_workflow("wf1").unwrap();
        assert!(store.list_workflows().unwrap().is_empty());
    }

    #[test]
    fn test_execution_crud() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_execution(
                "e1",
                Some("a1"),
                Some("w1"),
                "running",
                "{}",
                "2024-01-01T00:00:00Z",
                None,
            )
            .unwrap();
        let execs = store.list_executions().unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0].3, "running");
    }

    #[test]
    fn test_snapshot_export_import_roundtrip() {
        use crate::snapshot::Snapshot;
        use crate::worker::WorkerId;

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("store.db");
        let store1 = Store::open(&path).unwrap();
        store1.set_setting("hello", "world").unwrap();
        store1
            .insert_agent(
                "a1",
                "agent",
                "{}",
                "2024-01-01T00:00:00Z",
                "2024-01-01T00:00:00Z",
            )
            .unwrap();

        let key = crate::cluster_key::ClusterKey::generate();
        let worker_id = WorkerId::generate();
        let snapshot = Snapshot::from_store(&store1, &worker_id, &key).unwrap();

        let path2 = tmp.path().join("store2.db");
        let store2 = Store::open(&path2).unwrap();
        snapshot.restore_into_store(&store2, &key).unwrap();

        assert_eq!(
            store2.get_setting("hello").unwrap(),
            Some("world".to_string())
        );
        assert_eq!(store2.list_agents().unwrap().len(), 1);
    }

    #[test]
    fn test_identity_wallet_roundtrip_in_snapshot() {
        use crate::cluster_key::ClusterKey;
        use crate::encrypted_wallet::IdentityWallet;
        use crate::snapshot::Snapshot;
        use crate::worker::WorkerId;

        let tmp = tempfile::tempdir().unwrap();
        let store1 = Store::open(tmp.path().join("store1.db")).unwrap();
        let identity = IdentityWallet::new(
            ClusterKey::generate().to_base64(),
            "test-cluster",
            "ca-cert-pem",
            "ca-key-pem",
        );
        let sealed = identity.seal(b"passphrase").unwrap();
        store1.set_cluster_wallet(&sealed).unwrap();

        let key = ClusterKey::generate();
        let worker_id = WorkerId::generate();
        let snapshot = Snapshot::from_store(&store1, &worker_id, &key).unwrap();

        let store2 = Store::open(tmp.path().join("store2.db")).unwrap();
        snapshot.restore_into_store(&store2, &key).unwrap();

        let loaded = store2
            .get_cluster_wallet()
            .unwrap()
            .expect("wallet missing");
        let opened = IdentityWallet::open(&loaded, b"passphrase").unwrap();
        assert_eq!(opened, identity);
    }
}
