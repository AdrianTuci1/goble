//! Integration tests for the MCP connector flow, driven through the app's AI
//! action callbacks. These run against the mock fallback (no backend) because
//! a real install runs npm over the network — the point here is the app's
//! state transitions (install, discover, toggle, delete) stay correct.

use std::cell::RefCell;
use std::rc::Rc;

use goble_app::ai::{make_ai_actions, AiState};
use goble_app::ui::AiActions;

fn build() -> (Rc<RefCell<AiState>>, AiActions) {
    // `AiState::mock()` ships with sample connectors; start from a clean slate
    // so we control exactly which connector is installed by the actions.
    let mut state = AiState::mock();
    state.connectors.clear();
    let state = Rc::new(RefCell::new(state));
    let actions = make_ai_actions(Rc::clone(&state), None);
    (state, actions)
}

fn install(actions: &AiActions) {
    (actions.on_install_open.borrow_mut())();
    (actions.on_install_name_change.borrow_mut())("Postgres".to_string());
    (actions.on_install_source_change.borrow_mut())("npm".to_string());
    (actions.on_install_source_value_change.borrow_mut())("@mcp/postgres".to_string());
    (actions.on_install_submit.borrow_mut())();
}

#[test]
fn install_connector_adds_entry_and_closes_drawer() {
    let (state, actions) = build();

    (actions.on_install_open.borrow_mut())();
    assert!(state.borrow().install_open);

    (actions.on_install_name_change.borrow_mut())("Postgres".to_string());
    (actions.on_install_source_change.borrow_mut())("npm".to_string());
    (actions.on_install_source_value_change.borrow_mut())("@mcp/postgres".to_string());
    (actions.on_install_submit.borrow_mut())();

    let state = state.borrow();
    assert!(!state.install_open);
    assert!(state.install_error.is_none());
    assert_eq!(state.connectors.len(), 1);
    assert_eq!(state.connectors[0].name, "Postgres");
    assert_eq!(state.connectors[0].source, "npm");
    assert_eq!(
        state.connectors[0].source_value.as_deref(),
        Some("@mcp/postgres")
    );
}

#[test]
fn empty_name_sets_error_and_keeps_drawer_open() {
    let (state, actions) = build();

    (actions.on_install_open.borrow_mut())();
    (actions.on_install_name_change.borrow_mut())("   ".to_string());
    (actions.on_install_source_change.borrow_mut())("npm".to_string());
    (actions.on_install_submit.borrow_mut())();

    let state = state.borrow();
    assert!(state.install_open);
    assert_eq!(
        state.install_error.as_deref(),
        Some("Connector name is required")
    );
    assert!(state.connectors.is_empty());
}

#[test]
fn discover_then_toggle_tools() {
    let (state, actions) = build();
    install(&actions);

    let connector_id = state.borrow().connectors[0].id.clone();
    assert!(state.borrow().connectors[0].discovered_tools.is_empty());

    (actions.on_connector_discover.borrow_mut())(connector_id.clone());

    {
        let state = state.borrow();
        let server = &state.connectors[0];
        assert_eq!(
            server.discovered_tools,
            vec!["mock_tool_one", "mock_tool_two"]
        );
        assert_eq!(server.enabled_tools, server.discovered_tools);
    }

    (actions.on_connector_toggle.borrow_mut())(connector_id.clone(), false);
    assert!(state.borrow().connectors[0].enabled_tools.is_empty());

    (actions.on_connector_toggle.borrow_mut())(connector_id.clone(), true);
    assert_eq!(
        state.borrow().connectors[0].enabled_tools,
        vec!["mock_tool_one", "mock_tool_two"]
    );
}

#[test]
fn delete_connector_removes_entry() {
    let (state, actions) = build();
    install(&actions);

    let connector_id = state.borrow().connectors[0].id.clone();
    (actions.on_connector_delete.borrow_mut())(connector_id);

    assert!(state.borrow().connectors.is_empty());
}
