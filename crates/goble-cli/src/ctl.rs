//! `goble ctl`: the human-facing control client.
//!
//! One verb becomes one typed [`Command`] on the app's control socket. The
//! default output is a short text summary; `--json` prints the typed payload.
//! Failures print a structured error (`{"error": {"code": ..., "message":
//! ...}}` under `--json`) and exit non-zero, so a script can tell "the app is
//! not running" from "that pane does not exist".

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use goble_control::{Client, Command, ControlError, Payload};

use crate::cli::CtlAction;

/// How long a CLI call waits for the app. The app answers UI verbs within 30s
/// or reports `unavailable`; this is the outer belt to that braces.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(45);

pub(crate) fn run(action: CtlAction, json: bool, socket: Option<PathBuf>) -> Result<()> {
    let command = command_for(action);
    match call(socket, command) {
        Ok(payload) => {
            print_payload(&payload, json);
            Ok(())
        }
        Err(error) => {
            print_error(&error, json);
            std::process::exit(1);
        }
    }
}

fn command_for(action: CtlAction) -> Command {
    match action {
        CtlAction::ListPanes => Command::PanesList,
        CtlAction::NewPane {
            space_id,
            cwd,
            kind,
        } => Command::PanesNew {
            space_id,
            cwd,
            kind: Some(kind.into()),
        },
        CtlAction::SplitPane { pane_id, axis } => Command::PanesSplit {
            pane_id,
            axis: axis.into(),
        },
        CtlAction::ClosePane { pane_id } => Command::PanesClose { pane_id },
        CtlAction::FocusPane { pane_id } => Command::PanesFocus { pane_id },
        CtlAction::SendInput {
            pane_id,
            text,
            enter,
        } => Command::PanesSend {
            pane_id,
            text,
            enter,
        },
        CtlAction::ResizePane {
            pane_id,
            cols,
            rows,
        } => Command::PanesResize {
            pane_id,
            cols,
            rows,
        },
        CtlAction::ListSpaces => Command::SpacesList,
        CtlAction::SwitchSpace { space_id } => Command::SpacesSwitch { space_id },
        CtlAction::AgentState { pane_id } => Command::AgentsState { pane_id },
        CtlAction::PromptAgent { pane_id, text } => Command::AgentsPrompt { pane_id, text },
        CtlAction::Inbox => Command::InboxList,
        CtlAction::AckInbox { id, all } => Command::InboxAck { id, all },
        CtlAction::Layout { space_id, kind } => match kind {
            Some(kind) => Command::LayoutSet {
                space_id,
                kind: kind.into(),
            },
            None => Command::LayoutGet { space_id },
        },
        CtlAction::PtyList => Command::PtyList,
        CtlAction::PtyAttach { session_id } => Command::PtyAttach { session_id },
        CtlAction::PtyDetach { session_id } => Command::PtyDetach { session_id },
        CtlAction::PtyKill { session_id } => Command::PtyKill { session_id },
    }
}

fn call(socket: Option<PathBuf>, command: Command) -> Result<Payload, ControlError> {
    let mut client = match socket {
        Some(path) => Client::connect(path),
        None => Client::connect_default(),
    }?;
    client.set_read_timeout(Some(CONTROL_TIMEOUT))?;
    client.call(command)
}

fn print_payload(payload: &Payload, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(payload).unwrap_or_default());
        return;
    }

    match payload {
        Payload::Panes { panes } => {
            println!("PANE\tSPACE\tKIND\tFOCUSED\tTITLE");
            for pane in panes {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    pane.pane_id, pane.space_id, pane.kind, pane.focused, pane.title
                );
            }
        }
        Payload::Pane { pane } => {
            println!(
                "pane {}\tspace {}\t{}\t{}\tfocused={}",
                pane.pane_id, pane.space_id, pane.kind, pane.title, pane.focused
            );
        }
        Payload::Closed { pane_id } => println!("closed pane {pane_id}"),
        Payload::Spaces { spaces } => {
            println!("SPACE\tNAME\tMEDIUM\tPANES\tLAYOUT\tACTIVE");
            for space in spaces {
                println!(
                    "{}\t{}\t{}\t{}\t{:?}\t{}",
                    space.space_id,
                    space.name,
                    space.medium,
                    space.pane_count,
                    space.layout,
                    space.active
                );
            }
        }
        Payload::Space { space } => println!(
            "space {}\t{}\t{}\t{} panes\t{:?}\tactive={}",
            space.space_id, space.name, space.medium, space.pane_count, space.layout, space.active
        ),
        Payload::AgentStates { agents } => {
            println!("PANE\tSTATE\tBUSY\tCONVERSATION");
            for agent in agents {
                println!(
                    "{}\t{}\t{}\t{}",
                    agent.pane_id,
                    agent.state,
                    agent.busy,
                    agent.conversation_id.as_deref().unwrap_or("-")
                );
            }
        }
        Payload::Prompt { prompt } => {
            println!("pane {}: started={}", prompt.pane_id, prompt.started)
        }
        Payload::Inbox { items } => {
            println!("ID\tPANE\tKIND\tACKNOWLEDGED\tTITLE");
            for item in items {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    item.id, item.pane_id, item.kind, item.acknowledged, item.title
                );
            }
        }
        Payload::Acknowledged { acknowledged } => {
            println!("acknowledged {acknowledged} item(s)")
        }
        Payload::Layout { layout } => {
            println!(
                "space {}: {:?}, active pane {}",
                layout.space_id, layout.kind, layout.active_pane_id
            );
            println!(
                "{}",
                serde_json::to_string_pretty(&layout.root).unwrap_or_default()
            );
        }
        Payload::PtySessions { sessions } => {
            println!("SESSION\tPANE\tALIVE\tCOLS\tROWS");
            for session in sessions {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    session.session_id, session.pane_id, session.alive, session.cols, session.rows
                );
            }
        }
        Payload::PtySession { session } => println!(
            "{} (pane {})\talive={}\t{}x{}",
            session.session_id, session.pane_id, session.alive, session.cols, session.rows
        ),
        Payload::Sent { send } => println!(
            "sent {} bytes to pane {}{}",
            send.bytes,
            send.pane_id,
            if send.entered { " and pressed Enter" } else { "" }
        ),
        Payload::Ok => println!("ok"),
    }
}

fn print_error(error: &ControlError, json: bool) {
    if json {
        eprintln!(
            "{}",
            serde_json::json!({ "error": error })
        );
    } else {
        eprintln!("error: {}: {}", code_name(error), error.message);
    }
}

fn code_name(error: &ControlError) -> String {
    serde_json::to_value(error.code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{:?}", error.code))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{
        AxisArg, CtlAction, LayoutKindArg, PaneKindArg,
    };

    #[test]
    fn every_verb_maps_to_its_control_command() {
        assert_eq!(command_for(CtlAction::ListPanes), Command::PanesList);
        assert_eq!(
            command_for(CtlAction::NewPane {
                space_id: Some(0),
                cwd: Some("/tmp".into()),
                kind: PaneKindArg::Chat,
            }),
            Command::PanesNew {
                space_id: Some(0),
                cwd: Some("/tmp".into()),
                kind: Some(goble_control::PaneKind::Chat),
            }
        );
        assert_eq!(
            command_for(CtlAction::SplitPane {
                pane_id: 2,
                axis: AxisArg::Vertical,
            }),
            Command::PanesSplit {
                pane_id: 2,
                axis: goble_control::SplitAxis::Vertical,
            }
        );
        assert_eq!(
            command_for(CtlAction::AckInbox {
                id: Some("i1".into()),
                all: false,
            }),
            Command::InboxAck {
                id: Some("i1".into()),
                all: false,
            }
        );
        assert_eq!(
            command_for(CtlAction::Layout {
                space_id: 3,
                kind: None,
            }),
            Command::LayoutGet { space_id: 3 }
        );
        assert_eq!(
            command_for(CtlAction::Layout {
                space_id: 3,
                kind: Some(LayoutKindArg::Single),
            }),
            Command::LayoutSet {
                space_id: 3,
                kind: goble_control::LayoutKind::Single,
            }
        );
    }

    #[test]
    fn an_error_code_prints_in_wire_form() {
        let error = ControlError::missing_pane(7);
        assert_eq!(code_name(&error), "missing_pane");
    }
}
