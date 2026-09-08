//! Durable storage for reversible-execution checkpoints and the durable
//! Project/Session/Task entities that describe the workspace and its runs.
//!
//! This is the persistence layer that makes the reversibility substrate survive
//! a restart — the analogue of Shepherd storing its trajectory in a real Git
//! repo on disk. It serializes [`goble_replay::Checkpoint`]s (which already carry
//! all turn records + events) into a SQLite table keyed by session, and lets the
//! daemon persist them through the [`CheckpointSink`] hook without the model
//! crate knowing anything about storage.
//!
//! Alongside checkpoints it stores the durable entity tables — `projects`,
//! `sessions` and `tasks` — so the composition root can seed the running state
//! from what was persisted on a previous run.
//!
//! Only `rusqlite` is used (already a workspace dependency); no Diesel, no ORM.

use std::path::Path;

use goble_harness_types::{MediumId, ProjectId, SessionId};
use goble_replay::{Checkpoint, CheckpointSink};
use parking_lot::Mutex;
use rusqlite::Connection;
use thiserror::Error;

/// Storage errors.
#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS checkpoints (
    session_id      TEXT NOT NULL PRIMARY KEY,
    checkpoint_json TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
    project_id TEXT NOT NULL PRIMARY KEY,
    directory   TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT NOT NULL,
    medium_id  TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tasks (
    task_id     TEXT NOT NULL PRIMARY KEY,
    session_id  TEXT NOT NULL,
    trigger     TEXT NOT NULL,
    status      TEXT NOT NULL,
    created_at  TEXT NOT NULL
);
";

/// A durable project: the directory / mounted workspace work happens in.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub project_id: ProjectId,
    pub directory: String,
    pub created_at: String,
}

/// A durable session: one conversation / run on a project, on a medium.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub session_id: SessionId,
    pub project_id: ProjectId,
    pub medium_id: MediumId,
    pub created_at: String,
}

/// A durable task: a scheduled or manual unit of work on a session.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Task {
    pub task_id: String,
    pub session_id: SessionId,
    pub trigger: String,
    pub status: String,
    pub created_at: String,
}

/// Migrate an on-disk database that was written before the entity tables
/// existed. Earlier versions stored checkpoints in a table named `sessions`
/// (with a `checkpoint_json` column); those rows are carried into the new
/// `checkpoints` table so no previously-persisted reversible history is lost,
/// then the legacy `sessions` table is dropped to free the name for the durable
/// Session entity table. A fresh or already-migrated database is a no-op.
fn migrate_legacy_checkpoint_table(conn: &Connection) -> Result<(), PersistenceError> {
    let has_sessions: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='sessions')",
        [],
        |r| r.get(0),
    )?;
    if !has_sessions {
        return Ok(());
    }
    // Only migrate if the existing `sessions` table is the legacy checkpoint
    // table (it carries a `checkpoint_json` column), not the entity table.
    let is_legacy: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('sessions') WHERE name='checkpoint_json')",
        [],
        |r| r.get(0),
    )?;
    if is_legacy {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS checkpoints (
                session_id      TEXT NOT NULL PRIMARY KEY,
                checkpoint_json TEXT NOT NULL,
                updated_at      TEXT NOT NULL
             );
             INSERT INTO checkpoints (session_id, checkpoint_json, updated_at)
               SELECT session_id, checkpoint_json, updated_at FROM sessions;
             DROP TABLE sessions;",
        )?;
    }
    Ok(())
}

/// A checkpoint store backed by a single SQLite file (or in-memory for tests).
/// Thread-safe: the raw connection is wrapped in a [`Mutex`]. Implements
/// [`CheckpointSink`], so it can be handed straight to the daemon.
pub struct CheckpointStore {
    conn: Mutex<Connection>,
}

impl CheckpointStore {
    /// Open (or create) a store at `path`.
    pub fn open(path: &Path) -> Result<Self, PersistenceError> {
        Self::init(Connection::open(path)?)
    }

    /// A store backed by an in-memory SQLite database. Useful for tests.
    pub fn in_memory() -> Self {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        Self::init(conn).expect("init in-memory sqlite")
    }

    fn init(conn: Connection) -> Result<Self, PersistenceError> {
        migrate_legacy_checkpoint_table(&conn)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Upsert the checkpoint for a session, replacing any prior one.
    pub fn save_checkpoint(&self, cp: &Checkpoint) -> Result<(), PersistenceError> {
        let json = serde_json::to_string(cp)?;
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO checkpoints (session_id, checkpoint_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(session_id) DO UPDATE SET
               checkpoint_json = excluded.checkpoint_json,
               updated_at      = excluded.updated_at",
            rusqlite::params![cp.session_id.0, json, now],
        )?;
        Ok(())
    }

    /// Load a session's checkpoint, if one was persisted.
    pub fn load_checkpoint(&self, session_id: &SessionId) -> Result<Option<Checkpoint>, PersistenceError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT checkpoint_json FROM checkpoints WHERE session_id = ?1")?;
        let mut rows = stmt.query(rusqlite::params![session_id.0])?;
        if let Some(row) = rows.next()? {
            let json: String = row.get(0)?;
            Ok(Some(serde_json::from_str(&json)?))
        } else {
            Ok(None)
        }
    }

    /// List the persisted checkpoint sessions, most recently updated first.
    pub fn list_checkpoint_sessions(&self) -> Result<Vec<SessionId>, PersistenceError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT session_id FROM checkpoints ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(SessionId::new(row?));
        }
        Ok(out)
    }

    /// Delete a session's checkpoint.
    pub fn delete_checkpoint_session(&self, session_id: &SessionId) -> Result<(), PersistenceError> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM checkpoints WHERE session_id = ?1",
            rusqlite::params![session_id.0],
        )?;
        Ok(())
    }

    /// Upsert a project, replacing any prior row with the same id.
    pub fn upsert_project(&self, project: &Project) -> Result<(), PersistenceError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO projects (project_id, directory, created_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(project_id) DO UPDATE SET
               directory  = excluded.directory,
               created_at = excluded.created_at",
            rusqlite::params![project.project_id.0, project.directory, project.created_at],
        )?;
        Ok(())
    }

    /// Load a project by id, if one was persisted.
    pub fn load_project(&self, project_id: &ProjectId) -> Result<Option<Project>, PersistenceError> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT project_id, directory, created_at FROM projects WHERE project_id = ?1")?;
        let mut rows = stmt.query(rusqlite::params![project_id.0])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Project {
                project_id: ProjectId::new(row.get::<_, String>(0)?),
                directory: row.get::<_, String>(1)?,
                created_at: row.get::<_, String>(2)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// List all persisted projects, oldest first.
    pub fn list_projects(&self) -> Result<Vec<Project>, PersistenceError> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT project_id, directory, created_at FROM projects ORDER BY created_at ASC")?;
        let rows = stmt.query_map([], |r| {
            Ok(Project {
                project_id: ProjectId::new(r.get::<_, String>(0)?),
                directory: r.get::<_, String>(1)?,
                created_at: r.get::<_, String>(2)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Delete a project by id.
    pub fn delete_project(&self, project_id: &ProjectId) -> Result<(), PersistenceError> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM projects WHERE project_id = ?1",
            rusqlite::params![project_id.0],
        )?;
        Ok(())
    }

    /// Upsert a session, replacing any prior row with the same id.
    pub fn upsert_session(&self, session: &Session) -> Result<(), PersistenceError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO sessions (session_id, project_id, medium_id, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(session_id) DO UPDATE SET
               project_id = excluded.project_id,
               medium_id  = excluded.medium_id,
               created_at = excluded.created_at",
            rusqlite::params![
                session.session_id.0,
                session.project_id.0,
                session.medium_id.0,
                session.created_at,
            ],
        )?;
        Ok(())
    }

    /// Load a session by id, if one was persisted.
    pub fn load_session(&self, session_id: &SessionId) -> Result<Option<Session>, PersistenceError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT session_id, project_id, medium_id, created_at FROM sessions WHERE session_id = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![session_id.0])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Session {
                session_id: SessionId::new(row.get::<_, String>(0)?),
                project_id: ProjectId::new(row.get::<_, String>(1)?),
                medium_id: MediumId::new(row.get::<_, String>(2)?),
                created_at: row.get::<_, String>(3)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// List all persisted sessions, oldest first.
    pub fn list_sessions(&self) -> Result<Vec<Session>, PersistenceError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT session_id, project_id, medium_id, created_at FROM sessions ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Session {
                session_id: SessionId::new(r.get::<_, String>(0)?),
                project_id: ProjectId::new(r.get::<_, String>(1)?),
                medium_id: MediumId::new(r.get::<_, String>(2)?),
                created_at: r.get::<_, String>(3)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Delete a session by id.
    pub fn delete_session(&self, session_id: &SessionId) -> Result<(), PersistenceError> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM sessions WHERE session_id = ?1",
            rusqlite::params![session_id.0],
        )?;
        Ok(())
    }

    /// Upsert a task, replacing any prior row with the same id.
    pub fn upsert_task(&self, task: &Task) -> Result<(), PersistenceError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO tasks (task_id, session_id, trigger, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(task_id) DO UPDATE SET
               session_id = excluded.session_id,
               trigger    = excluded.trigger,
               status     = excluded.status,
               created_at = excluded.created_at",
            rusqlite::params![
                task.task_id,
                task.session_id.0,
                task.trigger,
                task.status,
                task.created_at,
            ],
        )?;
        Ok(())
    }

    /// Load a task by id, if one was persisted.
    pub fn load_task(&self, task_id: &str) -> Result<Option<Task>, PersistenceError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT task_id, session_id, trigger, status, created_at FROM tasks WHERE task_id = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![task_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Task {
                task_id: row.get::<_, String>(0)?,
                session_id: SessionId::new(row.get::<_, String>(1)?),
                trigger: row.get::<_, String>(2)?,
                status: row.get::<_, String>(3)?,
                created_at: row.get::<_, String>(4)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// List all persisted tasks, oldest first.
    pub fn list_tasks(&self) -> Result<Vec<Task>, PersistenceError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT task_id, session_id, trigger, status, created_at FROM tasks ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Task {
                task_id: r.get::<_, String>(0)?,
                session_id: SessionId::new(r.get::<_, String>(1)?),
                trigger: r.get::<_, String>(2)?,
                status: r.get::<_, String>(3)?,
                created_at: r.get::<_, String>(4)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Delete a task by id.
    pub fn delete_task(&self, task_id: &str) -> Result<(), PersistenceError> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM tasks WHERE task_id = ?1",
            rusqlite::params![task_id],
        )?;
        Ok(())
    }
}

impl CheckpointSink for CheckpointStore {
    fn persist(&self, checkpoint: &Checkpoint) {
        let _ = self.save_checkpoint(checkpoint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_harness_protocol::HarnessServerEvent;
    use goble_harness_types::{HarnessId, HarnessTurn};
    use goble_replay::{ReplayLedger, TurnStatus};
    use std::path::Path;

    fn ledger_with_one_turn(session: &str) -> ReplayLedger {
        let ledger = ReplayLedger::new(SessionId::new(session));
        let turn = HarnessTurn::new(HarnessId::new("mock"), SessionId::new(session), "fix bug");
        ledger.record_turn(turn);
        ledger
            .append_events(vec![
                HarnessServerEvent::AssistantDelta {
                    session_id: SessionId::new(session),
                    delta: "hello".to_string(),
                },
                HarnessServerEvent::Done {
                    session_id: SessionId::new(session),
                },
            ])
            .unwrap();
        ledger.settle_turn(TurnStatus::Success).unwrap();
        ledger
    }

    fn sample_project(id: &str) -> Project {
        Project {
            project_id: ProjectId::new(id),
            directory: format!("/workspace/{id}"),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    fn sample_session(id: &str, project: &str) -> Session {
        Session {
            session_id: SessionId::new(id),
            project_id: ProjectId::new(project),
            medium_id: MediumId::new("local"),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    fn sample_task(id: &str, session: &str) -> Task {
        Task {
            task_id: id.to_string(),
            session_id: SessionId::new(session),
            trigger: "manual".to_string(),
            status: "pending".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn save_and_load_roundtrips() {
        let store = CheckpointStore::in_memory();
        let ledger = ledger_with_one_turn("s1");
        let cp = ledger.checkpoint();
        store.save_checkpoint(&cp).unwrap();

        let loaded = store.load_checkpoint(&SessionId::new("s1")).unwrap().unwrap();
        assert_eq!(loaded, cp);
        assert_eq!(loaded.at, 1);

        // A missing session returns None, not an error.
        assert!(store.load_checkpoint(&SessionId::new("nope")).unwrap().is_none());
    }

    #[test]
    fn save_replaces_prior_checkpoint() {
        let store = CheckpointStore::in_memory();
        store.save_checkpoint(&ledger_with_one_turn("s1").checkpoint()).unwrap();

        // A second, longer transcript replaces the first.
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        for i in 0..3 {
            let turn = HarnessTurn::new(HarnessId::new("mock"), SessionId::new("s1"), format!("g{i}"));
            ledger.record_turn(turn);
        }
        store.save_checkpoint(&ledger.checkpoint()).unwrap();

        let loaded = store.load_checkpoint(&SessionId::new("s1")).unwrap().unwrap();
        assert_eq!(loaded.at, 3);
        assert_eq!(store.list_checkpoint_sessions().unwrap(), vec![SessionId::new("s1")]);
    }

    #[test]
    fn list_and_delete_checkpoint_sessions() {
        let store = CheckpointStore::in_memory();
        store.save_checkpoint(&ledger_with_one_turn("a").checkpoint()).unwrap();
        store.save_checkpoint(&ledger_with_one_turn("b").checkpoint()).unwrap();

        let mut sessions = store.list_checkpoint_sessions().unwrap();
        sessions.sort_by(|x, y| x.0.cmp(&y.0));
        assert_eq!(sessions, vec![SessionId::new("a"), SessionId::new("b")]);

        store.delete_checkpoint_session(&SessionId::new("a")).unwrap();
        assert_eq!(store.list_checkpoint_sessions().unwrap(), vec![SessionId::new("b")]);
    }

    #[test]
    fn persisted_checkpoint_restores_a_ledger() {
        let store = CheckpointStore::in_memory();
        let original = ledger_with_one_turn("s1");
        store.save_checkpoint(&original.checkpoint()).unwrap();

        // After a "restart", rebuild the ledger from the store and replay.
        let loaded = store.load_checkpoint(&SessionId::new("s1")).unwrap().unwrap();
        let rebuilt = ReplayLedger::from_checkpoint(&loaded);
        let replayed = rebuilt.replay(0);
        assert_eq!(replayed.len(), 2);
        assert!(matches!(
            &replayed[0],
            HarnessServerEvent::AssistantDelta { delta, .. } if delta == "hello"
        ));
        // And the re-built ledger can itself be rewound/forked.
        assert_eq!(rebuilt.rewind_to(0), 1);
        assert!(rebuilt.is_empty());
    }

    #[test]
    fn store_is_a_checkpoint_sink() {
        let store = CheckpointStore::in_memory();
        let cp = ledger_with_one_turn("s1").checkpoint();
        store.persist(&cp);
        assert_eq!(store.load_checkpoint(&SessionId::new("s1")).unwrap().unwrap(), cp);
    }

    #[test]
    fn persists_to_a_real_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("checkpoints.sqlite");
        let cp = ledger_with_one_turn("s1").checkpoint();

        {
            let store = CheckpointStore::open(&path).unwrap();
            store.save_checkpoint(&cp).unwrap();
        }
        // Re-open and read back.
        let store = CheckpointStore::open(Path::new(&path)).unwrap();
        assert_eq!(store.load_checkpoint(&SessionId::new("s1")).unwrap().unwrap(), cp);
    }

    #[test]
    fn project_upsert_load_list_delete() {
        let store = CheckpointStore::in_memory();
        store.upsert_project(&sample_project("p1")).unwrap();

        let loaded = store.load_project(&ProjectId::new("p1")).unwrap().unwrap();
        assert_eq!(loaded, sample_project("p1"));
        assert_eq!(loaded.project_id, ProjectId::new("p1"));

        // A missing project returns None.
        assert!(store.load_project(&ProjectId::new("nope")).unwrap().is_none());

        // Upsert replaces a prior row for the same id.
        let mut updated = sample_project("p1");
        updated.directory = "/workspace/p1-moved".to_string();
        updated.created_at = "2026-02-01T00:00:00Z".to_string();
        store.upsert_project(&updated).unwrap();
        assert_eq!(store.list_projects().unwrap(), vec![updated]);

        // List/delete.
        store.upsert_project(&sample_project("p2")).unwrap();
        let mut listed = store.list_projects().unwrap();
        listed.sort_by(|a, b| a.project_id.0.cmp(&b.project_id.0));
        assert_eq!(listed.len(), 2);

        store.delete_project(&ProjectId::new("p1")).unwrap();
        assert_eq!(store.list_projects().unwrap(), vec![sample_project("p2")]);
    }

    #[test]
    fn session_upsert_load_list_delete() {
        let store = CheckpointStore::in_memory();
        store.upsert_session(&sample_session("s1", "p1")).unwrap();

        let loaded = store.load_session(&SessionId::new("s1")).unwrap().unwrap();
        assert_eq!(loaded, sample_session("s1", "p1"));
        assert_eq!(loaded.medium_id, MediumId::new("local"));
        assert!(store.load_session(&SessionId::new("nope")).unwrap().is_none());

        // Upsert replaces a prior row for the same id.
        let mut updated = sample_session("s1", "p1");
        updated.medium_id = MediumId::new("remote");
        store.upsert_session(&updated).unwrap();
        assert_eq!(store.list_sessions().unwrap(), vec![updated]);

        // List/delete.
        store.upsert_session(&sample_session("s2", "p2")).unwrap();
        assert_eq!(store.list_sessions().unwrap().len(), 2);

        store.delete_session(&SessionId::new("s1")).unwrap();
        assert_eq!(store.list_sessions().unwrap(), vec![sample_session("s2", "p2")]);
    }

    #[test]
    fn task_upsert_load_list_delete() {
        let store = CheckpointStore::in_memory();
        store.upsert_task(&sample_task("t1", "s1")).unwrap();

        let loaded = store.load_task("t1").unwrap().unwrap();
        assert_eq!(loaded, sample_task("t1", "s1"));
        assert!(store.load_task("nope").unwrap().is_none());

        // Upsert replaces a prior row for the same id.
        let mut updated = sample_task("t1", "s1");
        updated.status = "running".to_string();
        store.upsert_task(&updated).unwrap();
        assert_eq!(store.list_tasks().unwrap(), vec![updated]);

        // List/delete.
        store.upsert_task(&sample_task("t2", "s2")).unwrap();
        assert_eq!(store.list_tasks().unwrap().len(), 2);

        store.delete_task("t1").unwrap();
        assert_eq!(store.list_tasks().unwrap(), vec![sample_task("t2", "s2")]);
    }

    #[test]
    fn entities_persist_to_a_real_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("entities.sqlite");
        {
            let store = CheckpointStore::open(&path).unwrap();
            store.upsert_project(&sample_project("p1")).unwrap();
            store.upsert_session(&sample_session("s1", "p1")).unwrap();
            store.upsert_task(&sample_task("t1", "s1")).unwrap();
        }
        // Re-open and read back.
        let store = CheckpointStore::open(Path::new(&path)).unwrap();
        assert_eq!(store.load_project(&ProjectId::new("p1")).unwrap().unwrap(), sample_project("p1"));
        assert_eq!(store.load_session(&SessionId::new("s1")).unwrap().unwrap(), sample_session("s1", "p1"));
        assert_eq!(store.load_task("t1").unwrap().unwrap(), sample_task("t1", "s1"));
    }

    #[test]
    fn migrates_legacy_checkpoint_sessions_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.sqlite");
        let cp = ledger_with_one_turn("s1").checkpoint();
        {
            // Build a pre-entity database: checkpoints live in a `sessions` table
            // that carries a `checkpoint_json` column.
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (
                    session_id      TEXT NOT NULL PRIMARY KEY,
                    checkpoint_json TEXT NOT NULL,
                    updated_at      TEXT NOT NULL
                 );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO sessions (session_id, checkpoint_json, updated_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![cp.session_id.0, serde_json::to_string(&cp).unwrap(), "2026-01-01T00:00:00Z"],
            )
            .unwrap();
        }
        // Re-open through the store: the legacy rows land in `checkpoints`.
        let store = CheckpointStore::open(&path).unwrap();
        assert_eq!(store.load_checkpoint(&SessionId::new("s1")).unwrap().unwrap(), cp);
        assert_eq!(store.list_checkpoint_sessions().unwrap(), vec![SessionId::new("s1")]);

        // The legacy table was dropped and the new entity table created.
        let conn = rusqlite::Connection::open(&path).unwrap();
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='table' AND name IN ('sessions','checkpoints')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 2);
        assert_eq!(
            check_table_column(&conn, "sessions", "project_id"),
            true,
            "the new entity `sessions` table must be created with its own columns"
        );
    }

    fn check_table_column(conn: &rusqlite::Connection, table: &str, column: &str) -> bool {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
            rusqlite::params![table, column],
            |r| r.get(0),
        )
        .unwrap()
    }
}
