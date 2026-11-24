//! The one project picker shared by the Conversations "+", the Projects "+"
//! and the composer's project pill. Each surface only changes the context: which
//! selection is checked and what New Project / No Project do.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::PopupMenuItem;

use super::actions::UiActions;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectMenuContext {
    Conversations,
    Projects,
    Composer,
}

/// The menu rows for `context`. Remote and SSH Host are submenu parents (the
/// chevron the reference shows); the second group is New Project; No Project is
/// the checked default for the conversation surfaces.
pub(crate) fn items(
    actions: &UiActions,
    context: ProjectMenuContext,
    no_project_selected: bool,
) -> Vec<PopupMenuItem> {
    let remote = actions.on_setup_remote_with_agent.clone();
    let ssh = actions.on_open_ssh_target.clone();
    let mut rows = vec![
        PopupMenuItem::new("Remote")
            .with_icon("environment-cloud")
            .with_children(vec![PopupMenuItem::new("Set up Remote…")
                .with_on_select(move || (remote.borrow_mut())())]),
        PopupMenuItem::new("SSH Host")
            .with_icon("terminal-input")
            .with_children(vec![
                PopupMenuItem::new("Choose SSH Host…").with_on_select(move || (ssh.borrow_mut())())
            ]),
        PopupMenuItem::new("New Project")
            .with_icon("folder")
            .with_separator_before(),
    ];
    if context != ProjectMenuContext::Projects {
        rows.push(PopupMenuItem {
            selected: no_project_selected,
            ..PopupMenuItem::new("No Project")
                .with_icon("file")
                .with_separator_before()
        });
    }
    rows
}

/// Run the selected row for `context`. Remote/SSH Host rows are submenu
/// parents and never reach here.
pub(crate) fn on_select(actions: &UiActions, context: ProjectMenuContext, index: usize) {
    match (context, index) {
        (_, 2) => match context {
            ProjectMenuContext::Projects => (actions.on_add_workspace.borrow_mut())(),
            ProjectMenuContext::Conversations => {
                (actions.on_new_project_conversation.borrow_mut())()
            }
            ProjectMenuContext::Composer => (actions.on_bind_project.borrow_mut())(),
        },
        (ProjectMenuContext::Conversations, 3) => (actions.on_create_submit.borrow_mut())(),
        (ProjectMenuContext::Composer, 3) => (actions.on_clear_project.borrow_mut())(),
        _ => {}
    }
}

/// A menu open flag that closes its sibling surfaces, so only one project
/// picker can be up at a time.
pub(crate) fn close_siblings(open: &Rc<RefCell<bool>>, siblings: &[Rc<RefCell<bool>>]) {
    if !*open.borrow() {
        for sibling in siblings {
            *sibling.borrow_mut() = false;
        }
    }
}
