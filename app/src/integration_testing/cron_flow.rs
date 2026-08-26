//! Integration tests for scheduled tasks (crons/workflows): creating a
//! workflow with a cron trigger, deleting it, and triggering it — both through
//! the app's action callbacks and directly against [`DesktopState`].

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_app::actions::make_actions;
use goble_app::state::UiState;
use goble_app::ui::UiActions;
use goble_core::agent::Trigger;
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
fn create_workflow_with_cron_shows_in_crons() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    (actions.on_cron_create.borrow_mut())();

    {
        let state = state.borrow();
        assert_eq!(state.crons.len(), 1);
        assert_eq!(state.crons[0].name, "New scheduled task");
        assert_eq!(state.crons[0].schedule, "0 12 * * *");
        assert!(state.crons[0].enabled);
    }

    let workflows = desktop.list_workflows();
    assert_eq!(workflows.len(), 1);
    assert_eq!(workflows[0].name, "New scheduled task");
}

#[test]
fn delete_workflow_removes_cron() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    (actions.on_cron_create.borrow_mut())();
    let cron_id = state.borrow().crons[0].id.clone();
    assert_eq!(desktop.list_workflows().len(), 1);

    (actions.on_cron_delete.borrow_mut())(cron_id);

    assert!(state.borrow().crons.is_empty());
    assert!(desktop.list_workflows().is_empty());
}

#[test]
fn trigger_cron_appends_message_and_sets_busy() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    (actions.on_cron_create.borrow_mut())();
    let cron_id = state.borrow().crons[0].id.clone();
    let cron_name = state.borrow().crons[0].name.clone();

    (actions.on_cron_trigger.borrow_mut())(cron_id);

    let state = state.borrow();
    assert!(state.agent_busy);
    assert_eq!(state.chat_messages.len(), 1);
    assert_eq!(state.chat_messages[0].role, ChatRole::Assistant);
    let text = message_text(&state.chat_messages[0]);
    assert!(text.contains(&cron_name), "expected cron name in: {text}");
}

#[test]
fn workflow_crud_direct() {
    let (desktop, _dir) = common::desktop_state();

    let wf = desktop
        .create_workflow(
            "Nightly backup",
            "Backup the vault every night",
            vec![],
            Trigger::Cron {
                expression: "0 2 * * *".to_string(),
            },
        )
        .expect("create workflow");

    assert_eq!(desktop.list_workflows().len(), 1);
    assert_eq!(desktop.list_workflows()[0].name, "Nightly backup");

    desktop
        .delete_workflow(&goble_core::workflow::WorkflowId(wf.id.clone()))
        .expect("delete workflow");
    assert!(desktop.list_workflows().is_empty());
}
