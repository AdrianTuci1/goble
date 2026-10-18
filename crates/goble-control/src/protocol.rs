//! The control protocol's wire types.
//!
//! One JSON object per line in both directions. A request is
//! `{"id": 7, "verb": "panes.send", "params": {...}}`; a response is
//! `{"id": 7, "result": {"kind": "sent", ...}}` or
//! `{"id": 7, "error": {"code": "missing_pane", "message": "..."}}`.
//!
//! [`Command`] is the typed form of a request: callers build one, the client
//! turns it into a [`Request`], and the server turns a request back into a
//! command. An unknown verb becomes a structured [`ErrorCode::UnknownVerb`],
//! never a panic or a dropped connection.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

/// The largest accepted message, checked before parsing and before writing.
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

/// Every verb the protocol knows, in the order the product surface lists them.
pub const VERBS: &[&str] = &[
    "panes.list",
    "panes.new",
    "panes.split",
    "panes.close",
    "panes.focus",
    "panes.send",
    "panes.resize",
    "spaces.list",
    "spaces.switch",
    "agents.state",
    "agents.prompt",
    "inbox.list",
    "inbox.ack",
    "layout.get",
    "layout.set",
    "pty.list",
    "pty.attach",
    "pty.detach",
    "pty.kill",
];

/// The structured error every failure travels back as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[error("{code:?}: {message}")]
pub struct ControlError {
    pub code: ErrorCode,
    pub message: String,
}

/// The machine-readable half of [`ControlError`]. Callers branch on this;
/// `message` is for the human.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The verb is not in [`VERBS`].
    UnknownVerb,
    /// A known verb's parameters could not be parsed or are out of range.
    InvalidRequest,
    /// The request names a pane that is not open.
    MissingPane,
    /// The request names a space that is not open.
    MissingSpace,
    /// The request names an inbox item that is no longer there.
    MissingItem,
    /// The verb is valid but this app state cannot perform it.
    Unsupported,
    /// The PTY daemon could not be reached or refused the operation.
    PtyUnavailable,
    /// The app is not running, is shutting down, or did not answer in time.
    Unavailable,
    /// The control socket itself failed.
    Io,
    /// A bug the handler hit; the app keeps running.
    Internal,
}

impl ControlError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn unknown_verb(verb: &str) -> Self {
        Self::new(
            ErrorCode::UnknownVerb,
            format!("unknown control verb `{verb}`"),
        )
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidRequest, message)
    }

    pub fn missing_pane(pane_id: u64) -> Self {
        Self::new(ErrorCode::MissingPane, format!("pane {pane_id} is not open"))
    }

    pub fn missing_space(space_id: usize) -> Self {
        Self::new(
            ErrorCode::MissingSpace,
            format!("space {space_id} is not open"),
        )
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unavailable, message)
    }

    pub fn unavailable_detail(message: impl std::fmt::Display) -> Self {
        Self::new(ErrorCode::Unavailable, message.to_string())
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Io, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
}

/// A raw request as it appears on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub verb: String,
    #[serde(default)]
    pub params: Value,
}

impl Request {
    /// Build the wire request for a typed command.
    pub fn from_command(id: u64, command: &Command) -> Result<Self, ControlError> {
        let value = serde_json::to_value(command)
            .map_err(|e| ControlError::internal(format!("encode `{}`: {e}", command.verb())))?;
        let mut object = match value {
            Value::Object(object) => object,
            _ => {
                return Err(ControlError::internal(
                    "a command did not encode as a JSON object",
                ))
            }
        };
        let verb = object
            .remove("verb")
            .and_then(|value| value.as_str().map(str::to_string))
            .ok_or_else(|| ControlError::internal("a command encoded without a verb"))?;
        let params = object.remove("params").unwrap_or(Value::Null);
        Ok(Self { id, verb, params })
    }

    /// Parse the typed command this request names.
    pub fn command(&self) -> Result<Command, ControlError> {
        if !is_known_verb(&self.verb) {
            return Err(ControlError::unknown_verb(&self.verb));
        }
        let mut object = Map::new();
        object.insert("verb".to_string(), Value::String(self.verb.clone()));
        if !self.params.is_null() {
            object.insert("params".to_string(), self.params.clone());
        }
        serde_json::from_value(Value::Object(object)).map_err(|e| {
            ControlError::invalid_request(format!(
                "invalid parameters for `{}`: {e}",
                self.verb
            ))
        })
    }
}

/// Whether `verb` is one of [`VERBS`].
pub fn is_known_verb(verb: &str) -> bool {
    VERBS.contains(&verb)
}

/// A response as it appears on the wire. Exactly one of `result` and `error`
/// is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Payload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ControlError>,
}

impl Response {
    pub fn ok(id: u64, result: Payload) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: u64, error: ControlError) -> Self {
        Self {
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// Which way a pane tree splits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitAxis {
    /// First pane on the left, second on the right.
    Horizontal,
    /// First pane on top, second below.
    Vertical,
}

/// The kind of pane a `panes.new` or `panes.split` creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneKind {
    Chat,
    Terminal,
}

/// A pane-tree orientation, as `layout.get` reports and `layout.set` applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutKind {
    /// One leaf; `layout.set` closes every other pane in the space.
    #[serde(alias = "focus")]
    Single,
    /// Every split runs left/right.
    #[serde(alias = "columns", alias = "row", alias = "split_horizontal")]
    Horizontal,
    /// Every split runs top/bottom.
    #[serde(alias = "rows", alias = "column", alias = "split_vertical")]
    Vertical,
    /// The binary split tree, keeping the orientation its splits already have.
    #[serde(alias = "binary", alias = "tree")]
    Bsp,
    /// The first pane takes ~60% of the width; the rest stack on the right.
    MasterStack,
    /// Every pane has a rect of its own, with move/resize and a z-order.
    Floating,
}

/// The typed verbs. Parameters are the fields of each variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verb", content = "params", rename_all = "snake_case")]
pub enum Command {
    #[serde(rename = "panes.list")]
    PanesList,

    #[serde(rename = "panes.new")]
    PanesNew {
        #[serde(default)]
        space_id: Option<usize>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        kind: Option<PaneKind>,
    },

    #[serde(rename = "panes.split")]
    PanesSplit { pane_id: u64, axis: SplitAxis },

    #[serde(rename = "panes.close")]
    PanesClose { pane_id: u64 },

    #[serde(rename = "panes.focus")]
    PanesFocus { pane_id: u64 },

    #[serde(rename = "panes.send")]
    PanesSend {
        pane_id: u64,
        text: String,
        /// Whether to press Enter after the text. Off unless asked: a send is
        /// not assumed to run a command.
        #[serde(default)]
        enter: bool,
    },

    #[serde(rename = "panes.resize")]
    PanesResize { pane_id: u64, cols: u16, rows: u16 },

    #[serde(rename = "spaces.list")]
    SpacesList,

    #[serde(rename = "spaces.switch")]
    SpacesSwitch { space_id: usize },

    #[serde(rename = "agents.state")]
    AgentsState {
        #[serde(default)]
        pane_id: Option<u64>,
    },

    #[serde(rename = "agents.prompt")]
    AgentsPrompt { pane_id: u64, text: String },

    #[serde(rename = "inbox.list")]
    InboxList,

    #[serde(rename = "inbox.ack")]
    InboxAck {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        all: bool,
    },

    #[serde(rename = "layout.get")]
    LayoutGet { space_id: usize },

    #[serde(rename = "layout.set")]
    LayoutSet {
        space_id: usize,
        kind: LayoutKind,
    },

    #[serde(rename = "pty.list")]
    PtyList,

    #[serde(rename = "pty.attach")]
    PtyAttach { session_id: String },

    #[serde(rename = "pty.detach")]
    PtyDetach { session_id: String },

    #[serde(rename = "pty.kill")]
    PtyKill { session_id: String },
}

impl Command {
    /// The wire verb this command names.
    pub fn verb(&self) -> &'static str {
        match self {
            Command::PanesList => "panes.list",
            Command::PanesNew { .. } => "panes.new",
            Command::PanesSplit { .. } => "panes.split",
            Command::PanesClose { .. } => "panes.close",
            Command::PanesFocus { .. } => "panes.focus",
            Command::PanesSend { .. } => "panes.send",
            Command::PanesResize { .. } => "panes.resize",
            Command::SpacesList => "spaces.list",
            Command::SpacesSwitch { .. } => "spaces.switch",
            Command::AgentsState { .. } => "agents.state",
            Command::AgentsPrompt { .. } => "agents.prompt",
            Command::InboxList => "inbox.list",
            Command::InboxAck { .. } => "inbox.ack",
            Command::LayoutGet { .. } => "layout.get",
            Command::LayoutSet { .. } => "layout.set",
            Command::PtyList => "pty.list",
            Command::PtyAttach { .. } => "pty.attach",
            Command::PtyDetach { .. } => "pty.detach",
            Command::PtyKill { .. } => "pty.kill",
        }
    }

    /// Whether this command belongs to the PTY daemon rather than to the app's
    /// UI state. The app's handler uses this to route `pty.*` off the winit
    /// thread.
    pub fn is_pty(&self) -> bool {
        matches!(
            self,
            Command::PtyList
                | Command::PtyAttach { .. }
                | Command::PtyDetach { .. }
                | Command::PtyKill { .. }
        )
    }
}

/// One leaf pane as the control surface reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneInfo {
    pub pane_id: u64,
    pub space_id: usize,
    pub kind: String,
    pub title: String,
    pub cwd: Option<String>,
    pub focused: bool,
    pub active_space: bool,
}

/// One workspace tab as the control surface reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceInfo {
    pub space_id: usize,
    pub name: String,
    pub medium: String,
    pub pane_count: usize,
    pub layout: LayoutKind,
    pub active: bool,
}

/// One pane's agent state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStateInfo {
    pub pane_id: u64,
    pub state: String,
    pub busy: bool,
    pub conversation_id: Option<String>,
}

/// One global inbox item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxInfo {
    pub id: String,
    pub pane_id: u64,
    pub space_id: usize,
    pub conversation_id: String,
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub created_at: String,
    pub acknowledged: bool,
}

/// A pane tree node, as `layout.get` reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LayoutNode {
    Leaf {
        pane_id: u64,
        kind: String,
    },
    Split {
        split_id: u64,
        axis: SplitAxis,
        ratio: f32,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

/// One space's layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutInfo {
    pub space_id: usize,
    pub kind: LayoutKind,
    pub active_pane_id: u64,
    pub root: LayoutNode,
}

/// One PTY daemon session, as `pty.list` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtySessionInfo {
    pub session_id: String,
    pub pane_id: String,
    pub alive: bool,
    pub cols: u16,
    pub rows: u16,
}

/// A successful `panes.send`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendResult {
    pub pane_id: u64,
    pub bytes: usize,
    pub entered: bool,
}

/// A successful `agents.prompt`. `started` is false when no runnable model is
/// configured; that is an answer, not an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptResult {
    pub pane_id: u64,
    pub started: bool,
}

/// A successful `inbox.ack`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AckResult {
    pub acknowledged: usize,
}

/// The typed result half of a [`Response`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Payload {
    Panes { panes: Vec<PaneInfo> },
    Pane { pane: PaneInfo },
    Closed { pane_id: u64 },
    Spaces { spaces: Vec<SpaceInfo> },
    Space { space: SpaceInfo },
    AgentStates { agents: Vec<AgentStateInfo> },
    Prompt { prompt: PromptResult },
    Inbox { items: Vec<InboxInfo> },
    Acknowledged { acknowledged: usize },
    Layout { layout: LayoutInfo },
    PtySessions { sessions: Vec<PtySessionInfo> },
    PtySession { session: PtySessionInfo },
    Sent { send: SendResult },
    Ok,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commands() -> Vec<Command> {
        vec![
            Command::PanesList,
            Command::PanesNew {
                space_id: Some(0),
                cwd: Some("/tmp".into()),
                kind: Some(PaneKind::Terminal),
            },
            Command::PanesSplit {
                pane_id: 2,
                axis: SplitAxis::Vertical,
            },
            Command::PanesClose { pane_id: 3 },
            Command::PanesFocus { pane_id: 4 },
            Command::PanesSend {
                pane_id: 5,
                text: "echo hi".into(),
                enter: true,
            },
            Command::PanesResize {
                pane_id: 6,
                cols: 120,
                rows: 40,
            },
            Command::SpacesList,
            Command::SpacesSwitch { space_id: 1 },
            Command::AgentsState { pane_id: Some(7) },
            Command::AgentsPrompt {
                pane_id: 8,
                text: "ship it".into(),
            },
            Command::InboxList,
            Command::InboxAck {
                id: Some("i1".into()),
                all: false,
            },
            Command::LayoutGet { space_id: 0 },
            Command::LayoutSet {
                space_id: 0,
                kind: LayoutKind::Horizontal,
            },
            Command::PtyList,
            Command::PtyAttach {
                session_id: "s1".into(),
            },
            Command::PtyDetach {
                session_id: "s1".into(),
            },
            Command::PtyKill {
                session_id: "s1".into(),
            },
        ]
    }

    #[test]
    fn every_command_round_trips_through_the_wire() {
        for command in commands() {
            let request = Request::from_command(9, &command).unwrap();
            assert_eq!(request.verb, command.verb());
            let decoded = request.command().unwrap();
            assert_eq!(decoded, command, "round trip for {}", command.verb());
            assert_eq!(Request::from_command(9, &decoded).unwrap(), request);
        }
    }

    #[test]
    fn an_unknown_verb_is_a_structured_error() {
        let request = Request {
            id: 1,
            verb: "panes.dance".into(),
            params: Value::Null,
        };
        let error = request.command().unwrap_err();
        assert_eq!(error.code, ErrorCode::UnknownVerb);
        assert!(error.message.contains("panes.dance"));
    }

    #[test]
    fn missing_parameters_are_a_structured_error() {
        let request = Request {
            id: 1,
            verb: "panes.split".into(),
            params: serde_json::json!({ "pane_id": 3 }),
        };
        let error = request.command().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
    }

    #[test]
    fn a_response_round_trips_with_a_result_or_an_error() {
        let ok = Response::ok(
            4,
            Payload::Sent {
                send: SendResult {
                    pane_id: 1,
                    bytes: 3,
                    entered: true,
                },
            },
        );
        let text = serde_json::to_string(&ok).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&text).unwrap(), ok);

        let failed = Response::error(5, ControlError::missing_pane(9));
        let text = serde_json::to_string(&failed).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&text).unwrap(), failed);
        assert!(text.contains("\"missing_pane\""));
    }

    #[test]
    fn every_verb_has_a_command() {
        for verb in VERBS {
            assert!(
                commands().iter().any(|command| command.verb() == *verb),
                "no command builds {verb}"
            );
        }
    }
}
