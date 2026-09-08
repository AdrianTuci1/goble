//! Callback wiring for the projects domain: turns app state + backend into
//! [`crate::ui::ProjectsActions`] closures.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::DesktopState;

use crate::ui::ProjectsActions;

use super::state::ProjectsState;

pub fn make_projects_actions(
    state: Rc<RefCell<ProjectsState>>,
    desktop: Option<Arc<DesktopState>>,
) -> ProjectsActions {
    let desktop_refresh = desktop.clone();
    let on_refresh = Rc::clone(&state);
    ProjectsActions {
        on_refresh: Rc::new(RefCell::new(move || {
            if let Some(desktop) = &desktop_refresh {
                on_refresh.borrow_mut().refresh(desktop);
            }
        })),
    }
}
