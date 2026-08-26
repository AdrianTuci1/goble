//! Integration tests for the routine lifecycle, driven through the app's real
//! action callbacks against a live [`DesktopState`]: creating a routine,
//! switching between routines, and switching tabs.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_app::actions::make_actions;
use goble_app::state::UiState;
use goble_app::ui::{AppTab, UiActions};
use goble_desktop_service::DesktopState;

fn build(desktop: &Arc<DesktopState>) -> (Rc<RefCell<UiState>>, UiActions) {
    let state = Rc::new(RefCell::new(UiState::from_desktop(desktop)));
    let actions = make_actions(Rc::clone(&state), Some(Arc::clone(desktop)));
    (state, actions)
}

#[test]
fn create_routine_persists_and_selects() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    assert!(state.borrow().routines.is_empty());

    (actions.on_create_change.borrow_mut())("Planul de lansare".to_string());
    (actions.on_create_submit.borrow_mut())();

    {
        let state = state.borrow();
        assert_eq!(state.new_conversation_draft, "");
        assert!(state.selected_id.is_some());
        assert_eq!(state.routines.len(), 1);
        assert_eq!(state.routines[0].name, "Planul de lansare");
    }

    let workflows = desktop.list_workflows();
    assert_eq!(workflows.len(), 1);
    assert_eq!(workflows[0].name, "Planul de lansare");
}

#[test]
fn blank_title_creates_default_routine() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    (actions.on_create_change.borrow_mut())("   ".to_string());
    (actions.on_create_submit.borrow_mut())();

    {
        let state = state.borrow();
        assert_eq!(state.routines.len(), 1);
        assert_eq!(state.routines[0].name, "New routine");
        assert!(state.selected_id.is_some());
    }
    assert_eq!(desktop.list_workflows().len(), 1);
}

#[test]
fn select_routine_updates_selected_id() {
    let (desktop, _dir) = common::desktop_state();
    let wf_a = desktop
        .create_workflow("A", "", vec![], goble_core::agent::Trigger::Manual)
        .expect("create workflow A");
    let wf_b = desktop
        .create_workflow("B", "", vec![], goble_core::agent::Trigger::Manual)
        .expect("create workflow B");

    let (state, actions) = build(&desktop);

    let first_id = state.borrow().selected_id.clone().expect("a routine is selected");
    let second_id = if first_id == wf_a.id { &wf_b.id } else { &wf_a.id };

    (actions.on_select_conversation.borrow_mut())(second_id.clone());
    assert_eq!(
        state.borrow().selected_id.as_deref(),
        Some(second_id.as_str())
    );

    (actions.on_select_conversation.borrow_mut())(first_id.clone());
    assert_eq!(
        state.borrow().selected_id.as_deref(),
        Some(first_id.as_str())
    );
}

#[test]
fn select_tab_switches_views() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    (actions.on_select_tab.borrow_mut())(AppTab::Settings);
    assert_eq!(state.borrow().current_tab, AppTab::Settings);
}

#[test]
fn delete_routine_removes_it() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    (actions.on_create_submit.borrow_mut())();
    let routine_id = state.borrow().routines[0].id.clone();

    (actions.on_agent_delete.borrow_mut())(routine_id.clone());
    assert!(state.borrow().routines.is_empty());
    assert!(desktop.list_workflows().is_empty());
}

#[test]
fn toggle_routine_enabled_flips_flag() {
    let (desktop, _dir) = common::desktop_state();
    let (state, actions) = build(&desktop);

    (actions.on_create_submit.borrow_mut())();
    let routine_id = state.borrow().routines[0].id.clone();
    assert!(state.borrow().routines[0].enabled);

    (actions.on_routine_toggle_enabled.borrow_mut())(routine_id.clone());
    assert!(!state.borrow().routines[0].enabled);

    (actions.on_routine_toggle_enabled.borrow_mut())(routine_id.clone());
    assert!(state.borrow().routines[0].enabled);
}
