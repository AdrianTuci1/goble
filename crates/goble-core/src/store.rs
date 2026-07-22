use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{Connection, params};

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
                manifest TEXT NOT NULL,
                credentials_key TEXT,
                installed_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE INDEX IF NOT EXISTS idx_chat_messages_chat_id ON chat_messages(chat_id);
            CREATE INDEX IF NOT EXISTS idx_executions_agent_id ON executions(agent_id);
            CREATE INDEX IF NOT EXISTS idx_team_members_team_id ON team_members(team_id);
            "#,
        )
        .context("failed to run migrations")?;
        Ok(())
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
        manifest: &str,
        credentials_key: Option<&str>,
        installed_at: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO mcp_servers (id, name, source, manifest, credentials_key, installed_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, source=excluded.source, manifest=excluded.manifest,
                                           credentials_key=excluded.credentials_key, updated_at=excluded.updated_at",
            params![id, name, source, manifest, credentials_key, installed_at, updated_at],
        )?;
        Ok(())
    }

    pub fn list_mcp_servers(
        &self,
    ) -> Result<
        Vec<(
            String,
            String,
            String,
            String,
            Option<String>,
            String,
            String,
        )>,
    > {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, name, source, manifest, credentials_key, installed_at, updated_at FROM mcp_servers ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

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
                "vps-1",
                Some("10.0.0.1"),
                "paired",
                None,
                "{}",
                "2024-01-01T00:00:00Z",
                "2024-01-01T00:00:00Z",
            )
            .unwrap();
        let workers = store.list_workers().unwrap();
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].1, "vps-1");
    }

    #[test]
    fn test_chat_messages() {
        let store = Store::open_in_memory().unwrap();
        store.conn.lock().execute(
            "INSERT INTO chats (id, title, created_at, updated_at) VALUES ('c1', 'hello', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
            [],
        ).unwrap();
        store
            .insert_chat_message("m1", "c1", "user", "hi", None, "2024-01-01T00:00:01Z")
            .unwrap();
        let messages = store.list_chat_messages("c1").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].2, "hi");
    }

    #[test]
    fn test_mcp_servers() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_mcp_server(
                "m1",
                "postgres",
                "github",
                "{}",
                Some("secret/m1"),
                "2024-01-01T00:00:00Z",
                "2024-01-01T00:00:00Z",
            )
            .unwrap();
        let servers = store.list_mcp_servers().unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].5, "2024-01-01T00:00:00Z");
    }
}
