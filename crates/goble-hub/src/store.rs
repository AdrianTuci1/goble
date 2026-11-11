//! Durable hub storage: deployed revisions, runs, leases, deliveries and the
//! signed audit chain.
//!
//! One SQLite connection behind a mutex is the single-user deployment. The
//! record types and the state machine are storage-independent, so a team
//! deployment can swap in Postgres without changing dispatch.

use std::path::Path;
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard};

use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::workflow::{content_hash, WorkflowError, WorkflowFile};

const RUN_COLUMNS: &str = "id, workflow, revision_hash, idempotency_key, state, inputs, \
     attempts, max_attempts, target_tags, sandbox, session_id, lease_worker, \
     lease_deadline, last_heartbeat, created_at, updated_at, error";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("hub database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("the hub store lock is poisoned")]
    Lock,
    #[error("invalid stored run state: {0}")]
    State(String),
    #[error("unknown run {0}")]
    UnknownRun(String),
    #[error("unknown workflow revision {path}@{hash}")]
    UnknownRevision { path: String, hash: String },
    #[error("workflow {0} has not been deployed")]
    NotDeployed(String),
    #[error("illegal run transition {from} -> {to}")]
    InvalidTransition { from: RunState, to: RunState },
    #[error("the run is leased to another worker")]
    LeaseMismatch,
    #[error("the run exhausted its {0} attempts")]
    AttemptsExhausted(u32),
    #[error("the revision belongs to workflow {found}, not {expected}")]
    WorkflowMismatch { expected: String, found: String },
    #[error("the workflow file is invalid: {0}")]
    Workflow(#[from] WorkflowError),
    #[error("the hub path is empty")]
    EmptyPath,
    #[error("the idempotency key is empty")]
    EmptyIdempotencyKey,
    #[error("the system random source failed")]
    Random,
    #[error("the audit chain is tampered at sequence {0}")]
    AuditTampered(i64),
    #[error("the audit signature is not valid")]
    AuditSignature,
    #[error("invalid stored value: {0}")]
    Encoding(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Queued,
    Leased,
    Running,
    AwaitingApproval,
    Succeeded,
    Failed,
    Cancelled,
}

impl RunState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Leased => "leased",
            Self::Running => "running",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Leased)
                | (Self::Queued, Self::Cancelled)
                | (Self::Leased, Self::Running)
                | (Self::Leased, Self::Queued)
                | (Self::Leased, Self::Failed)
                | (Self::Leased, Self::Cancelled)
                | (Self::Running, Self::AwaitingApproval)
                | (Self::Running, Self::Queued)
                | (Self::Running, Self::Succeeded)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::Cancelled)
                | (Self::AwaitingApproval, Self::Running)
                | (Self::AwaitingApproval, Self::Failed)
                | (Self::AwaitingApproval, Self::Cancelled)
        )
    }
}

impl std::fmt::Display for RunState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RunState {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "leased" => Ok(Self::Leased),
            "running" => Ok(Self::Running),
            "awaiting_approval" => Ok(Self::AwaitingApproval),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub id: String,
    pub workflow: String,
    pub revision_hash: String,
    pub idempotency_key: String,
    pub state: RunState,
    pub inputs: Value,
    pub attempts: u32,
    pub max_attempts: u32,
    pub target_tags: Vec<String>,
    pub sandbox: String,
    pub session_id: Option<String>,
    pub lease_worker: Option<String>,
    pub lease_deadline: Option<i64>,
    pub last_heartbeat: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRevision {
    pub path: String,
    pub hash: String,
    pub content: String,
    pub author: String,
    pub deployed_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunRequest {
    pub path: String,
    /// `None` deploys the latest revision of the path.
    pub revision_hash: Option<String>,
    pub idempotency_key: String,
    pub inputs: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    User,
    Device,
    Worker,
    System,
    Trigger,
}

impl ActorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Device => "device",
            Self::Worker => "worker",
            Self::System => "system",
            Self::Trigger => "trigger",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub kind: ActorKind,
    pub id: String,
}

impl Actor {
    pub fn new(kind: ActorKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
        }
    }

    pub fn user(id: impl Into<String>) -> Self {
        Self::new(ActorKind::User, id)
    }

    pub fn trigger(id: impl Into<String>) -> Self {
        Self::new(ActorKind::Trigger, id)
    }

    pub fn system(id: impl Into<String>) -> Self {
        Self::new(ActorKind::System, id)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuditEvent {
    pub seq: i64,
    pub at: i64,
    pub actor_kind: String,
    pub actor_id: String,
    pub run_id: Option<String>,
    pub kind: String,
    pub payload: Value,
    pub prev_hash: String,
    pub hash: String,
    pub signature: String,
}

pub struct AuditSigner {
    key: Ed25519KeyPair,
}

impl AuditSigner {
    pub fn generate() -> Result<Self, StoreError> {
        let pkcs8 =
            Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| StoreError::Random)?;
        let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).map_err(|_| StoreError::Random)?;
        Ok(Self { key })
    }

    pub fn from_seed(seed: [u8; 32]) -> Result<Self, StoreError> {
        let key = Ed25519KeyPair::from_seed_unchecked(&seed).map_err(|_| StoreError::Random)?;
        Ok(Self { key })
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.key
            .public_key()
            .as_ref()
            .try_into()
            .expect("Ed25519 public keys are 32 bytes")
    }

    fn sign(&self, message: &[u8]) -> String {
        hex_encode(self.key.sign(message).as_ref())
    }
}

/// Durable hub state. Every method takes `&self` and serializes through the
/// mutex, so a single process cannot interleave two run transitions.
pub struct HubStore {
    connection: Mutex<Connection>,
}

impl HubStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::from_connection(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self, StoreError> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS hub_revisions (
                path        TEXT NOT NULL,
                hash        TEXT NOT NULL,
                content     TEXT NOT NULL,
                author      TEXT NOT NULL,
                deployed_at INTEGER NOT NULL,
                PRIMARY KEY (path, hash)
            );
            CREATE TABLE IF NOT EXISTS hub_deployments (
                path        TEXT PRIMARY KEY,
                hash        TEXT NOT NULL,
                deployed_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS hub_runs (
                id              TEXT PRIMARY KEY,
                workflow        TEXT NOT NULL,
                revision_hash   TEXT NOT NULL,
                idempotency_key TEXT NOT NULL UNIQUE,
                state           TEXT NOT NULL,
                inputs          TEXT NOT NULL,
                attempts        INTEGER NOT NULL,
                max_attempts    INTEGER NOT NULL,
                target_tags     TEXT NOT NULL,
                sandbox         TEXT NOT NULL,
                session_id      TEXT,
                lease_worker    TEXT,
                lease_deadline  INTEGER,
                last_heartbeat  INTEGER,
                created_at      INTEGER NOT NULL,
                updated_at      INTEGER NOT NULL,
                error           TEXT
            );
            CREATE INDEX IF NOT EXISTS hub_runs_state
                ON hub_runs (state, created_at);
            CREATE TABLE IF NOT EXISTS hub_deliveries (
                source       TEXT NOT NULL,
                delivery_id  TEXT NOT NULL,
                received_at  INTEGER NOT NULL,
                PRIMARY KEY (source, delivery_id)
            );
            CREATE TABLE IF NOT EXISTS hub_audit (
                seq        INTEGER PRIMARY KEY AUTOINCREMENT,
                at         INTEGER NOT NULL,
                actor_kind TEXT NOT NULL,
                actor_id   TEXT NOT NULL,
                run_id     TEXT,
                kind       TEXT NOT NULL,
                payload    TEXT NOT NULL,
                prev_hash  TEXT NOT NULL,
                hash       TEXT NOT NULL,
                signature  TEXT NOT NULL
            );
            "#,
        )?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::Lock)
    }

    /// Validate a workflow file and make it the deployed revision for its path.
    /// The content hash is the revision identity; the exact bytes are kept so
    /// a later run executes what was deployed.
    pub fn deploy_revision(
        &self,
        path: &str,
        content: &str,
        author: &str,
        now: i64,
    ) -> Result<WorkflowRevision, StoreError> {
        if path.trim().is_empty() {
            return Err(StoreError::EmptyPath);
        }
        WorkflowFile::parse(content)?;
        let hash = content_hash(content);
        {
            let connection = self.connection()?;
            connection.execute(
                "INSERT INTO hub_revisions (path, hash, content, author, deployed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(path, hash) DO NOTHING",
                params![path, hash, content, author, now],
            )?;
            connection.execute(
                "INSERT INTO hub_deployments (path, hash, deployed_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(path) DO UPDATE SET hash = excluded.hash,
                                                deployed_at = excluded.deployed_at",
                params![path, hash, now],
            )?;
        }
        self.revision(path, &hash)?
            .ok_or_else(|| StoreError::UnknownRevision {
                path: path.to_string(),
                hash,
            })
    }

    pub fn revision(&self, path: &str, hash: &str) -> Result<Option<WorkflowRevision>, StoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT path, hash, content, author, deployed_at
                 FROM hub_revisions WHERE path = ?1 AND hash = ?2",
                params![path, hash],
                revision_from_row,
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn latest_revision(&self, path: &str) -> Result<Option<WorkflowRevision>, StoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT r.path, r.hash, r.content, r.author, r.deployed_at
                 FROM hub_deployments d
                 JOIN hub_revisions r ON r.path = d.path AND r.hash = d.hash
                 WHERE d.path = ?1",
                [path],
                revision_from_row,
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn list_revisions(&self, path: &str) -> Result<Vec<WorkflowRevision>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT path, hash, content, author, deployed_at
             FROM hub_revisions WHERE path = ?1 ORDER BY deployed_at DESC, hash",
        )?;
        let rows = statement.query_map([path], revision_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Create one run per idempotency key. A redelivered webhook returns the
    /// existing run and `false`.
    pub fn create_run(&self, request: RunRequest, now: i64) -> Result<(Run, bool), StoreError> {
        if request.idempotency_key.trim().is_empty() {
            return Err(StoreError::EmptyIdempotencyKey);
        }
        if let Some(run) = self.run_by_idempotency_key(&request.idempotency_key)? {
            return Ok((run, false));
        }
        let revision = match request.revision_hash.as_deref() {
            Some(hash) => {
                self.revision(&request.path, hash)?
                    .ok_or_else(|| StoreError::UnknownRevision {
                        path: request.path.clone(),
                        hash: hash.to_string(),
                    })?
            }
            None => self
                .latest_revision(&request.path)?
                .ok_or_else(|| StoreError::NotDeployed(request.path.clone()))?,
        };
        let workflow = WorkflowFile::parse(&revision.content)?;
        let sandbox = workflow.target.sandbox.as_str().to_string();
        let tags = serde_json::to_string(&workflow.target.tags)
            .map_err(|error| StoreError::Encoding(error.to_string()))?;
        let inputs = serde_json::to_string(&request.inputs)
            .map_err(|error| StoreError::Encoding(error.to_string()))?;
        let id = new_run_id()?;
        let connection = self.connection()?;
        let inserted = connection.execute(
            "INSERT INTO hub_runs
                (id, workflow, revision_hash, idempotency_key, state, inputs, attempts,
                 max_attempts, target_tags, sandbox, session_id, lease_worker, lease_deadline,
                 last_heartbeat, created_at, updated_at, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, ?9, NULL, NULL, NULL, NULL, ?10, ?10, NULL)
             ON CONFLICT(idempotency_key) DO NOTHING",
            params![
                id,
                workflow.workflow.name,
                revision.hash,
                request.idempotency_key,
                RunState::Queued.as_str(),
                inputs,
                workflow.target.max_attempts,
                tags,
                sandbox,
                now,
            ],
        )?;
        drop(connection);
        if inserted == 0 {
            let existing = self
                .run_by_idempotency_key(&request.idempotency_key)?
                .ok_or_else(|| StoreError::UnknownRun(request.idempotency_key.clone()))?;
            return Ok((existing, false));
        }
        let run = self
            .run(&id)?
            .ok_or_else(|| StoreError::UnknownRun(id.clone()))?;
        Ok((run, true))
    }

    pub fn run(&self, run_id: &str) -> Result<Option<Run>, StoreError> {
        let connection = self.connection()?;
        query_run(&connection, run_id).map_err(StoreError::from)
    }

    pub fn run_by_idempotency_key(&self, key: &str) -> Result<Option<Run>, StoreError> {
        let connection = self.connection()?;
        let sql = format!("SELECT {RUN_COLUMNS} FROM hub_runs WHERE idempotency_key = ?1");
        connection
            .query_row(&sql, [key], run_from_row)
            .optional()
            .map_err(StoreError::from)
    }

    pub fn runs_in_state(&self, state: RunState) -> Result<Vec<Run>, StoreError> {
        let connection = self.connection()?;
        let sql =
            format!("SELECT {RUN_COLUMNS} FROM hub_runs WHERE state = ?1 ORDER BY created_at, id");
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map([state.as_str()], run_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Lease a queued run. The attempt is counted here, so an expired lease
    /// that requeues cannot retry forever.
    pub fn lease(
        &self,
        run_id: &str,
        worker_id: &str,
        deadline: i64,
        now: i64,
    ) -> Result<Run, StoreError> {
        {
            let mut connection = self.connection()?;
            let transaction = connection.transaction()?;
            let run = query_run(&transaction, run_id)?
                .ok_or_else(|| StoreError::UnknownRun(run_id.to_string()))?;
            if run.state != RunState::Queued {
                return Err(StoreError::InvalidTransition {
                    from: run.state,
                    to: RunState::Leased,
                });
            }
            if run.attempts >= run.max_attempts {
                return Err(StoreError::AttemptsExhausted(run.max_attempts));
            }
            transaction.execute(
                "UPDATE hub_runs
                 SET state = 'leased', lease_worker = ?1, lease_deadline = ?2,
                     last_heartbeat = ?3, attempts = attempts + 1, updated_at = ?3,
                     error = NULL
                 WHERE id = ?4",
                params![worker_id, deadline, now, run_id],
            )?;
            transaction.commit()?;
        }
        self.run(run_id)?
            .ok_or_else(|| StoreError::UnknownRun(run_id.to_string()))
    }

    /// Extend a lease. The worker sends this every ten seconds; a missed
    /// deadline is what releases the run.
    pub fn heartbeat(
        &self,
        run_id: &str,
        worker_id: &str,
        deadline: i64,
        now: i64,
    ) -> Result<Run, StoreError> {
        {
            let connection = self.connection()?;
            let run = query_run(&connection, run_id)?
                .ok_or_else(|| StoreError::UnknownRun(run_id.to_string()))?;
            if !matches!(run.state, RunState::Leased | RunState::Running) {
                return Err(StoreError::InvalidTransition {
                    from: run.state,
                    to: run.state,
                });
            }
            if run.lease_worker.as_deref() != Some(worker_id) {
                return Err(StoreError::LeaseMismatch);
            }
            connection.execute(
                "UPDATE hub_runs SET lease_deadline = ?1, last_heartbeat = ?2, updated_at = ?2
                 WHERE id = ?3",
                params![deadline, now, run_id],
            )?;
        }
        self.run(run_id)?
            .ok_or_else(|| StoreError::UnknownRun(run_id.to_string()))
    }

    pub fn start_run(&self, run_id: &str, worker_id: &str, now: i64) -> Result<Run, StoreError> {
        self.transition_leased(run_id, worker_id, RunState::Running, now)
    }

    /// Mark a run as waiting for a human approval gate.
    pub fn await_approval(
        &self,
        run_id: &str,
        worker_id: &str,
        now: i64,
    ) -> Result<Run, StoreError> {
        self.transition_leased(run_id, worker_id, RunState::AwaitingApproval, now)
    }

    /// Resume a run after an approval. The worker keeps its lease.
    pub fn approve_run(&self, run_id: &str, now: i64) -> Result<Run, StoreError> {
        let connection = self.connection()?;
        let run = query_run(&connection, run_id)?
            .ok_or_else(|| StoreError::UnknownRun(run_id.to_string()))?;
        if run.state != RunState::AwaitingApproval {
            return Err(StoreError::InvalidTransition {
                from: run.state,
                to: RunState::Running,
            });
        }
        connection.execute(
            "UPDATE hub_runs SET state = 'running', updated_at = ?1, error = NULL WHERE id = ?2",
            params![now, run_id],
        )?;
        drop(connection);
        self.run(run_id)?
            .ok_or_else(|| StoreError::UnknownRun(run_id.to_string()))
    }

    pub fn reject_run(&self, run_id: &str, reason: &str, now: i64) -> Result<Run, StoreError> {
        self.finish_run(run_id, None, RunOutcome::Failed, Some(reason), now)
    }

    /// A worker reports success or failure. Only the leased worker may finish
    /// the run it holds.
    pub fn finish_run(
        &self,
        run_id: &str,
        worker_id: Option<&str>,
        outcome: RunOutcome,
        error: Option<&str>,
        now: i64,
    ) -> Result<Run, StoreError> {
        let next = match outcome {
            RunOutcome::Succeeded => RunState::Succeeded,
            RunOutcome::Failed => RunState::Failed,
        };
        let connection = self.connection()?;
        let run = query_run(&connection, run_id)?
            .ok_or_else(|| StoreError::UnknownRun(run_id.to_string()))?;
        if !run.state.can_transition_to(next) {
            return Err(StoreError::InvalidTransition {
                from: run.state,
                to: next,
            });
        }
        if let Some(worker_id) = worker_id {
            if run.lease_worker.as_deref() != Some(worker_id) {
                return Err(StoreError::LeaseMismatch);
            }
        }
        connection.execute(
            "UPDATE hub_runs
             SET state = ?1, error = ?2, lease_worker = NULL, lease_deadline = NULL,
                 last_heartbeat = NULL, updated_at = ?3
             WHERE id = ?4",
            params![next.as_str(), error, now, run_id],
        )?;
        drop(connection);
        self.run(run_id)?
            .ok_or_else(|| StoreError::UnknownRun(run_id.to_string()))
    }

    pub fn cancel_run(&self, run_id: &str, now: i64) -> Result<Run, StoreError> {
        let connection = self.connection()?;
        let run = query_run(&connection, run_id)?
            .ok_or_else(|| StoreError::UnknownRun(run_id.to_string()))?;
        if run.state.is_terminal() {
            return Err(StoreError::InvalidTransition {
                from: run.state,
                to: RunState::Cancelled,
            });
        }
        connection.execute(
            "UPDATE hub_runs
             SET state = 'cancelled', error = 'cancelled', lease_worker = NULL,
                 lease_deadline = NULL, last_heartbeat = NULL, updated_at = ?1
             WHERE id = ?2",
            params![now, run_id],
        )?;
        drop(connection);
        self.run(run_id)?
            .ok_or_else(|| StoreError::UnknownRun(run_id.to_string()))
    }

    pub fn link_session(
        &self,
        run_id: &str,
        session_id: &str,
        now: i64,
    ) -> Result<Run, StoreError> {
        {
            let connection = self.connection()?;
            let run = query_run(&connection, run_id)?
                .ok_or_else(|| StoreError::UnknownRun(run_id.to_string()))?;
            if run.state.is_terminal() {
                return Err(StoreError::InvalidTransition {
                    from: run.state,
                    to: run.state,
                });
            }
            connection.execute(
                "UPDATE hub_runs SET session_id = ?1, updated_at = ?2 WHERE id = ?3",
                params![session_id, now, run_id],
            )?;
        }
        self.run(run_id)?
            .ok_or_else(|| StoreError::UnknownRun(run_id.to_string()))
    }

    /// Requeue or fail runs whose lease deadline passed. An exhausted run is
    /// failed with its last error rather than disappearing.
    pub fn release_expired_leases(&self, now: i64) -> Result<Vec<Run>, StoreError> {
        let expired: Vec<(String, RunState, u32, u32)> = {
            let connection = self.connection()?;
            let mut statement = connection.prepare(
                "SELECT id, state, attempts, max_attempts FROM hub_runs
                 WHERE state IN ('leased', 'running')
                   AND lease_deadline IS NOT NULL
                   AND lease_deadline < ?1
                 ORDER BY lease_deadline",
            )?;
            let rows = statement.query_map([now], |row| {
                let state: String = row.get(1)?;
                Ok((
                    row.get(0)?,
                    state,
                    row.get::<_, u32>(2)?,
                    row.get::<_, u32>(3)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .map(|(id, state, attempts, max_attempts)| {
                    let state = state
                        .parse::<RunState>()
                        .map_err(|error| StoreError::State(format!("{error} for run {id}")))?;
                    Ok((id, state, attempts, max_attempts))
                })
                .collect::<Result<Vec<_>, StoreError>>()?
        };
        let mut released = Vec::with_capacity(expired.len());
        {
            let connection = self.connection()?;
            for (run_id, state, attempts, max_attempts) in &expired {
                let exhausted = *attempts >= *max_attempts;
                let next = if exhausted {
                    RunState::Failed
                } else {
                    RunState::Queued
                };
                if !state.can_transition_to(next) {
                    return Err(StoreError::InvalidTransition {
                        from: *state,
                        to: next,
                    });
                }
                let error = if exhausted {
                    Some(format!("lease expired after {} attempt(s)", attempts))
                } else {
                    None
                };
                connection.execute(
                    "UPDATE hub_runs
                     SET state = ?1, lease_worker = NULL, lease_deadline = NULL,
                         last_heartbeat = NULL, updated_at = ?2, error = ?3
                     WHERE id = ?4",
                    params![next.as_str(), now, error, run_id],
                )?;
                released.push(run_id.clone());
            }
        }
        let mut runs = Vec::with_capacity(released.len());
        for run_id in released {
            if let Some(run) = self.run(&run_id)? {
                runs.push(run);
            }
        }
        Ok(runs)
    }

    /// Record a webhook delivery. Returns `false` for a replay inside the
    /// deduplication window.
    pub fn accept_delivery(
        &self,
        source: &str,
        delivery_id: &str,
        now: i64,
        window_secs: i64,
    ) -> Result<bool, StoreError> {
        let connection = self.connection()?;
        connection.execute(
            "DELETE FROM hub_deliveries WHERE received_at < ?1",
            [now.saturating_sub(window_secs)],
        )?;
        let inserted = connection.execute(
            "INSERT OR IGNORE INTO hub_deliveries (source, delivery_id, received_at)
             VALUES (?1, ?2, ?3)",
            params![source, delivery_id, now],
        )?;
        Ok(inserted > 0)
    }

    /// Append one signed audit event. The previous hash makes the log a chain;
    /// the sequence is assigned by SQLite under the store mutex.
    pub fn append_audit(
        &self,
        signer: &AuditSigner,
        actor: &Actor,
        run_id: Option<&str>,
        kind: &str,
        payload: Value,
        now: i64,
    ) -> Result<AuditEvent, StoreError> {
        let payload_json = serde_json::to_string(&payload)
            .map_err(|error| StoreError::Encoding(error.to_string()))?;
        let connection = self.connection()?;
        let previous: Option<String> = connection
            .query_row(
                "SELECT hash FROM hub_audit ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let prev_hash = previous.unwrap_or_else(zero_hash);
        let hash = audit_hash(
            &prev_hash,
            now,
            actor.kind.as_str(),
            &actor.id,
            run_id,
            kind,
            &payload_json,
        );
        let signature = signer.sign(hash.as_bytes());
        connection.execute(
            "INSERT INTO hub_audit
                (at, actor_kind, actor_id, run_id, kind, payload, prev_hash, hash, signature)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                now,
                actor.kind.as_str(),
                actor.id,
                run_id,
                kind,
                payload_json,
                prev_hash,
                hash,
                signature
            ],
        )?;
        let seq = connection.last_insert_rowid();
        Ok(AuditEvent {
            seq,
            at: now,
            actor_kind: actor.kind.as_str().to_string(),
            actor_id: actor.id.clone(),
            run_id: run_id.map(str::to_string),
            kind: kind.to_string(),
            payload,
            prev_hash,
            hash,
            signature,
        })
    }

    pub fn audit_events(&self) -> Result<Vec<AuditEvent>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT seq, at, actor_kind, actor_id, run_id, kind, payload, prev_hash, hash, signature
             FROM hub_audit ORDER BY seq",
        )?;
        let rows = statement.query_map([], audit_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Verify every link and signature. A single edited row, deleted row or
    /// reordered pair fails here.
    pub fn verify_audit_chain(&self, public_key: &[u8; 32]) -> Result<(), StoreError> {
        let events = self.audit_events()?;
        let mut previous = zero_hash();
        let mut previous_seq = 0;
        for event in events {
            if event.seq != previous_seq + 1 || event.prev_hash != previous {
                return Err(StoreError::AuditTampered(event.seq));
            }
            let payload_json = serde_json::to_string(&event.payload)
                .map_err(|error| StoreError::Encoding(error.to_string()))?;
            let recomputed = audit_hash(
                &event.prev_hash,
                event.at,
                &event.actor_kind,
                &event.actor_id,
                event.run_id.as_deref(),
                &event.kind,
                &payload_json,
            );
            if recomputed != event.hash {
                return Err(StoreError::AuditTampered(event.seq));
            }
            let signature = hex_decode(&event.signature).ok_or(StoreError::AuditSignature)?;
            UnparsedPublicKey::new(&ED25519, public_key)
                .verify(event.hash.as_bytes(), &signature)
                .map_err(|_| StoreError::AuditSignature)?;
            previous = event.hash;
            previous_seq = event.seq;
        }
        Ok(())
    }

    /// Merkle root over the audit hashes. A daily root is what the hub signs
    /// and publishes; the chain itself is checked by `verify_audit_chain`.
    pub fn audit_root(&self) -> Result<Option<String>, StoreError> {
        let mut hashes = Vec::new();
        for event in self.audit_events()? {
            let bytes =
                hex_decode(&event.hash).ok_or_else(|| StoreError::Encoding("audit hash".into()))?;
            let hash: [u8; 32] = bytes
                .try_into()
                .map_err(|_| StoreError::Encoding("audit hash length".into()))?;
            hashes.push(hash);
        }
        Ok(merkle_root(&hashes).map(|root| hex_encode(&root)))
    }

    fn transition_leased(
        &self,
        run_id: &str,
        worker_id: &str,
        next: RunState,
        now: i64,
    ) -> Result<Run, StoreError> {
        {
            let connection = self.connection()?;
            let run = query_run(&connection, run_id)?
                .ok_or_else(|| StoreError::UnknownRun(run_id.to_string()))?;
            if !run.state.can_transition_to(next) {
                return Err(StoreError::InvalidTransition {
                    from: run.state,
                    to: next,
                });
            }
            if run.lease_worker.as_deref() != Some(worker_id) {
                return Err(StoreError::LeaseMismatch);
            }
            connection.execute(
                "UPDATE hub_runs SET state = ?1, updated_at = ?2 WHERE id = ?3",
                params![next.as_str(), now, run_id],
            )?;
        }
        self.run(run_id)?
            .ok_or_else(|| StoreError::UnknownRun(run_id.to_string()))
    }
}

fn query_run(connection: &Connection, run_id: &str) -> rusqlite::Result<Option<Run>> {
    let sql = format!("SELECT {RUN_COLUMNS} FROM hub_runs WHERE id = ?1");
    connection
        .query_row(&sql, [run_id], run_from_row)
        .optional()
}

fn run_from_row(row: &Row<'_>) -> rusqlite::Result<Run> {
    let state_value: String = row.get(4)?;
    let state = state_value.parse::<RunState>().map_err(|_| {
        rusqlite::Error::InvalidColumnType(4, state_value.clone(), rusqlite::types::Type::Text)
    })?;
    let inputs_value: String = row.get(5)?;
    let tags_value: String = row.get(8)?;
    Ok(Run {
        id: row.get(0)?,
        workflow: row.get(1)?,
        revision_hash: row.get(2)?,
        idempotency_key: row.get(3)?,
        state,
        inputs: serde_json::from_str(&inputs_value).unwrap_or(Value::Null),
        attempts: row.get(6)?,
        max_attempts: row.get(7)?,
        target_tags: serde_json::from_str(&tags_value).unwrap_or_default(),
        sandbox: row.get(9)?,
        session_id: row.get(10)?,
        lease_worker: row.get(11)?,
        lease_deadline: row.get(12)?,
        last_heartbeat: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
        error: row.get(16)?,
    })
}

fn revision_from_row(row: &Row<'_>) -> rusqlite::Result<WorkflowRevision> {
    Ok(WorkflowRevision {
        path: row.get(0)?,
        hash: row.get(1)?,
        content: row.get(2)?,
        author: row.get(3)?,
        deployed_at: row.get(4)?,
    })
}

fn audit_from_row(row: &Row<'_>) -> rusqlite::Result<AuditEvent> {
    let payload: String = row.get(6)?;
    Ok(AuditEvent {
        seq: row.get(0)?,
        at: row.get(1)?,
        actor_kind: row.get(2)?,
        actor_id: row.get(3)?,
        run_id: row.get(4)?,
        kind: row.get(5)?,
        payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
        prev_hash: row.get(7)?,
        hash: row.get(8)?,
        signature: row.get(9)?,
    })
}

fn new_run_id() -> Result<String, StoreError> {
    let mut bytes = [0u8; 12];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| StoreError::Random)?;
    Ok(format!("run_{}", hex_encode(&bytes)))
}

fn audit_hash(
    prev_hash: &str,
    at: i64,
    actor_kind: &str,
    actor_id: &str,
    run_id: Option<&str>,
    kind: &str,
    payload: &str,
) -> String {
    let at_text = at.to_string();
    let mut hasher = Sha256::new();
    hasher.update(b"goble-hub-audit-v1");
    for field in [
        prev_hash.as_bytes(),
        at_text.as_bytes(),
        actor_kind.as_bytes(),
        actor_id.as_bytes(),
        run_id.unwrap_or("").as_bytes(),
        kind.as_bytes(),
        payload.as_bytes(),
    ] {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field);
    }
    hex_encode(&hasher.finalize())
}

fn merkle_root(leaves: &[[u8; 32]]) -> Option<[u8; 32]> {
    if leaves.is_empty() {
        return None;
    }
    let mut level = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for pair in level.chunks(2) {
            let left = pair[0];
            let right = pair.get(1).copied().unwrap_or(left);
            let mut hasher = Sha256::new();
            hasher.update([1u8]);
            hasher.update(left);
            hasher.update(right);
            next.push(hasher.finalize().into());
        }
        level = next;
    }
    Some(level[0])
}

fn zero_hash() -> String {
    "0".repeat(64)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_decode(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const WORKFLOW: &str = r#"
[workflow]
name = "triage-issue"
on = "github.issues.opened"

[target]
tags = ["builder", "linux"]
sandbox = "modal"
timeout_secs = 1800
max_attempts = 2

[steps.triage]
prompt = "Read the issue."
grants = "ReadOnly"
"#;

    fn store() -> HubStore {
        HubStore::in_memory().unwrap()
    }

    fn deploy(store: &HubStore) -> WorkflowRevision {
        store
            .deploy_revision(".goble/workflows/triage.toml", WORKFLOW, "owner", 100)
            .unwrap()
    }

    fn request(key: &str) -> RunRequest {
        RunRequest {
            path: ".goble/workflows/triage.toml".into(),
            revision_hash: None,
            idempotency_key: key.into(),
            inputs: serde_json::json!({"issue": 12}),
        }
    }

    #[test]
    fn a_deployed_revision_is_retrievable_by_hash_and_as_latest() {
        let store = store();
        let revision = deploy(&store);
        assert_eq!(revision.hash, content_hash(WORKFLOW));
        assert_eq!(revision.author, "owner");
        assert_eq!(
            store.latest_revision(&revision.path).unwrap(),
            Some(revision.clone())
        );
        assert_eq!(
            store.revision(&revision.path, &revision.hash).unwrap(),
            Some(revision)
        );
    }

    #[test]
    fn an_invalid_workflow_is_not_stored() {
        let store = store();
        assert!(store
            .deploy_revision("bad.toml", "not a workflow", "owner", 100)
            .is_err());
        assert!(store.latest_revision("bad.toml").unwrap().is_none());
    }

    #[test]
    fn a_changed_file_becomes_a_new_revision_and_the_old_one_stays() {
        let store = store();
        let first = deploy(&store);
        let changed = WORKFLOW.replace("Read the issue.", "Read the issue carefully.");
        let second = store
            .deploy_revision(&first.path, &changed, "owner", 200)
            .unwrap();
        assert_ne!(first.hash, second.hash);
        assert_eq!(
            store.latest_revision(&first.path).unwrap().unwrap().hash,
            second.hash
        );
        assert_eq!(store.list_revisions(&first.path).unwrap().len(), 2);
        assert!(store.revision(&first.path, &first.hash).unwrap().is_some());
    }

    #[test]
    fn a_run_is_created_from_the_latest_revision() {
        let store = store();
        let revision = deploy(&store);
        let (run, created) = store.create_run(request("delivery-1"), 300).unwrap();
        assert!(created);
        assert_eq!(run.workflow, "triage-issue");
        assert_eq!(run.revision_hash, revision.hash);
        assert_eq!(run.state, RunState::Queued);
        assert_eq!(run.attempts, 0);
        assert_eq!(run.max_attempts, 2);
        assert_eq!(run.target_tags, vec!["builder", "linux"]);
        assert_eq!(run.sandbox, "modal");
        assert_eq!(run.inputs["issue"], 12);
    }

    #[test]
    fn an_idempotency_key_returns_the_same_run() {
        let store = store();
        deploy(&store);
        let (first, created) = store.create_run(request("delivery-1"), 300).unwrap();
        assert!(created);
        let (second, created) = store.create_run(request("delivery-1"), 301).unwrap();
        assert!(!created);
        assert_eq!(first.id, second.id);
        assert_eq!(store.runs_in_state(RunState::Queued).unwrap().len(), 1);
    }

    #[test]
    fn a_run_needs_a_deployed_revision() {
        let store = store();
        assert!(matches!(
            store.create_run(request("delivery-1"), 300),
            Err(StoreError::NotDeployed(_))
        ));
        assert!(matches!(
            store.create_run(
                RunRequest {
                    revision_hash: Some("missing".into()),
                    ..request("delivery-2")
                },
                300
            ),
            Err(StoreError::UnknownRevision { .. })
        ));
    }

    #[test]
    fn an_empty_idempotency_key_is_refused() {
        let store = store();
        deploy(&store);
        assert!(matches!(
            store.create_run(request("  "), 300),
            Err(StoreError::EmptyIdempotencyKey)
        ));
    }

    #[test]
    fn lease_start_and_success_follow_the_state_machine() {
        let store = store();
        deploy(&store);
        let (run, _) = store.create_run(request("delivery-1"), 300).unwrap();
        let leased = store.lease(&run.id, "builder-1", 330, 301).unwrap();
        assert_eq!(leased.state, RunState::Leased);
        assert_eq!(leased.attempts, 1);
        assert_eq!(leased.lease_worker.as_deref(), Some("builder-1"));
        let running = store.start_run(&run.id, "builder-1", 302).unwrap();
        assert_eq!(running.state, RunState::Running);
        let done = store
            .finish_run(&run.id, Some("builder-1"), RunOutcome::Succeeded, None, 303)
            .unwrap();
        assert_eq!(done.state, RunState::Succeeded);
        assert!(done.lease_worker.is_none());
        assert!(done.error.is_none());
    }

    #[test]
    fn a_run_cannot_be_leased_twice() {
        let store = store();
        deploy(&store);
        let (run, _) = store.create_run(request("delivery-1"), 300).unwrap();
        store.lease(&run.id, "builder-1", 330, 301).unwrap();
        assert!(matches!(
            store.lease(&run.id, "builder-2", 340, 302),
            Err(StoreError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn a_finished_run_cannot_transition_again() {
        let store = store();
        deploy(&store);
        let (run, _) = store.create_run(request("delivery-1"), 300).unwrap();
        store.lease(&run.id, "builder-1", 330, 301).unwrap();
        store.start_run(&run.id, "builder-1", 302).unwrap();
        store
            .finish_run(&run.id, Some("builder-1"), RunOutcome::Succeeded, None, 303)
            .unwrap();
        assert!(matches!(
            store.cancel_run(&run.id, 304),
            Err(StoreError::InvalidTransition { .. })
        ));
        assert!(matches!(
            store.finish_run(&run.id, Some("builder-1"), RunOutcome::Failed, None, 305),
            Err(StoreError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn only_the_leased_worker_can_start_heartbeat_or_finish() {
        let store = store();
        deploy(&store);
        let (run, _) = store.create_run(request("delivery-1"), 300).unwrap();
        store.lease(&run.id, "builder-1", 330, 301).unwrap();
        assert!(matches!(
            store.start_run(&run.id, "builder-2", 302),
            Err(StoreError::LeaseMismatch)
        ));
        assert!(matches!(
            store.heartbeat(&run.id, "builder-2", 360, 302),
            Err(StoreError::LeaseMismatch)
        ));
        store.start_run(&run.id, "builder-1", 302).unwrap();
        assert!(matches!(
            store.finish_run(&run.id, Some("builder-2"), RunOutcome::Succeeded, None, 303),
            Err(StoreError::LeaseMismatch)
        ));
        let beat = store.heartbeat(&run.id, "builder-1", 360, 303).unwrap();
        assert_eq!(beat.lease_deadline, Some(360));
        assert_eq!(beat.last_heartbeat, Some(303));
    }

    #[test]
    fn an_expired_lease_requeues_until_the_attempt_budget_is_used() {
        let store = store();
        deploy(&store);
        let (run, _) = store.create_run(request("delivery-1"), 300).unwrap();
        store.lease(&run.id, "builder-1", 310, 301).unwrap();
        let released = store.release_expired_leases(311).unwrap();
        assert_eq!(released.len(), 1);
        assert_eq!(released[0].state, RunState::Queued);
        assert_eq!(released[0].attempts, 1);
        assert!(released[0].lease_worker.is_none());

        store.lease(&run.id, "builder-1", 320, 312).unwrap();
        let released = store.release_expired_leases(321).unwrap();
        assert_eq!(released[0].state, RunState::Failed);
        assert_eq!(released[0].attempts, 2);
        assert!(released[0]
            .error
            .as_deref()
            .unwrap()
            .contains("lease expired"));
    }

    #[test]
    fn an_exhausted_attempt_budget_refuses_a_new_lease() {
        let store = store();
        deploy(&store);
        let (run, _) = store.create_run(request("delivery-1"), 300).unwrap();
        store.lease(&run.id, "builder-1", 310, 301).unwrap();
        store.release_expired_leases(311).unwrap();
        store.lease(&run.id, "builder-1", 320, 312).unwrap();
        store.release_expired_leases(321).unwrap();
        assert!(matches!(
            store.lease(&run.id, "builder-1", 330, 322),
            Err(StoreError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn a_lease_that_has_not_expired_is_left_alone() {
        let store = store();
        deploy(&store);
        let (run, _) = store.create_run(request("delivery-1"), 300).unwrap();
        store.lease(&run.id, "builder-1", 400, 301).unwrap();
        assert!(store.release_expired_leases(399).unwrap().is_empty());
        assert_eq!(store.run(&run.id).unwrap().unwrap().state, RunState::Leased);
    }

    #[test]
    fn an_approval_gate_pauses_and_resumes_the_run() {
        let store = store();
        deploy(&store);
        let (run, _) = store.create_run(request("delivery-1"), 300).unwrap();
        store.lease(&run.id, "builder-1", 330, 301).unwrap();
        store.start_run(&run.id, "builder-1", 302).unwrap();
        let waiting = store.await_approval(&run.id, "builder-1", 303).unwrap();
        assert_eq!(waiting.state, RunState::AwaitingApproval);
        let resumed = store.approve_run(&run.id, 304).unwrap();
        assert_eq!(resumed.state, RunState::Running);
        store
            .finish_run(&run.id, Some("builder-1"), RunOutcome::Succeeded, None, 305)
            .unwrap();
    }

    #[test]
    fn a_rejected_approval_fails_the_run() {
        let store = store();
        deploy(&store);
        let (run, _) = store.create_run(request("delivery-1"), 300).unwrap();
        store.lease(&run.id, "builder-1", 330, 301).unwrap();
        store.start_run(&run.id, "builder-1", 302).unwrap();
        store.await_approval(&run.id, "builder-1", 303).unwrap();
        let failed = store.reject_run(&run.id, "not approved", 304).unwrap();
        assert_eq!(failed.state, RunState::Failed);
        assert_eq!(failed.error.as_deref(), Some("not approved"));
    }

    #[test]
    fn cancel_works_from_queued_and_clears_the_lease() {
        let store = store();
        deploy(&store);
        let (run, _) = store.create_run(request("delivery-1"), 300).unwrap();
        let cancelled = store.cancel_run(&run.id, 301).unwrap();
        assert_eq!(cancelled.state, RunState::Cancelled);
        assert!(cancelled.lease_worker.is_none());

        let (second, _) = store.create_run(request("delivery-2"), 302).unwrap();
        store.lease(&second.id, "builder-1", 330, 303).unwrap();
        let cancelled = store.cancel_run(&second.id, 304).unwrap();
        assert_eq!(cancelled.state, RunState::Cancelled);
        assert!(cancelled.lease_worker.is_none());
    }

    #[test]
    fn a_session_is_linked_to_the_run() {
        let store = store();
        deploy(&store);
        let (run, _) = store.create_run(request("delivery-1"), 300).unwrap();
        let linked = store.link_session(&run.id, "session-42", 301).unwrap();
        assert_eq!(linked.session_id.as_deref(), Some("session-42"));
    }

    #[test]
    fn a_delivery_is_deduplicated_inside_the_window() {
        let store = store();
        assert!(store
            .accept_delivery("github", "delivery-1", 100, 86_400)
            .unwrap());
        assert!(!store
            .accept_delivery("github", "delivery-1", 101, 86_400)
            .unwrap());
        assert!(store
            .accept_delivery("github", "delivery-2", 102, 86_400)
            .unwrap());
        assert!(store
            .accept_delivery("linear", "delivery-1", 103, 86_400)
            .unwrap());
        assert!(!store
            .accept_delivery("linear", "delivery-1", 104, 86_400)
            .unwrap());
    }

    #[test]
    fn an_old_delivery_is_forgotten_after_the_window() {
        let store = store();
        store
            .accept_delivery("github", "delivery-1", 100, 50)
            .unwrap();
        assert!(store
            .accept_delivery("github", "delivery-1", 200, 50)
            .unwrap());
    }

    #[test]
    fn a_restart_keeps_revisions_runs_and_audit() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("hub.db");
        let signer = AuditSigner::generate().unwrap();
        let (run_id, revision_hash) = {
            let store = HubStore::open(&path).unwrap();
            let revision = deploy(&store);
            let (run, _) = store.create_run(request("delivery-1"), 300).unwrap();
            store
                .append_audit(
                    &signer,
                    &Actor::trigger("github"),
                    Some(&run.id),
                    "run.created",
                    serde_json::json!({"workflow": "triage-issue"}),
                    300,
                )
                .unwrap();
            (run.id, revision.hash)
        };
        let store = HubStore::open(&path).unwrap();
        assert_eq!(
            store
                .latest_revision(".goble/workflows/triage.toml")
                .unwrap()
                .unwrap()
                .hash,
            revision_hash
        );
        assert_eq!(store.run(&run_id).unwrap().unwrap().state, RunState::Queued);
        store.verify_audit_chain(&signer.public_key()).unwrap();
    }

    #[test]
    fn the_audit_chain_verifies_and_links_hashes() {
        let store = store();
        let signer = AuditSigner::generate().unwrap();
        let first = store
            .append_audit(
                &signer,
                &Actor::user("owner"),
                None,
                "workflow.deployed",
                serde_json::json!({"path": "triage.toml"}),
                100,
            )
            .unwrap();
        let second = store
            .append_audit(
                &signer,
                &Actor::trigger("github"),
                Some("run_1"),
                "run.created",
                serde_json::json!({"issue": 1}),
                101,
            )
            .unwrap();
        assert_eq!(first.prev_hash, zero_hash());
        assert_eq!(second.prev_hash, first.hash);
        assert_eq!(second.seq, first.seq + 1);
        store.verify_audit_chain(&signer.public_key()).unwrap();
    }

    #[test]
    fn a_tampered_audit_payload_is_detected() {
        let store = store();
        let signer = AuditSigner::generate().unwrap();
        store
            .append_audit(
                &signer,
                &Actor::user("owner"),
                None,
                "workflow.deployed",
                serde_json::json!({"path": "triage.toml"}),
                100,
            )
            .unwrap();
        {
            let connection = store.connection().unwrap();
            connection
                .execute(
                    "UPDATE hub_audit SET payload = ?1 WHERE seq = 1",
                    [r#"{"path":"evil.toml"}"#],
                )
                .unwrap();
        }
        assert!(matches!(
            store.verify_audit_chain(&signer.public_key()),
            Err(StoreError::AuditTampered(1))
        ));
    }

    #[test]
    fn a_deleted_audit_row_breaks_the_chain() {
        let store = store();
        let signer = AuditSigner::generate().unwrap();
        for index in 0..3 {
            store
                .append_audit(
                    &signer,
                    &Actor::system("test"),
                    None,
                    "tick",
                    serde_json::json!({"index": index}),
                    100 + index,
                )
                .unwrap();
        }
        {
            let connection = store.connection().unwrap();
            connection
                .execute("DELETE FROM hub_audit WHERE seq = 2", [])
                .unwrap();
        }
        assert!(matches!(
            store.verify_audit_chain(&signer.public_key()),
            Err(StoreError::AuditTampered(3))
        ));
    }

    #[test]
    fn a_wrong_audit_key_is_detected() {
        let store = store();
        let signer = AuditSigner::generate().unwrap();
        store
            .append_audit(
                &signer,
                &Actor::system("test"),
                None,
                "tick",
                serde_json::json!({}),
                100,
            )
            .unwrap();
        let other = AuditSigner::generate().unwrap();
        assert!(matches!(
            store.verify_audit_chain(&other.public_key()),
            Err(StoreError::AuditSignature)
        ));
    }

    #[test]
    fn the_audit_root_is_stable_and_covers_every_row() {
        let store = store();
        assert!(store.audit_root().unwrap().is_none());
        let signer = AuditSigner::from_seed([7u8; 32]).unwrap();
        for index in 0..3 {
            store
                .append_audit(
                    &signer,
                    &Actor::system("test"),
                    None,
                    "tick",
                    serde_json::json!({"index": index}),
                    100 + index,
                )
                .unwrap();
        }
        let root = store.audit_root().unwrap().unwrap();
        assert_eq!(root.len(), 64);
        assert_eq!(store.audit_root().unwrap().unwrap(), root);
        store
            .append_audit(
                &signer,
                &Actor::system("test"),
                None,
                "tick",
                serde_json::json!({"index": 3}),
                104,
            )
            .unwrap();
        assert_ne!(store.audit_root().unwrap().unwrap(), root);
    }

    #[test]
    fn merkle_root_promotes_an_odd_leaf() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        let c = [3u8; 32];
        assert_eq!(merkle_root(&[]), None);
        assert_eq!(merkle_root(&[a]), Some(a));
        assert_ne!(merkle_root(&[a, b]), merkle_root(&[a, b, c]));
        assert_eq!(merkle_root(&[a, b, c]), merkle_root(&[a, b, c]));
    }

    #[test]
    fn run_state_transitions_are_explicit() {
        assert!(RunState::Queued.can_transition_to(RunState::Leased));
        assert!(!RunState::Queued.can_transition_to(RunState::Succeeded));
        assert!(RunState::Running.can_transition_to(RunState::AwaitingApproval));
        assert!(RunState::AwaitingApproval.can_transition_to(RunState::Running));
        assert!(RunState::Failed.is_terminal());
        assert!(!RunState::Succeeded.can_transition_to(RunState::Queued));
    }
}
