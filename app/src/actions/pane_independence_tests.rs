use super::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::store::Store;
use goble_desktop_service::DesktopState;
use goble_desktop_service::ThreadStore;
use goble_ui::platform::WindowControl;

use crate::media::MediaState;
use crate::state::UiState;
use crate::ui::{AppTab, Pane, PaneKind, UiActions, WorkspaceRouting};

fn desktop_state() -> (Arc<DesktopState>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = DesktopState::new(
        Store::open_in_memory().expect("store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    );
    (state, dir)
}

fn build(
    desktop: &Arc<DesktopState>,
) -> (Rc<RefCell<UiState>>, UiActions, Rc<RefCell<MediaState>>) {
    let state = Rc::new(RefCell::new(UiState::from_desktop(desktop)));
    let media = Rc::new(RefCell::new(MediaState::mock()));
    let actions = make_actions(
        Rc::clone(&state),
        Some(Arc::clone(desktop)),
        Rc::clone(&media),
        WindowControl::default(),
        Rc::new(RefCell::new(1.0)),
    );
    (state, actions, media)
}

#[test]
fn split_binds_new_pane_to_a_distinct_conversation() {
    let (desktop, _dir) = desktop_state();
    let chat_id = desktop.create_chat("Initial", None, None).expect("create chat");
    let (state, actions, _media) = build(&desktop);

    (actions.on_split_right.borrow_mut())();

    let s = state.borrow();
    assert_eq!(s.active_pane_id, 3, "split creates + focuses a new leaf");
    let conv1 = s
        .pane_sessions
        .get(&1)
        .map(|x| x.conversation_id.clone())
        .unwrap_or_default();
    let conv3 = s
        .pane_sessions
        .get(&3)
        .map(|x| x.conversation_id.clone())
        .unwrap_or_default();
    assert!(!conv3.is_empty(), "split pane binds a new conversation");
    assert_ne!(conv1, conv3, "two panes never share a conversation");
    assert!(
        !s.pane_sessions.get(&3).unwrap().path.is_empty(),
        "new pane initializes a cwd"
    );
    assert_eq!(
        s.selected_id.as_deref(),
        Some(conv3.as_str()),
        "selected_id mirrors the active pane's conversation"
    );
    drop(s);
    let _ = chat_id;
}

#[test]
fn send_routes_to_active_pane_conversation() {
    let (desktop, _dir) = desktop_state();
    let chat_id = desktop.create_chat("Initial", None, None).expect("create chat");
    let (state, actions, _media) = build(&desktop);

    (actions.on_split_right.borrow_mut())();
    let conv3 = { state.borrow().pane_sessions.get(&3).unwrap().conversation_id.clone() };
    assert_ne!(conv3, chat_id);

    // No key configured: the user message + an honest assistant reply are
    // kept, routed to pane 3's own conversation, not pane 1's.
    (actions.on_composer_change.borrow_mut())("hi".to_string());
    (actions.on_send_message.borrow_mut())("hi".to_string());

    let messages3 = desktop.list_chat_messages(&conv3).expect("list pane 3");
    assert_eq!(messages3.len(), 2);
    assert_eq!(messages3[0].role, "user");
    assert_eq!(messages3[1].role, "assistant");
    let messages1 = desktop.list_chat_messages(&chat_id).expect("list pane 1");
    assert_eq!(messages1.len(), 0, "pane 1's conversation stays untouched");
}

#[test]
fn cmd_enter_appends_to_the_pane_conversation() {
    let (desktop, _dir) = desktop_state();
    let (state, actions, _media) = build(&desktop);
    // Bind pane 1 to an initial conversation so Cmd/Ctrl+Enter has a thread
    // to append to instead of starting a brand-new one each send.
    let initial = desktop.create_chat("Initial", None, None).expect("create chat");
    state
        .borrow_mut()
        .bind_active_pane_conversation(initial.clone(), Some(&desktop));
    let initial = state.borrow().pane_conversation_id(1).unwrap();

    // No key configured: Cmd/Ctrl+Enter keeps the turn on the pane's own
    // conversation and appends the user message + an honest assistant reply;
    // it does NOT create a new conversation.
    (actions.on_cmd_enter.borrow_mut())("hello from cmd+enter".to_string());

    let conv = state.borrow().pane_conversation_id(1).unwrap();
    assert_eq!(conv, initial, "Cmd+Enter reuses the pane's conversation");
    let msgs = desktop.list_chat_messages(&conv).expect("list conv");
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[1].role, "assistant");
}

#[test]
fn closing_a_space_removes_its_sessions_and_reindexes_active() {
    let (desktop, _dir) = desktop_state();
    let (state, actions, _media) = build(&desktop);
    // Add a second space so there are two tabs to close from.
    (actions.on_add_space.borrow_mut())();
    assert_eq!(state.borrow().spaces.len(), 2);
    // Give the first space's pane (id 1) a runtime so we can see it dropped.
    state.borrow_mut().pane_runtime.entry(1).or_default();
    // Close the first (non-active) space tab.
    (actions.on_close_space.borrow_mut())(0);
    let s = state.borrow();
    assert_eq!(s.spaces.len(), 1, "one space remains after closing one of two");
    assert_eq!(s.active_space, 0, "active index shifts down onto the survivor");
    assert!(
        !s.pane_sessions.contains_key(&1),
        "closed space's pane session is dropped"
    );
    assert!(
        !s.pane_runtime.contains_key(&1),
        "closed space's pane runtime is dropped"
    );
    assert_ne!(s.active_pane_id, 1, "focus moves to the surviving space's leaf");
}

#[test]
fn closing_the_last_space_resets_to_a_fresh_space() {
    let (desktop, _dir) = desktop_state();
    let (state, actions, _media) = build(&desktop);
    assert_eq!(state.borrow().spaces.len(), 1);
    // Close the only tab: the app must not empty out (that would tear down
    // the window); instead a fresh space replaces it.
    (actions.on_close_space.borrow_mut())(0);
    let s = state.borrow();
    assert_eq!(s.spaces.len(), 1, "closing the last tab never empties the app");
    assert_eq!(s.active_space, 0);
    assert_ne!(
        s.pane_sessions.get(&s.active_pane_id).map(|x| x.conversation_id.as_str()),
        None,
        "the fresh space gets a pane session"
    );
}

#[test]
fn bang_prefix_runs_a_terminal_command_instead_of_an_agent_turn() {
    let (desktop, _dir) = desktop_state();
    let (state, actions, _media) = build(&desktop);
    // Bind pane 1 to a real conversation before routing an input to it.
    let initial = desktop.create_chat("Initial", None, None).expect("create chat");
    state
        .borrow_mut()
        .bind_active_pane_conversation(initial.clone(), Some(&desktop));
    let conv = state
        .borrow()
        .pane_conversation_id(state.borrow().active_pane_id)
        .unwrap();

    // `!pwd` is classified as a terminal command (agent mode + `!`), so it
    // must NOT add an agent message to the pane's conversation.
    (actions.on_send_message.borrow_mut())("!pwd".to_string());
    let msgs = desktop.list_chat_messages(&conv).unwrap();
    assert_eq!(
        msgs.len(),
        0,
        "a `!`-prefixed input runs in the shell, not the agent"
    );
}

#[test]
fn selecting_environment_session_updates_active_pane_path() {
    let (desktop, _dir) = desktop_state();
    let (state, _actions, media) = build(&desktop);

    let media_actions = crate::media::make_media_actions(
        Rc::clone(&media),
        Rc::clone(&state),
        Some(Arc::clone(&desktop)),
    );
    // Picking the remote session leaf scopes the turn to medium+project+session
    // and points the active pane's cwd at that session's path.
    (media_actions.on_select_session.borrow_mut())(
        "remote-xrdp".to_string(),
        "remote".to_string(),
        "s-remote-xrdp".to_string(),
    );

    assert_eq!(state.borrow().composer_path, "/workspace/remote");
    assert_eq!(
        state.borrow().pane_sessions.get(&1).unwrap().path,
        "/workspace/remote",
        "active pane's cwd is stored per-pane"
    );
    let media_b = media.borrow();
    assert_eq!(media_b.selected_medium_id(), "remote-xrdp");
    assert_eq!(media_b.selected_project_id(), "remote");
    assert_eq!(media_b.selected_session_id(), "s-remote-xrdp");
}

#[test]
fn on_select_harness_selects_medium() {
    // The composer's left pill selects the work environment (medium), not a
    // registered harness: picking a medium resets the project to its default
    // and closes the menu; an unknown id leaves the selection unchanged.
    let (desktop, _dir) = desktop_state();
    let (_state, actions, media) = build(&desktop);
    assert_eq!(media.borrow().selected_medium_id(), "local");

    (actions.on_select_harness.borrow_mut())(1, "remote-xrdp".to_string());
    assert_eq!(media.borrow().selected_medium_id(), "remote-xrdp");
    assert_eq!(media.borrow().selected_project_id(), "remote");
    assert_eq!(media.borrow().selected_session_id(), "");

    (actions.on_select_harness.borrow_mut())(1, "nope".to_string());
    assert_eq!(
        media.borrow().selected_medium_id(),
        "remote-xrdp",
        "an unknown medium id leaves the selection unchanged"
    );
}

#[test]
fn add_space_opens_a_plain_pty_without_a_conversation() {
    let (desktop, _dir) = desktop_state();
    let (state, actions, _media) = build(&desktop);
    assert_eq!(desktop.list_chats().len(), 0, "no conversation to start with");

    (actions.on_add_space.borrow_mut())();

    let s = state.borrow();
    let last = s.spaces.last().expect("a new space was added");
    assert!(
        matches!(last.root, Pane::Leaf { kind: PaneKind::Terminal, .. }),
        "a new workspace is a plain PTY"
    );
    assert!(
        !s.pane_owns_conversation(s.active_pane_id),
        "opening a workspace must not create a conversation"
    );
    drop(s);
    assert_eq!(
        desktop.list_chats().len(),
        0,
        "opening a workspace must not persist an empty conversation"
    );
}

#[test]
fn first_agent_turn_in_a_pty_workspace_creates_its_conversation() {
    let (desktop, _dir) = desktop_state();
    let (state, actions, _media) = build(&desktop);
    (actions.on_add_space.borrow_mut())();
    assert_eq!(desktop.list_chats().len(), 0);

    let pane_id = state.borrow().active_pane_id;
    (actions.on_terminal_command.borrow_mut())(pane_id, "salut".to_string());

    let s = state.borrow();
    let conv = s
        .pane_sessions
        .get(&pane_id)
        .map(|x| x.conversation_id.clone())
        .unwrap_or_default();
    assert!(!conv.is_empty(), "the pane owns a conversation after its turn");
    drop(s);
    assert_eq!(
        desktop.list_chats().len(),
        1,
        "the thread is created lazily, on the pane's first agent turn"
    );
}

#[test]
fn add_space_with_medium_sets_medium_and_workspace_routing() {
    let (desktop, _dir) = desktop_state();
    let (state, actions, media) = build(&desktop);

    (actions.on_add_space_with_medium.borrow_mut())("remote-xrdp".to_string());

    let s = state.borrow();
    let last = s.spaces.last().expect("a new space was added");
    assert_eq!(last.medium, "remote-xrdp", "space carries its default medium");
    assert_eq!(
        s.workspace_routing,
        Some(WorkspaceRouting::Remote),
        "new thread routes remote"
    );
    assert_eq!(s.active_space, s.spaces.len() - 1, "new space becomes active");
    drop(s);
    assert_eq!(
        media.borrow().selected_medium_id(),
        "remote-xrdp",
        "the chosen environment becomes the active medium"
    );
}

#[test]
fn on_add_medium_adds_persists_and_selects_custom_medium() {
    let (desktop, _dir) = desktop_state();
    let (state, _actions, media) = build(&desktop);
    let media_actions = crate::media::make_media_actions(
        Rc::clone(&media),
        Rc::clone(&state),
        Some(Arc::clone(&desktop)),
    );

    (media_actions.on_add_medium.borrow_mut())("Staging VPS".to_string());

    assert!(
        media.borrow().mediums.iter().any(|m| m.label == "Staging VPS"),
        "the custom medium is added to the tree"
    );
    assert_eq!(media.borrow().selected_medium_id(), "staging-vps");
    assert!(
        desktop.get_ui_mediums().is_some(),
        "the custom medium is persisted so it reappears"
    );
    assert!(
        !state.borrow().add_medium_dialog_open,
        "the add-medium dialog closes after adding"
    );
}

#[test]
fn new_terminal_splits_and_binds_own_conversation_and_cwd() {
    let (desktop, _dir) = desktop_state();
    let chat_id = desktop.create_chat("Initial", None, None).expect("create chat");
    let (state, actions, _media) = build(&desktop);

    (actions.on_new_terminal.borrow_mut())();

    let s = state.borrow();
    let term_pane = s.active_pane_id;
    assert_eq!(s.spaces[0].root.first_leaf_id(), 1, "chat leaf stays first");
    let term_conv = s.pane_sessions.get(&term_pane).unwrap().conversation_id.clone();
    assert_ne!(term_conv, chat_id, "terminal pane binds a distinct conversation");
    assert!(
        !s.pane_sessions.get(&term_pane).unwrap().path.is_empty(),
        "terminal pane initializes a cwd to spawn its shell"
    );
    drop(s);
}

#[test]
fn terminal_command_appends_to_the_pane_conversation() {
    let (desktop, _dir) = desktop_state();
    let chat_id = desktop.create_chat("Initial", None, None).expect("create chat");
    let (state, actions, _media) = build(&desktop);

    (actions.on_new_terminal.borrow_mut())();
    let (term_pane, term_conv) = {
        let s = state.borrow();
        let pane = s.active_pane_id;
        (
            pane,
            s.pane_sessions.get(&pane).unwrap().conversation_id.clone(),
        )
    };
    assert_ne!(term_conv, chat_id);

    // No API key configured: Cmd+Enter in the terminal keeps the turn on the
    // terminal pane's own conversation and appends a user message + an
    // honest assistant reply; it does NOT create a brand-new conversation.
    (actions.on_terminal_command.borrow_mut())(term_pane, "run me as an agent".to_string());

    let conv = state
        .borrow()
        .pane_sessions
        .get(&term_pane)
        .unwrap()
        .conversation_id
        .clone();
    assert_eq!(conv, term_conv, "Cmd+Enter reuses the terminal pane's conversation");
    let term_msgs = desktop.list_chat_messages(&conv).expect("list terminal conv");
    assert_eq!(term_msgs.len(), 2);
    assert_eq!(term_msgs[0].role, "user");
    assert_eq!(term_msgs[1].role, "assistant");
    // The pre-existing chat conversation stays untouched.
    let chat_msgs = desktop.list_chat_messages(&chat_id).expect("list chat conv");
    assert_eq!(chat_msgs.len(), 0, "chat conversation stays untouched");
}

#[test]
fn harness_tab_navigation_toggles_a_page_and_returns_to_chat() {
    let (desktop, _dir) = desktop_state();
    let (state, actions, _media) = build(&desktop);

    assert_eq!(state.borrow().current_tab, AppTab::Chat, "starts on chat");

    // Navigate to a harness observability tab via its topbar action.
    (actions.on_workflows.borrow_mut())();
    assert_eq!(state.borrow().current_tab, AppTab::Workflows);

    (actions.on_mcps.borrow_mut())();
    assert_eq!(state.borrow().current_tab, AppTab::Mcps);

    (actions.on_timeline.borrow_mut())();
    assert_eq!(state.borrow().current_tab, AppTab::Timeline);

    // Clicking the currently-open page again returns to the chat workspace.
    (actions.on_timeline.borrow_mut())();
    assert_eq!(state.borrow().current_tab, AppTab::Chat, "returns to chat");
}

#[test]
fn toggle_fullscreen_flips_state_and_requests_window() {
    let (desktop, _dir) = desktop_state();
    let state = Rc::new(RefCell::new(UiState::from_desktop(&desktop)));

    // Install a handler that records the requested fullscreen value.
    let last = Rc::new(RefCell::new(None));
    let last_clone = last.clone();
    let window_control = WindowControl::default();
    window_control.install(move |fullscreen| *last_clone.borrow_mut() = Some(fullscreen));

    let actions = make_actions(
        Rc::clone(&state),
        Some(Arc::clone(&desktop)),
        Rc::new(RefCell::new(MediaState::mock())),
        window_control,
        Rc::new(RefCell::new(1.0)),
    );

    assert!(!state.borrow().fullscreen, "window starts windowed");
    (actions.on_toggle_fullscreen.borrow_mut())();
    assert!(state.borrow().fullscreen, "toggle enters fullscreen");
    assert_eq!(last.borrow_mut().take(), Some(true), "window requested fullscreen");

    (actions.on_toggle_fullscreen.borrow_mut())();
    assert!(!state.borrow().fullscreen, "toggle leaves fullscreen");
    assert_eq!(last.borrow_mut().take(), Some(false), "window requested windowed");
}

#[test]
fn clear_transcript_clears_active_pane_messages() {
    let (desktop, _dir) = desktop_state();
    let chat_id = desktop.create_chat("Initial", None, None).expect("create chat");
    let state = Rc::new(RefCell::new(UiState::from_desktop(&desktop)));
    {
        let mut s = state.borrow_mut();
        s.bind_active_pane_conversation(chat_id.clone(), Some(desktop.as_ref()));
        desktop
            .add_chat_message(&chat_id, "user", "hello")
            .expect("add message");
        s.refresh_messages(&desktop);
        assert!(!s.chat_messages.is_empty(), "transcript is populated");
        let pane_id = s.active_pane_id;
        let rt = s.pane_runtime.entry(pane_id).or_default();
        rt.pending_command = Some(goble_ui::CommandProposalUi::new(
            "call-1".to_string(),
            vec!["git status".to_string()],
            "/workspace".to_string(),
        ));
        rt.command_selection = Some(Rc::new(RefCell::new(0)));
    }
    let actions = make_actions(
        Rc::clone(&state),
        Some(Arc::clone(&desktop)),
        Rc::new(RefCell::new(MediaState::mock())),
        WindowControl::default(),
        Rc::new(RefCell::new(1.0)),
    );
    (actions.on_clear_transcript.borrow_mut())();
    assert!(
        state.borrow().chat_messages.is_empty(),
        "clear empties the active pane's transcript"
    );
    let s = state.borrow();
    let rt = s.pane_runtime.get(&s.active_pane_id).expect("pane runtime");
    assert!(
        rt.pending_command.is_none(),
        "clear drops the pending approval"
    );
    assert!(
        rt.command_selection.is_none(),
        "clear drops the approval's selected candidate"
    );
}

#[test]
fn stop_drops_the_pending_approval() {
    let (desktop, _dir) = desktop_state();
    let (state, actions, _media) = build(&desktop);
    {
        let mut s = state.borrow_mut();
        let pane_id = s.active_pane_id;
        let rt = s.pane_runtime.entry(pane_id).or_default();
        rt.busy = true;
        rt.pending_command = Some(goble_ui::CommandProposalUi::new(
            "call-1".to_string(),
            vec!["git status".to_string()],
            "/workspace".to_string(),
        ));
        rt.command_selection = Some(Rc::new(RefCell::new(0)));
    }

    (actions.on_stop.borrow_mut())();

    let s = state.borrow();
    let rt = s.pane_runtime.get(&s.active_pane_id).expect("pane runtime");
    assert!(!rt.busy, "stop frees the pane");
    assert!(
        rt.pending_command.is_none(),
        "stop drops the pending approval"
    );
    assert!(
        rt.command_selection.is_none(),
        "stop drops the approval's selected candidate"
    );
}
