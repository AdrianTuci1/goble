//! The remote DSH runtime: the DeepSeek Harness headless profile, driven by the
//! worker **inside** its sandbox.
//!
//! A Goble worker normally answers a conversation turn from its own embedded
//! daemon/harness. An image built with the DSH runtime (`deploy/worker`
//! `RUNTIME=dsh`, `GOBLE_HARNESS=dsh`) instead shells out to
//! `dsh --profile headless --patch <harness.yml> --json ... -`, feeds the task
//! on stdin and relays the newline-delimited JSON events over the **existing**
//! worker protocol frames, so the gateway and the web/mobile clients need no
//! change:
//!
//! | DSH `--json` event | worker `WorkerMessage` |
//! |---|---|
//! | `session` | (captured: the native id a later turn resumes) |
//! | `text` | [`WorkerMessage::AssistantDelta`] |
//! | `thinking` | [`WorkerMessage::AgentLog`] (debug; no reasoning frame exists) |
//! | `tool_call` | [`WorkerMessage::ToolCallStarted`] |
//! | `tool_result` `completed` | [`WorkerMessage::ToolCallFinished`] |
//! | `tool_result` `error` | [`WorkerMessage::ToolCallError`] |
//! | `status` `step_end` with `usage` | [`WorkerMessage::AgentLog`] (info) |
//! | `status` `turn_end` | [`WorkerMessage::Done`] |
//! | `final` | the turn's reply text |
//! | `error` | [`WorkerMessage::AgentLog`] (error) |
//!
//! The runner owns the process lifetime: it spawns the harness in its own
//! process group (so a cancel kills the tools it started, not just the node
//! process), enforces a timeout, and resumes a conversation with `--session-id`
//! when the worker remembers the native session for the thread.
//!
//! Credentials are installed for the duration of a run and removed afterwards
//! (and before a worker-triggered snapshot): `$DSH_HOME/.credentials.yaml` is
//! never part of the image and never of a snapshot. See
//! `docs/remote-dsh-runtime.md`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use goble_core::execution::{ExecutionStatus, ExecutionTrace, LogLevel};
use goble_core::harness::remote::{DSH as HARNESS_DSH, HARNESS_ENV, NATIVE_SESSION_ENV};
use goble_core::protocol::{DesktopMessage, WorkerMessage};
use parking_lot::Mutex;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::watch;

use crate::state::AppState;

/// The `dsh` binary. Overridable so a test (or a custom image) can point at a
/// wrapper script.
pub const DSH_BIN_ENV: &str = "GOBLE_DSH_BIN";
/// The composition file installed by the image, passed as `--patch`.
pub const DSH_HARNESS_PATH_ENV: &str = "GOBLE_DSH_HARNESS";
/// The harness home (`DSH_HOME`): credentials and JSONL sessions live under it.
pub const DSH_HOME_ENV: &str = "GOBLE_DSH_HOME";
/// The workspace the harness runs in. Falls back to the session's workspace.
pub const DSH_WORKSPACE_ENV: &str = "GOBLE_DSH_WORKSPACE";
/// The default model for this worker. Falls back to the image composition's
/// model when unset.
pub const DSH_MODEL_ENV: &str = "GOBLE_DSH_MODEL";
/// The per-run budget in seconds.
pub const DSH_TIMEOUT_ENV: &str = "GOBLE_DSH_TIMEOUT_SECONDS";
/// A file holding the DeepSeek account credentials (`dsh login` store). Read
/// once per run; the file itself is not the worker's to delete.
pub const DSH_CREDENTIALS_FILE_ENV: &str = "DSH_CREDENTIALS_FILE";
/// Inline DeepSeek account credentials, as the dsh-remote gateway passes them
/// to its sandbox. The worker writes them for the run and deletes them after.
pub const DSH_CREDENTIALS_YAML_ENV: &str = "DSH_CREDENTIALS_YAML";
/// The worker secret (vault name) that holds the DeepSeek account credentials.
pub const DSH_CREDENTIALS_SECRET_ENV: &str = "GOBLE_DSH_CREDENTIALS_SECRET";
/// Setting prefix under which the native session id is remembered per thread.
pub const DSH_SESSION_SETTING_PREFIX: &str = "dsh_session:";

/// Default worker secret name for the DeepSeek account credentials.
pub const DEFAULT_CREDENTIALS_SECRET: &str = "dsh_credentials";
/// Default harness home, matching the `goble-worker-dsh` image.
pub const DEFAULT_DSH_HOME: &str = "/home/goble/.dsh";
/// Default composition path, matching the `goble-worker-dsh` image.
pub const DEFAULT_HARNESS_PATH: &str = "/opt/dsh/harness.yml";
/// A turn with no answer after this long is killed.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Name of the generated overlay that pins the session store and model.
pub const RUNTIME_PATCH_FILE: &str = "goble-runtime.yml";
/// The JSONL session directory (relative to the harness home).
pub const SESSIONS_DIR: &str = "sessions";

/// Whether this worker was provisioned to drive the remote DSH runtime instead
/// of the native harness. An image that sets `GOBLE_HARNESS=dsh` selects it; a
/// plain worker image leaves the variable unset and keeps the native runtime.
pub fn dsh_runtime_selected() -> bool {
    matches!(
        std::env::var(HARNESS_ENV)
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some(HARNESS_DSH)
    )
}

/// Configured DSH runtime.
#[derive(Debug, Clone, PartialEq)]
pub struct DshConfig {
    /// The program to spawn (`dsh` in the image, a fixture in tests).
    pub bin: String,
    /// The composition passed as `--patch`.
    pub harness_path: PathBuf,
    /// `DSH_HOME`: credentials, JSONL sessions and the generated overlay.
    pub home: PathBuf,
    /// The model override, applied through a generated overlay.
    pub model: Option<String>,
    /// The working directory of the harness run.
    pub workspace: PathBuf,
    /// Per-run budget; the process group is killed when it expires.
    pub timeout: Duration,
    /// Credentials content for this run. When set by this worker (rather than
    /// read from a file the caller manages) the credentials file is removed
    /// after the run.
    pub credentials_yaml: Option<String>,
    /// Keep the credentials file after the run: it was supplied by a file the
    /// worker does not own (or by a secret provider that already manages it).
    pub keep_credentials: bool,
    /// The worker secret name the credentials are read from when no inline or
    /// file source is configured.
    pub credentials_secret: String,
    /// Extra launcher/app arguments, after the fixed flags.
    pub extra_args: Vec<String>,
    /// Extra environment for the harness process (the worker's own environment
    /// is inherited; this is for callers that need to pass something explicit).
    pub extra_env: Vec<(String, String)>,
    /// The conversation the sandbox was created to resume
    /// (`GOBLE_NATIVE_SESSION_ID`). Used for a thread that has no newer session
    /// of its own, so restoring a sandbox resumes the binding's conversation.
    pub binding_session_id: Option<String>,
}

impl DshConfig {
    /// Read the runtime from the environment, using `workspace_hint` when the
    /// session does not name a workspace.
    pub fn from_env(workspace_hint: Option<PathBuf>) -> Self {
        Self::from_lookup(|name| std::env::var(name).ok(), workspace_hint)
    }

    /// Environment parsing with an injectable lookup, so tests never mutate
    /// process environment.
    pub fn from_lookup(
        lookup: impl Fn(&str) -> Option<String>,
        workspace_hint: Option<PathBuf>,
    ) -> Self {
        let get = |name: &str| {
            lookup(name)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let home = get(DSH_HOME_ENV)
            .or_else(|| get("DSH_HOME"))
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".dsh")))
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DSH_HOME));

        let (credentials_yaml, keep_credentials) = match get(DSH_CREDENTIALS_YAML_ENV) {
            Some(yaml) => (Some(yaml), false),
            None => match get(DSH_CREDENTIALS_FILE_ENV).map(PathBuf::from) {
                // A caller-managed file is read but never deleted: it may be
                // the image's own mount or a supervisor's file.
                Some(path) if path.is_file() => (
                    std::fs::read_to_string(&path)
                        .ok()
                        .map(|text| text.trim().to_string())
                        .filter(|text| !text.is_empty()),
                    true,
                ),
                _ => (None, false),
            },
        };

        let workspace = get(DSH_WORKSPACE_ENV)
            .map(PathBuf::from)
            .or_else(|| get("GOBLE_WORKSPACE_PATH").map(PathBuf::from))
            .or(workspace_hint)
            .filter(|path| path.is_dir())
            .unwrap_or_else(|| home.clone());

        let timeout = get(DSH_TIMEOUT_ENV)
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|seconds| *seconds > 0)
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_TIMEOUT);

        Self {
            bin: get(DSH_BIN_ENV).unwrap_or_else(|| HARNESS_DSH.to_string()),
            harness_path: get(DSH_HARNESS_PATH_ENV)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_HARNESS_PATH)),
            home,
            model: get(DSH_MODEL_ENV),
            workspace,
            timeout,
            credentials_yaml,
            keep_credentials,
            credentials_secret: get(DSH_CREDENTIALS_SECRET_ENV)
                .unwrap_or_else(|| DEFAULT_CREDENTIALS_SECRET.to_string()),
            extra_args: Vec::new(),
            extra_env: Vec::new(),
            binding_session_id: get(NATIVE_SESSION_ENV),
        }
    }

    /// The file the harness reads the account credentials from.
    pub fn credentials_path(&self) -> PathBuf {
        self.home.join(".credentials.yaml")
    }

    /// The file the runner writes the model/session overlay to.
    pub fn runtime_patch_path(&self) -> PathBuf {
        self.home.join(RUNTIME_PATCH_FILE)
    }

    /// The JSONL session directory.
    pub fn sessions_path(&self) -> PathBuf {
        self.home.join(SESSIONS_DIR)
    }

    /// The launcher and app arguments for one run.
    ///
    /// dsh-remote's shape: the `headless` profile, a Goble-owned composition,
    /// then (when configured) a generated overlay that pins the JSONL session
    /// store and the model; the app's own `--json`/policy flags; `--session-id`
    /// only when resuming; and `-` so the task comes from stdin.
    pub fn command_line(&self, session_id: Option<&str>) -> Vec<String> {
        let mut args = vec![
            "--profile".to_string(),
            "headless".to_string(),
            "--patch".to_string(),
            self.harness_path.display().to_string(),
        ];
        if self.model.is_some() {
            args.push("--patch".to_string());
            args.push(self.runtime_patch_path().display().to_string());
        }
        args.extend([
            "--json".to_string(),
            "--permission-mode".to_string(),
            "danger-full-access".to_string(),
            "--approval-policy".to_string(),
            "never".to_string(),
        ]);
        if let Some(session_id) = session_id.map(str::trim).filter(|id| !id.is_empty()) {
            args.push("--session-id".to_string());
            args.push(session_id.to_string());
        }
        args.extend(self.extra_args.iter().cloned());
        args.push("-".to_string());
        args
    }

    /// The generated overlay: JSONL sessions under this home and, when set, the
    /// model. Values are JSON-quoted, which YAML reads as a double-quoted
    /// scalar, so a path with spaces or a colon survives.
    pub fn runtime_patch(&self) -> String {
        let root = serde_json::to_string(&self.sessions_path().display().to_string())
            .unwrap_or_else(|_| "\"sessions\"".to_string());
        let mut patch = String::from(
            "# Generated by the goblin worker for each DSH run; edits are overwritten.\n",
        );
        patch.push_str("- id: session-persistence-jsonl\n  config:\n    root: ");
        patch.push_str(&root);
        patch.push_str("\n    compression: none\n");
        if let Some(model) = self
            .model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
        {
            let model =
                serde_json::to_string(model).unwrap_or_else(|_| "\"deepseek-flash\"".to_string());
            patch.push_str(
                "- id: agent-default-model\n  config:\n    provider: deepseek-account\n    model: ",
            );
            patch.push_str(&model);
            patch.push('\n');
        }
        patch
    }

    /// Make the harness home usable and install the run's credentials.
    ///
    /// Returns `true` when this call wrote the credentials file (so the caller
    /// should delete it after the run unless another run is active). The file
    /// is written with mode 0600.
    pub fn prepare(&self) -> std::io::Result<bool> {
        std::fs::create_dir_all(&self.home)?;
        std::fs::create_dir_all(self.sessions_path())?;
        std::fs::create_dir_all(&self.workspace)?;
        std::fs::write(self.runtime_patch_path(), self.runtime_patch())?;
        let Some(yaml) = self
            .credentials_yaml
            .as_deref()
            .map(str::trim)
            .filter(|y| !y.is_empty())
        else {
            return Ok(false);
        };
        if self.keep_credentials {
            // Do not rewrite a file the caller owns; it is already there.
            if self.credentials_path().is_file() {
                return Ok(false);
            }
        }
        let path = self.credentials_path();
        std::fs::write(&path, format!("{yaml}\n"))?;
        set_private_permissions(&path)?;
        Ok(!self.keep_credentials)
    }

    /// Remove the credentials before a filesystem snapshot, unless the file is
    /// one the caller manages (then it is not the worker's to delete).
    pub fn cleanup_before_snapshot(&self) -> std::io::Result<()> {
        if self.keep_credentials {
            return Ok(());
        }
        self.cleanup_credentials()
    }

    /// Remove `$DSH_HOME/.credentials.yaml`, so it cannot reach a filesystem
    /// snapshot. Called after the last concurrent run finishes and before the
    /// worker uploads a snapshot.
    pub fn cleanup_credentials(&self) -> std::io::Result<()> {
        match std::fs::remove_file(self.credentials_path()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

fn set_private_permissions(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// One token-usage report from a `step_end` event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DshUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

impl DshUsage {
    fn from_value(value: &Value) -> Option<Self> {
        let usage = value.as_object()?;
        let number = |key: &str| usage.get(key).and_then(Value::as_u64);
        Some(Self {
            input_tokens: number("inputTokens")?,
            output_tokens: number("outputTokens")?,
            total_tokens: number("totalTokens"),
            cache_read_tokens: number("cacheReadTokens"),
            cache_write_tokens: number("cacheWriteTokens"),
            reasoning_tokens: number("reasoningTokens"),
        })
    }

    /// A one-line summary for the trace log.
    pub fn summary(&self) -> String {
        let mut line = format!(
            "usage: {} in / {} out tokens",
            self.input_tokens, self.output_tokens
        );
        if let Some(total) = self.total_tokens {
            line.push_str(&format!(" (total {total})"));
        }
        if let Some(cache) = self.cache_read_tokens {
            line.push_str(&format!(", {cache} cache read"));
        }
        if let Some(cache) = self.cache_write_tokens {
            line.push_str(&format!(", {cache} cache write"));
        }
        if let Some(reasoning) = self.reasoning_tokens {
            line.push_str(&format!(", {reasoning} reasoning"));
        }
        line
    }
}

/// One parsed event from the harness's `--json` stream.
#[derive(Debug, Clone, PartialEq)]
pub enum DshEvent {
    /// Opening event: the identity the run used.
    Session { session_id: String },
    /// Committed assistant text.
    Text { text: String },
    /// Committed reasoning text. Goble has no reasoning frame, so it lands in
    /// the trace log.
    Thinking { text: String },
    /// A tool call the harness is about to run.
    ToolCall {
        call_id: String,
        tool: String,
        input: Value,
    },
    /// The result of a tool call.
    ToolResult {
        call_id: String,
        completed: bool,
        result: String,
        error: Option<String>,
    },
    /// A lifecycle phase (`turn_start`, `step_start`, `step_end`, `turn_end`).
    Status {
        phase: String,
        usage: Option<DshUsage>,
        reason: Option<String>,
    },
    /// Terminal event: the answer and the durable turn reason.
    Final {
        session_id: Option<String>,
        text: String,
        reason: Option<String>,
    },
    /// The runner failed outside a turn.
    Error { message: String },
    /// A line that is not JSON (the harness writes diagnostics to stderr, so
    /// this is unexpected and only ever logged).
    Raw { text: String },
    /// A blank line.
    Blank,
}

impl DshEvent {
    /// Parse one stdout line. Never fails: a line the harness wrote that is not
    /// a JSON object becomes [`DshEvent::Raw`], so an unexpected diagnostic
    /// cannot abort a turn.
    pub fn parse(line: &str) -> Self {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Self::Blank;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => Self::from_value(&value),
            Err(_) => Self::Raw {
                text: trimmed.to_string(),
            },
        }
    }

    fn from_value(value: &Value) -> Self {
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let text = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        match kind {
            "session" => Self::Session {
                session_id: text("sessionId"),
            },
            "text" => Self::Text { text: text("text") },
            "thinking" => Self::Thinking { text: text("text") },
            "tool_call" => Self::ToolCall {
                call_id: text("callId"),
                tool: text("tool"),
                input: value.get("input").cloned().unwrap_or(Value::Null),
            },
            "tool_result" => {
                let completed = value
                    .get("status")
                    .and_then(Value::as_str)
                    .map(|status| status == "completed")
                    .unwrap_or(false);
                let error = value
                    .get("error")
                    .filter(|error| !error.is_null())
                    .map(|error| {
                        let name = error.get("name").and_then(Value::as_str);
                        let reason = error.get("reason").and_then(Value::as_str);
                        match (name, reason) {
                            (Some(name), Some(reason)) => format!("{name}: {reason}"),
                            (Some(name), None) => name.to_string(),
                            (None, Some(reason)) => reason.to_string(),
                            (None, None) => error.to_string(),
                        }
                    })
                    .or_else(|| {
                        let result = text("result");
                        (!completed && !result.is_empty()).then_some(result)
                    });
                Self::ToolResult {
                    call_id: text("callId"),
                    completed,
                    result: text("result"),
                    error,
                }
            }
            "status" => Self::Status {
                phase: text("phase"),
                usage: value.get("usage").and_then(DshUsage::from_value),
                reason: value
                    .get("reason")
                    .and_then(|reason| reason.get("kind"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
            },
            "final" => Self::Final {
                session_id: value
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                text: text("text"),
                reason: value
                    .get("reason")
                    .and_then(|reason| reason.get("kind"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
            },
            "error" => Self::Error {
                message: text("message"),
            },
            other => Self::Raw {
                text: format!("unhandled dsh event type `{other}`"),
            },
        }
    }
}

/// One run request.
#[derive(Debug, Clone, PartialEq)]
pub struct DshRequest {
    pub trace_id: String,
    pub task: String,
    pub native_session_id: Option<String>,
}

/// What one run produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DshOutcome {
    /// The process exited 0 and the stream carried a terminal `final` event
    /// whose reason is `completed`.
    pub ok: bool,
    pub exit_code: Option<i32>,
    /// The `final` event's text (the lossless answer), else the concatenated
    /// deltas.
    pub final_text: String,
    /// The text deltas, joined; used when no `final` text arrived.
    pub streamed_text: String,
    pub native_session_id: Option<String>,
    pub usage: Option<DshUsage>,
    pub turn_ended: bool,
    pub cancelled: bool,
    pub timed_out: bool,
    pub error: Option<String>,
    /// The last stderr lines, for the failure message.
    pub stderr_tail: Vec<String>,
    /// Whether any event was projected from the harness.
    pub event_count: usize,
}

impl DshOutcome {
    fn failure(error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::default()
        }
    }
}

/// Where the runner sends the protocol frames it projects.
pub trait DshEventSink: Send + Sync {
    fn emit(&self, message: WorkerMessage);
}

impl DshEventSink for AppState {
    fn emit(&self, message: WorkerMessage) {
        AppState::emit(self, message);
    }
}

#[derive(Debug, Clone)]
struct RunSlot {
    cancelled: Arc<AtomicU64>,
    /// `watch` channel: `true` means "stop this run".
    stop_tx: watch::Sender<bool>,
}

impl RunSlot {
    fn new() -> Self {
        let (stop_tx, _) = watch::channel(false);
        Self {
            cancelled: Arc::new(AtomicU64::new(0)),
            stop_tx,
        }
    }

    fn stop(&self) -> bool {
        self.cancelled.fetch_add(1, Ordering::SeqCst) == 0 && self.stop_tx.send(true).is_ok()
    }

    fn cancel_count(&self) -> u64 {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// The worker's live DSH runs, keyed by trace id, so a `cancel_run` frame can
/// stop the process group behind a turn.
#[derive(Default)]
pub struct DshRuns {
    runs: Mutex<HashMap<String, RunSlot>>,
    /// Serializes credential install/removal so one finishing run cannot delete
    /// the file a starting run just wrote.
    credentials: Mutex<()>,
}

impl DshRuns {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn register(&self, trace_id: &str) -> (RunSlot, watch::Receiver<bool>) {
        let slot = RunSlot::new();
        let receiver = slot.stop_tx.subscribe();
        self.runs.lock().insert(trace_id.to_string(), slot.clone());
        (slot, receiver)
    }

    fn finish(&self, trace_id: &str) {
        self.runs.lock().remove(trace_id);
    }

    /// Ask the run behind `trace_id` to stop. Returns `false` when no run by
    /// that id is live (already settled, or never started), which is a no-op,
    /// not an error.
    pub fn cancel(&self, trace_id: &str) -> bool {
        self.runs
            .lock()
            .get(trace_id)
            .map(|slot| slot.stop())
            .unwrap_or(false)
    }

    /// Whether a run by `trace_id` is live.
    pub fn is_running(&self, trace_id: &str) -> bool {
        self.runs.lock().contains_key(trace_id)
    }

    /// How many runs are live.
    pub fn active_count(&self) -> usize {
        self.runs.lock().len()
    }
}

/// Run one DSH turn and project its events onto `sink`.
///
/// The caller owns the trace lifecycle (`AgentStarted`/`AgentFinished`) and the
/// reply; this function only projects the harness's own stream. It never
/// returns `Err`: a harness that cannot start, a timeout and a kill all come
/// back as a [`DshOutcome`] with `ok == false` (or `cancelled`), so the caller
/// can settle the trace with the right status.
pub async fn run(
    config: &DshConfig,
    request: &DshRequest,
    sink: &dyn DshEventSink,
    runs: &Arc<DshRuns>,
) -> DshOutcome {
    let trace_id = request.trace_id.clone();
    // Install the runtime overlay and, when this worker owns them, the
    // credentials, before the run is registered. The lock is held only for the
    // synchronous file work, so two runs cannot interleave a write with the
    // other's cleanup.
    let wrote_credentials = {
        let _guard = runs.credentials.lock();
        match config.prepare() {
            Ok(wrote) => wrote,
            Err(error) => {
                let outcome = DshOutcome::failure(format!(
                    "could not prepare the DSH home {}: {error}",
                    config.home.display()
                ));
                emit_failure(sink, &trace_id, &outcome);
                return outcome;
            }
        }
    };

    let (slot, mut stop_rx) = runs.register(&trace_id);
    let result = run_registered(config, request, sink, &mut stop_rx, &slot).await;
    runs.finish(&trace_id);
    if result.cancelled {
        tracing::info!(trace_id = %trace_id, "dsh run stopped");
    }

    // The last run out deletes the credentials, so they never reach a filesystem
    // snapshot. A caller-managed file is never deleted.
    if wrote_credentials && !config.keep_credentials {
        let _guard = runs.credentials.lock();
        if runs.active_count() == 0 {
            if let Err(error) = config.cleanup_credentials() {
                tracing::warn!("could not delete the DSH credentials file: {error}");
            }
        }
    }
    result
}

async fn run_registered(
    config: &DshConfig,
    request: &DshRequest,
    sink: &dyn DshEventSink,
    stop_rx: &mut watch::Receiver<bool>,
    slot: &RunSlot,
) -> DshOutcome {
    let trace_id = request.trace_id.clone();

    let args = config.command_line(request.native_session_id.as_deref());
    let mut command = Command::new(&config.bin);
    command
        .args(&args)
        .current_dir(&config.workspace)
        .env("DSH_HOME", &config.home)
        // The credentials are a file for the duration of the run; the model's
        // shell tools must not see the value in their environment as well.
        .env_remove(DSH_CREDENTIALS_YAML_ENV)
        .envs(config.extra_env.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        // Its own process group, so a cancel/timeout kills the tools the
        // harness started too (the negative pid in `kill_process_group`).
        command.process_group(0);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let outcome = DshOutcome::failure(format!(
                "could not start the DSH runtime `{}`: {error}",
                config.bin
            ));
            emit_failure(sink, &trace_id, &outcome);
            return outcome;
        }
    };

    let mut outcome = DshOutcome::default();
    // The task goes on stdin; dsh reads it until EOF.
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = async {
            stdin.write_all(request.task.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.shutdown().await
        }
        .await
        {
            outcome.error = Some(format!("could not send the task to dsh: {error}"));
        }
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stderr_tail = Arc::new(Mutex::new(Vec::<String>::new()));
    let stderr_task = stderr.map(|stderr| {
        let tail = stderr_tail.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim_end().to_string();
                if line.is_empty() {
                    continue;
                }
                tracing::debug!(target: "dsh", "{line}");
                let mut tail = tail.lock();
                if tail.len() >= 40 {
                    tail.remove(0);
                }
                tail.push(line);
            }
        })
    });

    let deadline = tokio::time::Instant::now() + config.timeout;
    if let Some(stdout) = stdout {
        let mut lines = BufReader::new(stdout).lines();
        loop {
            tokio::select! {
                biased;
                // Poll the stop channel unconditionally: a run waiting on a
                // silent harness has no other event that would wake this
                // loop, so a guarded branch would only notice the cancel at
                // the next stdout line or at the deadline.
                _ = stop_rx.changed() => {
                    outcome.cancelled = true;
                    terminate(&mut child).await;
                    break;
                }
                _ = tokio::time::sleep_until(deadline), if !outcome.cancelled => {
                    outcome.timed_out = true;
                    outcome.error = Some(format!(
                        "dsh did not finish within {}s",
                        config.timeout.as_secs()
                    ));
                    terminate(&mut child).await;
                    break;
                }
                next = lines.next_line() => match next {
                    Ok(Some(line)) => {
                        outcome.event_count += 1;
                        project_event(&trace_id, &line, sink, &mut outcome);
                    }
                    Ok(None) => break,
                    Err(error) => {
                        outcome.error.get_or_insert_with(|| {
                            format!("could not read the dsh event stream: {error}")
                        });
                        break;
                    }
                },
            }
        }
    }
    // Re-read the cancel count: a stop that raced the last event still has to
    // report as cancelled rather than as a completed turn.
    if slot.cancel_count() > 0 {
        outcome.cancelled = true;
    }

    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .ok()
        .and_then(Result::ok);
    outcome.exit_code = status.and_then(|status| status.code());
    if let Some(task) = stderr_task {
        let _ = tokio::time::timeout(Duration::from_secs(2), task).await;
    }
    outcome.stderr_tail = stderr_tail.lock().clone();

    // A cancelled or timed-out run is reported as such, never as a success.
    if outcome.cancelled || outcome.timed_out {
        outcome.ok = false;
        if outcome.error.is_none() && outcome.cancelled {
            outcome.error = Some("the run was cancelled".to_string());
        }
        outcome.final_text = fallback_text(&outcome);
        if outcome.timed_out {
            emit_failure(sink, &trace_id, &outcome);
        }
        return outcome;
    }

    if outcome.final_text.is_empty() {
        outcome.final_text = fallback_text(&outcome);
    }
    let completed_reason = outcome.error.is_none();
    outcome.ok = outcome.exit_code == Some(0) && completed_reason && outcome.turn_ended;
    if !outcome.ok && outcome.error.is_none() {
        outcome.error = Some(match outcome.exit_code {
            Some(code) => format!("dsh exited with status {code}"),
            None => "dsh ended without an exit status".to_string(),
        });
    }
    if !outcome.ok {
        emit_failure(sink, &trace_id, &outcome);
    }
    outcome
}

fn fallback_text(outcome: &DshOutcome) -> String {
    if !outcome.streamed_text.is_empty() {
        outcome.streamed_text.clone()
    } else {
        String::new()
    }
}

fn emit_failure(sink: &dyn DshEventSink, trace_id: &str, outcome: &DshOutcome) {
    let mut message = outcome
        .error
        .clone()
        .unwrap_or_else(|| "the DSH run failed".to_string());
    if !outcome.stderr_tail.is_empty() {
        message.push_str(" — ");
        message.push_str(
            &outcome
                .stderr_tail
                .iter()
                .rev()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(" | "),
        );
    }
    sink.emit(WorkerMessage::AgentLog {
        trace_id: trace_id.to_string(),
        step_id: "dsh".to_string(),
        level: LogLevel::Error,
        message,
    });
}

fn project_event(trace_id: &str, line: &str, sink: &dyn DshEventSink, outcome: &mut DshOutcome) {
    match DshEvent::parse(line) {
        DshEvent::Session { session_id } => {
            if !session_id.is_empty() {
                outcome.native_session_id = Some(session_id);
            }
        }
        DshEvent::Text { text } => {
            if !text.is_empty() {
                outcome.streamed_text.push_str(&text);
                sink.emit(WorkerMessage::AssistantDelta {
                    trace_id: trace_id.to_string(),
                    delta: text,
                });
            }
        }
        DshEvent::Thinking { text } => {
            let line = one_line(&text, 400);
            if !line.is_empty() {
                sink.emit(WorkerMessage::AgentLog {
                    trace_id: trace_id.to_string(),
                    step_id: "thinking".to_string(),
                    level: LogLevel::Debug,
                    message: line,
                });
            }
        }
        DshEvent::ToolCall {
            call_id,
            tool,
            input,
        } => sink.emit(WorkerMessage::ToolCallStarted {
            trace_id: trace_id.to_string(),
            id: call_id,
            name: tool,
            arguments: input,
        }),
        DshEvent::ToolResult {
            call_id,
            completed,
            result,
            error,
        } => {
            if completed {
                sink.emit(WorkerMessage::ToolCallFinished {
                    trace_id: trace_id.to_string(),
                    id: call_id,
                    result,
                });
            } else {
                sink.emit(WorkerMessage::ToolCallError {
                    trace_id: trace_id.to_string(),
                    id: call_id,
                    message: error.unwrap_or(result),
                });
            }
        }
        DshEvent::Status {
            phase,
            usage,
            reason,
        } => {
            if let Some(usage) = usage {
                outcome.usage = Some(usage);
                sink.emit(WorkerMessage::AgentLog {
                    trace_id: trace_id.to_string(),
                    step_id: "usage".to_string(),
                    level: LogLevel::Info,
                    message: usage.summary(),
                });
            }
            if phase == "turn_end" {
                outcome.turn_ended = true;
                if let Some(reason) = reason.filter(|reason| reason != "completed") {
                    outcome
                        .error
                        .get_or_insert_with(|| format!("dsh turn ended: {reason}"));
                }
                sink.emit(WorkerMessage::Done {
                    trace_id: trace_id.to_string(),
                });
            }
        }
        DshEvent::Final {
            session_id,
            text,
            reason,
        } => {
            if let Some(session_id) = session_id.filter(|id| !id.is_empty()) {
                outcome.native_session_id = Some(session_id);
            }
            outcome.final_text = text;
            if let Some(reason) = reason.filter(|reason| reason != "completed") {
                outcome
                    .error
                    .get_or_insert_with(|| format!("the DSH turn did not complete: {reason}"));
            }
        }
        DshEvent::Error { message } => {
            outcome.error = Some(message.clone());
            sink.emit(WorkerMessage::AgentLog {
                trace_id: trace_id.to_string(),
                step_id: "dsh".to_string(),
                level: LogLevel::Error,
                message,
            });
        }
        DshEvent::Raw { text } => {
            if !text.is_empty() {
                sink.emit(WorkerMessage::AgentLog {
                    trace_id: trace_id.to_string(),
                    step_id: "dsh".to_string(),
                    level: LogLevel::Debug,
                    message: one_line(&text, 400),
                });
            }
        }
        DshEvent::Blank => {}
    }
}

/// A short, single-line rendering of `text` for a log frame.
fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut shortened: String = flat.chars().take(max).collect();
    shortened.push('…');
    shortened
}

/// Kill the harness process group (unix) and the child itself (all platforms).
async fn terminate(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        kill_process_group(pid);
    }
    #[cfg(not(unix))]
    let _ = child;
    let _ = child.start_kill();
}

#[cfg(unix)]
fn kill_process_group(pid: u32) {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    // Negative pid = the whole group; the child is the group leader because it
    // was spawned with `process_group(0)`.
    let _ = kill(Pid::from_raw(-(pid as i32)), Signal::SIGKILL);
    let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
}

#[cfg(not(unix))]
fn kill_process_group(_pid: u32) {}

/// The setting key under which a thread's native DSH session is remembered.
pub fn session_setting_key(thread_id: &str) -> String {
    format!("{DSH_SESSION_SETTING_PREFIX}{thread_id}")
}

/// Start a remote-DSH thread reply in its own task, if `msg` is one.
///
/// The connection loop calls this instead of awaiting the run: while the
/// harness works, the loop has to stay free to read a `cancel_run` frame (and
/// to keep other frames flowing). Returns the task it spawned, or `None` when
/// `msg` is not a thread reply.
pub fn spawn_thread_reply(
    state: Arc<AppState>,
    msg: &DesktopMessage,
) -> Option<tokio::task::JoinHandle<()>> {
    let DesktopMessage::RunAgentForThreadReply {
        trace_id,
        thread_id,
        agent_id,
        prompt,
        spec,
        mcp_servers,
    } = msg
    else {
        return None;
    };
    // The same bookkeeping the blocking handler did before the run: the agent
    // and its MCP servers are stored, and the trace is tied to its thread so a
    // later answer resumes the same conversation.
    state.store_agent(spec.clone());
    for server in mcp_servers.clone() {
        state.store_mcp(server);
    }
    {
        let mut threads = state.threads_by_trace.lock();
        if threads.len() > 512 {
            threads.clear();
        }
        threads.insert(trace_id.clone(), thread_id.clone());
    }
    let trace_id = trace_id.clone();
    let thread_id = thread_id.clone();
    let agent_id = agent_id.clone();
    let prompt = prompt.clone();
    Some(tokio::spawn(async move {
        let config = DshConfig::from_env(Some(state.config.lock().workspace_root.clone()));
        match run_thread_reply(
            &state,
            &config,
            trace_id.clone(),
            thread_id.clone(),
            agent_id,
            prompt,
        )
        .await
        {
            Ok(Some(content)) => {
                state.emit(WorkerMessage::ThreadAgentReply {
                    trace_id,
                    thread_id,
                    content,
                });
            }
            Ok(None) => {}
            Err(error) => {
                // A failure raised before the run started (the store, the
                // request) is reported the way the connection handler reports
                // one, so the client that asked hears about it.
                state.emit(WorkerMessage::AgentLog {
                    trace_id: trace_id.clone(),
                    step_id: "worker".to_string(),
                    level: LogLevel::Error,
                    message: error.to_string(),
                });
                state.emit(WorkerMessage::AgentFinished {
                    trace_id,
                    status: ExecutionStatus::Failure(error.to_string()),
                });
            }
        }
    }))
}

/// Run one thread reply through the DSH runtime and return the reply text.
///
/// Mirrors [`crate::runner::Runner::run_agent_for_thread_reply`] for the parts a
/// client can see: the conversation row and user message are stored, the trace
/// is opened as `AgentStarted`, and the reply is stored as the assistant message
/// and returned (the caller emits `ThreadAgentReply`). The native session id is
/// remembered per thread so the next turn resumes with `--session-id`.
pub async fn run_thread_reply(
    state: &Arc<AppState>,
    config: &DshConfig,
    trace_id: String,
    thread_id: String,
    agent_id: goble_core::agent::AgentId,
    prompt: String,
) -> anyhow::Result<Option<String>> {
    let store = state.store()?;
    let now = chrono::Utc::now().to_rfc3339();
    if store.get_chat_title(&thread_id)?.is_none() {
        let title: String = prompt.trim().chars().take(54).collect();
        let title = if title.is_empty() {
            "Phone session".to_string()
        } else {
            title
        };
        store.insert_chat(&thread_id, &title, None, None, &now, &now)?;
    }
    store.insert_chat_message(
        &uuid::Uuid::new_v4().to_string(),
        &thread_id,
        "user",
        &prompt,
        None,
        &now,
    )?;

    // Open the trace the client watches. The harness itself does not know it.
    let mut trace = ExecutionTrace::new(agent_id.clone());
    trace.id = trace_id.clone();
    trace.worker_id = Some(state.worker_id.clone());
    trace.status = ExecutionStatus::Running;
    state.store_trace(trace);
    state.emit(WorkerMessage::AgentStarted {
        trace_id: trace_id.clone(),
        agent_id,
    });

    // A thread's own remembered session wins; a sandbox restored from a
    // snapshot otherwise resumes the conversation its binding named
    // (`GOBLE_NATIVE_SESSION_ID`, written by the provisioner).
    let native_session_id = store
        .get_setting(&session_setting_key(&thread_id))
        .ok()
        .flatten()
        .filter(|id| !id.trim().is_empty())
        .or_else(|| {
            config
                .binding_session_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
        });

    // Credentials come from the env/file when the provisioner supplied them, or
    // from the worker's own vault (a `dsh_credentials` secret the desktop
    // pushes). The runner installs the file for the run and deletes it after
    // the last concurrent run, so it never reaches a snapshot.
    let mut run_config = config.clone();
    if run_config.credentials_yaml.is_none() {
        if let Some(yaml) = credentials_from_vault(state, &run_config.credentials_secret) {
            run_config.credentials_yaml = Some(yaml);
            run_config.keep_credentials = false;
        }
    }

    let request = DshRequest {
        trace_id: trace_id.clone(),
        task: prompt,
        native_session_id,
    };
    let outcome = run(&run_config, &request, state.as_ref(), &state.dsh_runs).await;

    // Remember the native session for the next turn, even when the turn failed:
    // a resumed session may still be the right conversation.
    if let Some(session_id) = outcome.native_session_id.as_deref() {
        if !session_id.is_empty() {
            if let Err(error) = store.set_setting(&session_setting_key(&thread_id), session_id) {
                tracing::warn!("could not remember the DSH session id: {error}");
            }
        }
    }

    if outcome.cancelled {
        settle_trace(state, &trace_id, ExecutionStatus::Cancelled);
        return Ok(None);
    }
    if !outcome.ok {
        // The runner already emitted the error log line. Settle the same trace
        // here (instead of returning `Err`) so the connection handler does not
        // report the failure a second time.
        let error = outcome
            .error
            .clone()
            .unwrap_or_else(|| "the DSH run failed".to_string());
        settle_trace(state, &trace_id, ExecutionStatus::Failure(error));
        return Ok(None);
    }

    let answer = if outcome.final_text.trim().is_empty() {
        "The run finished without text.".to_string()
    } else {
        outcome.final_text
    };
    store.insert_chat_message(
        &uuid::Uuid::new_v4().to_string(),
        &thread_id,
        "assistant",
        &answer,
        None,
        &chrono::Utc::now().to_rfc3339(),
    )?;
    settle_trace(state, &trace_id, ExecutionStatus::Success);
    Ok(Some(answer))
}

/// Read the DeepSeek account credentials from the worker's vault.
fn credentials_from_vault(state: &AppState, name: &str) -> Option<String> {
    let vault_value = state
        .file_vault
        .lock()
        .get(name, b"")
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    if vault_value.is_some() {
        return vault_value;
    }
    state
        .secrets
        .lock()
        .values()
        .find(|secret| secret.name == name)
        .and_then(|secret| String::from_utf8(secret.encrypted_value.clone()).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn settle_trace(state: &AppState, trace_id: &str, status: ExecutionStatus) {
    state.update_trace(trace_id, |trace| trace.status = status.clone());
    state.emit(WorkerMessage::AgentFinished {
        trace_id: trace_id.to_string(),
        status,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;
    use tempfile::TempDir;

    #[derive(Default)]
    struct RecordingSink {
        messages: StdMutex<Vec<WorkerMessage>>,
    }

    impl RecordingSink {
        fn kinds(&self) -> Vec<String> {
            self.messages
                .lock()
                .unwrap()
                .iter()
                .map(|message| {
                    serde_json::to_value(message).unwrap()["kind"]
                        .as_str()
                        .unwrap()
                        .to_string()
                })
                .collect()
        }

        fn messages(&self) -> Vec<WorkerMessage> {
            self.messages.lock().unwrap().clone()
        }
    }

    impl DshEventSink for RecordingSink {
        fn emit(&self, message: WorkerMessage) {
            self.messages.lock().unwrap().push(message);
        }
    }

    fn config_for(dir: &TempDir) -> DshConfig {
        DshConfig {
            bin: "dsh".to_string(),
            harness_path: dir.path().join("harness.yml"),
            home: dir.path().join("home"),
            model: None,
            workspace: dir.path().to_path_buf(),
            timeout: Duration::from_secs(10),
            credentials_yaml: None,
            keep_credentials: false,
            credentials_secret: DEFAULT_CREDENTIALS_SECRET.to_string(),
            extra_args: Vec::new(),
            extra_env: Vec::new(),
            binding_session_id: None,
        }
    }

    #[test]
    fn the_command_line_matches_the_dsh_remote_shape() {
        let dir = TempDir::new().unwrap();
        let mut config = config_for(&dir);
        let harness = config.harness_path.display().to_string();
        assert_eq!(
            config.command_line(None),
            vec![
                "--profile".to_string(),
                "headless".to_string(),
                "--patch".to_string(),
                harness.clone(),
                "--json".to_string(),
                "--permission-mode".to_string(),
                "danger-full-access".to_string(),
                "--approval-policy".to_string(),
                "never".to_string(),
                "-".to_string(),
            ]
        );

        config.model = Some("deepseek-pro".to_string());
        let argv = config.command_line(Some(" session-1 "));
        assert_eq!(
            argv[..6].to_vec(),
            vec![
                "--profile".to_string(),
                "headless".to_string(),
                "--patch".to_string(),
                harness,
                "--patch".to_string(),
                config.runtime_patch_path().display().to_string(),
            ]
        );
        let session_at = argv
            .iter()
            .position(|arg| arg == "--session-id")
            .expect("a resumed run names its session");
        assert_eq!(argv[session_at + 1], "session-1");
        assert_eq!(argv.last().map(String::as_str), Some("-"));
    }

    #[test]
    fn the_runtime_patch_pins_jsonl_sessions_and_the_model() {
        let dir = TempDir::new().unwrap();
        let mut config = config_for(&dir);
        let patch = config.runtime_patch();
        assert!(patch.contains("session-persistence-jsonl"));
        assert!(patch.contains(&config.sessions_path().display().to_string()));
        assert!(!patch.contains("agent-default-model"));

        config.model = Some("deepseek-flash".to_string());
        let patch = config.runtime_patch();
        assert!(patch.contains("agent-default-model"));
        assert!(patch.contains("provider: deepseek-account"));
        assert!(patch.contains("\"deepseek-flash\""));
    }

    #[test]
    fn environment_parsing_defaults_and_overrides() {
        let dir = TempDir::new().unwrap();
        let home = dir.path().join("dsh-home");
        std::fs::create_dir_all(dir.path().join("ws")).unwrap();
        let env = |name: &str| match name {
            DSH_HOME_ENV => Some(home.display().to_string()),
            DSH_WORKSPACE_ENV => Some(dir.path().join("ws").display().to_string()),
            DSH_MODEL_ENV => Some("deepseek-pro".to_string()),
            DSH_TIMEOUT_ENV => Some("42".to_string()),
            _ => None,
        };
        let config = DshConfig::from_lookup(env, None);
        assert_eq!(config.home, home);
        assert_eq!(config.model.as_deref(), Some("deepseek-pro"));
        assert_eq!(config.timeout, Duration::from_secs(42));
        assert_eq!(config.bin, "dsh");
        assert_eq!(config.harness_path, PathBuf::from(DEFAULT_HARNESS_PATH));

        let config = DshConfig::from_lookup(|_| None, Some(dir.path().join("ws")));
        assert_eq!(config.model, None);
        assert_eq!(config.timeout, DEFAULT_TIMEOUT);
        assert_eq!(
            config.workspace,
            dir.path().join("ws"),
            "the workspace hint is used when nothing names a workspace"
        );
    }

    #[test]
    fn a_binding_session_is_read_from_the_sandbox_environment() {
        let dir = TempDir::new().unwrap();
        let config = DshConfig::from_lookup(
            |name| match name {
                NATIVE_SESSION_ENV => Some(" session-binding ".to_string()),
                _ => None,
            },
            Some(dir.path().to_path_buf()),
        );
        assert_eq!(
            config.binding_session_id.as_deref(),
            Some("session-binding")
        );

        let config = DshConfig::from_lookup(|_| None, Some(dir.path().to_path_buf()));
        assert_eq!(config.binding_session_id, None);
    }

    #[test]
    fn credentials_are_installed_privately_and_removed_by_cleanup() {
        let dir = TempDir::new().unwrap();
        let mut config = config_for(&dir);
        config.credentials_yaml = Some("token: secret".to_string());
        assert!(config.prepare().unwrap());
        let path = config.credentials_path();
        assert!(path.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600,
                "the credentials file is private"
            );
        }
        config.cleanup_credentials().unwrap();
        assert!(!path.exists(), "cleanup deletes the credentials file");
        config.cleanup_credentials().unwrap();
    }

    #[test]
    fn a_caller_managed_credentials_file_is_not_rewritten_or_deleted() {
        let dir = TempDir::new().unwrap();
        let mut config = config_for(&dir);
        std::fs::create_dir_all(&config.home).unwrap();
        std::fs::write(config.credentials_path(), "token: mine\n").unwrap();
        config.credentials_yaml = Some("token: mine".to_string());
        config.keep_credentials = true;
        assert!(
            !config.prepare().unwrap(),
            "prepare does not claim the file"
        );
    }

    #[test]
    fn every_event_shape_dsh_headless_documents_is_parsed() {
        let cases = [
            (
                r#"{"type":"session","sessionId":"session-1","cwd":"/workspace"}"#,
                DshEvent::Session {
                    session_id: "session-1".to_string(),
                },
            ),
            (
                r#"{"type":"text","text":"hello"}"#,
                DshEvent::Text {
                    text: "hello".to_string(),
                },
            ),
            (
                r#"{"type":"thinking","text":"hmm"}"#,
                DshEvent::Thinking {
                    text: "hmm".to_string(),
                },
            ),
            (
                r#"{"type":"tool_call","callId":"c1","tool":"bash","input":{"command":"ls"}}"#,
                DshEvent::ToolCall {
                    call_id: "c1".to_string(),
                    tool: "bash".to_string(),
                    input: serde_json::json!({"command": "ls"}),
                },
            ),
            (
                r#"{"type":"tool_result","callId":"c1","status":"completed","result":"ok"}"#,
                DshEvent::ToolResult {
                    call_id: "c1".to_string(),
                    completed: true,
                    result: "ok".to_string(),
                    error: None,
                },
            ),
            (
                r#"{"type":"tool_result","callId":"c2","status":"error","result":"","error":{"name":"BashError","reason":"exit 1"}}"#,
                DshEvent::ToolResult {
                    call_id: "c2".to_string(),
                    completed: false,
                    result: String::new(),
                    error: Some("BashError: exit 1".to_string()),
                },
            ),
            (
                r#"{"type":"status","phase":"turn_start","turn":1}"#,
                DshEvent::Status {
                    phase: "turn_start".to_string(),
                    usage: None,
                    reason: None,
                },
            ),
            (
                r#"{"type":"status","phase":"step_end","turn":1,"step":1,"usage":{"inputTokens":10,"outputTokens":5,"totalTokens":15,"cacheReadTokens":2}}"#,
                DshEvent::Status {
                    phase: "step_end".to_string(),
                    usage: Some(DshUsage {
                        input_tokens: 10,
                        output_tokens: 5,
                        total_tokens: Some(15),
                        cache_read_tokens: Some(2),
                        cache_write_tokens: None,
                        reasoning_tokens: None,
                    }),
                    reason: None,
                },
            ),
            (
                r#"{"type":"status","phase":"turn_end","turn":1,"reason":{"kind":"completed"}}"#,
                DshEvent::Status {
                    phase: "turn_end".to_string(),
                    usage: None,
                    reason: Some("completed".to_string()),
                },
            ),
            (
                r#"{"type":"final","sessionId":"session-1","reason":{"kind":"completed"},"text":"done"}"#,
                DshEvent::Final {
                    session_id: Some("session-1".to_string()),
                    text: "done".to_string(),
                    reason: Some("completed".to_string()),
                },
            ),
            (
                r#"{"type":"error","message":"boom"}"#,
                DshEvent::Error {
                    message: "boom".to_string(),
                },
            ),
        ];
        for (line, expected) in cases {
            assert_eq!(DshEvent::parse(line), expected, "line: {line}");
        }
        assert_eq!(DshEvent::parse(""), DshEvent::Blank);
        assert_eq!(DshEvent::parse("  "), DshEvent::Blank);
        assert_eq!(
            DshEvent::parse("a warning on stdout"),
            DshEvent::Raw {
                text: "a warning on stdout".to_string()
            }
        );
    }

    #[test]
    fn usage_summaries_name_only_the_fields_that_arrived() {
        let usage = DshUsage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: Some(15),
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
        };
        let summary = usage.summary();
        assert!(summary.contains("10 in / 5 out"));
        assert!(summary.contains("total 15"));
        assert!(!summary.contains("cache"));
    }

    #[test]
    fn a_cancel_reaches_only_the_run_it_names() {
        let runs = DshRuns::new();
        let (_slot, mut rx) = runs.register("trace-1");
        assert!(runs.is_running("trace-1"));
        assert_eq!(runs.active_count(), 1);
        assert!(runs.cancel("trace-1"));
        assert!(*rx.borrow_and_update());
        assert!(!runs.cancel("trace-2"), "an unknown trace is a no-op");
        runs.finish("trace-1");
        assert!(!runs.is_running("trace-1"));
        assert_eq!(runs.active_count(), 0);
    }

    #[test]
    fn the_session_setting_key_names_the_thread() {
        assert_eq!(session_setting_key("thread-1"), "dsh_session:thread-1");
    }
}
