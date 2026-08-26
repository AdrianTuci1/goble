//! Integration tests for the first-run model-connect flow, driven through the
//! app's real action callbacks against a live [`DesktopState`]:
//! no key -> banner on first send -> model-provider dialog -> save key ->
//! workspace choice -> local. With no key the send path keeps the user's
//! message but emits no canned assistant reply (nothing is actually running);
//! the agent harness is a separate slice, so these tests assert the state +
//! navigation transitions.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_app::actions::make_actions;
use goble_app::state::UiState;
use goble_app::ui::{AppTab, UiActions, WorkspaceRouting};
use goble_desktop_service::DesktopState;

fn build(desktop: &Arc<DesktopState>) -> (Rc<RefCell<UiState>>, UiActions) {
    let state = Rc::new(RefCell::new(UiState::from_desktop(desktop)));
    let actions = make_actions(Rc::clone(&state), Some(Arc::clone(desktop)));
    (state, actions)
}

/// Configure a model key through the settings save action, as the LLM pane does.
fn save_key(actions: &UiActions, api_key: &str) {
    (actions.on_save_llm.borrow_mut())(
        "openai".to_string(),
        "gpt-4o".to_string(),
        api_key.to_string(),
        String::new(),
        "0.7".to_string(),
    );
}

#[test]
fn banner_click_opens_model_dialog() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    (actions.on_send_message.borrow_mut())("Hi".to_string());
    assert!(state.borrow().show_llm_key_banner);

    // Clicking the banner opens the model-provider dialog over the chat.
    (actions.on_config_llm_key.borrow_mut())();
    {
        let s = state.borrow();
        assert!(
            s.llm_dialog_open,
            "banner click should open the model dialog"
        );
        assert_eq!(
            s.current_tab,
            AppTab::Chat,
            "dialog overlays chat, no navigation"
        );
        // The form is pre-filled with the configured provider/model.
        assert_eq!(*s.llm_dialog_provider.borrow(), "openai");
        assert_eq!(*s.llm_dialog_model.borrow(), "gpt-4o");
    }
}

#[test]
fn model_dialog_cancel_closes_without_saving() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    (actions.on_send_message.borrow_mut())("Hi".to_string());
    (actions.on_config_llm_key.borrow_mut())();
    assert!(state.borrow().llm_dialog_open);

    (actions.on_close_llm_dialog.borrow_mut())();
    {
        let s = state.borrow();
        assert!(!s.llm_dialog_open, "cancel should close the dialog");
        assert!(s.settings_llm_api_key.is_empty(), "no key saved on cancel");
        assert!(
            s.show_llm_key_banner,
            "banner stays since no key is configured"
        );
        assert!(!s.show_workspace_choice);
    }
}

#[test]
fn model_dialog_save_closes_and_presents_choice() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    (actions.on_send_message.borrow_mut())("Hi".to_string());
    (actions.on_config_llm_key.borrow_mut())();
    assert!(state.borrow().llm_dialog_open);

    // Saving through the same action the Settings LLM pane uses.
    save_key(&actions, "sk-test");

    {
        let s = state.borrow();
        assert!(!s.llm_dialog_open, "save should close the dialog");
        assert_eq!(s.settings_llm_api_key, "sk-test");
        assert!(!s.show_llm_key_banner, "banner clears once a key is set");
        assert!(s.show_workspace_choice, "workspace choice should present");
    }

    // The key is persisted to the backend store.
    let setting = desktop
        .get_llm_setting("openai")
        .expect("setting persisted");
    assert_eq!(setting.api_key, "sk-test");
}

#[test]
fn key_configured_presents_workspace_choice() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    (actions.on_send_message.borrow_mut())("Hi".to_string());
    assert!(state.borrow().show_llm_key_banner);

    save_key(&actions, "sk-test");

    {
        let s = state.borrow();
        assert_eq!(s.settings_llm_api_key, "sk-test");
        assert!(!s.show_llm_key_banner, "banner clears once a key is set");
        assert!(s.show_workspace_choice, "workspace choice should present");
        assert_eq!(s.workspace_routing, None);
    }

    // The key is persisted to the backend store.
    let setting = desktop
        .get_llm_setting("openai")
        .expect("setting persisted");
    assert_eq!(setting.api_key, "sk-test");
}

#[test]
fn choosing_local_sets_routing_and_continues_conversation() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    (actions.on_send_message.borrow_mut())("Hi".to_string());
    save_key(&actions, "sk-test");
    assert!(state.borrow().show_workspace_choice);

    (actions.on_choose_workspace.borrow_mut())(WorkspaceRouting::Local);

    {
        let s = state.borrow();
        assert_eq!(s.workspace_routing, Some(WorkspaceRouting::Local));
        assert!(!s.show_workspace_choice, "choice clears after deciding");
        assert!(!s.show_llm_key_banner, "banner stays cleared");
        assert_eq!(s.current_tab, AppTab::Chat, "returns to the conversation");
        // Only the user's message remains (no canned reply for the no-key send).
        assert_eq!(s.chat_messages.len(), 1);
    }
}

#[test]
fn choosing_remote_sets_routing() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    (actions.on_send_message.borrow_mut())("Hi".to_string());
    save_key(&actions, "sk-test");

    (actions.on_choose_workspace.borrow_mut())(WorkspaceRouting::Remote);
    {
        let s = state.borrow();
        assert_eq!(s.workspace_routing, Some(WorkspaceRouting::Remote));
        assert!(!s.show_workspace_choice);
    }
}


