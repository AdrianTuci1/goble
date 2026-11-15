//! Integration tests for the agent-led setup: Settings → Agent & Approval opens
//! a conversation of its own for Remote or the hub, and the brief it starts
//! from is the one `goble-cli setup brief` prints.
//!
//! The turn itself needs a model the machine may not have, so these tests
//! assert the surface the action owns: the new tab, the conversation it binds,
//! the title it gives it, and that the send path ran instead of the no-key
//! refusal. The brief's own text is pinned in `goble_core::config::setup`.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_app::actions::make_actions;
use goble_app::media::MediaState;
use goble_app::state::UiState;
use goble_app::ui::UiActions;
use goble_desktop_service::DesktopState;
use goble_ui::platform::WindowControl;

fn build(desktop: &Arc<DesktopState>) -> (Rc<RefCell<UiState>>, UiActions) {
    let state = Rc::new(RefCell::new(UiState::from_desktop(desktop)));
    let media = Rc::new(RefCell::new(MediaState::mock()));
    let actions = make_actions(
        Rc::clone(&state),
        Some(Arc::clone(desktop)),
        Rc::clone(&media),
        WindowControl::default(),
        Rc::new(RefCell::new(1.0)),
    );
    (state, actions)
}

/// A runnable model, so the send path runs the brief instead of showing the
/// no-key banner. The provider is the app's mock, so nothing leaves the process.
fn configure_model(state: &Rc<RefCell<UiState>>) {
    let mut s = state.borrow_mut();
    s.settings_llm_provider = "mock".to_string();
    s.settings_llm_api_key = "test-key".to_string();
    s.settings_llm_model = "mock".to_string();
    s.selected_model = "mock".to_string();
}

/// The conversation an action opened: its id and the title the store holds.
fn opened_conversation(
    state: &Rc<RefCell<UiState>>,
    desktop: &Arc<DesktopState>,
) -> (String, String) {
    let conversation = state
        .borrow()
        .pane_conversation_id(state.borrow().active_pane_id)
        .expect("the setup pane is bound to a conversation");
    let title = desktop
        .list_chats()
        .into_iter()
        .find(|chat| chat.id == conversation)
        .map(|chat| chat.title)
        .expect("the conversation is in the store");
    (conversation, title)
}

#[test]
fn setting_up_remote_opens_a_tab_and_a_named_conversation() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);
    configure_model(&state);
    let spaces_before = state.borrow().spaces.len();

    (actions.on_setup_remote_with_agent.borrow_mut())();

    let s = state.borrow();
    assert_eq!(s.spaces.len(), spaces_before + 1, "a tab of its own");
    assert!(
        s.pane_owns_conversation(s.active_pane_id),
        "the new pane owns a fresh conversation"
    );
    assert!(
        !s.show_llm_key_banner,
        "the brief went to the send path rather than the no-key refusal"
    );
    drop(s);

    let (_conversation, title) = opened_conversation(&state, &desktop);
    assert_eq!(title, "Remote setup");
}

#[test]
fn setting_up_the_hub_opens_its_own_conversation() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);
    configure_model(&state);
    let spaces_before = state.borrow().spaces.len();

    (actions.on_setup_hub_with_agent.borrow_mut())();

    assert_eq!(state.borrow().spaces.len(), spaces_before + 1);
    let (_conversation, title) = opened_conversation(&state, &desktop);
    assert_eq!(title, "Hub setup");
}

/// The two rows are focus stops on Agent & Approval, in the order the page
/// draws them, so the keyboard can reach the agent-led path.
#[test]
fn the_setup_rows_are_in_the_agent_page_focus_order() {
    let (desktop, _dir) = common::desktop_state();
    let (state, _actions) = build(&desktop);
    {
        let mut s = state.borrow_mut();
        s.open_settings_tab(Some(&desktop));
        s.settings_set_page(goble_app::ui::SettingsCategory::AgentApproval);
    }
    let controls = state.borrow().settings_pane_controls();
    let remote = controls
        .iter()
        .position(|control| *control == goble_app::ui::SettingsControl::SetupRemoteWithAgent)
        .expect("the Remote row is a focus stop");
    let hub = controls
        .iter()
        .position(|control| *control == goble_app::ui::SettingsControl::SetupHubWithAgent)
        .expect("the hub row is a focus stop");
    assert_eq!(hub, remote + 1, "Remote, then the hub, then the file");
}
