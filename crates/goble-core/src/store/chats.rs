use anyhow::Result;
use rusqlite::params;

use super::Store;

impl Store {
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

    pub fn delete_chat(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM chat_messages WHERE chat_id = ?1", params![id])?;
        conn.execute("DELETE FROM chats WHERE id = ?1", params![id])?;
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

    /// Persist the workspace routing (`local` / `remote`) for a chat.
    pub fn set_chat_workspace_routing(&self, id: &str, routing: Option<&str>) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE chats SET workspace_routing = ?1 WHERE id = ?2",
            params![routing, id],
        )?;
        Ok(())
    }

    /// Read the workspace routing for a chat, if it has been chosen.
    pub fn get_chat_workspace_routing(&self, id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT workspace_routing FROM chats WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(row.get::<_, Option<String>>(0)?)
        } else {
            Ok(None)
        }
    }

    /// Open a sub-agent's own conversation: a `chats` row keyed by the child's
    /// id (its `SubAgentId`) carrying the `parent_chat_id` that spawned it. The
    /// child's messages live under its own id, never in the parent's rows
    /// (`.agents/04-agent-runtime/subagents.md`, S2).
    pub fn insert_subagent_chat(
        &self,
        id: &str,
        title: &str,
        parent_chat_id: &str,
        provider: Option<&str>,
        model: Option<&str>,
        created_at: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO chats (id, title, provider, model, parent_chat_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, title, provider, model, parent_chat_id, created_at, updated_at],
        )?;
        Ok(())
    }

    /// The conversation that spawned this chat, or `None` for a top-level chat
    /// (and for a chat that does not exist).
    pub fn get_chat_parent(&self, id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT parent_chat_id FROM chats WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(row.get::<_, Option<String>>(0)?)
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

    /// Append `delta` to an existing chat message's content. Used by the harness
    /// to stream assistant deltas into a single message row so the renderer can
    /// show the reply progressively.
    pub fn append_chat_message_content(&self, message_id: &str, delta: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE chat_messages SET content = content || ?1 WHERE id = ?2",
            params![delta, message_id],
        )?;
        Ok(())
    }

    /// Attach tool-call JSON to an existing chat message. Used by the harness to
    /// record the tool-call metadata on the streamed assistant message once the
    /// current turn's stream finishes.
    pub fn set_chat_message_tool_calls(&self, message_id: &str, tool_calls: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE chat_messages SET tool_calls = ?1 WHERE id = ?2",
            params![tool_calls, message_id],
        )?;
        Ok(())
    }
}
