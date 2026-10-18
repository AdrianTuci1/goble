//! End-to-end tests over a real Unix socket: one reply per request, structured
//! errors, and a socket that is gone after shutdown.

use std::sync::Mutex;

use goble_control::{
    Client, Command, ControlError, ErrorCode, Handler, PaneInfo, Payload, Request, Response,
    Server, SplitAxis,
};

#[derive(Debug, Default)]
struct MockHandler {
    seen: Mutex<Vec<&'static str>>,
}

impl Handler for MockHandler {
    fn handle(&self, command: Command) -> Result<Payload, ControlError> {
        self.seen.lock().unwrap().push(command.verb());
        match command {
            Command::PanesList => Ok(Payload::Panes {
                panes: vec![PaneInfo {
                    pane_id: 1,
                    space_id: 0,
                    kind: "chat".into(),
                    title: "New Agent".into(),
                    cwd: Some("/tmp".into()),
                    focused: true,
                    active_space: true,
                }],
            }),
            Command::PanesSplit { pane_id, axis } => {
                if pane_id != 1 {
                    return Err(ControlError::missing_pane(pane_id));
                }
                assert_eq!(axis, SplitAxis::Vertical);
                Ok(Payload::Pane {
                    pane: PaneInfo {
                        pane_id: 2,
                        space_id: 0,
                        kind: "terminal".into(),
                        title: "/tmp".into(),
                        cwd: Some("/tmp".into()),
                        focused: true,
                        active_space: true,
                    },
                })
            }
            Command::PanesClose { pane_id } => Err(ControlError::missing_pane(pane_id)),
            other => Err(ControlError::new(
                ErrorCode::Unsupported,
                format!("mock does not handle {}", other.verb()),
            )),
        }
    }
}

fn server() -> (Server<MockHandler>, tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("control.sock");
    let server = Server::start(&socket, MockHandler::default()).unwrap();
    (server, dir, socket)
}

#[test]
fn a_request_receives_its_reply() {
    let (server, _dir, socket) = server();
    let mut client = Client::connect(&socket).unwrap();

    let payload = client.call(Command::PanesList).unwrap();
    match payload {
        Payload::Panes { panes } => {
            assert_eq!(panes.len(), 1);
            assert_eq!(panes[0].pane_id, 1);
        }
        other => panic!("unexpected payload: {other:?}"),
    }
    assert_eq!(
        *server.handler().seen.lock().unwrap(),
        vec!["panes.list"]
    );
}

#[test]
fn a_missing_pane_is_a_structured_error() {
    let (_server, _dir, socket) = server();
    let mut client = Client::connect(&socket).unwrap();

    let error = client.call(Command::PanesClose { pane_id: 77 }).unwrap_err();
    assert_eq!(error.code, ErrorCode::MissingPane);
    assert!(error.message.contains("77"));
}

#[test]
fn an_unknown_verb_gets_a_structured_reply() {
    let (_server, _dir, socket) = server();
    let mut client = Client::connect(&socket).unwrap();

    let response: Response = client
        .request(Request {
            id: 41,
            verb: "panes.dance".into(),
            params: serde_json::Value::Null,
        })
        .unwrap();
    assert_eq!(response.id, 41);
    assert!(response.result.is_none());
    let error = response.error.expect("a structured error");
    assert_eq!(error.code, ErrorCode::UnknownVerb);
    assert!(error.message.contains("panes.dance"));
}

#[test]
fn invalid_parameters_get_a_structured_reply() {
    let (_server, _dir, socket) = server();
    let mut client = Client::connect(&socket).unwrap();

    let response = client
        .request(Request {
            id: 42,
            verb: "panes.split".into(),
            params: serde_json::json!({ "pane_id": 1 }),
        })
        .unwrap();
    assert_eq!(
        response.error.expect("a structured error").code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn shutdown_removes_the_socket_and_the_client_says_so() {
    let (server, _dir, socket) = server();
    assert!(socket.exists());
    server.shutdown();
    assert!(!socket.exists(), "shutdown removes the socket");

    let error = Client::connect(&socket).unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(
        error.message.contains("is the Goble app running?"),
        "{}",
        error.message
    );
}

#[test]
fn several_clients_are_served_independently() {
    let (_server, _dir, socket) = server();
    let mut one = Client::connect(&socket).unwrap();
    let mut two = Client::connect(&socket).unwrap();
    assert!(matches!(
        one.call(Command::PanesList).unwrap(),
        Payload::Panes { .. }
    ));
    assert!(matches!(
        two.call(Command::PanesSplit {
            pane_id: 1,
            axis: SplitAxis::Vertical,
        })
        .unwrap(),
        Payload::Pane { .. }
    ));
}
