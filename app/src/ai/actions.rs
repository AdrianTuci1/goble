//! Callback wiring for the AI domain: turns app state + backend into
//! [`crate::ui::AiActions`] closures.
//!
//! Most MCP mutations go through `DesktopState`, which requires a tokio
//! runtime entered on the calling thread (main.rs keeps one alive for the
//! app lifetime).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::ui::{AiActions, McpServerEntry, VaultSecretEntry};
use goble_desktop_service::DesktopState;

use super::state::AiState;

fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("mcp-{nanos}")
}

pub fn make_ai_actions(
    state: Rc<RefCell<AiState>>,
    desktop: Option<Arc<DesktopState>>,
) -> AiActions {
    let on_open_connectors = Rc::clone(&state);
    let on_close_connectors = Rc::clone(&state);
    let on_open_vault = Rc::clone(&state);
    let on_close_vault = Rc::clone(&state);
    let on_vault_unlock_draft_change = Rc::clone(&state);
    let on_vault_unlock = Rc::clone(&state);
    let on_vault_new_key_change = Rc::clone(&state);
    let on_vault_new_value_change = Rc::clone(&state);
    let on_vault_secret_add = Rc::clone(&state);
    let on_vault_secret_delete = Rc::clone(&state);
    let on_connector_search_change = Rc::clone(&state);
    let on_install_open = Rc::clone(&state);
    let on_install_edit = Rc::clone(&state);
    let on_install_pick = Rc::clone(&state);
    let on_install_close = Rc::clone(&state);
    let on_install_name_change = Rc::clone(&state);
    let on_install_source_change = Rc::clone(&state);
    let on_install_source_value_change = Rc::clone(&state);
    let on_install_search_change = Rc::clone(&state);
    let on_install_secret_toggle = Rc::clone(&state);
    let on_install_submit = Rc::clone(&state);
    let on_connector_delete = Rc::clone(&state);
    let on_connector_discover = Rc::clone(&state);
    let on_connector_toggle = Rc::clone(&state);

    let desktop_unlock = desktop.clone();
    let desktop_vault_add = desktop.clone();
    let desktop_vault_delete = desktop.clone();
    let desktop_install = desktop.clone();
    let desktop_delete = desktop.clone();
    let desktop_discover = desktop.clone();
    let desktop_toggle = desktop.clone();
    let desktop_search = desktop.clone();
    let desktop_search_edit = desktop.clone();
    let desktop_search_change = desktop.clone();

    AiActions {
        on_open_connectors: Rc::new(RefCell::new(move || {
            on_open_connectors.borrow_mut().connectors_open = true;
        })),
        on_close_connectors: Rc::new(RefCell::new(move || {
            let mut s = on_close_connectors.borrow_mut();
            s.connectors_open = false;
            s.install_open = false;
        })),
        on_open_vault: Rc::new(RefCell::new(move || {
            on_open_vault.borrow_mut().vault_open = true;
        })),
        on_close_vault: Rc::new(RefCell::new(move || {
            on_close_vault.borrow_mut().vault_open = false;
        })),
        on_vault_unlock_draft_change: Rc::new(RefCell::new(move |value: String| {
            on_vault_unlock_draft_change.borrow_mut().vault_unlock_draft = value;
        })),
        on_vault_unlock: Rc::new(RefCell::new(move || {
            let mut s = on_vault_unlock.borrow_mut();
            if let Some(desktop) = &desktop_unlock {
                let passphrase = s.vault_unlock_draft.clone();
                match desktop.unlock_vault(passphrase) {
                    Ok(_) => {
                        s.vault_unlock_draft.clear();
                        s.vault_error = None;
                        s.refresh_vault(desktop);
                    }
                    Err(e) => s.vault_error = Some(format!("{e}")),
                }
            } else {
                s.vault_unlocked = true;
                s.vault_error = None;
            }
        })),
        on_vault_new_key_change: Rc::new(RefCell::new(move |value: String| {
            on_vault_new_key_change.borrow_mut().vault_new_key = value;
        })),
        on_vault_new_value_change: Rc::new(RefCell::new(move |value: String| {
            on_vault_new_value_change.borrow_mut().vault_new_value = value;
        })),
        on_vault_secret_add: Rc::new(RefCell::new(move || {
            let mut s = on_vault_secret_add.borrow_mut();
            let key = s.vault_new_key.trim().to_string();
            let value = s.vault_new_value.clone();
            if key.is_empty() {
                s.vault_error = Some("Secret key is required".to_string());
                return;
            }
            if let Some(desktop) = &desktop_vault_add {
                match desktop.set_vault_secret(&key, &value) {
                    Ok(_) => {
                        s.vault_new_key.clear();
                        s.vault_new_value.clear();
                        s.vault_error = None;
                        s.refresh_vault(desktop);
                    }
                    Err(e) => s.vault_error = Some(format!("{e}")),
                }
            } else {
                s.vault_secrets.push(VaultSecretEntry {
                    key,
                    updated_at: "now".to_string(),
                });
                s.vault_new_key.clear();
                s.vault_new_value.clear();
                s.vault_error = None;
            }
        })),
        on_vault_secret_delete: Rc::new(RefCell::new(move |key: String| {
            let mut s = on_vault_secret_delete.borrow_mut();
            if let Some(desktop) = &desktop_vault_delete {
                if let Err(e) = desktop.delete_vault_secret(&key) {
                    s.vault_error = Some(format!("{e}"));
                } else {
                    s.vault_error = None;
                    s.refresh_vault(desktop);
                }
            } else {
                s.vault_secrets.retain(|sec| sec.key != key);
            }
            s.install_selected_secrets.retain(|k| k != &key);
        })),
        on_connector_search_change: Rc::new(RefCell::new(move |value: String| {
            on_connector_search_change.borrow_mut().connector_search = value;
        })),
        on_install_open: Rc::new(RefCell::new(move || {
            let mut s = on_install_open.borrow_mut();
            s.install_open = true;
            s.install_editing_id = None;
            s.install_name.clear();
            s.install_source = "npm".to_string();
            s.install_source_value.clear();
            s.install_selected_secrets.clear();
            s.install_error = None;
            if let Some(desktop) = &desktop_search {
                s.refresh_search(desktop);
            }
        })),
        on_install_edit: Rc::new(RefCell::new(move |id: String| {
            let mut s = on_install_edit.borrow_mut();
            if let Some(server) = s.connectors.iter().find(|c| c.id == id).cloned() {
                s.install_open = true;
                s.install_editing_id = Some(server.id.clone());
                s.install_name = server.name.clone();
                s.install_source = server.source.clone();
                s.install_source_value = server.source_value.unwrap_or_default();
                s.install_selected_secrets = server.secret_ids.clone();
                s.install_error = None;
                if let Some(desktop) = &desktop_search_edit {
                    s.refresh_search(desktop);
                }
            }
        })),
        on_install_pick: Rc::new(RefCell::new(
            move |name: String, source: String, source_value: String| {
                let mut s = on_install_pick.borrow_mut();
                s.install_open = true;
                s.install_editing_id = None;
                s.install_name = name;
                s.install_source = source;
                s.install_source_value = source_value;
                s.install_selected_secrets.clear();
                s.install_error = None;
            },
        )),
        on_install_close: Rc::new(RefCell::new(move || {
            let mut s = on_install_close.borrow_mut();
            s.install_open = false;
            s.install_editing_id = None;
            s.installing = false;
            s.install_error = None;
        })),
        on_install_name_change: Rc::new(RefCell::new(move |value: String| {
            on_install_name_change.borrow_mut().install_name = value;
        })),
        on_install_source_change: Rc::new(RefCell::new(move |value: String| {
            on_install_source_change.borrow_mut().install_source = value;
        })),
        on_install_source_value_change: Rc::new(RefCell::new(move |value: String| {
            on_install_source_value_change
                .borrow_mut()
                .install_source_value = value;
        })),
        on_install_search_change: Rc::new(RefCell::new(move |value: String| {
            let mut s = on_install_search_change.borrow_mut();
            s.install_search_query = value;
            if let Some(desktop) = &desktop_search_change {
                s.refresh_search(desktop);
            }
        })),
        on_install_secret_toggle: Rc::new(RefCell::new(move |key: String, checked: bool| {
            let mut s = on_install_secret_toggle.borrow_mut();
            if checked {
                if !s.install_selected_secrets.contains(&key) {
                    s.install_selected_secrets.push(key);
                }
            } else {
                s.install_selected_secrets.retain(|k| k != &key);
            }
        })),
        on_install_submit: Rc::new(RefCell::new(move || {
            let mut s = on_install_submit.borrow_mut();
            let name = s.install_name.trim().to_string();
            let source = s.install_source.clone();
            let source_value = s.install_source_value.trim().to_string();
            let secrets = s.install_selected_secrets.clone();
            if name.is_empty() {
                s.install_error = Some("Connector name is required".to_string());
                return;
            }
            if let Some(desktop) = &desktop_install {
                s.installing = true;
                let result = if let Some(id) = s.install_editing_id.clone() {
                    desktop.update_mcp_server(
                        &id,
                        Some(&name),
                        Some(&source_value),
                        Some(secrets),
                        None,
                    )
                } else {
                    let id = new_id();
                    desktop.install_mcp_server(
                        &id,
                        &name,
                        &source,
                        Some(&source_value),
                        secrets,
                        None,
                    )
                };
                s.installing = false;
                match result {
                    Ok(_) => {
                        s.install_open = false;
                        s.install_editing_id = None;
                        s.install_error = None;
                        s.refresh_connectors(desktop);
                    }
                    Err(e) => s.install_error = Some(format!("{e}")),
                }
            } else {
                let entry = McpServerEntry {
                    id: new_id(),
                    name,
                    source,
                    source_value: Some(source_value),
                    capabilities: vec!["mock".to_string()],
                    auth_required: false,
                    discovered_tools: Vec::new(),
                    secret_ids: secrets,
                    enabled_tools: Vec::new(),
                };
                s.connectors.push(entry);
                s.install_open = false;
                s.install_editing_id = None;
                s.install_error = None;
            }
        })),
        on_connector_delete: Rc::new(RefCell::new(move |id: String| {
            let mut s = on_connector_delete.borrow_mut();
            if let Some(desktop) = &desktop_delete {
                if let Err(e) = desktop.delete_mcp_server(&id) {
                    log::warn!("delete_mcp_server({id}) failed: {e}");
                }
                s.refresh_connectors(desktop);
            } else {
                s.connectors.retain(|c| c.id != id);
            }
        })),
        on_connector_discover: Rc::new(RefCell::new(move |id: String| {
            let mut s = on_connector_discover.borrow_mut();
            if let Some(desktop) = &desktop_discover {
                if let Err(e) = desktop.discover_mcp_tools(&id) {
                    log::warn!("discover_mcp_tools({id}) failed: {e}");
                }
                s.refresh_connectors(desktop);
            } else if let Some(server) = s.connectors.iter_mut().find(|c| c.id == id) {
                server.discovered_tools =
                    vec!["mock_tool_one".to_string(), "mock_tool_two".to_string()];
                server.enabled_tools = server.discovered_tools.clone();
            }
        })),
        on_connector_toggle: Rc::new(RefCell::new(move |id: String, enabled: bool| {
            let mut s = on_connector_toggle.borrow_mut();
            if let Some(desktop) = &desktop_toggle {
                let current = s.connectors.iter().find(|c| c.id == id).cloned();
                if let Some(server) = current {
                    let enabled_tools = if enabled {
                        server.discovered_tools.clone()
                    } else {
                        Vec::new()
                    };
                    if let Err(e) =
                        desktop.update_mcp_server_meta(&id, server.secret_ids, enabled_tools)
                    {
                        log::warn!("update_mcp_server_meta({id}) failed: {e}");
                    }
                    s.refresh_connectors(desktop);
                }
            } else if let Some(server) = s.connectors.iter_mut().find(|c| c.id == id) {
                if enabled {
                    server.enabled_tools = server.discovered_tools.clone();
                } else {
                    server.enabled_tools.clear();
                }
            }
        })),
    }
}
