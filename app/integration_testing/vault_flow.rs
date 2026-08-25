//! Integration tests for the vault (secrets) flow, driven through the app's
//! AI action callbacks against a live [`DesktopState`]: unlocking, adding and
//! removing secrets, and the validation/error paths.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_app::ai::{make_ai_actions, AiState};
use goble_desktop_service::DesktopState;
use goble_ui_hot::AiActions;

fn build(desktop: &Arc<DesktopState>) -> (Rc<RefCell<AiState>>, AiActions) {
    let state = Rc::new(RefCell::new(AiState::from_desktop(desktop)));
    let actions = make_ai_actions(Rc::clone(&state), Some(Arc::clone(desktop)));
    (state, actions)
}

fn unlock(actions: &AiActions) {
    (actions.on_vault_unlock_draft_change.borrow_mut())("correct horse".to_string());
    (actions.on_vault_unlock.borrow_mut())();
}

#[test]
fn unlock_then_add_and_delete_secret() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    // Locked by default.
    assert!(!state.borrow().vault_unlocked);

    unlock(&actions);
    {
        let state = state.borrow();
        assert!(state.vault_unlocked);
        assert_eq!(state.vault_unlock_draft, "");
        assert!(state.vault_error.is_none());
    }

    (actions.on_vault_new_key_change.borrow_mut())("openai_api_key".to_string());
    (actions.on_vault_new_value_change.borrow_mut())("sk-test".to_string());
    (actions.on_vault_secret_add.borrow_mut())();

    {
        let state = state.borrow();
        assert!(state.vault_error.is_none());
        assert_eq!(state.vault_secrets.len(), 1);
        assert_eq!(state.vault_secrets[0].key, "openai_api_key");
    }
    assert_eq!(desktop.list_vault_secrets().len(), 1);

    (actions.on_vault_secret_delete.borrow_mut())("openai_api_key".to_string());

    assert!(state.borrow().vault_secrets.is_empty());
    assert!(desktop.list_vault_secrets().is_empty());
}

#[test]
fn empty_key_sets_error_without_adding() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    unlock(&actions);

    (actions.on_vault_new_key_change.borrow_mut())("   ".to_string());
    (actions.on_vault_new_value_change.borrow_mut())("value".to_string());
    (actions.on_vault_secret_add.borrow_mut())();

    let state = state.borrow();
    assert_eq!(state.vault_error.as_deref(), Some("Secret key is required"));
    assert!(state.vault_secrets.is_empty());
}

#[test]
fn adding_secret_requires_unlock() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    // Not unlocked yet: the backend refuses to persist a secret.
    (actions.on_vault_new_key_change.borrow_mut())("api_key".to_string());
    (actions.on_vault_new_value_change.borrow_mut())("value".to_string());
    (actions.on_vault_secret_add.borrow_mut())();

    let state = state.borrow();
    assert!(state.vault_error.is_some());
    assert!(state.vault_secrets.is_empty());
    assert!(desktop.list_vault_secrets().is_empty());
}
