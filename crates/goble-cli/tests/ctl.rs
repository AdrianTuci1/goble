//! `goble ctl` parse tests: every verb, the global `--json`/`--socket`
//! options in either position, and default values.

#![cfg(unix)]

use std::path::PathBuf;

use clap::Parser;
use goble_cli::{Args, AxisArg, Command, CtlAction, LayoutKindArg, PaneKindArg};

fn parse(values: &[&str]) -> Args {
    Args::parse_from(values)
}

fn ctl(args: Args) -> (bool, Option<PathBuf>, CtlAction) {
    match args.command {
        Command::Ctl {
            json,
            socket,
            action,
        } => (json, socket, action),
        other => panic!("expected ctl, got {other:?}"),
    }
}

#[test]
fn every_ctl_verb_parses() {
    let cases: Vec<Vec<&str>> = vec![
        vec!["ctl", "list-panes"],
        vec!["ctl", "new-pane", "--space-id", "0", "--cwd", "/tmp", "--kind", "chat"],
        vec!["ctl", "split-pane", "4", "--axis", "vertical"],
        vec!["ctl", "close-pane", "4"],
        vec!["ctl", "focus-pane", "4"],
        vec!["ctl", "send-input", "4", "echo hi", "--enter"],
        vec!["ctl", "resize-pane", "4", "120", "40"],
        vec!["ctl", "list-spaces"],
        vec!["ctl", "switch-space", "2"],
        vec!["ctl", "agent-state", "--pane-id", "4"],
        vec!["ctl", "prompt-agent", "4", "ship it"],
        vec!["ctl", "inbox"],
        vec!["ctl", "ack-inbox", "--all"],
        vec!["ctl", "layout", "0"],
        vec!["ctl", "layout", "0", "--kind", "single"],
        vec!["ctl", "pty-list"],
        vec!["ctl", "pty-attach", "session-1"],
        vec!["ctl", "pty-detach", "session-1"],
        vec!["ctl", "pty-kill", "session-1"],
    ];

    let argv = vec!["goble-cli"];
    for case in cases {
        let mut args = argv.clone();
        args.extend(case);
        let parsed = Args::try_parse_from(&args)
            .unwrap_or_else(|error| panic!("{} did not parse: {error}", args.join(" ")));
        let _ = ctl(parsed);
    }
}

#[test]
fn new_pane_defaults_to_a_terminal() {
    let (_, _, action) = ctl(parse(&["goble-cli", "ctl", "new-pane"]));
    assert!(matches!(
        action,
        CtlAction::NewPane {
            space_id: None,
            cwd: None,
            kind: PaneKindArg::Terminal,
        }
    ));
}

#[test]
fn split_defaults_to_horizontal() {
    let (_, _, action) = ctl(parse(&["goble-cli", "ctl", "split-pane", "3"]));
    assert!(matches!(
        action,
        CtlAction::SplitPane {
            pane_id: 3,
            axis: AxisArg::Horizontal,
        }
    ));
}

#[test]
fn layout_reads_without_a_kind_and_sets_with_one() {
    let (_, _, action) = ctl(parse(&["goble-cli", "ctl", "layout", "1"]));
    assert!(matches!(
        action,
        CtlAction::Layout {
            space_id: 1,
            kind: None,
        }
    ));

    let (_, _, action) = ctl(parse(&[
        "goble-cli", "ctl", "layout", "1", "--kind", "vertical",
    ]));
    assert!(matches!(
        action,
        CtlAction::Layout {
            space_id: 1,
            kind: Some(LayoutKindArg::Vertical),
        }
    ));
}

#[test]
fn json_and_socket_work_before_or_after_the_verb() {
    let before = ctl(parse(&[
        "goble-cli", "ctl", "--json", "--socket", "/tmp/a.sock", "list-panes",
    ]));
    let after = ctl(parse(&[
        "goble-cli", "ctl", "list-panes", "--json", "--socket", "/tmp/a.sock",
    ]));
    assert!(before.0);
    assert_eq!(before.1.as_deref(), Some(std::path::Path::new("/tmp/a.sock")));
    assert!(after.0);
    assert_eq!(after.1.as_deref(), Some(std::path::Path::new("/tmp/a.sock")));
}

#[test]
fn ack_inbox_accepts_only_one_target() {
    let (_, _, action) = ctl(parse(&["goble-cli", "ctl", "ack-inbox", "--id", "i1"]));
    assert!(matches!(
        action,
        CtlAction::AckInbox {
            id: Some(_),
            all: false,
        }
    ));

    let result = Args::try_parse_from([
        "goble-cli", "ctl", "ack-inbox", "--id", "i1", "--all",
    ]);
    assert!(result.is_err(), "--id and --all conflict");
}

#[test]
fn existing_commands_still_parse() {
    let parsed = parse(&[
        "goble-cli", "worker-list",
    ]);
    assert!(matches!(parsed.command, Command::WorkerList));

    let parsed = parse(&[
        "goble-cli",
        "secret",
        "get",
        "--worker",
        "w1",
        "--url",
        "wss://example/ws",
        "--name",
        "token",
    ]);
    assert!(matches!(parsed.command, Command::Secret { .. }));
}
