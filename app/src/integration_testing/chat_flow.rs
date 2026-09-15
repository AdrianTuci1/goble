//! Integration tests for the chat lifecycle, driven through the app's real
//! action callbacks against a live [`DesktopState`]: creating a conversation,
//! sending a message, switching between conversations, and switching tabs.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_app::actions::make_actions;
use goble_app::media::MediaState;
use goble_app::state::UiState;
use goble_app::ui::{AppTab, UiActions};
use goble_desktop_service::DesktopState;
use goble_ui::platform::WindowControl;
use goble_ui::{ChatFragmentKind, ChatMessage};

/// Concatenate the human-readable text of a message's inline fragments.
#[allow(dead_code)]
fn message_text(msg: &ChatMessage) -> String {
    msg.fragments
        .iter()
        .filter_map(|f| match &f.kind {
            ChatFragmentKind::Text(s)
            | ChatFragmentKind::Bold(s)
            | ChatFragmentKind::Italic(s)
            | ChatFragmentKind::BoldItalic(s) => Some(s.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

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

/// Give the app a runnable model (a provider, a key and a model name), the way
/// a configured machine has one. Creating a conversation is refused while
/// nothing could answer it, so the tests that start one configure this first.
fn configure_model(state: &Rc<RefCell<UiState>>) {
    let mut s = state.borrow_mut();
    s.settings_llm_provider = "mock".to_string();
    s.settings_llm_api_key = "test-key".to_string();
    s.settings_llm_model = "mock".to_string();
    s.selected_model = "mock".to_string();
}

#[test]
fn create_chat_persists_and_selects() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);
    configure_model(&state);

    assert!(state.borrow().conversations.is_empty());

    (actions.on_create_change.borrow_mut())("Planul de lansare".to_string());
    (actions.on_create_submit.borrow_mut())();

    {
        let state = state.borrow();
        assert_eq!(state.new_conversation_draft, "");
        assert!(state.selected_id.is_some());
        assert_eq!(state.conversations.len(), 1);
        assert_eq!(state.conversations[0].name, "Planul de lansare");
    }

    let chats = desktop.list_chats();
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].title, "Planul de lansare");
}

#[test]
fn blank_title_creates_default_agent() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);
    configure_model(&state);

    (actions.on_create_change.borrow_mut())("   ".to_string());
    (actions.on_create_submit.borrow_mut())();

    {
        let state = state.borrow();
        assert_eq!(state.conversations.len(), 1);
        assert_eq!(state.conversations[0].name, "New conversation");
        assert!(state.selected_id.is_some());
    }
    assert_eq!(desktop.list_chats().len(), 1);
}

#[test]
fn a_prompt_with_no_model_is_never_persisted() {
    let (desktop, _dir) = common::desktop_state();
    let chat_id = desktop
        .create_chat("Demo", None, None)
        .expect("create chat");
    let (state, actions) = build(&desktop);

    (actions.on_composer_change.borrow_mut())("Salut!".to_string());
    (actions.on_send_message.borrow_mut())("Salut!".to_string());

    // No key is configured, so nothing can answer this prompt: it is not sent,
    // not written into the transcript and not persisted — the pane shows the
    // model-key notice instead, and the composer keeps what was typed.
    {
        let state = state.borrow();
        assert_eq!(state.composer_draft, "Salut!");
        assert!(
            state.chat_messages.is_empty(),
            "an unanswerable prompt writes nothing into the transcript"
        );
        assert!(
            state.show_llm_key_banner,
            "no key -> the notice band should surface"
        );
        assert!(!state.agent_busy, "no turn started");
    }

    let messages = desktop.list_chat_messages(&chat_id).expect("list messages");
    assert!(
        messages.is_empty(),
        "nothing should reach the store for a prompt that could not run: {messages:?}"
    );
}

#[test]
fn a_prompt_with_no_model_opens_the_agent_view_and_persists_no_message() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);
    assert!(desktop.list_chats().is_empty());

    // Cmd+Enter at a pane is the composer's two calls: the harness switch first,
    // then the prompt. With no runnable model the switch still happens — the
    // view needs a conversation to draw, so one is bound — while the prompt
    // nothing can answer is not sent and writes nothing into the store.
    let pane_id = state.borrow().active_pane_id;
    (actions.on_set_pane_harness_mode.borrow_mut())(pane_id, true);
    (actions.on_cmd_enter.borrow_mut())("Salut!".to_string());

    let conversation = {
        let s = state.borrow();
        assert!(
            s.show_llm_key_banner,
            "no key -> the notice band is what reports the missing model"
        );
        assert_eq!(s.llm_notice_heading(), "No API key configured");
        assert!(
            s.pane_controls(pane_id).harness_mode,
            "the pane is in terminal + agent mode"
        );
        assert!(
            matches!(
                s.pane_view(pane_id),
                goble_terminal::blocks::BlockView::Agent { .. }
            ),
            "the pane shows the agent view"
        );
        s.pane_conversation_id(pane_id)
            .expect("the agent view has a conversation to draw")
    };
    assert!(
        desktop
            .list_chat_messages(&conversation)
            .expect("list messages")
            .is_empty(),
        "a prompt that cannot run writes nothing into its conversation"
    );

    // The sidebar's own "New conversation" row opens a tab of its own and binds
    // a fresh conversation there: the band, not a refusal, is what reports the
    // missing model.
    let before = state.borrow().spaces.len();
    (actions.on_create_submit.borrow_mut())();
    let s = state.borrow();
    assert_eq!(s.spaces.len(), before + 1, "the row opens a new tab");
    assert!(
        s.pane_owns_conversation(s.active_pane_id),
        "the new tab's pane owns a conversation"
    );
    assert!(s.show_llm_key_banner);
}

#[test]
fn select_conversation_refreshes_messages() {
    let (desktop, _dir) = common::desktop_state();
    let chat_a = desktop.create_chat("A", None, None).expect("create chat A");
    let chat_b = desktop.create_chat("B", None, None).expect("create chat B");
    desktop
        .add_chat_message(&chat_a, "user", "mesaj din A")
        .expect("add message");

    let (state, actions) = build(&desktop);

    assert_eq!(state.borrow().selected_id.as_deref(), Some(chat_a.as_str()));
    assert_eq!(state.borrow().chat_messages.len(), 1);

    (actions.on_select_conversation.borrow_mut())(chat_b.clone());
    assert!(state.borrow().chat_messages.is_empty());

    (actions.on_select_conversation.borrow_mut())(chat_a.clone());
    assert_eq!(state.borrow().chat_messages.len(), 1);
}

#[test]
fn select_tab_switches_views() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    (actions.on_select_tab.borrow_mut())(AppTab::Projects);
    assert_eq!(state.borrow().current_tab, AppTab::Projects);
}

#[test]
fn repeated_new_conversation_opens_a_tab_each_time() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);
    configure_model(&state);
    let tabs_before = state.borrow().spaces.len();

    // The sidebar "New conversation" row opens a tab of its own every time: it
    // neither refuses nor reuses the conversation already on screen, so three
    // clicks are three tabs with three conversations.
    (actions.on_create_submit.borrow_mut())();
    (actions.on_create_submit.borrow_mut())();
    (actions.on_create_submit.borrow_mut())();

    let s = state.borrow();
    assert_eq!(
        s.spaces.len(),
        tabs_before + 3,
        "each click appends one space"
    );
    assert_eq!(s.conversations.len(), 3, "each tab has its own conversation");
    assert_eq!(desktop.list_chats().len(), 3);
    let ids: std::collections::HashSet<String> = s
        .pane_sessions
        .values()
        .map(|session| session.conversation_id.clone())
        .filter(|id| !id.is_empty())
        .collect();
    assert_eq!(ids.len(), 3, "the tabs never share a transcript");
}

#[test]
fn new_conversation_after_a_message_creates_a_fresh_one() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);
    configure_model(&state);

    (actions.on_create_submit.borrow_mut())();
    let first = state.borrow().selected_id.clone().expect("first conversation");
    desktop
        .add_chat_message(&first, "user", "salut")
        .expect("add a message");

    // The pane's conversation now has content, and the click opens a tab of its
    // own with a conversation that is not the one it holds.
    (actions.on_create_submit.borrow_mut())();
    assert_eq!(desktop.list_chats().len(), 2);
    assert_ne!(state.borrow().selected_id.as_deref(), Some(first.as_str()));
}

#[test]
fn sidebar_toggle_flips_visibility() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    assert!(state.borrow().sidebar_visible);
    (actions.on_toggle_sidebar.borrow_mut())();
    assert!(!state.borrow().sidebar_visible);
    (actions.on_toggle_sidebar.borrow_mut())();
    assert!(state.borrow().sidebar_visible);
}
