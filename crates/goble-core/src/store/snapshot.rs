use anyhow::{Context, Result};
use base64::Engine;

use super::Store;

impl Store {
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
    "access_grants",
    "vault_secrets",
    "workflows",
    "executions",
    "llm_settings",
    "credentials",
    "missions",
    "reasoning_steps",
    "pending_asks",
    "pending_commands",
    "agent_memory",
];
