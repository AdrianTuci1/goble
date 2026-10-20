//! The MCP protocol state machine and its control backend.
//!
//! [`McpServer`] is generic over [`Control`] so tests can use a mock while the
//! binary uses [`SocketControl`], which opens one control-socket connection per
//! call. [`serve`] is the stdio loop: one JSON object per line in, one per line
//! out, with notifications left unanswered.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use goble_control::{Client, Command, ControlError, Payload, MAX_MESSAGE_BYTES};
use serde_json::{json, Value};

use crate::jsonrpc::{
    self, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR,
};
use crate::tools;
use crate::{DEFAULT_PROTOCOL_VERSION, SERVER_VERSION, SUPPORTED_PROTOCOL_VERSIONS};

/// How long one control call may wait for the app before the tool reports an
/// error. The app's own UI wait is shorter, so a wedged window is named as
/// unavailable rather than hanging the MCP client.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(45);

/// The control operations an MCP tool call needs.
pub trait Control: Send {
    fn call(&mut self, command: Command) -> Result<Payload, ControlError>;
}

/// A [`Control`] backed by the app's Unix control socket.
#[derive(Debug, Clone)]
pub struct SocketControl {
    socket: PathBuf,
}

impl SocketControl {
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
        }
    }

    pub fn default_socket() -> Self {
        Self::new(goble_control::resolve_socket_path())
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket
    }
}

impl Control for SocketControl {
    fn call(&mut self, command: Command) -> Result<Payload, ControlError> {
        let mut client = Client::connect(&self.socket)?;
        client.set_read_timeout(Some(CONTROL_TIMEOUT))?;
        client.call(command)
    }
}

/// The MCP server. One instance serves one stdio session.
#[derive(Debug)]
pub struct McpServer<C: Control> {
    control: C,
    protocol_version: &'static str,
    initialized: bool,
}

impl<C: Control> McpServer<C> {
    pub fn new(control: C) -> Self {
        Self {
            control,
            protocol_version: DEFAULT_PROTOCOL_VERSION,
            initialized: false,
        }
    }

    /// The protocol revision this session negotiated.
    pub fn protocol_version(&self) -> &'static str {
        self.protocol_version
    }

    /// Whether the client sent `notifications/initialized`.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Handle one decoded JSON-RPC message. `None` means the message was a
    /// notification (or a response) and must not be answered.
    pub fn handle_message(&mut self, message: Value) -> Option<Value> {
        let Some(object) = message.as_object() else {
            return Some(jsonrpc::error(
                Value::Null,
                INVALID_REQUEST,
                "a JSON-RPC message must be an object",
            ));
        };

        let id = object.get("id").cloned();
        let method = match object.get("method") {
            Some(value) => match value.as_str() {
                Some(method) => method,
                None => {
                    return Some(jsonrpc::error(
                        id.unwrap_or(Value::Null),
                        INVALID_REQUEST,
                        "`method` must be a string",
                    ))
                }
            },
            None => {
                // A response (from an earlier server request) is legal on the
                // wire and gets no reply. Anything else with no method is not.
                if object.contains_key("result") || object.contains_key("error") {
                    return None;
                }
                return Some(jsonrpc::error(
                    id.unwrap_or(Value::Null),
                    INVALID_REQUEST,
                    "missing `method`",
                ));
            }
        };

        let Some(id) = id else {
            self.handle_notification(method);
            return None;
        };

        let params = object.get("params").cloned().unwrap_or(Value::Null);
        match method {
            "initialize" => Some(self.initialize(id, params)),
            "ping" => Some(jsonrpc::result(id, json!({}))),
            "tools/list" => Some(jsonrpc::result(id, tools::tool_definitions())),
            "tools/call" => Some(self.call_tool(id, params)),
            other => Some(jsonrpc::error(
                id,
                METHOD_NOT_FOUND,
                format!("unknown method `{other}`"),
            )),
        }
    }

    fn handle_notification(&mut self, method: &str) {
        if method == "notifications/initialized" {
            self.initialized = true;
        }
    }

    fn initialize(&mut self, id: Value, params: Value) -> Value {
        let requested = params.get("protocolVersion").and_then(Value::as_str);
        self.protocol_version = requested
            .and_then(|version| {
                SUPPORTED_PROTOCOL_VERSIONS
                    .iter()
                    .copied()
                    .find(|supported| *supported == version)
            })
            .unwrap_or(DEFAULT_PROTOCOL_VERSION);

        jsonrpc::result(
            id,
            json!({
                "protocolVersion": self.protocol_version,
                "capabilities": {
                    "tools": { "listChanged": false }
                },
                "serverInfo": {
                    "name": "goble-mcp",
                    "version": SERVER_VERSION
                },
                "instructions": "Control the running Goble window: panes, terminal input, agent state, the inbox, spaces, and layouts."
            }),
        )
    }

    fn call_tool(&mut self, id: Value, params: Value) -> Value {
        let Some(params) = params.as_object() else {
            return jsonrpc::error(id, INVALID_PARAMS, "`params` must be an object");
        };
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return jsonrpc::error(id, INVALID_PARAMS, "`params.name` must be a string");
        };
        let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));

        let command = match tools::command_for(name, arguments) {
            Ok(command) => command,
            Err(error) => return jsonrpc::error(id, INVALID_PARAMS, error.to_string()),
        };

        match self.control.call(command) {
            Ok(payload) => match serde_json::to_string(&payload) {
                Ok(text) => jsonrpc::result(id, tool_result(text, false)),
                Err(error) => jsonrpc::error(
                    id,
                    jsonrpc::INTERNAL_ERROR,
                    format!("could not encode the control result: {error}"),
                ),
            },
            Err(error) => {
                let text = serde_json::to_string(&json!({ "error": error }))
                    .unwrap_or_else(|_| "{\"error\":{\"code\":\"internal\"}}".to_string());
                jsonrpc::result(id, tool_result(text, true))
            }
        }
    }

    /// Serve `reader`/`writer` until EOF. Blank lines are ignored.
    pub fn serve<R: BufRead, W: Write>(
        &mut self,
        mut reader: R,
        mut writer: W,
    ) -> io::Result<()> {
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = reader.read_until(b'\n', &mut line)?;
            if read == 0 {
                break;
            }
            while matches!(line.last(), Some(b'\n' | b'\r')) {
                line.pop();
            }
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            if line.len() > MAX_MESSAGE_BYTES {
                write_response(
                    &mut writer,
                    &jsonrpc::error(
                        Value::Null,
                        PARSE_ERROR,
                        "message exceeds the size limit",
                    ),
                )?;
                continue;
            }
            let response = match serde_json::from_slice::<Value>(&line) {
                Ok(message) => self.handle_message(message),
                Err(error) => Some(jsonrpc::error(
                    Value::Null,
                    PARSE_ERROR,
                    format!("invalid JSON: {error}"),
                )),
            };
            if let Some(response) = response {
                write_response(&mut writer, &response)?;
            }
        }
        Ok(())
    }
}

fn tool_result(text: String, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    })
}

fn write_response<W: Write>(writer: &mut W, response: &Value) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, response)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// Run one MCP session over `reader`/`writer`.
pub fn serve<R: BufRead, W: Write>(
    control: impl Control,
    reader: R,
    writer: W,
) -> io::Result<()> {
    McpServer::new(control).serve(reader, writer)
}
