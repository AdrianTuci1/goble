//! Where a `!` line typed on an agent surface runs, and which surface a
//! harness's sidebar row opens.
//!
//! The agent composer and the pane's terminal are two surfaces with two shells:
//! a line typed on the agent surface belongs to the conversation and runs in
//! the conversation's own shell, and Esc back to the terminal never shows it.
//! The failure this file pins is the reverse: a line typed in agent mode
//! reaching the pane's terminal, which happens when the composer is read as the
//! terminal's or when the pane is left in terminal mode by a sidebar click.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_terminal::HookEvent;
use goble_ui::platform::WindowControl;

use super::*;
use crate::actions::make_actions;
use crate::media::MediaState;
use crate::state::UiState;
use crate::ui::{Pane, PaneKind, Space, UiActions};

/// A single-window app whose pane terminal and agent shell are captures
/// instead of ptys, so a test reads which shell a line reached. A session that
/// is missing from a capture's registry entry was never spawned.
struct Fixture {
    state: Rc<RefCell<UiState>>,
    actions: UiActions,
    desktop: Arc<DesktopState>,
    terminal_bytes: Arc<Mutex<Vec<u8>>>,
    agent_bytes: Arc<Mutex<Vec<u8>>>,
    _store_dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let store_dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(store_dir.path()).expect("thread store"),
    );
    let state = Rc::new(RefCell::new(UiState::from_desktop(&desktop)));
    let media = Rc::new(RefCell::new(MediaState::mock()));
    let actions = make_actions(
        Rc::clone(&state),
        Some(Arc::clone(&desktop)),
        media,
        WindowControl::default(),
        Rc::new(RefCell::new(1.0)),
    );
    Fixture {
        state,
        actions,
        desktop,
        terminal_bytes: Arc::new(Mutex::new(Vec::new())),
        agent_bytes: Arc::new(Mutex::new(Vec::new())),
        _store_dir: store_dir,
    }
}

/// A shell that has bootstrapped its integration, so a `!` line written into it
/// is filed right away instead of waiting for the handshake.
fn bootstrapped() -> TerminalSession {
    let mut session = TerminalSession::with_emulator(Emulator::new(80, 24));
    session.feed_hook(HookEvent::Bootstrapped(Default::default()));
    session
}

fn captured(write: &Arc<Mutex<Vec<u8>>>) -> Vec<u8> {
    write.lock().unwrap().clone()
}

fn clear(write: &Arc<Mutex<Vec<u8>>>) {
    write.lock().unwrap().clear();
}

/// A path to an executable named `name`, so a launch line names a program that
/// is really there whatever the machine has installed.
fn installed_harness(name: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create a temp dir");
    let program = dir.path().join(name);
    std::fs::write(&program, "#!/bin/sh\n").expect("write the program");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
            .expect("make it executable");
    }
    let path = program.to_str().expect("a path").to_string();
    (dir, path)
}

/// A chat pane's composer is the conversation's agent composer, whatever the
/// pane's terminal filter says. A `!` line typed there runs in the
/// conversation's own shell: the pane has no terminal surface, so nothing may
/// be spawned behind it and no pane terminal may receive the line.
#[test]
fn a_bang_line_on_a_chat_panes_agent_surface_runs_in_the_conversations_shell() {
    let fixture = fixture();
    let conversation = fixture
        .desktop
        .create_chat("Thread", None, None)
        .expect("create a chat");
    let pane_id = 1;
    // The pane a window starts on: a chat pane bound to the conversation the
    // sidebar shows. Its agent mode was never switched on explicitly, which is
    // the state that must not turn the composer into the shell's.
    {
        let mut state = fixture.state.borrow_mut();
        state.spaces = vec![Space::new(
            "Conversation",
            Pane::Leaf {
                id: pane_id,
                kind: PaneKind::Chat,
            },
        )];
        state.active_space = 0;
        state.active_pane_id = pane_id;
        state.selected_id = Some(conversation.clone());
        let session = state
            .pane_sessions
            .get_mut(&pane_id)
            .expect("pane 1 has a session");
        session.conversation_id = conversation.clone();
        // Both shells are captures: the agent shell is what the line must
        // reach, and a pane terminal is what it must not.
        let mut agent = bootstrapped();
        agent.writer = Some(Box::new(CaptureWriter(Arc::clone(&fixture.agent_bytes))));
        let mut terminal = bootstrapped();
        terminal.writer = Some(Box::new(CaptureWriter(Arc::clone(&fixture.terminal_bytes))));
        let mut registry = state.terminal.borrow_mut();
        registry.agent_sessions.insert(pane_id, agent);
        registry.sessions.insert(pane_id, terminal);
    }

    (fixture.actions.on_send_message.borrow_mut())("!printf 'AGENT-ONLY\\n'".to_string());

    assert_eq!(
        captured(&fixture.agent_bytes),
        b"printf 'AGENT-ONLY\\n'\n",
        "the line runs in the conversation's own shell"
    );
    assert!(
        captured(&fixture.terminal_bytes).is_empty(),
        "the pane's terminal never sees a line typed on the agent surface"
    );
}

/// The terminal and the harness's agent surface stay separate across a whole
/// round trip: launch the harness from a terminal pane, Esc back to the shell,
/// click the harness's sidebar row, type a `!` line. The row opens the
/// conversation's agent view — not the terminal — and the line runs in the
/// conversation's own shell.
#[test]
fn a_harness_reopened_from_the_sidebar_keeps_bang_lines_out_of_the_terminal() {
    let fixture = fixture();
    {
        let mut state = fixture.state.borrow_mut();
        state.spaces = vec![Space::new(
            "Shell",
            Pane::Leaf {
                id: 1,
                kind: PaneKind::Terminal,
            },
        )];
        state.active_space = 0;
        state.active_pane_id = 1;
        state
            .pane_sessions
            .get_mut(&1)
            .expect("pane 1 has a session")
            .conversation_id
            .clear();
        let mut terminal = bootstrapped();
        terminal.writer = Some(Box::new(CaptureWriter(Arc::clone(&fixture.terminal_bytes))));
        let mut agent = bootstrapped();
        agent.writer = Some(Box::new(CaptureWriter(Arc::clone(&fixture.agent_bytes))));
        let mut registry = state.terminal.borrow_mut();
        registry.sessions.insert(1, terminal);
        registry.agent_sessions.insert(1, agent);
    }

    // The launch line at the pane's own bar, the way a harness is started.
    let (_claude_dir, claude) = installed_harness("claude");
    (fixture.actions.on_run_shell_command.borrow_mut())(1, claude);
    let conversation = {
        let state = fixture.state.borrow();
        assert_eq!(
            state.pane_view(1),
            goble_terminal::blocks::BlockView::Agent {
                conversation_id: conversation_of(&state)
            },
            "the launch opens the harness's agent view"
        );
        conversation_of(&state)
    };

    // Esc, back to the shell the harness runs in.
    (fixture.actions.on_set_pane_harness_mode.borrow_mut())(1, false);
    assert_eq!(
        fixture.state.borrow().pane_view(1),
        goble_terminal::blocks::BlockView::Terminal,
        "the premise: Esc left the pane on its shell"
    );
    // The launch line itself is in the terminal; only the next command matters.
    clear(&fixture.terminal_bytes);
    clear(&fixture.agent_bytes);

    // The harness's sidebar row.
    (fixture.actions.on_select_conversation.borrow_mut())(conversation.clone());
    assert_eq!(
        fixture.state.borrow().pane_view(1),
        goble_terminal::blocks::BlockView::Agent {
            conversation_id: conversation.clone()
        },
        "the row brings the harness forward on its agent view, not its terminal"
    );

    (fixture.actions.on_send_message.borrow_mut())("!printf 'AGENT-ONLY\\n'".to_string());

    assert_eq!(
        captured(&fixture.agent_bytes),
        b"printf 'AGENT-ONLY\\n'\n",
        "the line runs in the conversation's own shell"
    );
    assert!(
        captured(&fixture.terminal_bytes).is_empty(),
        "the pane's terminal never sees the conversation's command"
    );
}

/// `!grok` on an agent surface opens the grok harness's view; the harness's
/// TUI is never started in the pane's terminal. Esc therefore returns to the
/// shell's own history with the conversation's card in it, rather than to a
/// full-screen program the user never launched there.
#[test]
fn a_bang_harness_line_opens_the_view_without_starting_its_tui_in_the_terminal() {
    let fixture = fixture();
    {
        let mut state = fixture.state.borrow_mut();
        state.spaces = vec![Space::new(
            "Shell",
            Pane::Leaf {
                id: 1,
                kind: PaneKind::Terminal,
            },
        )];
        state.active_space = 0;
        state.active_pane_id = 1;
        state
            .pane_sessions
            .get_mut(&1)
            .expect("pane 1 has a session")
            .conversation_id
            .clear();
        let mut terminal = bootstrapped();
        terminal.writer = Some(Box::new(CaptureWriter(Arc::clone(&fixture.terminal_bytes))));
        state.terminal.borrow_mut().sessions.insert(1, terminal);
    }

    // An agent surface over the pane's shell, then the harness line typed in it.
    (fixture.actions.on_set_pane_harness_mode.borrow_mut())(1, true);
    let (_grok_dir, grok) = installed_harness("grok");
    (fixture.actions.on_send_message.borrow_mut())(format!("!{grok}"));

    let conversation = {
        let state = fixture.state.borrow();
        let conversation = conversation_of(&state);
        assert_eq!(
            state.pane_view(1),
            goble_terminal::blocks::BlockView::Agent {
                conversation_id: conversation.clone()
            },
            "the line opens the harness's agent view"
        );
        assert!(
            !state.terminal.borrow().agent_sessions.contains_key(&1),
            "the launch runs no command in the conversation's shell"
        );
        conversation
    };
    assert!(
        captured(&fixture.terminal_bytes).is_empty(),
        "the harness's TUI is never started in the pane's terminal"
    );
    assert_eq!(
        fixture
            .desktop
            .list_chats()
            .into_iter()
            .find(|chat| chat.id == conversation)
            .and_then(|chat| chat.provider),
        Some("grok".to_string()),
        "the view is the grok conversation's"
    );

    // Esc, back to the terminal: the conversation's card is what the shell
    // shows, and nothing of the harness is drawn there.
    (fixture.actions.on_set_pane_harness_mode.borrow_mut())(1, false);
    let cards: Vec<String> = fixture
        .state
        .borrow()
        .pane_terminal_view(1)
        .into_iter()
        .filter_map(|block| block.card.map(|card| card.conversation_id))
        .collect();
    assert!(
        cards.contains(&conversation),
        "the terminal holds the harness conversation's card: {cards:?}"
    );
}

fn conversation_of(state: &UiState) -> String {
    state
        .pane_sessions
        .get(&1)
        .map(|session| session.conversation_id.clone())
        .filter(|conversation| !conversation.is_empty())
        .expect("the launch bound the pane to a conversation")
}
