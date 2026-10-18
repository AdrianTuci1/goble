//! The CLI against a real socket: one successful call to an in-process control
//! server, and the structured error a missing app produces.

#![cfg(unix)]

use std::process::Command;

use goble_control::{ControlError, Handler, PaneInfo, Payload, Server};
use serde_json::Value;

#[derive(Debug)]
struct AppHandler;

impl Handler for AppHandler {
    fn handle(&self, command: goble_control::Command) -> Result<Payload, ControlError> {
        match command {
            goble_control::Command::PanesList => Ok(Payload::Panes {
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
            other => Err(ControlError::new(
                goble_control::ErrorCode::Unsupported,
                format!("test app does not handle {}", other.verb()),
            )),
        }
    }
}

fn goble_ctl(socket: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_goble-cli"))
        .arg("ctl")
        .args(args)
        .arg("--json")
        .arg("--socket")
        .arg(socket)
        .output()
        .expect("run goble-cli")
}

#[test]
fn a_call_reaches_an_in_process_control_server() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("control.sock");
    let server = Server::start(&socket, AppHandler).unwrap();

    let output = goble_ctl(&socket, &["list-panes"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["kind"], "panes");
    assert_eq!(payload["panes"][0]["pane_id"], 1);
    assert_eq!(payload["panes"][0]["title"], "New Agent");

    server.shutdown();
}

#[test]
fn a_missing_socket_is_a_structured_error() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("nobody.sock");

    let output = goble_ctl(&socket, &["list-panes"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8(output.stderr).unwrap();
    let error: Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(error["error"]["code"], "unavailable");
    assert!(error["error"]["message"]
        .as_str()
        .unwrap()
        .contains("is the Goble app running?"));
}
