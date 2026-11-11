//! The rich input's writes into a pane's pty: the deferred writes a CLI agent
//! needs split across frames, and how a prompt reaches the agent it belongs to.

use std::time::Duration;

use super::*;

/// A detached session whose writes are captured instead of sent to a pty.
fn capturing_session() -> (TerminalSession, CaptureWriter) {
    let mut session = detached();
    let captured = CaptureWriter::default();
    session.writer = Some(Box::new(captured.clone()));
    (session, captured)
}

fn captured_bytes(captured: &CaptureWriter) -> Vec<u8> {
    captured.0.lock().unwrap().clone()
}

// -- Deferred writes --------------------------------------------------------

#[test]
fn a_zero_delay_write_waits_for_the_next_pump() {
    let (mut session, captured) = capturing_session();
    session.write_after(Duration::ZERO, b"now".to_vec());

    assert!(
        captured_bytes(&captured).is_empty(),
        "queueing a write writes nothing"
    );
    assert_eq!(session.pending_writes.len(), 1);

    session.pump();
    assert_eq!(captured_bytes(&captured), b"now");
    assert!(
        session.pending_writes.is_empty(),
        "a flushed write is taken off the queue"
    );
}

#[test]
fn a_write_due_in_an_hour_is_kept_back() {
    let (mut session, captured) = capturing_session();
    session.write_after(Duration::from_secs(3600), b"later".to_vec());

    session.pump();

    assert!(
        captured_bytes(&captured).is_empty(),
        "a write that is not due yet is not sent"
    );
    assert_eq!(
        session.pending_writes.len(),
        1,
        "the write stays queued for a later frame"
    );
}

#[test]
fn a_write_due_in_a_few_milliseconds_goes_out_on_the_next_pump() {
    let (mut session, captured) = capturing_session();
    session.write_after(Duration::from_millis(10), b"soon".to_vec());

    session.pump();
    assert!(
        captured_bytes(&captured).is_empty(),
        "not due yet on the frame that queued it"
    );

    std::thread::sleep(Duration::from_millis(30));
    session.pump();
    assert_eq!(captured_bytes(&captured), b"soon");
    assert!(session.pending_writes.is_empty());
}

#[test]
fn due_writes_flush_in_the_order_they_were_queued() {
    let (mut session, captured) = capturing_session();
    session.write_after(Duration::ZERO, b"first".to_vec());
    session.write_after(Duration::ZERO, b"second".to_vec());

    session.pump();

    assert_eq!(captured_bytes(&captured), b"firstsecond");
    assert!(session.pending_writes.is_empty());
}

// -- How a prompt reaches the agent -----------------------------------------

/// A registry with one detached pane in `mode` whose writes are captured.
fn pane(mode: TerminalMode) -> (TerminalRegistry, CaptureWriter) {
    let mut reg = TerminalRegistry::default();
    let (session, captured) = capturing_session();
    reg.sessions.insert(1, session);
    reg.set_mode(1, mode);
    (reg, captured)
}

/// The bytes queued for pane 1, as `(deadline, bytes)`.
fn pending(reg: &TerminalRegistry) -> Vec<(std::time::Instant, Vec<u8>)> {
    reg.sessions[&1].pending_writes.clone()
}

#[test]
fn a_codex_prompt_is_a_bracketed_paste_and_a_later_enter() {
    let (mut reg, captured) = pane(TerminalMode::Agent(TuiAgent::Codex));
    reg.submit_agent_prompt(1, "hello");

    let queued = pending(&reg);
    assert_eq!(queued.len(), 2, "the paste and the enter are two writes");
    assert_eq!(queued[0].1, b"\x1b[200~hello\x1b[201~");
    assert_eq!(queued[1].1, b"\r");
    assert!(
        queued[1].0 > queued[0].0,
        "the enter is queued for a later frame than the paste"
    );
    assert!(
        captured_bytes(&captured).is_empty(),
        "nothing reaches the pty before a pump"
    );

    std::thread::sleep(Duration::from_millis(80));
    reg.sessions
        .get_mut(&1)
        .expect("the session is there")
        .pump();
    assert_eq!(
        captured_bytes(&captured),
        b"\x1b[200~hello\x1b[201~\r",
        "the paste goes out before the enter"
    );
}

#[test]
fn a_copilot_prompt_gets_a_later_enter_than_codexs() {
    let copilot = goble_core::harness::harness_by_id("copilot").expect("copilot is in the catalog");
    let (mut reg, _captured) = pane(TerminalMode::Agent(TuiAgent::Catalog(copilot)));
    reg.submit_agent_prompt(1, "hello");

    let queued = pending(&reg);
    assert_eq!(queued.len(), 2);
    assert_eq!(queued[0].1, b"\x1b[200~hello\x1b[201~");
    assert_eq!(queued[1].1, b"\r");
    assert!(
        queued[1].0.duration_since(queued[0].0) >= Duration::from_millis(300),
        "copilot's editor needs a longer gap before the enter"
    );
}

#[test]
fn a_claude_prompt_is_the_text_and_then_the_enter() {
    let (mut reg, captured) = pane(TerminalMode::Agent(TuiAgent::Claude));
    reg.submit_agent_prompt(1, "hello");

    let queued = pending(&reg);
    assert_eq!(queued.len(), 2);
    assert_eq!(queued[0].1, b"hello", "no bracketed paste for claude");
    assert_eq!(queued[1].1, b"\r");
    assert!(queued[1].0 > queued[0].0);

    std::thread::sleep(Duration::from_millis(80));
    reg.sessions
        .get_mut(&1)
        .expect("the session is there")
        .pump();
    assert_eq!(captured_bytes(&captured), b"hello\r");
}

#[test]
fn an_inline_harness_gets_the_prompt_and_the_enter_together() {
    let (mut reg, captured) = pane(TerminalMode::Agent(TuiAgent::Grok));
    reg.submit_agent_prompt(1, "hello");

    let queued = pending(&reg);
    assert_eq!(queued.len(), 1, "one write carries the line and its enter");
    assert_eq!(queued[0].1, b"hello\r");

    reg.sessions
        .get_mut(&1)
        .expect("the session is there")
        .pump();
    assert_eq!(captured_bytes(&captured), b"hello\r");
}

#[test]
fn a_leading_bang_is_its_own_write_and_the_rest_follows() {
    let (mut reg, captured) = pane(TerminalMode::Agent(TuiAgent::Claude));
    reg.submit_agent_prompt(1, "!ls");

    assert_eq!(
        captured_bytes(&captured),
        b"!",
        "the agent's mode switch goes out before the text"
    );
    let queued = pending(&reg);
    assert_eq!(queued.len(), 2);
    assert_eq!(queued[0].1, b"ls", "the mode byte is not sent twice");
    assert_eq!(queued[1].1, b"\r");

    std::thread::sleep(Duration::from_millis(130));
    reg.sessions
        .get_mut(&1)
        .expect("the session is there")
        .pump();
    assert_eq!(captured_bytes(&captured), b"!ls\r");
}

#[test]
fn a_leading_ampersand_is_its_own_write_too() {
    let (mut reg, captured) = pane(TerminalMode::Agent(TuiAgent::Claude));
    reg.submit_agent_prompt(1, "&sleep 5");

    assert_eq!(captured_bytes(&captured), b"&");
    let queued = pending(&reg);
    assert_eq!(queued.len(), 2);
    assert_eq!(queued[0].1, b"sleep 5");

    std::thread::sleep(Duration::from_millis(130));
    reg.sessions
        .get_mut(&1)
        .expect("the session is there")
        .pump();
    assert_eq!(captured_bytes(&captured), b"&sleep 5\r");
}

#[test]
fn a_blank_prompt_is_not_submitted() {
    let (mut reg, captured) = pane(TerminalMode::Agent(TuiAgent::Claude));
    reg.set_input(1, "a draft".to_string());

    for text in ["", "   ", "\n\t "] {
        reg.submit_agent_prompt(1, text);
    }

    assert!(pending(&reg).is_empty(), "there is nothing to deliver");
    assert!(captured_bytes(&captured).is_empty());
    assert_eq!(
        reg.input(1),
        "a draft",
        "a refused submit leaves the pane's input alone"
    );
}

#[test]
fn a_pane_without_a_session_is_not_submitted() {
    let (mut reg, captured) = pane(TerminalMode::Agent(TuiAgent::Claude));

    reg.submit_agent_prompt(2, "hello");

    assert!(!reg.sessions.contains_key(&2));
    assert!(pending(&reg).is_empty());
    assert!(captured_bytes(&captured).is_empty());
}

#[test]
fn a_submitted_prompt_clears_the_panes_input() {
    let (mut reg, _captured) = pane(TerminalMode::Agent(TuiAgent::Codex));
    reg.set_input(1, "hello".to_string());

    reg.submit_agent_prompt(1, "hello");

    assert_eq!(reg.input(1), "");
}

// -- The per-harness strategy ------------------------------------------------

#[test]
fn every_harness_gets_the_submit_shape_its_editor_needs() {
    use crate::terminal::registry::AgentSubmit;

    assert_eq!(
        AgentSubmit::for_harness("codex"),
        AgentSubmit::BracketedPaste
    );
    assert_eq!(
        AgentSubmit::for_harness("copilot"),
        AgentSubmit::BracketedPasteDelayedEnter
    );
    for id in ["claude", "gemini", "opencode", "auggie", "cursor"] {
        assert_eq!(
            AgentSubmit::for_harness(id),
            AgentSubmit::DelayedEnter,
            "{id} reads a whole line, so the enter follows the text"
        );
    }
    for id in ["", "goble", "goose", "grok", "a-harness-not-in-the-catalog"] {
        assert_eq!(AgentSubmit::for_harness(id), AgentSubmit::Inline, "{id}");
    }
}
