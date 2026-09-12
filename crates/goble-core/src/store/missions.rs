use anyhow::Result;
use rusqlite::params;

use super::Store;

impl Store {
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

    /// Record a command the harness proposed and is waiting on the user to
    /// approve, edit or reject (A6). `message_id` is the assistant row holding
    /// the call's tool-call record, so the resume path can rewrite its outcome.
    pub fn insert_pending_command(
        &self,
        call_id: &str,
        chat_id: &str,
        message_id: &str,
        candidates: &str,
        cwd: &str,
        status: &str,
        created_at: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO pending_commands (call_id, chat_id, message_id, candidates, cwd, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(call_id) DO UPDATE SET chat_id=excluded.chat_id, message_id=excluded.message_id,
                                                candidates=excluded.candidates, cwd=excluded.cwd,
                                                status=excluded.status, updated_at=excluded.updated_at",
            params![call_id, chat_id, message_id, candidates, cwd, status, created_at, updated_at],
        )?;
        Ok(())
    }

    /// The oldest pending command proposal for a chat, as
    /// `(call_id, message_id, candidates, cwd)`.
    pub fn get_pending_command(
        &self,
        chat_id: &str,
    ) -> Result<Option<(String, String, String, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT call_id, message_id, candidates, cwd FROM pending_commands WHERE chat_id = ?1 AND status = 'pending' ORDER BY created_at ASC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![chat_id])?;
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

    pub fn resolve_pending_command(
        &self,
        call_id: &str,
        status: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE pending_commands SET status = ?1, updated_at = ?2 WHERE call_id = ?3",
            params![status, updated_at, call_id],
        )?;
        Ok(())
    }
}
