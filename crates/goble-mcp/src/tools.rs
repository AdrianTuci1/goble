//! The MCP tool surface over the control protocol.
//!
//! Every tool is one typed [`Command`]; the server owns execution and this
//! module owns the names, the JSON schemas an MCP client reads, and argument
//! decoding. Arguments are strict (`deny_unknown_fields`) so a typo is an
//! `InvalidParams` error rather than a silently ignored option.

use std::fmt;

use goble_control::{Command, LayoutKind, PaneKind, SplitAxis};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

/// Every tool this server exposes, in `tools/list` order.
pub const TOOL_NAMES: &[&str] = &[
    "goble_list_panes",
    "goble_new_pane",
    "goble_split_pane",
    "goble_close_pane",
    "goble_send_input",
    "goble_agent_state",
    "goble_inbox",
    "goble_ack_inbox",
    "goble_switch_space",
    "goble_layout",
];

/// Why a `tools/call` could not become a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    /// No tool has this name.
    UnknownTool(String),
    /// The tool exists but its arguments were missing or malformed.
    InvalidArguments(String),
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTool(name) => write!(f, "unknown tool `{name}`"),
            Self::InvalidArguments(message) => write!(f, "invalid arguments: {message}"),
        }
    }
}

/// The `tools/list` result: `{ "tools": [...] }`.
pub fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": "goble_list_panes",
                "description": "List every open pane, with its space, kind, title, cwd, and focus.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }
            },
            {
                "name": "goble_new_pane",
                "description": "Open a new pane in a space. Defaults to the active space, a terminal, and the pane's current directory.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "space_id": { "type": "integer", "minimum": 0, "description": "Workspace tab index; defaults to the active space." },
                        "cwd": { "type": "string", "description": "Working directory for a terminal pane." },
                        "kind": { "type": "string", "enum": ["chat", "terminal"], "description": "Pane kind; defaults to terminal." }
                    },
                    "additionalProperties": false
                }
            },
            {
                "name": "goble_split_pane",
                "description": "Split a pane and return the new pane. Defaults to a horizontal split (side by side).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pane_id": { "type": "integer", "minimum": 0 },
                        "axis": { "type": "string", "enum": ["horizontal", "vertical"], "description": "horizontal is left/right; vertical is top/bottom. Defaults to horizontal." }
                    },
                    "required": ["pane_id"],
                    "additionalProperties": false
                }
            },
            {
                "name": "goble_close_pane",
                "description": "Close a pane. The last pane in a space cannot be closed.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pane_id": { "type": "integer", "minimum": 0 }
                    },
                    "required": ["pane_id"],
                    "additionalProperties": false
                }
            },
            {
                "name": "goble_send_input",
                "description": "Write text to a pane's terminal. Press Enter only when `enter` is true.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pane_id": { "type": "integer", "minimum": 0 },
                        "text": { "type": "string" },
                        "enter": { "type": "boolean", "description": "Press Enter after the text; defaults to false." }
                    },
                    "required": ["pane_id", "text"],
                    "additionalProperties": false
                }
            },
            {
                "name": "goble_agent_state",
                "description": "Report each pane's agent state, or one pane when `pane_id` is given.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pane_id": { "type": "integer", "minimum": 0 }
                    },
                    "additionalProperties": false
                }
            },
            {
                "name": "goble_inbox",
                "description": "List the global agent inbox (finished, failed, and waiting items).",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }
            },
            {
                "name": "goble_ack_inbox",
                "description": "Acknowledge one inbox item by id, or every item with `all`.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Inbox item id." },
                        "all": { "type": "boolean", "description": "Acknowledge every item." }
                    },
                    "additionalProperties": false
                }
            },
            {
                "name": "goble_switch_space",
                "description": "Switch the window to a workspace tab.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "space_id": { "type": "integer", "minimum": 0 }
                    },
                    "required": ["space_id"],
                    "additionalProperties": false
                }
            },
            {
                "name": "goble_layout",
                "description": "Read a space's pane layout, or set its layout with `kind`.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "space_id": { "type": "integer", "minimum": 0 },
                        "kind": { "type": "string", "enum": ["single", "horizontal", "vertical", "bsp", "master_stack", "floating"], "description": "When omitted the layout is read; when set it is applied. `single`/`horizontal`/`vertical` orient the BSP tree; `bsp`, `master_stack` and `floating` select a window-manager layout." }
                    },
                    "required": ["space_id"],
                    "additionalProperties": false
                }
            }
        ]
    })
}

/// Decode `arguments` into the command `name` names.
pub fn command_for(name: &str, arguments: Value) -> Result<Command, ToolError> {
    let arguments = if arguments.is_null() {
        json!({})
    } else {
        arguments
    };
    match name {
        "goble_list_panes" => {
            parse::<Empty>(arguments)?;
            Ok(Command::PanesList)
        }
        "goble_new_pane" => {
            let args: NewPaneArgs = parse(arguments)?;
            Ok(Command::PanesNew {
                space_id: args.space_id,
                cwd: args.cwd,
                kind: args.kind,
            })
        }
        "goble_split_pane" => {
            let args: SplitPaneArgs = parse(arguments)?;
            Ok(Command::PanesSplit {
                pane_id: args.pane_id,
                axis: args.axis.unwrap_or(SplitAxis::Horizontal),
            })
        }
        "goble_close_pane" => {
            let args: ClosePaneArgs = parse(arguments)?;
            Ok(Command::PanesClose {
                pane_id: args.pane_id,
            })
        }
        "goble_send_input" => {
            let args: SendInputArgs = parse(arguments)?;
            Ok(Command::PanesSend {
                pane_id: args.pane_id,
                text: args.text,
                enter: args.enter,
            })
        }
        "goble_agent_state" => {
            let args: AgentStateArgs = parse(arguments)?;
            Ok(Command::AgentsState {
                pane_id: args.pane_id,
            })
        }
        "goble_inbox" => {
            parse::<Empty>(arguments)?;
            Ok(Command::InboxList)
        }
        "goble_ack_inbox" => {
            let args: AckInboxArgs = parse(arguments)?;
            if args.id.is_none() && !args.all {
                return Err(ToolError::InvalidArguments(
                    "give either `id` or `all: true`".to_string(),
                ));
            }
            Ok(Command::InboxAck {
                id: args.id,
                all: args.all,
            })
        }
        "goble_switch_space" => {
            let args: SwitchSpaceArgs = parse(arguments)?;
            Ok(Command::SpacesSwitch {
                space_id: args.space_id,
            })
        }
        "goble_layout" => {
            let args: LayoutArgs = parse(arguments)?;
            Ok(match args.kind {
                Some(kind) => Command::LayoutSet {
                    space_id: args.space_id,
                    kind,
                },
                None => Command::LayoutGet {
                    space_id: args.space_id,
                },
            })
        }
        other => Err(ToolError::UnknownTool(other.to_string())),
    }
}

fn parse<T: DeserializeOwned>(arguments: Value) -> Result<T, ToolError> {
    serde_json::from_value(arguments)
        .map_err(|error| ToolError::InvalidArguments(error.to_string()))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewPaneArgs {
    #[serde(default)]
    space_id: Option<usize>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    kind: Option<PaneKind>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SplitPaneArgs {
    pane_id: u64,
    #[serde(default)]
    axis: Option<SplitAxis>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClosePaneArgs {
    pane_id: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SendInputArgs {
    pane_id: u64,
    text: String,
    #[serde(default)]
    enter: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentStateArgs {
    #[serde(default)]
    pane_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AckInboxArgs {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    all: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SwitchSpaceArgs {
    space_id: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutArgs {
    space_id: usize,
    #[serde(default)]
    kind: Option<LayoutKind>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_advertised_tool_decodes() {
        let cases = [
            ("goble_list_panes", json!({})),
            (
                "goble_new_pane",
                json!({ "space_id": 0, "cwd": "/tmp", "kind": "chat" }),
            ),
            (
                "goble_split_pane",
                json!({ "pane_id": 2, "axis": "vertical" }),
            ),
            ("goble_close_pane", json!({ "pane_id": 3 })),
            (
                "goble_send_input",
                json!({ "pane_id": 4, "text": "echo hi", "enter": true }),
            ),
            ("goble_agent_state", json!({ "pane_id": 5 })),
            ("goble_inbox", json!({})),
            ("goble_ack_inbox", json!({ "id": "i1" })),
            ("goble_switch_space", json!({ "space_id": 1 })),
            ("goble_layout", json!({ "space_id": 0 })),
        ];
        for (name, arguments) in &cases {
            command_for(name, arguments.clone()).unwrap_or_else(|error| panic!("{name}: {error}"));
        }
        for name in TOOL_NAMES {
            assert!(
                cases.iter().any(|(candidate, _)| candidate == name),
                "no decode case for {name}"
            );
        }
    }

    #[test]
    fn arguments_are_strict() {
        let error = command_for("goble_list_panes", json!({ "pane_id": 1 })).unwrap_err();
        assert!(matches!(error, ToolError::InvalidArguments(_)));

        let error = command_for("goble_split_pane", json!({})).unwrap_err();
        assert!(matches!(error, ToolError::InvalidArguments(_)));

        let error = command_for("goble_ack_inbox", json!({})).unwrap_err();
        assert!(matches!(error, ToolError::InvalidArguments(_)));
    }

    #[test]
    fn layout_reads_without_a_kind_and_sets_with_one() {
        assert_eq!(
            command_for("goble_layout", json!({ "space_id": 2 })).unwrap(),
            Command::LayoutGet { space_id: 2 }
        );
        assert_eq!(
            command_for("goble_layout", json!({ "space_id": 2, "kind": "vertical" })).unwrap(),
            Command::LayoutSet {
                space_id: 2,
                kind: LayoutKind::Vertical,
            }
        );
        assert_eq!(
            command_for("goble_layout", json!({ "space_id": 2, "kind": "master_stack" })).unwrap(),
            Command::LayoutSet {
                space_id: 2,
                kind: LayoutKind::MasterStack,
            }
        );
    }

    #[test]
    fn an_unknown_tool_is_named_in_the_error() {
        let error = command_for("goble_dance", json!({})).unwrap_err();
        assert_eq!(error, ToolError::UnknownTool("goble_dance".into()));
    }
}
