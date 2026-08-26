//! Integration tests for the chat lifecycle, driven through the app's real
//! action callbacks against a live [`DesktopState`]: creating a conversation,
//! sending a message, switching between conversations, and switching tabs.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_app::actions::make_actions;
use goble_app::state::UiState;
use goble_app::ui::{AppTab, UiActions};
use goble_desktop_service::DesktopState;
use goble_ui::{ChatFragmentKind, ChatMessage, ChatRole};

/// Concatenate the human-readable text of a message's inline fragments.
fn message_text(msg: &ChatMessage) -> String {
    msg.fragments
        .iter()
        .filter_map(|f| match &f.kind {
            ChatFragmentKind::Text(s)
            | ChatFragmentKind::Bold(s)
            | ChatFragmentKind::Italic(s)
            | ChatFragmentKind::BoldItalic(s)
            | ChatFragmentKind::BlockQuote(s) => Some(s.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn build(desktop: &Arc<DesktopState>) -> (Rc<RefCell<UiState>>, UiActions) {
    let state = Rc::new(RefCell::new(UiState::from_desktop(desktop)));
    let actions = make_actions(Rc::clone(&state), Some(Arc::clone(desktop)));
    (state, actions)
}

#[test]
fn create_chat_persists_and_selects() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

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

    (actions.on_create_change.borrow_mut())("   ".to_string());
    (actions.on_create_submit.borrow_mut())();

    {
        let state = state.borrow();
        assert_eq!(state.conversations.len(), 1);
        assert_eq!(state.conversations[0].name, "New agent");
        assert!(state.selected_id.is_some());
    }
    assert_eq!(desktop.list_chats().len(), 1);
}

#[test]
fn send_message_appends_and_persists() {
    let (desktop, _dir) = common::desktop_state();
    let chat_id = desktop
        .create_chat("Demo", None, None)
        .expect("create chat");
    let (state, actions) = build(&desktop);

    (actions.on_composer_change.borrow_mut())("Salut!".to_string());
    (actions.on_send_message.borrow_mut())("Salut!".to_string());

    // No key is configured, so the send path keeps the user's message (no
    // fabricated assistant reply) and surfaces the model-key banner overlay.
    {
        let state = state.borrow();
        assert_eq!(state.composer_draft, "");
        assert_eq!(state.chat_messages.len(), 1);
        assert_eq!(state.chat_messages[0].role, ChatRole::User);
        assert!(
            state.show_llm_key_banner,
            "no key -> banner overlay should surface"
        );
    }

    let messages = desktop.list_chat_messages(&chat_id).expect("list messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, "user");
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

    (actions.on_select_tab.borrow_mut())(AppTab::Settings);
    assert_eq!(state.borrow().current_tab, AppTab::Settings);
}
