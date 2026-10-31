//! End-to-end tests over a real Unix socket and real ptys.
//!
//! These are the survival properties the daemon exists for: a session keeps
//! running after its client detaches, a new client can replay it from byte
//! zero, several clients see the same broadcast, and a stale socket is cleaned
//! before auto-starting.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use goble_pty::client::Client;
use goble_pty::protocol::{
    decode_bytes, encode_bytes, ProtocolError, Request, ServerMessage, SessionInfo,
};
use goble_pty::server::Server;
use goble_pty::{pid_alive, pid_path_for, read_pidfile};

const TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_millis(100);

struct TestServer {
    _dir: tempfile::TempDir,
    socket: PathBuf,
    server: Option<Server>,
}

impl TestServer {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("pty.sock");
        let server = Server::start(&socket).unwrap();
        Self {
            _dir: dir,
            socket,
            server: Some(server),
        }
    }

    fn connect(&self, name: &str) -> Client {
        let mut client = Client::connect(&self.socket).unwrap();
        client
            .set_read_timeout(Some(READ_TIMEOUT))
            .expect("set a read timeout");
        let id = client.next_id();
        let response = client
            .request(Request::Hello {
                id,
                client_id: name.into(),
            })
            .unwrap();
        assert_eq!(
            response,
            ServerMessage::Ok {
                id,
                session: None
            }
        );
        client
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        drop(self.server.take());
    }
}

fn create(client: &mut Client, pane_id: &str, cwd: &Path) -> SessionInfo {
    let id = client.next_id();
    match client
        .request(Request::Create {
            id,
            pane_id: pane_id.into(),
            cwd: cwd.display().to_string(),
            cols: 100,
            rows: 30,
            shell: Some("/bin/sh".into()),
            args: Vec::new(),
            env: Vec::new(),
        })
        .unwrap()
    {
        ServerMessage::Ok {
            session: Some(info),
            ..
        } => info,
        other => panic!("create failed: {other:?}"),
    }
}

fn attach(client: &mut Client, session_id: &str, from_seq: u64) -> SessionInfo {
    let id = client.next_id();
    match client
        .request(Request::Attach {
            id,
            session_id: session_id.into(),
            from_seq,
        })
        .unwrap()
    {
        ServerMessage::Ok {
            session: Some(info),
            ..
        } => info,
        other => panic!("attach failed: {other:?}"),
    }
}

fn detach(client: &mut Client, session_id: &str) {
    let id = client.next_id();
    let response = client
        .request(Request::Detach {
            id,
            session_id: session_id.into(),
        })
        .unwrap();
    assert!(matches!(response, ServerMessage::Ok { .. }));
}

fn input(client: &mut Client, session_id: &str, bytes: &[u8]) {
    let id = client.next_id();
    let response = client
        .request(Request::Input {
            id,
            session_id: session_id.into(),
            bytes_b64: encode_bytes(bytes),
        })
        .unwrap();
    assert!(matches!(response, ServerMessage::Ok { .. }), "{response:?}");
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

/// Read output events until `needle` appears, returning every output chunk seen
/// first. Events of other kinds are skipped.
fn wait_for_output(client: &mut Client, needle: &[u8]) -> Vec<(u64, Vec<u8>)> {
    let deadline = Instant::now() + TIMEOUT;
    let mut chunks = Vec::new();
    loop {
        match client.next_message() {
            Ok(ServerMessage::Output {
                seq, bytes_b64, ..
            }) => {
                let bytes = decode_bytes(&bytes_b64).unwrap();
                let found = contains(&bytes, needle);
                chunks.push((seq, bytes));
                if found {
                    return chunks;
                }
            }
            Ok(_) => {}
            Err(ProtocolError::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => panic!("read failed while waiting for output: {e}"),
        }
        if Instant::now() >= deadline {
            let seen: Vec<u8> = chunks.iter().flat_map(|(_, b)| b.clone()).collect();
            panic!(
                "timed out waiting for {:?}; saw {:?}",
                String::from_utf8_lossy(needle),
                String::from_utf8_lossy(&seen)
            );
        }
    }
}

fn wait_for_exit(client: &mut Client) -> i32 {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match client.next_message() {
            Ok(ServerMessage::Exit { code, .. }) => return code,
            Ok(_) => {}
            Err(ProtocolError::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => panic!("read failed while waiting for exit: {e}"),
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for an exit event");
        }
    }
}

fn joined(chunks: &[(u64, Vec<u8>)]) -> Vec<u8> {
    chunks.iter().flat_map(|(_, bytes)| bytes.clone()).collect()
}

#[test]
fn a_session_survives_detach_and_replays_for_a_new_attach() {
    let server = TestServer::start();
    let shell_cwd = tempfile::tempdir().unwrap();

    let mut first = server.connect("a");
    let info = create(&mut first, "pane-1", shell_cwd.path());
    let attached = attach(&mut first, &info.session_id, 0);
    assert!(attached.alive);
    assert_eq!(attached.pane_id, "pane-1");

    input(&mut first, &info.session_id, b"printf 'F1A-SURVIVE\\n'\n");
    let seen = wait_for_output(&mut first, b"F1A-SURVIVE");
    assert!(
        contains(&joined(&seen), b"F1A-SURVIVE"),
        "the first attach sees the command's output"
    );
    assert_eq!(seen.first().map(|(seq, _)| *seq), Some(0));

    detach(&mut first, &info.session_id);
    drop(first);

    // A new client starts from byte zero and gets everything the ring still
    // holds, including output produced before it existed.
    let mut second = server.connect("b");
    let reattached = attach(&mut second, &info.session_id, 0);
    assert!(reattached.alive, "the session is still running");
    let replay = wait_for_output(&mut second, b"F1A-SURVIVE");
    assert!(
        contains(&joined(&replay), b"F1A-SURVIVE"),
        "the replay contains the detached client's output"
    );

    // And it is live, not just a recording.
    input(&mut second, &info.session_id, b"printf 'F1A-LIVE\\n'\n");
    let live = wait_for_output(&mut second, b"F1A-LIVE");
    assert!(contains(&joined(&live), b"F1A-LIVE"));

    let id = second.next_id();
    match second.request(Request::List { id }).unwrap() {
        ServerMessage::Sessions { sessions, .. } => {
            assert_eq!(sessions.len(), 1);
            assert!(sessions[0].alive);
            assert_eq!(sessions[0].session_id, info.session_id);
        }
        other => panic!("list failed: {other:?}"),
    }

    let id = second.next_id();
    let killed = second
        .request(Request::Kill {
            id,
            session_id: info.session_id.clone(),
        })
        .unwrap();
    assert!(matches!(killed, ServerMessage::Ok { .. }), "{killed:?}");
    assert_eq!(wait_for_exit(&mut second), -1);
}

#[test]
fn output_is_broadcast_to_every_attached_client() {
    let server = TestServer::start();
    let shell_cwd = tempfile::tempdir().unwrap();

    let mut a = server.connect("a");
    let mut b = server.connect("b");
    let info = create(&mut a, "pane-1", shell_cwd.path());
    attach(&mut a, &info.session_id, 0);
    attach(&mut b, &info.session_id, 0);

    input(&mut a, &info.session_id, b"printf 'F1A-BOTH\\n'\n");
    let from_a = wait_for_output(&mut a, b"F1A-BOTH");
    let from_b = wait_for_output(&mut b, b"F1A-BOTH");

    let marker_a = from_a
        .iter()
        .find(|(_, bytes)| contains(bytes, b"F1A-BOTH"))
        .expect("A saw the marker");
    let marker_b = from_b
        .iter()
        .find(|(_, bytes)| contains(bytes, b"F1A-BOTH"))
        .expect("B saw the marker");
    assert_eq!(marker_a, marker_b, "both clients get the same byte range");

    // Detaching one client changes nothing for the other.
    detach(&mut a, &info.session_id);
    input(&mut b, &info.session_id, b"printf 'F1A-STILL\\n'\n");
    let still = wait_for_output(&mut b, b"F1A-STILL");
    assert!(contains(&joined(&still), b"F1A-STILL"));

    let id = b.next_id();
    b.request(Request::Kill {
        id,
        session_id: info.session_id,
    })
    .unwrap();
}

#[test]
fn a_second_client_replays_while_the_first_stays_attached() {
    let server = TestServer::start();
    let shell_cwd = tempfile::tempdir().unwrap();

    let mut first = server.connect("first");
    let info = create(&mut first, "pane-1", shell_cwd.path());
    attach(&mut first, &info.session_id, 0);

    input(&mut first, &info.session_id, b"printf 'F5B-BEFORE\\n'\n");
    assert!(contains(&joined(&wait_for_output(&mut first, b"F5B-BEFORE")), b"F5B-BEFORE"));

    // A second client attaches to the same session from byte zero: it gets the
    // replay and does not replace the first client's subscription.
    let mut second = server.connect("second");
    let attached = attach(&mut second, &info.session_id, 0);
    assert!(attached.alive, "the second attach does not kill the session");
    let replay = wait_for_output(&mut second, b"F5B-BEFORE");
    assert!(
        contains(&joined(&replay), b"F5B-BEFORE"),
        "the second client sees the output produced before it attached"
    );

    // Both are live recipients of the same session.
    input(&mut first, &info.session_id, b"printf 'F5B-BOTH\\n'\n");
    assert!(contains(&joined(&wait_for_output(&mut first, b"F5B-BOTH")), b"F5B-BOTH"));
    assert!(contains(&joined(&wait_for_output(&mut second, b"F5B-BOTH")), b"F5B-BOTH"));

    // The second client leaving does not detach the first or kill the shell.
    detach(&mut second, &info.session_id);
    drop(second);
    input(&mut first, &info.session_id, b"printf 'F5B-AFTER\\n'\n");
    assert!(contains(&joined(&wait_for_output(&mut first, b"F5B-AFTER")), b"F5B-AFTER"));

    let id = first.next_id();
    match first.request(Request::List { id }).unwrap() {
        ServerMessage::Sessions { sessions, .. } => {
            assert_eq!(sessions.len(), 1);
            assert!(sessions[0].alive, "the session survived both attaches");
            assert_eq!(sessions[0].session_id, info.session_id);
        }
        other => panic!("list failed: {other:?}"),
    }

    let id = first.next_id();
    first
        .request(Request::Kill {
            id,
            session_id: info.session_id,
        })
        .unwrap();
}

#[test]
fn a_create_passes_its_args_and_environment_to_the_shell() {
    let server = TestServer::start();
    let shell_cwd = tempfile::tempdir().unwrap();

    let mut client = server.connect("args");
    let id = client.next_id();
    let info = match client
        .request(Request::Create {
            id,
            pane_id: "pane-env".into(),
            cwd: shell_cwd.path().display().to_string(),
            cols: 80,
            rows: 24,
            shell: Some("/bin/sh".into()),
            args: vec![
                "-c".into(),
                "printf 'F1B-ARGS:%s\\n' \"$GOBLE_MARK\"; exec sleep 30".into(),
            ],
            env: vec![("GOBLE_MARK".into(), "from-env".into())],
        })
        .unwrap()
    {
        ServerMessage::Ok {
            session: Some(info),
            ..
        } => info,
        other => panic!("create failed: {other:?}"),
    };
    attach(&mut client, &info.session_id, 0);

    let seen = wait_for_output(&mut client, b"F1B-ARGS:from-env");
    assert!(
        contains(&joined(&seen), b"F1B-ARGS:from-env"),
        "the shell received its argv and its environment: {:?}",
        String::from_utf8_lossy(&joined(&seen))
    );

    let id = client.next_id();
    let killed = client
        .request(Request::Kill {
            id,
            session_id: info.session_id,
        })
        .unwrap();
    assert!(matches!(killed, ServerMessage::Ok { .. }), "{killed:?}");
}

#[test]
fn a_stale_socket_is_cleaned_and_the_daemon_auto_starts() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("pty.sock");
    let pidfile = pid_path_for(&socket);

    // A bound-then-dropped listener leaves a socket file with nobody behind
    // it, and the pidfile names a pid that cannot exist.
    {
        let _stale = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    }
    std::fs::write(&pidfile, b"999999\n").unwrap();
    assert!(socket.exists() && pidfile.exists());

    let exe = PathBuf::from(env!("CARGO_BIN_EXE_goble-pty-daemon"));
    let mut client = Client::connect_or_start_at(&socket, &exe)
        .expect("the stale socket is cleaned and the daemon starts");
    client.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
    let id = client.next_id();
    let hello = client
        .request(Request::Hello {
            id,
            client_id: "auto".into(),
        })
        .unwrap();
    assert_eq!(hello, ServerMessage::Ok { id, session: None });

    let id = client.next_id();
    let list = client.request(Request::List { id }).unwrap();
    assert!(matches!(list, ServerMessage::Sessions { sessions, .. } if sessions.is_empty()));

    let daemon_pid = read_pidfile(&pidfile).expect("the started daemon wrote its pidfile");
    assert_ne!(daemon_pid, 999999);
    assert!(pid_alive(daemon_pid));
    assert!(socket.exists());

    // Stop the detached daemon; it is outside this test's process lifetime.
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(daemon_pid.to_string())
        .status();
    let deadline = Instant::now() + Duration::from_secs(5);
    while pid_alive(daemon_pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A crashed daemon leaves a pidfile, and the OS can hand that pid to an
/// unrelated live process. The record then names a live pid with no socket
/// behind it; the old recovery path refused to clean it, so every connect
/// failed with "a daemon (pid …) owns … but did not answer". A fresh daemon
/// must start instead.
#[test]
fn a_live_pid_with_no_socket_does_not_block_the_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("pty.sock");
    let pidfile = pid_path_for(&socket);
    std::fs::write(&pidfile, format!("{}\n", std::process::id())).unwrap();
    assert!(!socket.exists());

    let exe = PathBuf::from(env!("CARGO_BIN_EXE_goble-pty-daemon"));
    let mut client = Client::connect_or_start_at(&socket, &exe)
        .expect("a live pid without a socket is a stale record");

    let id = client.next_id();
    let list = client.request(Request::List { id }).unwrap();
    assert!(matches!(list, ServerMessage::Sessions { .. }));

    let daemon_pid = read_pidfile(&pidfile).expect("the fresh daemon wrote its pidfile");
    assert_ne!(daemon_pid, std::process::id());
    assert!(pid_alive(daemon_pid));
    assert!(socket.exists());

    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(daemon_pid.to_string())
        .status();
    let deadline = Instant::now() + Duration::from_secs(5);
    while pid_alive(daemon_pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A shell that exits releases its pty and its session. Holding either would
/// leak one master descriptor per closed pane until the machine runs out of
/// ptys; a pane that has finished must also not look attachable.
#[test]
fn a_finished_session_is_released() {
    let server = TestServer::start();
    let shell_cwd = tempfile::tempdir().unwrap();

    let mut client = server.connect("a");
    let info = create(&mut client, "pane-done", shell_cwd.path());
    attach(&mut client, &info.session_id, 0);

    input(&mut client, &info.session_id, b"exit 7\n");
    assert_eq!(wait_for_exit(&mut client), 7);

    let id = client.next_id();
    match client.request(Request::List { id }).unwrap() {
        ServerMessage::Sessions { sessions, .. } => {
            assert!(
                sessions.is_empty(),
                "the exited session is no longer kept: {sessions:?}"
            );
        }
        other => panic!("list failed: {other:?}"),
    }

    let id = client.next_id();
    match client
        .request(Request::Attach {
            id,
            session_id: info.session_id,
            from_seq: 0,
        })
        .unwrap()
    {
        ServerMessage::Error { message, .. } => {
            assert!(
                message.contains("no session"),
                "attaching a released session reports it: {message}"
            );
        }
        other => panic!("attach should fail after exit: {other:?}"),
    }
}
