//! A1, A2 and A4: the pane a conversation routed to a worker gets — a viewer
//! session with no local shell behind it, which re-attaches to the session it
//! runs on instead of starting over and which leaves no session behind once its
//! pane or its tab is gone.
//!
//! A1's three asks are asserted through the real paths: the routing decision the
//! app persists decides the pane's kind, the worker's own `worker:status` report
//! (the payload `worker_messages.rs` emits) moves the connection between its
//! states, and neither a drop nor a command typed at the pane ever mounts a local
//! pty — with a sibling shell pane in the same space proving the frame is
//! otherwise willing to.
//!
//! A2's are asserted the same way, against a real session on a real daemon: the
//! pane opening on a conversation with history replays that session's own
//! transcript (a harness that writes no store rows, so nothing could be guessed
//! locally), a drop-and-return re-attaches to the same session id and replays
//! only the turns it missed, and a conversation with no session says so instead
//! of quietly starting one.
//!
//! A4's two are asserted over the app's own event bus as well: closing a tab
//! that held a viewer pane, and closing a viewer pane, each leave no viewer
//! session behind, and the `worker:status` reports that follow move the live
//! viewer pane beside them while touching no dead pane id.

use super::*;
use goble_core::protocol::WorkerMessage;
use goble_core::worker::WorkerId;

use crate::state::PaneAttach;
use crate::ui::{Pane, PaneKind, Space, SplitDir};

/// Like [`root_with_bus`], handing the backend back too, so a case can pair a
/// worker and route a conversation before the root reads either.
fn root_with_backend() -> (
    RootView,
    CollectingEventBus,
    Arc<DesktopState>,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    );
    let bus = CollectingEventBus::new();
    let view = RootView::new(&AppContext::default(), &desktop, Some(bus.clone()));
    (view, bus, desktop, dir)
}

/// A backend holding a paired worker, the way a paired VPS reaches the app: the
/// worker row plus the `Paired` message its channel confirms with. No live host
/// exists here — the pool's answer is what the pane resolves, not a socket.
fn pair_worker(desktop: &DesktopState, id: &str) {
    let worker = WorkerId(id.to_string());
    desktop
        .add_worker(
            worker.clone(),
            "vps".to_string(),
            "ws://vps:8787/ws".to_string(),
        )
        .expect("register the worker");
    desktop.handle_worker_message(&worker, WorkerMessage::Paired);
}

/// A conversation in the store, routed where `routing` says.
fn conversation(desktop: &DesktopState, title: &str, routing: &str) -> String {
    let id = desktop
        .create_chat(title, None, None)
        .expect("create the conversation");
    desktop
        .set_chat_workspace_routing(&id, Some(routing))
        .expect("persist the routing");
    id
}

/// The payload `worker_messages.rs` emits for a `StatusReport`: the service's
/// own `format!("{:?}", status)` words, plus the load it carries.
fn status_report(worker: &str, status: &str) -> serde_json::Value {
    serde_json::json!({ "worker_id": worker, "status": status, "load": 0 })
}

/// Whether the pane has a local shell behind it.
fn has_shell(state: &UiState, pane_id: u64) -> bool {
    state.terminal.borrow().sessions.contains_key(&pane_id)
}

/// The text one frame of the mounted root draws inside the pane area.
fn pane_texts(root: &mut Box<dyn Element>) -> Vec<String> {
    use goble_ui::geometry::vec2f;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;

    render_element(root, vec2f(1024.0, 768.0), &AppContext::default())
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. }
                if origin.x >= crate::ui::SIDEBAR_WIDTH =>
            {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}

/// The app's own actions over the mounted root's state, so a test can submit at
/// a pane exactly the way the rich input bar does.
fn actions_for(root: &RootView, desktop: &Arc<DesktopState>) -> crate::ui::UiActions {
    crate::actions::make_actions(
        root.state_rc(),
        Some(Arc::clone(desktop)),
        Rc::new(RefCell::new(MediaState::mock())),
        WindowControl::default(),
        Rc::new(RefCell::new(1.0)),
    )
}

/// Take the first-run overlays out of the frame, the way the other render cases
/// do, so what is drawn is the panes.
fn clear_overlays(state: &Rc<RefCell<UiState>>) {
    let mut state = state.borrow_mut();
    state.show_workspace_choice = false;
    state.show_llm_key_banner = false;
    state.show_onboarding_tip = false;
}

#[test]
fn a_remote_routed_conversation_builds_a_pane_with_no_local_terminal() {
    let (root, _bus, desktop, _dir) = root_with_backend();
    pair_worker(&desktop, "vps");
    let conv = conversation(&desktop, "Ship it remotely", "remote");

    // The pane starts as a shell pane with a live session behind it: the
    // routing decision is what has to take that away.
    {
        let state = root.state_rc();
        let state = state.borrow_mut();
        state.terminal.borrow_mut().ensure_session(1, "/tmp");
    }
    assert!(
        has_shell(&root.state_rc().borrow(), 1),
        "the fixture pane had a local shell to lose"
    );

    root.state_rc()
        .borrow_mut()
        .bind_active_pane_conversation(conv, Some(&desktop));

    let state = root.state_rc();
    let state = state.borrow();
    assert_eq!(
        state.spaces[state.active_space].leaf_kind(1),
        Some(&PaneKind::Worker),
        "a remote-routed conversation is a viewer pane"
    );
    assert!(state.pane_is_viewer(1), "and is read as one");
    assert_eq!(
        state.pane_worker(1).map(|w| w.worker_id.as_str()),
        Some("vps"),
        "the routing resolved to the paired worker"
    );
    assert_eq!(
        state.pane_attach(1),
        Some(&PaneAttach::Connecting),
        "a freshly opened session is connecting"
    );
    assert!(
        !has_shell(&state, 1),
        "nothing local backs the pane: no shell, no pty"
    );
}

#[test]
fn a_dropped_connection_moves_the_pane_to_its_detached_state() {
    let (mut root, bus, desktop, _dir) = root_with_backend();
    pair_worker(&desktop, "vps");
    let conv = conversation(&desktop, "Ship it remotely", "remote");
    root.state_rc()
        .borrow_mut()
        .bind_active_pane_conversation(conv, Some(&desktop));
    assert_eq!(
        root.state_rc().borrow().pane_attach(1),
        Some(&PaneAttach::Connecting),
        "the pane opens connecting"
    );

    // The worker reports it can still serve: the channel is live.
    bus.emit("worker:status", status_report("vps", "Online"));
    root.drain_events();
    assert_eq!(
        root.state_rc().borrow().pane_attach(1),
        Some(&PaneAttach::Attached),
        "a serving report attaches the pane"
    );

    // The channel drops. The pane says so, and stays the pane it was.
    bus.emit("worker:status", status_report("vps", "Offline"));
    root.drain_events();
    let state = root.state_rc();
    let state = state.borrow();
    let reason = "worker vps reported Offline".to_string();
    assert_eq!(
        state.pane_attach(1),
        Some(&PaneAttach::Detached { reason }),
        "a dropped connection is the pane's detached state"
    );
    assert_eq!(
        state.spaces[state.active_space].leaf_kind(1),
        Some(&PaneKind::Worker),
        "the drop does not turn the pane into a shell pane"
    );
}

#[test]
fn a_detached_viewer_pane_never_falls_back_to_a_local_shell() {
    let (mut root, bus, desktop, _dir) = root_with_backend();
    pair_worker(&desktop, "vps");
    let conv = conversation(&desktop, "Ship it remotely", "remote");
    root.state_rc()
        .borrow_mut()
        .bind_active_pane_conversation(conv, Some(&desktop));

    // A sibling shell pane in the same space: "no session" is this pane's
    // doing, not the frame's.
    let second = {
        let state = root.state_rc();
        let mut state = state.borrow_mut();
        let (space, pane) = (state.active_space, state.active_pane_id);
        let mut next = state.next_pane_id;
        let second = state.spaces[space]
            .split_with_kind(pane, SplitDir::Vertical, &mut next, PaneKind::Terminal)
            .expect("open a terminal pane beside the viewer pane");
        state.next_pane_id = next;
        state.pane_sessions.entry(second).or_default().path = "/tmp".to_string();
        second
    };

    bus.emit("worker:status", status_report("vps", "Online"));
    root.drain_events();
    bus.emit("worker:status", status_report("vps", "Offline"));
    root.drain_events();

    // A command submitted at the pane is refused: there is no local shell to
    // run it in, whatever the line says.
    let actions = actions_for(&root, &desktop);
    (actions.on_run_shell_command.borrow_mut())(1, "echo hello".to_string());
    assert!(
        !has_shell(&root.state_rc().borrow(), 1),
        "a command at a viewer pane runs nowhere, not in a local shell"
    );

    // One frame with both panes drawn. The shell pane mounts its pty; the
    // viewer pane cannot, and draws its connection's state in place of one.
    assert!(
        !has_shell(&root.state_rc().borrow(), second),
        "the sibling shell pane mounts its pty when the frame is drawn, not before"
    );
    clear_overlays(&root.state_rc());
    let state = root.state_rc();
    let mut root: Box<dyn Element> = Box::new(root);
    let drawn = pane_texts(&mut root);

    let state = state.borrow();
    assert!(
        has_shell(&state, second),
        "the sibling shell pane mounted its pty, so the frame is not vacuously shell-less"
    );
    assert!(!has_shell(&state, 1), "and the viewer pane still has none");
    assert!(
        state.pane_is_viewer(1),
        "a dropped connection left the pane a viewer"
    );
    // The line carries the drop and what the pane found when it re-attached on
    // the report that came before it: this conversation has no session on the
    // worker, which the pane says instead of starting one (A2).
    assert!(
        drawn
            .iter()
            .any(|t| t == "vps · disconnected: worker vps reported Offline · no session to rejoin"),
        "the pane draws the drop where its shell would have been: {drawn:?}"
    );
}

#[test]
fn a_remote_conversation_with_no_paired_worker_fails_without_a_shell() {
    // Nothing paired: the routing cannot resolve a worker. That is not a reason
    // to run the conversation in a local terminal — the pane is a viewer that
    // says it could not connect.
    let (root, _bus, desktop, _dir) = root_with_backend();
    let conv = conversation(&desktop, "Ship it remotely", "remote");
    root.state_rc()
        .borrow_mut()
        .bind_active_pane_conversation(conv, Some(&desktop));

    let state = root.state_rc();
    let state = state.borrow();
    assert_eq!(
        state.spaces[state.active_space].leaf_kind(1),
        Some(&PaneKind::Worker),
        "an unresolved remote conversation is still a viewer pane"
    );
    match state.pane_attach(1) {
        Some(PaneAttach::Failed { message }) => assert!(
            message.contains("no paired worker"),
            "the pane says what is missing: {message}"
        ),
        other => panic!("expected a failed viewer session, got {other:?}"),
    }
    assert!(!has_shell(&state, 1), "no worker is not a local fallback");
    let worker_id = state
        .pane_worker(1)
        .map(|session| session.worker_id.as_str())
        .unwrap_or_default();
    let line = state
        .pane_attach(1)
        .map(|attach| attach.words(worker_id))
        .unwrap_or_default();
    assert!(
        line.starts_with("no paired worker"),
        "the pane's own line names what is missing: {line}"
    );
}

// --- A2: re-attach instead of starting over ---------------------------------

/// Run one turn on a registered mock harness under `session_id`, the key a
/// pane's conversation runs on, and wait for it to settle.
///
/// The mock writes no store rows: a transcript the pane shows after a re-attach
/// can only be the session's own recording.
fn run_session_turn(
    desktop: &Arc<DesktopState>,
    session_id: &str,
    prompt: &str,
    harness: &str,
    reply: &str,
) {
    desktop.register_harness(Arc::new(goble_harness_runtime::MockHarness::new(
        goble_harness_types::HarnessId::new(harness),
        reply,
    )));
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let _guard = rt.enter();
    let handle = desktop
        .run_chat_turn(
            session_id,
            prompt,
            "mock",
            "",
            "local",
            "default",
            session_id,
            None,
            Some(harness),
        )
        .expect("run the turn on the registered harness");
    rt.block_on(handle).expect("the turn settles");
}

/// The transcript a pane replayed from its session, as `(role, content)` rows.
fn replayed_rows(state: &UiState, pane_id: u64) -> Vec<(String, String)> {
    state
        .pane_runtime
        .get(&pane_id)
        .map(|rt| {
            rt.session_rows
                .iter()
                .map(|row| (row.role.clone(), row.content.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// The session a viewer pane re-attached to, as it stands now.
fn session_state(state: &UiState, pane_id: u64) -> Option<crate::state::WorkerPaneSession> {
    state.pane_worker(pane_id).and_then(|w| w.session.clone())
}

#[test]
fn an_open_on_a_viewer_pane_with_a_session_replays_that_session() {
    let (root, _bus, desktop, _dir) = root_with_backend();
    pair_worker(&desktop, "vps");
    let conv = conversation(&desktop, "Ship it remotely", "remote");
    // The conversation already ran on the worker: its session is there, under
    // the conversation's own id, and the run has settled.
    run_session_turn(&desktop, &conv, "run the tests", "cli-a", "all green");
    assert!(
        desktop.list_chat_messages(&conv).unwrap().is_empty(),
        "the fixture harness writes nothing to this machine's store"
    );

    root.state_rc()
        .borrow_mut()
        .bind_active_pane_conversation(conv.clone(), Some(&desktop));

    let state = root.state_rc();
    let state = state.borrow();
    assert_eq!(
        state.pane_attach(1),
        Some(&PaneAttach::Attached),
        "the session was rejoined, so the pane is attached to it"
    );
    match session_state(&state, 1) {
        Some(crate::state::WorkerPaneSession::Attached { session_id, turns }) => {
            assert_eq!(session_id, conv, "the session's own id, not a new one");
            assert_eq!(turns, 1, "the turn the session had already run");
        }
        other => panic!("expected a re-attached session, got {other:?}"),
    }
    assert_eq!(
        replayed_rows(&state, 1),
        vec![
            ("user".to_string(), "run the tests".to_string()),
            ("assistant".to_string(), "all green".to_string()),
        ],
        "the pane's transcript is the session's own recording"
    );
    drop(state);

    // And it is what the pane draws, over a store that holds none of it.
    clear_overlays(&root.state_rc());
    let mut root: Box<dyn Element> = Box::new(root);
    let drawn = pane_texts(&mut root);
    assert!(
        drawn.iter().any(|t| t.contains("all green")),
        "the session's reply is on screen: {drawn:?}"
    );
    assert!(
        drawn
            .iter()
            .any(|t| t.contains("session") && t.contains("replayed")),
        "the pane's report names the session it rejoined: {drawn:?}"
    );
}

#[test]
fn a_drop_and_return_re_attaches_to_the_same_session_id() {
    let (mut root, bus, desktop, _dir) = root_with_backend();
    pair_worker(&desktop, "vps");
    let conv = conversation(&desktop, "Ship it remotely", "remote");
    run_session_turn(&desktop, &conv, "run the tests", "cli-a", "all green");
    root.state_rc()
        .borrow_mut()
        .bind_active_pane_conversation(conv.clone(), Some(&desktop));
    let opened = match session_state(&root.state_rc().borrow(), 1) {
        Some(link) => link,
        None => panic!("the open must have rejoined the session"),
    };

    // The connection drops. The run does not: the worker drives its own daemon.
    bus.emit("worker:status", status_report("vps", "Offline"));
    root.drain_events();
    assert!(matches!(
        root.state_rc().borrow().pane_attach(1),
        Some(PaneAttach::Detached { .. })
    ));

    // While the client was away the session ran another turn.
    run_session_turn(&desktop, &conv, "and again", "cli-b", "still green");

    bus.emit("worker:status", status_report("vps", "Online"));
    root.drain_events();

    let state = root.state_rc();
    let state = state.borrow();
    assert_eq!(
        state.pane_attach(1),
        Some(&PaneAttach::Attached),
        "the report brought the pane back by re-attaching its session"
    );
    match session_state(&state, 1) {
        Some(crate::state::WorkerPaneSession::Attached { session_id, turns }) => {
            let left = match &opened {
                crate::state::WorkerPaneSession::Attached { session_id, .. } => session_id,
                other => panic!("expected an attached session, got {other:?}"),
            };
            assert_eq!(
                session_id.as_str(),
                left.as_str(),
                "the same session id it left, never a new session"
            );
            assert_eq!(turns, 2, "the turn the client missed is now held");
        }
        other => panic!("expected a re-attached session, got {other:?}"),
    }
    assert_eq!(
        replayed_rows(&state, 1),
        vec![
            ("user".to_string(), "run the tests".to_string()),
            ("assistant".to_string(), "all green".to_string()),
            ("user".to_string(), "and again".to_string()),
            ("assistant".to_string(), "still green".to_string()),
        ],
        "the missed turn is appended once, and nothing already held is replayed"
    );
}

#[test]
fn a_viewer_conversation_with_no_session_says_so_and_starts_none() {
    let (mut root, bus, desktop, _dir) = root_with_backend();
    pair_worker(&desktop, "vps");
    let conv = conversation(&desktop, "Never ran there", "remote");
    root.state_rc()
        .borrow_mut()
        .bind_active_pane_conversation(conv.clone(), Some(&desktop));

    let state = root.state_rc();
    let state = state.borrow();
    assert_eq!(
        session_state(&state, 1),
        Some(crate::state::WorkerPaneSession::NoSession),
        "the conversation has no session there, and the pane records that"
    );
    assert!(
        replayed_rows(&state, 1).is_empty(),
        "there is no session transcript to show"
    );
    drop(state);

    // The worker comes up and reports it is serving. The pane's channel is live
    // — and its conversation still has no session, which it says rather than
    // starting one.
    bus.emit("worker:status", status_report("vps", "Online"));
    root.drain_events();
    let state = root.state_rc();
    let state = state.borrow();
    assert_eq!(
        state.pane_attach(1),
        Some(&PaneAttach::Attached),
        "the channel to the worker is up"
    );
    assert_eq!(
        session_state(&state, 1),
        Some(crate::state::WorkerPaneSession::NoSession)
    );
    let line = state
        .pane_chat_snapshot()
        .get(&1)
        .and_then(|pane| pane.worker.as_ref())
        .map(|worker| worker.connection_line())
        .unwrap_or_default();
    assert!(
        line.contains("no session to rejoin"),
        "the pane's own line says so: {line}"
    );
    drop(state);

    assert!(
        desktop.daemon_client().snapshot().is_empty(),
        "no turn was started for it"
    );
    assert!(
        desktop.list_chat_messages(&conv).unwrap().is_empty(),
        "and nothing was written into the conversation"
    );
}

// --- A4: a viewer pane that is gone leaves no session behind -----------------

/// Add a tab holding one terminal pane, the way `on_add_space` does, and return
/// that pane's id. The pane gets its own `pane_sessions` entry, so binding a
/// conversation to it is the ordinary path.
fn add_tab(state: &Rc<RefCell<UiState>>) -> u64 {
    let mut state = state.borrow_mut();
    let id = state.next_pane_id;
    state.next_pane_id += 1;
    state.spaces.push(Space::unnamed(Pane::Leaf {
        id,
        kind: PaneKind::Terminal,
    }));
    state.active_space = state.spaces.len() - 1;
    state.active_pane_id = id;
    state.pane_sessions.entry(id).or_default();
    id
}

/// Split the active pane, giving the sibling `pane_id`'s space a second leaf of
/// `kind`, and return the new pane's id. The real split action is not what this
/// is about, so the tree is split directly and the sibling is left with no
/// conversation of its own.
fn split_beside(state: &Rc<RefCell<UiState>>, kind: PaneKind) -> u64 {
    let mut state = state.borrow_mut();
    let (space, pane) = (state.active_space, state.active_pane_id);
    let mut next = state.next_pane_id;
    let sibling = state.spaces[space]
        .split_with_kind(pane, SplitDir::Vertical, &mut next, kind)
        .expect("the pane splits");
    state.next_pane_id = next;
    state.pane_sessions.entry(sibling).or_default();
    state.active_pane_id = sibling;
    sibling
}

/// Whether any tab still holds `pane_id` as a leaf.
fn still_in_a_tree(state: &UiState, pane_id: u64) -> bool {
    state
        .spaces
        .iter()
        .any(|space| space.leaf_kind(pane_id).is_some())
}

/// A closed tab takes the viewer session of the pane it held with it.
///
/// The tab held two viewer panes on the same worker, one in each tab: the report
/// that follows the close has a live pane to move, so a dead pane id left alone
/// is this fix's doing and not a status path that goes nowhere.
#[test]
fn closing_a_tab_holding_a_viewer_pane_leaves_no_session_behind() {
    let (mut root, bus, desktop, _dir) = root_with_backend();
    pair_worker(&desktop, "vps");
    let first = conversation(&desktop, "Ship it remotely", "remote");
    root.state_rc()
        .borrow_mut()
        .bind_active_pane_conversation(first, Some(&desktop));
    assert!(
        root.state_rc().borrow().pane_is_viewer(1),
        "the fixture pane is a viewer pane"
    );

    let live = {
        let state = root.state_rc();
        let id = add_tab(&state);
        let conv = conversation(&desktop, "Also on the worker", "remote");
        state
            .borrow_mut()
            .bind_active_pane_conversation(conv, Some(&desktop));
        id
    };
    {
        let state = root.state_rc();
        let state = state.borrow();
        assert!(
            state.pane_is_viewer(1) && state.pane_is_viewer(live),
            "two viewer panes on vps, one per tab: {:?}",
            state.pane_workers.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            state.pane_worker(live).map(|w| w.worker_id.as_str()),
            Some("vps"),
            "the second tab's viewer session is on the same worker"
        );
    }

    let actions = actions_for(&root, &desktop);
    (actions.on_close_space.borrow_mut())(0);

    {
        let state = root.state_rc();
        let state = state.borrow();
        assert!(
            !state.pane_workers.contains_key(&1),
            "the closed tab's viewer pane kept no session: {:?}",
            state.pane_workers.keys().collect::<Vec<_>>()
        );
        assert!(state.pane_worker(1).is_none());
        assert!(
            !still_in_a_tree(&state, 1),
            "and the pane went with the tab"
        );
        assert!(
            !state.pane_runtime.contains_key(&1),
            "its runtime went with it too"
        );
        assert!(
            !state.pane_chat_snapshot().contains_key(&1),
            "nothing draws the closed pane"
        );
        assert!(
            state.pane_worker(live).is_some(),
            "the tab that did not close keeps its viewer session"
        );
    }

    // Every worker report after the close: the live pane's connection moves and
    // the dead pane id is not in the map to move.
    bus.emit("worker:status", status_report("vps", "Online"));
    root.drain_events();
    {
        let state = root.state_rc();
        let state = state.borrow();
        assert_eq!(
            state.pane_attach(live),
            Some(&PaneAttach::Attached),
            "the report reached the live viewer pane"
        );
        assert!(
            state.pane_attach(1).is_none(),
            "and touched no dead pane id: {:?}",
            state.pane_workers.keys().collect::<Vec<_>>()
        );
    }
    bus.emit("worker:status", status_report("vps", "Offline"));
    root.drain_events();
    {
        let state = root.state_rc();
        let state = state.borrow();
        assert!(
            matches!(state.pane_attach(live), Some(PaneAttach::Detached { .. })),
            "the drop reached the live viewer pane: {:?}",
            state.pane_attach(live)
        );
        assert!(
            !state.pane_workers.contains_key(&1),
            "and the dead pane id was not re-attached or re-detached"
        );
    }
}

/// The pane close is the other teardown path, and it is the same rule: the
/// closed pane's viewer session goes with it while the sibling viewer pane in
/// the same space keeps its own.
#[test]
fn closing_a_viewer_pane_leaves_no_session_behind() {
    let (mut root, bus, desktop, _dir) = root_with_backend();
    pair_worker(&desktop, "vps");
    let conv = conversation(&desktop, "Ship it remotely", "remote");
    root.state_rc()
        .borrow_mut()
        .bind_active_pane_conversation(conv, Some(&desktop));

    let sibling = {
        let state = root.state_rc();
        let sibling = split_beside(&state, PaneKind::Terminal);
        let conv = conversation(&desktop, "Also on the worker", "remote");
        state
            .borrow_mut()
            .bind_active_pane_conversation(conv, Some(&desktop));
        sibling
    };
    {
        let state = root.state_rc();
        let state = state.borrow();
        assert!(
            state.pane_is_viewer(1) && state.pane_is_viewer(sibling),
            "two viewer panes in the space: {:?}",
            state.pane_workers.keys().collect::<Vec<_>>()
        );
    }

    // The reports this worker sends are the ones the assertions below follow, so
    // they are shown to reach a viewer pane of this worker before the close.
    bus.emit("worker:status", status_report("vps", "Online"));
    root.drain_events();
    assert_eq!(
        root.state_rc().borrow().pane_attach(1),
        Some(&PaneAttach::Attached),
        "the worker's report reaches this pane before it is closed"
    );

    root.state_rc().borrow_mut().active_pane_id = 1;
    let actions = actions_for(&root, &desktop);
    (actions.on_close_pane.borrow_mut())();

    {
        let state = root.state_rc();
        let state = state.borrow();
        assert!(!still_in_a_tree(&state, 1), "the closed pane left the tree");
        assert!(
            !state.pane_workers.contains_key(&1),
            "and left no viewer session behind: {:?}",
            state.pane_workers.keys().collect::<Vec<_>>()
        );
        assert!(
            !state.pane_runtime.contains_key(&1),
            "its runtime went with it"
        );
        assert!(
            state.pane_worker(sibling).is_some(),
            "the pane beside it keeps its own session"
        );
    }

    bus.emit("worker:status", status_report("vps", "Online"));
    root.drain_events();
    {
        let state = root.state_rc();
        let state = state.borrow();
        assert_eq!(
            state.pane_attach(sibling),
            Some(&PaneAttach::Attached),
            "the report reached the pane that is still there"
        );
        assert!(
            state.pane_attach(1).is_none(),
            "and touched no dead pane id: {:?}",
            state.pane_workers.keys().collect::<Vec<_>>()
        );
    }
    bus.emit("worker:status", status_report("vps", "Offline"));
    root.drain_events();
    {
        let state = root.state_rc();
        let state = state.borrow();
        assert!(
            matches!(
                state.pane_attach(sibling),
                Some(PaneAttach::Detached { .. })
            ),
            "the drop reached the surviving pane: {:?}",
            state.pane_attach(sibling)
        );
        assert!(
            !state.pane_workers.contains_key(&1),
            "the dead pane id was not re-attached or re-detached"
        );
    }
}
