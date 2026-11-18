//! The ACP server proper: newline-delimited JSON-RPC 2.0 over stdio, with the
//! ACP method set implemented on top of the gateway client.
//!
//! Transport rules (captured from a real `dsh --profile acp` server):
//! - one compact JSON object per `\n`-terminated line, in both directions;
//!   no `Content-Length` framing;
//! - requests carry `id`, notifications do not, responses carry `result` or
//!   `error`;
//! - **stdout carries protocol traffic only** — all diagnostics go to stderr;
//! - the client may send `session/cancel` while `session/prompt` is in flight,
//!   so requests are handled on separate tasks and the reader never blocks on a
//!   running turn.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::config::{AcpConfig, PersistedSession, PersistedState};
use crate::gateway::{
    CancelToken, GatewayClient, GatewayError, GatewayRun, RunStatus, RunTerminal, StreamControl,
};
use crate::mapping::UpdateMapper;
use crate::prompt::{self, PromptError};

/// ACP protocol version this server speaks (`protocolVersion: 1`).
pub const PROTOCOL_VERSION: u64 = 1;
/// Current `goble-acp` version, reported in `agentInfo`.
pub const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

// JSON-RPC 2.0 error codes plus the implementation-defined server range.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
/// ACP/bridge specific: one prompt per session at a time, gateway 409.
const SERVER_ERROR_BUSY: i64 = -32000;
/// The ACP session id is unknown to this bridge (or the gateway session is gone).
const SERVER_ERROR_UNKNOWN_SESSION: i64 = -32002;

/// A JSON-RPC error, ready to be turned into a response frame.
#[derive(Debug, Clone, PartialEq)]
struct RpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    fn with_data(code: i64, message: impl Into<String>, data: Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }

    fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(INVALID_PARAMS, message)
    }

    fn method_not_found(method: &str) -> Self {
        Self::with_data(
            METHOD_NOT_FOUND,
            format!("Method not found: {method}"),
            json!({ "method": method }),
        )
    }
}

impl From<GatewayError> for RpcError {
    fn from(error: GatewayError) -> Self {
        let code = error.rpc_code();
        let message = match &error {
            GatewayError::Transport(_) => format!("gateway unreachable: {error}"),
            GatewayError::Conflict(_) => format!("session busy: {error}"),
            GatewayError::NotFound(_) => format!("unknown gateway session or run: {error}"),
            _ => error.to_string(),
        };
        Self::new(code, message)
    }
}

impl From<PromptError> for RpcError {
    fn from(error: PromptError) -> Self {
        Self::invalid_params(error.to_string())
    }
}

/// One ACP session known to this bridge process, mapped to a durable gateway
/// session binding.
struct SessionEntry {
    gateway_session_id: String,
    name: String,
    cwd: String,
    created_at_ms: u64,
    /// Set while a `session/prompt` turn is in flight.
    busy: Option<CancelToken>,
}

/// Shared server state. Every request task and the stdout writer coordinate
/// through this.
pub struct ServerState {
    config: AcpConfig,
    gateway: Arc<GatewayClient>,
    sessions: Mutex<HashMap<String, SessionEntry>>,
    out: mpsc::UnboundedSender<Value>,
}

impl ServerState {
    /// Queue a JSON-RPC frame for stdout.
    fn send(&self, frame: Value) {
        if self.out.send(frame).is_err() {
            tracing::debug!("stdout writer is gone; dropping frame");
        }
    }

    fn lock_sessions(&self) -> MutexGuard<'_, HashMap<String, SessionEntry>> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Persist the ACP↔gateway session map so `session/resume` survives a
    /// bridge restart. Failures are warnings: persistence is a convenience, not
    /// part of the protocol.
    fn persist(&self) {
        let Some(path) = self.config.sessions_path() else {
            return;
        };
        // The map key is the ACP session id, so `PersistedSession` can stay a
        // self-contained list.
        let mut sessions: Vec<PersistedSession> = self
            .lock_sessions()
            .iter()
            .map(|(id, entry)| PersistedSession {
                acp_session_id: id.clone(),
                gateway_session_id: entry.gateway_session_id.clone(),
                name: entry.name.clone(),
                cwd: entry.cwd.clone(),
                created_at_ms: entry.created_at_ms,
            })
            .collect();
        sessions.sort_by(|left, right| left.acp_session_id.cmp(&right.acp_session_id));
        if let Err(error) = write_state_file(&path, &PersistedState { sessions }) {
            tracing::warn!(
                "could not persist ACP sessions to {}: {error}",
                path.display()
            );
        }
    }
}

/// Run the ACP server until stdin closes.
pub async fn run(config: AcpConfig) -> anyhow::Result<()> {
    let gateway = Arc::new(GatewayClient::new(&config).map_err(anyhow::Error::msg)?);
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let writer = tokio::spawn(writer_task(out_rx));

    let mut sessions = HashMap::new();
    for persisted in load_persisted(&config) {
        sessions.insert(
            persisted.acp_session_id.clone(),
            SessionEntry {
                gateway_session_id: persisted.gateway_session_id,
                name: persisted.name,
                cwd: persisted.cwd,
                created_at_ms: persisted.created_at_ms,
                busy: None,
            },
        );
    }
    let resumed = sessions.len();
    let state = Arc::new(ServerState {
        config: config.clone(),
        gateway,
        sessions: Mutex::new(sessions),
        out: out_tx,
    });

    tracing::info!(
        "goble-acp {} ready (protocol v{}): {config}, resumed {resumed} session(s)",
        AGENT_VERSION,
        PROTOCOL_VERSION
    );

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => handle_line(&state, &line),
            Ok(None) => break,
            Err(error) => {
                tracing::error!("stdin read error: {error}");
                break;
            }
        }
    }
    tracing::info!("stdin closed; exiting");
    // Let already-queued response frames flush before the process exits.
    tokio::time::sleep(Duration::from_millis(50)).await;
    writer.abort();
    Ok(())
}

async fn writer_task(mut rx: mpsc::UnboundedReceiver<Value>) -> anyhow::Result<()> {
    let mut stdout = tokio::io::BufWriter::new(tokio::io::stdout());
    while let Some(frame) = rx.recv().await {
        let line = serde_json::to_string(&frame)?;
        stdout.write_all(line.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }
    Ok(())
}

/// Parse and dispatch one line from the client.
fn handle_line(state: &Arc<ServerState>, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let frame: Value = match serde_json::from_str(line) {
        Ok(frame) => frame,
        Err(error) => {
            state.send(error_response(
                Value::Null,
                PARSE_ERROR,
                format!("Parse error: {error}"),
            ));
            return;
        }
    };
    let Some(object) = frame.as_object() else {
        state.send(error_response(
            Value::Null,
            INVALID_REQUEST,
            "Invalid Request: expected a JSON-RPC object".to_string(),
        ));
        return;
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        state.send(error_response(
            Value::Null,
            INVALID_REQUEST,
            "Invalid Request: expected jsonrpc \"2.0\"".to_string(),
        ));
        return;
    }
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        state.send(error_response(
            Value::Null,
            INVALID_REQUEST,
            "Invalid Request: missing method".to_string(),
        ));
        return;
    };
    let method = method.to_string();
    let params = object.get("params").cloned().unwrap_or_else(|| json!({}));

    match object.get("id").cloned() {
        Some(id) => {
            // Requests: answer on a task so a long `session/prompt` never blocks
            // the reader (which must see `session/cancel` meanwhile).
            let state = state.clone();
            tokio::spawn(async move {
                let response = match dispatch_request(&state, &method, &params).await {
                    Ok(result) => result_response(id, result),
                    Err(error) => {
                        error_response(id, error.code, error.message).with_error_data(error.data)
                    }
                };
                state.send(response);
            });
        }
        None => {
            let state = state.clone();
            tokio::spawn(async move {
                dispatch_notification(&state, &method, &params).await;
            });
        }
    }
}

async fn dispatch_request(
    state: &Arc<ServerState>,
    method: &str,
    params: &Value,
) -> Result<Value, RpcError> {
    match method {
        "initialize" => Ok(initialize_result()),
        "authenticate" => Ok(json!({})),
        "session/new" => session_new(state, params).await,
        "session/prompt" => session_prompt(state, params).await,
        "session/cancel" => {
            // ACP defines this as a notification; tolerate a request form.
            cancel_turn(state, params).await;
            Ok(json!({}))
        }
        "session/resume" => session_resume(state, params).await,
        "session/close" => session_close(state, params),
        "session/load" => Err(RpcError::with_data(
            METHOD_NOT_FOUND,
            "Method not found: session/load".to_string(),
            json!({"method": "session/load"}),
        )),
        "session/list"
        | "session/set_config_option"
        | "session/set_mode"
        | "session/fork"
        | "session/delete" => Err(RpcError::method_not_found(method)),
        refused if refused.starts_with("fs/") || refused.starts_with("terminal/") => {
            Err(RpcError::with_data(
                METHOD_NOT_FOUND,
                format!(
                    "refused: {refused}: file and terminal access in Goble sessions runs \
                     inside the gateway sandbox, never on the local machine"
                ),
                json!({"method": refused}),
            ))
        }
        other => Err(RpcError::method_not_found(other)),
    }
}

async fn dispatch_notification(state: &Arc<ServerState>, method: &str, params: &Value) {
    match method {
        "session/cancel" => cancel_turn(state, params).await,
        // `initialized` and friends are informational; notifications never get a
        // response, so unknown ones are ignored rather than rejected.
        _ => tracing::debug!("ignoring notification {method}"),
    }
}

/// The `initialize` result: advertise only what the gateway can actually serve.
fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "agentInfo": {"name": "goble-acp", "version": AGENT_VERSION},
        "agentCapabilities": {
            // No history replay: gateway runs are process-local and not
            // replayable, so `loadSession` stays false and `session/load` is
            // refused with -32601, exactly like `dsh --profile acp`.
            "loadSession": false,
            "promptCapabilities": {
                "text": true,
                "image": false,
                "audio": false,
                "embeddedContext": true,
            },
            "mcpCapabilities": {"http": false, "sse": false},
            "sessionCapabilities": {"close": {}, "resume": {}},
        },
        "authMethods": [],
    })
}

async fn session_new(state: &Arc<ServerState>, params: &Value) -> Result<Value, RpcError> {
    let cwd = required_string(params, "cwd")?;
    check_session_setup(state, params, &cwd)?;

    let acp_session_id = uuid::Uuid::new_v4().to_string();
    let name = match state.config.session_name.as_deref() {
        Some(fixed) => fixed.to_string(),
        None => default_session_name(&cwd, &acp_session_id),
    };
    let gateway_session = state
        .gateway
        .create_session(&name, state.config.worker.as_deref())
        .await?;

    let created_at_ms = now_millis();
    {
        state.lock_sessions().insert(
            acp_session_id.clone(),
            SessionEntry {
                gateway_session_id: gateway_session.id.clone(),
                name: name.clone(),
                cwd,
                created_at_ms,
                busy: None,
            },
        );
    }
    state.persist();
    tracing::info!(
        "session/new {acp_session_id} -> gateway session {} ({name})",
        gateway_session.id
    );
    Ok(json!({
        "sessionId": acp_session_id,
        "configOptions": [],
        "_meta": {"bridge": {
            "gatewaySessionId": gateway_session.id,
            "name": name,
        }},
    }))
}

async fn session_resume(state: &Arc<ServerState>, params: &Value) -> Result<Value, RpcError> {
    let acp_session_id = required_string(params, "sessionId")?;
    let (name, cwd) = {
        let sessions = state.lock_sessions();
        let entry = sessions
            .get(&acp_session_id)
            .ok_or_else(|| unknown_session(&acp_session_id))?;
        (entry.name.clone(), entry.cwd.clone())
    };
    // Re-attach to the durable gateway binding: sessions are created-or-reused
    // by name, so the same name returns the same gateway session.
    let gateway_session = state
        .gateway
        .create_session(&name, state.config.worker.as_deref())
        .await?;
    {
        let mut sessions = state.lock_sessions();
        if let Some(entry) = sessions.get_mut(&acp_session_id) {
            entry.gateway_session_id = gateway_session.id.clone();
            entry.cwd = cwd;
        }
    }
    state.persist();
    tracing::info!(
        "session/resume {acp_session_id} re-attached to gateway session {} ({name})",
        gateway_session.id
    );
    Ok(json!({"configOptions": []}))
}

fn session_close(state: &Arc<ServerState>, params: &Value) -> Result<Value, RpcError> {
    let acp_session_id = required_string(params, "sessionId")?;
    let removed = state.lock_sessions().remove(&acp_session_id);
    if removed.is_none() {
        return Err(unknown_session(&acp_session_id));
    }
    state.persist();
    tracing::info!("session/close {acp_session_id} (gateway binding kept)");
    Ok(json!({}))
}

async fn session_prompt(state: &Arc<ServerState>, params: &Value) -> Result<Value, RpcError> {
    let acp_session_id = required_string(params, "sessionId")?;
    let blocks = params
        .get("prompt")
        .ok_or_else(|| RpcError::invalid_params("missing \"prompt\" parameter"))?;
    let task = prompt::render_prompt(blocks)?;

    // Claim the session for this turn.
    let cancel = CancelToken::new();
    let gateway_session_id = {
        let mut sessions = state.lock_sessions();
        let entry = sessions
            .get_mut(&acp_session_id)
            .ok_or_else(|| unknown_session(&acp_session_id))?;
        if entry.busy.is_some() {
            return Err(RpcError::with_data(
                SERVER_ERROR_BUSY,
                format!("session busy: a prompt is already running for session {acp_session_id}"),
                json!({"sessionId": acp_session_id}),
            ));
        }
        entry.busy = Some(cancel.clone());
        entry.gateway_session_id.clone()
    };

    let outcome = run_turn(state, &acp_session_id, &gateway_session_id, &task, &cancel).await;

    // Release the session for the next prompt. Only clear the token this turn
    // installed; a future turn cannot have claimed it while busy was set, but
    // the identity check makes that explicit.
    {
        let mut sessions = state.lock_sessions();
        if let Some(entry) = sessions.get_mut(&acp_session_id) {
            if entry
                .busy
                .as_ref()
                .is_some_and(|token| token.is_same(&cancel))
            {
                entry.busy = None;
            }
        }
    }
    outcome
}

/// Dispatch one prompt as a gateway run, stream its events as ACP updates, and
/// settle the ACP turn.
async fn run_turn(
    state: &Arc<ServerState>,
    acp_session_id: &str,
    gateway_session_id: &str,
    task: &str,
    cancel: &CancelToken,
) -> Result<Value, RpcError> {
    let run = state
        .gateway
        .create_run(gateway_session_id, task, state.config.run_timeout_s)
        .await?;
    tracing::info!(
        "prompt {acp_session_id} -> run {} (gateway session {gateway_session_id})",
        run.id
    );

    if cancel.is_cancelled() {
        request_remote_cancel(state, gateway_session_id);
        return Ok(json!({"stopReason": "cancelled"}));
    }

    let mut mapper =
        UpdateMapper::new(acp_session_id, &run.id).with_gateway_session(gateway_session_id);
    let mut terminal: Option<RunTerminal> = None;
    let stream_result = state
        .gateway
        .stream_run(&run.id, 0, cancel, |event| {
            let mapped = mapper.map(event);
            // The mapper emits complete `session/update` notifications (the
            // envelope and session id included), so they are forwarded as-is.
            for notification in mapped.updates {
                state.send(notification);
            }
            match mapped.terminal {
                Some(reason) => {
                    terminal = Some(reason);
                    StreamControl::Stop
                }
                None => StreamControl::Continue,
            }
        })
        .await;

    if let Err(error) = stream_result {
        if cancel.is_cancelled() {
            return Ok(json!({"stopReason": "cancelled"}));
        }
        return Err(RpcError::from(error));
    }
    if let Some(reason) = terminal {
        return settle(reason, &run);
    }
    if cancel.is_cancelled() {
        request_remote_cancel(state, gateway_session_id);
        return Ok(json!({"stopReason": "cancelled"}));
    }

    // The SSE stream closed without a `final` frame (transport drop that
    // exhausted reconnects). Ask the run record instead of inventing a result.
    let final_run = state.gateway.get_run(&run.id).await?;
    match final_run.status() {
        RunStatus::Completed => Ok(json!({"stopReason": "end_turn"})),
        RunStatus::Cancelled => Ok(json!({"stopReason": "cancelled"})),
        RunStatus::Failed => Err(turn_failed(
            final_run
                .error_message()
                .unwrap_or_else(|| "run failed".to_string()),
            &run,
        )),
        RunStatus::Timeout => Err(turn_failed(
            final_run
                .error_message()
                .unwrap_or_else(|| "run timed out".to_string()),
            &run,
        )),
        status @ (RunStatus::Queued | RunStatus::Running) => Err(RpcError::with_data(
            INTERNAL_ERROR,
            format!(
                "gateway stream ended before run {} finished (status {})",
                run.id,
                status.as_str()
            ),
            json!({"runId": run.id}),
        )),
        RunStatus::Unknown(raw) => Err(RpcError::with_data(
            INTERNAL_ERROR,
            format!("run {} ended with unknown status {raw:?}", run.id),
            json!({"runId": run.id}),
        )),
    }
}

/// Turn a terminal run reason into the ACP `session/prompt` result or error.
fn settle(reason: RunTerminal, run: &GatewayRun) -> Result<Value, RpcError> {
    match reason {
        RunTerminal::Completed => Ok(json!({"stopReason": "end_turn"})),
        RunTerminal::Cancelled => Ok(json!({"stopReason": "cancelled"})),
        RunTerminal::Failed { message } => Err(turn_failed(message, run)),
    }
}

fn turn_failed(message: String, run: &GatewayRun) -> RpcError {
    RpcError::with_data(
        INTERNAL_ERROR,
        format!("turn failed: {message}"),
        json!({"runId": run.id}),
    )
}

/// `session/cancel`: stop forwarding updates and settle the ACP turn as
/// cancelled, then ask the gateway to cancel the run. The worker-side turn may
/// keep running in the sandbox until it finishes — documented, not hidden.
async fn cancel_turn(state: &Arc<ServerState>, params: &Value) {
    let Some(acp_session_id) = params.get("sessionId").and_then(Value::as_str) else {
        tracing::warn!("session/cancel without a sessionId; ignoring");
        return;
    };
    let (token, gateway_session_id) = {
        let sessions = state.lock_sessions();
        match sessions.get(acp_session_id) {
            Some(entry) => (entry.busy.clone(), entry.gateway_session_id.clone()),
            None => {
                tracing::warn!("session/cancel for unknown session {acp_session_id}");
                return;
            }
        }
    };
    match token {
        Some(token) => {
            tracing::info!("session/cancel {acp_session_id}: settling the turn as cancelled");
            token.cancel();
            request_remote_cancel(state, &gateway_session_id);
        }
        None => tracing::debug!("session/cancel {acp_session_id} with no prompt in flight"),
    }
}

/// Fire-and-forget gateway cancel. Errors are logged; the ACP turn is already
/// settling as cancelled locally.
fn request_remote_cancel(state: &Arc<ServerState>, gateway_session_id: &str) {
    let gateway = state.gateway.clone();
    let gateway_session_id = gateway_session_id.to_string();
    tokio::spawn(async move {
        match tokio::time::timeout(
            Duration::from_secs(5),
            gateway.cancel_session(&gateway_session_id),
        )
        .await
        {
            Ok(Ok(_)) => tracing::info!("gateway cancel accepted for {gateway_session_id}"),
            Ok(Err(error)) => tracing::warn!(
                "gateway cancel failed for {gateway_session_id}: {error} (the remote run may continue)"
            ),
            Err(_) => tracing::warn!(
                "gateway cancel timed out for {gateway_session_id} (the remote run may continue)"
            ),
        }
    });
}

/// Warn about (or, in strict mode, refuse) `session/new` fields the gateway
/// cannot mount: MCP servers run inside the sandbox image, not per ACP session.
fn check_session_setup(
    state: &Arc<ServerState>,
    params: &Value,
    _cwd: &str,
) -> Result<(), RpcError> {
    let mut ignored = Vec::new();
    if let Some(servers) = params.get("mcpServers") {
        let servers = servers
            .as_array()
            .ok_or_else(|| RpcError::invalid_params("\"mcpServers\" must be an array"))?;
        if !servers.is_empty() {
            ignored.push(format!(
                "{} mcpServers entr{}",
                servers.len(),
                if servers.len() == 1 { "y" } else { "ies" }
            ));
        }
    }
    for key in ["additionalDirectories", "additional_directories"] {
        if let Some(directories) = params.get(key) {
            let directories = directories
                .as_array()
                .ok_or_else(|| RpcError::invalid_params(format!("\"{key}\" must be an array")))?;
            if !directories.is_empty() {
                ignored.push(key.to_string());
            }
        }
    }
    if ignored.is_empty() {
        return Ok(());
    }
    let summary = ignored.join(", ");
    if state.config.strict_setup {
        return Err(RpcError::invalid_params(format!(
            "refused: the gateway cannot mount {summary}; MCP servers and extra directories \
             are configured in the sandbox image, not per ACP session"
        )));
    }
    tracing::warn!(
        "session/new: ignoring {summary}: the gateway cannot mount them; MCP tools are \
         configured in the sandbox image"
    );
    Ok(())
}

fn required_string(params: &Value, key: &str) -> Result<String, RpcError> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| RpcError::invalid_params(format!("missing or empty \"{key}\" parameter")))
}

fn unknown_session(acp_session_id: &str) -> RpcError {
    RpcError::with_data(
        SERVER_ERROR_UNKNOWN_SESSION,
        format!("unknown session: {acp_session_id}"),
        json!({"sessionId": acp_session_id}),
    )
}

/// Derive the gateway session name from the ACP cwd, mirroring the reference
/// bridge: `acp/<cwd basename>/<acp session id>`.
fn default_session_name(cwd: &str, acp_session_id: &str) -> String {
    let basename = std::path::Path::new(cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("session");
    let safe: String = basename
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect();
    let safe = safe.trim_matches('-');
    let safe = if safe.is_empty() { "session" } else { safe };
    format!("acp/{safe}/{acp_session_id}")
}

fn result_response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error_response(id: Value, code: i64, message: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message},
    })
}

trait WithErrorData {
    fn with_error_data(self, data: Option<Value>) -> Self;
}

impl WithErrorData for Value {
    fn with_error_data(mut self, data: Option<Value>) -> Self {
        if let Some(data) = data {
            if let Some(error) = self.get_mut("error").and_then(Value::as_object_mut) {
                error.insert("data".to_string(), data);
            }
        }
        self
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn load_persisted(config: &AcpConfig) -> Vec<PersistedSession> {
    let Some(path) = config.sessions_path() else {
        return Vec::new();
    };
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<PersistedState>(&bytes) {
            Ok(state) => state.sessions,
            Err(error) => {
                tracing::warn!(
                    "ignoring unreadable ACP session state at {}: {error}",
                    path.display()
                );
                Vec::new()
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            tracing::warn!(
                "could not read ACP session state at {}: {error}",
                path.display()
            );
            Vec::new()
        }
    }
}

fn write_state_file(path: &std::path::Path, state: &PersistedState) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_advertises_only_gateway_served_capabilities() {
        let result = initialize_result();
        assert_eq!(result["protocolVersion"], json!(1));
        assert_eq!(result["agentInfo"]["name"], json!("goble-acp"));
        assert_eq!(result["agentCapabilities"]["loadSession"], json!(false));
        assert_eq!(
            result["agentCapabilities"]["promptCapabilities"],
            json!({"text": true, "image": false, "audio": false, "embeddedContext": true})
        );
        assert_eq!(result["authMethods"], json!([]));
    }

    #[test]
    fn session_names_follow_the_reference_scheme() {
        assert_eq!(
            default_session_name("/Users/me/projects/goble", "abc-123"),
            "acp/goble/abc-123"
        );
        assert_eq!(default_session_name("/", "abc-123"), "acp/session/abc-123");
        assert_eq!(
            default_session_name("/tmp/with space/na me", "id"),
            "acp/na-me/id"
        );
    }

    #[test]
    fn error_frames_have_the_jsonrpc_shape() {
        let frame = error_response(json!(7), INVALID_PARAMS, "bad params".into())
            .with_error_data(Some(json!({"field": "cwd"})));
        assert_eq!(
            frame,
            json!({
                "jsonrpc": "2.0",
                "id": 7,
                "error": {
                    "code": -32602,
                    "message": "bad params",
                    "data": {"field": "cwd"},
                },
            })
        );
    }

    #[test]
    fn gateway_errors_map_to_the_agreed_codes() {
        let busy: RpcError = GatewayError::Conflict("session_busy".into()).into();
        assert_eq!(busy.code, -32000);
        assert!(busy.message.starts_with("session busy:"));
        let missing: RpcError = GatewayError::NotFound("unknown_session".into()).into();
        assert_eq!(missing.code, -32002);
        let transport: RpcError = GatewayError::Transport("connection refused".into()).into();
        assert_eq!(transport.code, -32603);
        assert!(transport.message.starts_with("gateway unreachable:"));
    }

    #[test]
    fn prompt_errors_are_invalid_params() {
        let error: RpcError = PromptError::Empty.into();
        assert_eq!(error.code, -32602);
        assert!(error.message.contains("prompt is empty"));
    }

    #[test]
    fn unsupported_filesystem_methods_are_refused_with_an_explanation() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let state = test_state();
            let error = dispatch_request(
                &state,
                "fs/read_text_file",
                &json!({"sessionId": "s", "path": "/etc/passwd"}),
            )
            .await
            .expect_err("fs methods are refused");
            assert_eq!(error.code, METHOD_NOT_FOUND);
            assert!(error.message.contains("sandbox"));
        });
    }

    #[test]
    fn unknown_methods_and_session_load_are_method_not_found() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let state = test_state();
            let load = dispatch_request(&state, "session/load", &json!({}))
                .await
                .expect_err("load is refused");
            assert_eq!(load.code, METHOD_NOT_FOUND);
            assert_eq!(load.data, Some(json!({"method": "session/load"})));

            let unknown = dispatch_request(&state, "session/fork", &json!({}))
                .await
                .expect_err("fork is refused");
            assert_eq!(unknown.code, METHOD_NOT_FOUND);
        });
    }

    #[test]
    fn prompts_for_unknown_sessions_are_rejected() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let state = test_state();
            let error = session_prompt(
                &state,
                &json!({
                    "sessionId": "nope",
                    "prompt": [{"type": "text", "text": "hi"}],
                }),
            )
            .await
            .expect_err("unknown session");
            assert_eq!(error.code, SERVER_ERROR_UNKNOWN_SESSION);
        });
    }

    fn test_state() -> Arc<ServerState> {
        let config = AcpConfig {
            state_dir: None,
            ..AcpConfig::default()
        };
        let gateway = GatewayClient::new(&config).expect("client builds without network");
        let (out, _rx) = mpsc::unbounded_channel();
        Arc::new(ServerState {
            config,
            gateway: Arc::new(gateway),
            sessions: Mutex::new(HashMap::new()),
            out,
        })
    }
}
