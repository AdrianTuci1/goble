//! Callback wiring for the environment domain: turns app state into
//! [`crate::ui::MediaActions`] closures.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::DesktopState;

use crate::state::UiState;
use crate::ui::MediaActions;

use super::state::MediaState;

pub fn make_media_actions(
    state: Rc<RefCell<MediaState>>,
    ui: Rc<RefCell<UiState>>,
    desktop: Option<Arc<DesktopState>>,
) -> MediaActions {
    let on_toggle = Rc::clone(&state);
    let on_select_session = Rc::clone(&state);
    let on_select_medium = Rc::clone(&state);
    let ui_select_session = Rc::clone(&ui);
    let ui_add_medium = Rc::clone(&ui);
    let add_medium_desktop = desktop.clone();

    MediaActions {
        // Expand/collapse a branch (medium or project) by its key.
        on_toggle: Rc::new(RefCell::new(move |key: String| {
            on_toggle.borrow_mut().toggle(&key);
        })),
        // Select a session leaf, scoping the next turn to its medium+project+
        // session and pointing the active pane's cwd at the session's path.
        on_select_session: Rc::new(RefCell::new(
            move |medium_id: String, project_id: String, session_id: String| {
                if on_select_session
                    .borrow_mut()
                    .select_session(&medium_id, &project_id, &session_id)
                {
                    let path = on_select_session.borrow().selected_session_path();
                    let mut ui = ui_select_session.borrow_mut();
                    ui.set_active_pane_path(path);
                }
            },
        )),
        // Select a work environment (medium) by id, resetting the project and
        // session to that medium's defaults.
        on_select_medium: Rc::new(RefCell::new(move |medium_id: String| {
            let _ = on_select_medium.borrow_mut().select_medium(&medium_id);
        })),
        // Add a custom environment medium by label: derive a distinct id from
        // the label, add it to the tree, select it, persist it so it reappears,
        // and close the add-medium dialog.
        on_add_medium: Rc::new(RefCell::new(move |label: String| {
            let trimmed = label.trim();
            if trimmed.is_empty() {
                return;
            }
            let mut media = state.borrow_mut();
            let base = medium_id_from_label(trimmed);
            let mut id = base.clone();
            let mut suffix = 2;
            while media.mediums.iter().any(|m| m.id == id) {
                id = format!("{base}-{suffix}");
                suffix += 1;
            }
            if media.add_medium(&id, trimmed) {
                // Persist the updated custom-medium list so it reappears next run.
                if let Ok(json) = serde_json::to_string(media.custom_mediums()) {
                    if let Some(d) = &add_medium_desktop {
                        if let Err(e) = d.set_ui_mediums(&json) {
                            log::warn!("set_ui_mediums failed: {e}");
                        }
                    }
                }
                let _ = media.select_medium(&id);
            }
            drop(media);
            // Close the add-medium dialog (a custom medium is now selected).
            ui_add_medium.borrow_mut().add_medium_dialog_open = false;
        })),
    }
}

/// Turn a user-supplied label into a filesystem/cache-safe medium id: lowercase,
/// non-alphanumerics become dashes, and neighbouring dashes collapse. An empty
/// result falls back to `"medium"`.
fn medium_id_from_label(label: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in label.chars() {
        if ch.is_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let id = out.trim_matches('-').to_string();
    if id.is_empty() {
        "medium".to_string()
    } else {
        id
    }
}
