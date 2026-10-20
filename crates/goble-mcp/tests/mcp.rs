//! MCP protocol tests: the three methods against a mock control backend, then
//! the same tools end-to-end against a real in-process control server.

#![cfg(unix)]

use std::io::Cursor;
use std::sync::{Arc, Mutex};

use goble_control::{
    AgentStateInfo, Command, ControlError, ErrorCode, Handler, InboxInfo, LayoutInfo, LayoutKind,
    LayoutNode, PaneInfo, Payload, SendResult, Server, SplitAxis,
};
use goble_mcp::{Control, McpServer, SocketControl};
use serde_json::{json, Value};

#[derive(Debug, Default, Clone)]
struct MockControl {
    seen: Arc<Mutex<Vec<Command>>>,
}

impl Control for MockControl {
    fn call(&mut self, command: Command) -> Result<Payload, ControlError> {
        self.seen.lock().unwrap().push(command.clone());
        match command {
            Command::PanesList => Ok(Payload::Panes {
                panes: vec![pane(1, "chat", "New Agent", true)],
            }),
            Command::PanesSend {
                pane_id,
                text,
                enter,
            } => Ok(Payload::Sent {
                send: SendResult {
                    pane_id,
                    bytes: text.len(),
                    entered: enter,
                },
            }),
            Command::PanesClose { pane_id } => Err(ControlError::missing_pane(pane_id)),
            Command::AgentsState { .. } => Ok(Payload::AgentStates {
                agents: vec![AgentStateInfo {
                    pane_id: 1,
                    state: "idle".into(),
                    busy: false,
                    conversation_id: None,
                }],
            }),
            Command::InboxList => Ok(Payload::Inbox { items: Vec::new() }),
            Command::LayoutGet { space_id } => Ok(Payload::Layout {
                layout: LayoutInfo {
                    space_id,
                    kind: LayoutKind::Single,
                    active_pane_id: 1,
                    root: LayoutNode::Leaf {
                        pane_id: 1,
                        kind: "chat".into(),
                    },
                },
            }),
            _ => Ok(Payload::Ok),
        }
    }
}

fn pane(pane_id: u64, kind: &str, title: &str, focused: bool) -> PaneInfo {
    PaneInfo {
        pane_id,
        space_id: 0,
        kind: kind.into(),
        title: title.into(),
        cwd: Some("/tmp".into()),
        focused,
        active_space: true,
    }
}

fn serve<C: Control>(control: C, requests: Vec<Value>) -> Vec<Value> {
    let mut server = McpServer::new(control);
    let mut input = requests
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    input.push('\n');
    let mut output = Vec::new();
    server
        .serve(Cursor::new(input), &mut output)
        .expect("in-memory stdio");
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn text(response: &Value) -> &str {
    response["result"]["content"][0]["text"]
        .as_str()
        .expect("a text content block")
}

#[test]
fn initialize_tools_list_and_tools_call_against_a_mock() {
    let mock = MockControl::default();
    let seen = Arc::clone(&mock.seen);
    let responses = serve(
        mock,
        vec![
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": { "name": "test", "version": "1" }
                }
            }),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": { "name": "goble_list_panes", "arguments": {} }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "goble_send_input",
                    "arguments": { "pane_id": 7, "text": "echo hi", "enter": true }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": { "name": "goble_close_pane", "arguments": { "pane_id": 99 } }
            }),
        ],
    );

    assert_eq!(responses.len(), 5, "the notification gets no reply");

    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "goble-mcp");

    assert_eq!(responses[1]["id"], 2);
    let tools = responses[1]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), goble_mcp::TOOL_NAMES.len());
    let names: Vec<&str> = tools.iter().map(|tool| tool["name"].as_str().unwrap()).collect();
    for name in goble_mcp::TOOL_NAMES {
        assert!(names.contains(name), "tools/list is missing {name}");
    }

    assert_eq!(responses[2]["id"], 3);
    assert_eq!(responses[2]["result"]["isError"], false);
    match serde_json::from_str::<Payload>(text(&responses[2])).unwrap() {
        Payload::Panes { panes } => {
            assert_eq!(panes.len(), 1);
            assert_eq!(panes[0].pane_id, 1);
        }
        other => panic!("unexpected payload: {other:?}"),
    }

    assert_eq!(responses[3]["result"]["isError"], false);
    match serde_json::from_str::<Payload>(text(&responses[3])).unwrap() {
        Payload::Sent { send } => {
            assert_eq!(send.pane_id, 7);
            assert_eq!(send.bytes, "echo hi".len());
            assert!(send.entered);
        }
        other => panic!("unexpected payload: {other:?}"),
    }

    assert_eq!(responses[4]["result"]["isError"], true);
    assert!(text(&responses[4]).contains("missing_pane"));

    let seen = seen.lock().unwrap();
    assert_eq!(
        seen.as_slice(),
        &[
            Command::PanesList,
            Command::PanesSend {
                pane_id: 7,
                text: "echo hi".into(),
                enter: true,
            },
            Command::PanesClose { pane_id: 99 },
        ]
    );
}

#[test]
fn unknown_methods_and_bad_arguments_are_jsonrpc_errors() {
    let responses = serve(
        MockControl::default(),
        vec![
            json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/list" }),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": { "name": "goble_dance", "arguments": {} }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": { "name": "goble_split_pane", "arguments": {} }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": { "name": "goble_ack_inbox", "arguments": {} }
            }),
        ],
    );

    assert_eq!(responses[0]["error"]["code"], -32601);
    assert_eq!(responses[1]["error"]["code"], -32602);
    assert!(responses[1]["error"]["message"]
        .as_str()
        .unwrap()
        .contains("goble_dance"));
    assert_eq!(responses[2]["error"]["code"], -32602);
    assert_eq!(responses[3]["error"]["code"], -32602);
}

#[test]
fn a_malformed_line_is_a_parse_error() {
    let mut server = McpServer::new(MockControl::default());
    let mut output = Vec::new();
    server
        .serve(Cursor::new(b"not json\n"), &mut output)
        .unwrap();
    let response: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(response["error"]["code"], -32700);
    assert_eq!(response["id"], Value::Null);
}

#[derive(Debug, Default)]
struct AppHandler;

impl Handler for AppHandler {
    fn handle(&self, command: Command) -> Result<Payload, ControlError> {
        match command {
            Command::PanesList => Ok(Payload::Panes {
                panes: vec![pane(1, "chat", "New Agent", true)],
            }),
            Command::PanesSplit { pane_id, axis } => {
                if pane_id != 1 {
                    return Err(ControlError::missing_pane(pane_id));
                }
                assert_eq!(axis, SplitAxis::Vertical);
                Ok(Payload::Pane {
                    pane: pane(2, "terminal", "/tmp", true),
                })
            }
            Command::PanesClose { pane_id } => Err(ControlError::missing_pane(pane_id)),
            Command::InboxList => Ok(Payload::Inbox {
                items: vec![InboxInfo {
                    id: "i1".into(),
                    pane_id: 1,
                    space_id: 0,
                    conversation_id: "c1".into(),
                    kind: "done".into(),
                    title: "finished".into(),
                    detail: "agent finished".into(),
                    created_at: "2026-10-18T00:00:00Z".into(),
                    acknowledged: false,
                }],
            }),
            other => Err(ControlError::new(
                ErrorCode::Unsupported,
                format!("the test app does not handle {}", other.verb()),
            )),
        }
    }
}

#[test]
fn tools_run_end_to_end_against_an_in_process_control_server() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("control.sock");
    let server = Server::start(&socket, AppHandler).unwrap();

    let responses = serve(
        SocketControl::new(&socket),
        vec![
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": { "protocolVersion": "2024-11-05" }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": { "name": "goble_list_panes", "arguments": {} }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "goble_split_pane",
                    "arguments": { "pane_id": 1, "axis": "vertical" }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": { "name": "goble_inbox", "arguments": {} }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": { "name": "goble_close_pane", "arguments": { "pane_id": 1 } }
            }),
        ],
    );

    assert_eq!(
        responses[0]["result"]["protocolVersion"],
        Value::String("2024-11-05".into())
    );
    assert_eq!(responses[1]["result"]["isError"], false);
    assert_eq!(responses[2]["result"]["isError"], false);
    match serde_json::from_str::<Payload>(text(&responses[2])).unwrap() {
        Payload::Pane { pane } => {
            assert_eq!(pane.pane_id, 2);
            assert_eq!(pane.kind, "terminal");
        }
        other => panic!("unexpected payload: {other:?}"),
    }
    assert_eq!(responses[3]["result"]["isError"], false);
    match serde_json::from_str::<Payload>(text(&responses[3])).unwrap() {
        Payload::Inbox { items } => assert_eq!(items.len(), 1),
        other => panic!("unexpected payload: {other:?}"),
    }
    assert_eq!(responses[4]["result"]["isError"], true);
    assert!(text(&responses[4]).contains("missing_pane"));

    server.shutdown();
    assert!(!socket.exists());
}

#[test]
fn a_missing_app_is_a_structured_tool_error() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nobody.sock");
    let responses = serve(
        SocketControl::new(&missing),
        vec![json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "goble_list_panes", "arguments": {} }
        })],
    );

    assert_eq!(responses[0]["result"]["isError"], true);
    let error: Value = serde_json::from_str(text(&responses[0])).unwrap();
    assert_eq!(error["error"]["code"], "unavailable");
    assert!(error["error"]["message"]
        .as_str()
        .unwrap()
        .contains("is the Goble app running?"));
}
