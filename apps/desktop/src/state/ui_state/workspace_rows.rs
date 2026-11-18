//! The sidebar's Workspaces rows: the durable list of directories the user
//! added with the header's "+", the group rows' tray state, the "Rename
//! workspace" dialog, and the actions that write them through.
//!
//! A Workspace row in the sidebar is a directory (DSH's `Workspace`): the
//! Workspaces header's "+" opens the platform's directory picker, the picked
//! path becomes a row, and the row's hover controls create conversations inside
//! it, rename it, or delete it. The list is real durable state — it rides the
//! same `ui.*` setting blob the persisted pane layout and custom environments
//! use (`ui.workspaces`, see `DesktopState::get_ui_workspaces`) — so a
//! workspace with no conversations yet still comes back on the next launch.
//!
//! Conversations are grouped by the last segment of their `working_dir`
//! ([`conversation_group`](super::refresh::conversation_group)), which is the
//! same value [`SidebarWorkspace::folder`] derives, so a workspace's stored
//! title lands on the folder group its sessions form.

use super::*;

/// One Workspace the sidebar lists: the directory the user added and the title
/// they renamed it to, if they did. A workspace with no conversations still
/// draws its row (that is what the "+" is for); a folder group with sessions but
/// no stored workspace is listed from its sessions alone, with no delete action.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SidebarWorkspace {
    /// The absolute directory the user picked.
    pub path: String,
    /// The display title the user renamed the workspace to. Empty means the
    /// row reads the folder's own name (the directory's last segment).
    #[serde(default)]
    pub title: String,
}

impl SidebarWorkspace {
    /// A workspace over `path`, titled by its folder.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            title: String::new(),
        }
    }

    /// The group section's folder name: the directory's last segment, the value
    /// the sidebar groups sessions by (`conversation_group`).
    pub fn folder(&self) -> String {
        super::refresh::conversation_group(Some(&self.path))
    }

    /// The label the row draws: the stored title, else the folder's name.
    pub fn display_title(&self) -> String {
        let title = self.title.trim();
        if title.is_empty() {
            self.folder()
        } else {
            title.to_string()
        }
    }
}

/// The "Rename workspace" dialog's state: the workspace it names, the draft,
/// whether the field has the focus, and an inline error kept on failure. The
/// dialog is the same one the session rename uses (title/field copy aside).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarWorkspaceRename {
    /// The workspace's directory: the durable identity of the entry.
    pub path: String,
    /// The draft the field shows.
    pub draft: String,
    /// Whether the field holds the keyboard focus.
    pub focused: bool,
    /// The write failure, kept on the dialog while it is up.
    pub error: Option<String>,
}

impl UiState {
    /// Reload the durable Workspaces list from the store. Called with the other
    /// per-launch refreshes; a fresh install has none and the sidebar lists only
    /// the folders its conversations imply.
    pub fn refresh_workspaces(&mut self, desktop: &DesktopState) {
        let parsed = desktop
            .get_ui_workspaces()
            .and_then(|json| serde_json::from_str::<Vec<SidebarWorkspace>>(&json).ok());
        self.sidebar_workspaces = parsed.unwrap_or_default();
        // A row's tray cannot outlive the row it belongs to.
        let folders: Vec<String> = self
            .sidebar_workspaces
            .iter()
            .map(|workspace| workspace.folder())
            .collect();
        self.sidebar_group_ui
            .borrow_mut()
            .retain(|key, _| folders.iter().any(|folder| folder == key));
    }

    /// Persist the Workspaces list through the desktop service (a tests-only
    /// state has no store and keeps the list in memory).
    fn persist_workspaces(&self, desktop: Option<&DesktopState>) {
        let Some(desktop) = desktop else {
            return;
        };
        match serde_json::to_string(&self.sidebar_workspaces) {
            Ok(json) => {
                if let Err(error) = desktop.set_ui_workspaces(&json) {
                    log::warn!("set_ui_workspaces failed: {error}");
                }
            }
            Err(error) => log::warn!("serializing the Workspaces list failed: {error}"),
        }
    }

    /// The stored workspace for a group section, if the user added one.
    pub fn workspace_for_folder(&self, folder: &str) -> Option<&SidebarWorkspace> {
        self.sidebar_workspaces
            .iter()
            .find(|workspace| workspace.folder() == folder)
    }

    /// The stored workspace over exactly `path`, if any.
    fn workspace_over_path(&mut self, path: &str) -> Option<&mut SidebarWorkspace> {
        self.sidebar_workspaces
            .iter_mut()
            .find(|workspace| workspace.path == path)
    }

    /// Adopt the directory the platform's picker returned as a Workspace (the
    /// Workspaces header "+", DSH's add flow). The path is deduplicated: adding
    /// the same directory twice keeps the one row. Returns the row it created,
    /// or `None` when the answer was an empty path.
    pub fn add_workspace(
        &mut self,
        path: &str,
        desktop: Option<&DesktopState>,
    ) -> Option<SidebarWorkspace> {
        let path = path.trim().trim_end_matches('/').to_string();
        if path.is_empty() {
            return None;
        }
        if self
            .sidebar_workspaces
            .iter()
            .any(|workspace| workspace.path == path)
        {
            return None;
        }
        let workspace = SidebarWorkspace::new(path);
        self.sidebar_workspaces.push(workspace.clone());
        self.persist_workspaces(desktop);
        Some(workspace)
    }

    /// Delete a Workspace from the list (DSH's "Delete workspace": the folder
    /// and the conversations in it are kept — they simply fall back to their
    /// folder group, since the sidebar derives folders from sessions too).
    pub fn delete_workspace(&mut self, path: &str, desktop: Option<&DesktopState>) {
        let before = self.sidebar_workspaces.len();
        self.sidebar_workspaces.retain(|workspace| workspace.path != path);
        if self.sidebar_workspaces.len() == before {
            return;
        }
        let folder = super::refresh::conversation_group(Some(path));
        if let Some(ui) = self.sidebar_group_ui.borrow().get(&folder) {
            ui.borrow_mut().close_menu();
        }
        self.persist_workspaces(desktop);
    }

    /// Reveal a Workspace's directory in the platform's file manager — DSH's
    /// "Show in Finder" workspace menu row. The platform call is the real one
    /// (`NSWorkspace` on macOS, the desktop's file manager elsewhere); when it
    /// cannot reveal the folder the transient notice says so instead of
    /// pretending the folder opened.
    pub fn reveal_workspace_in_file_manager(&mut self, path: &str) {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return;
        }
        if goble_ui::platform::current::reveal_in_file_manager(trimmed) {
            return;
        }
        self.show_toast(super::ToastKind::RevealFailed);
    }

    /// Open the "Rename workspace" dialog for the group section `folder`. A
    /// folder that only exists through its conversations has no stored entry
    /// yet; the commit adopts one, which is what makes its rename durable.
    pub fn begin_workspace_rename(&mut self, path: &str, current_title: &str) {
        self.sidebar_workspace_rename = Some(SidebarWorkspaceRename {
            path: path.to_string(),
            draft: current_title.to_string(),
            focused: true,
            error: None,
        });
    }

    /// The rename field's draft changed.
    pub fn workspace_rename_draft(&mut self, draft: String) {
        if let Some(rename) = self.sidebar_workspace_rename.as_mut() {
            rename.draft = draft;
            rename.error = None;
        }
    }

    /// Leave the rename dialog without writing anything.
    pub fn cancel_workspace_rename(&mut self) {
        self.sidebar_workspace_rename = None;
    }

    /// Write the rename: the row's folder group is titled `draft` and the entry
    /// is created if the folder was only implied by its sessions (adopting it
    /// into the durable list).
    pub fn commit_workspace_rename(&mut self, desktop: Option<&DesktopState>) {
        let Some(rename) = self.sidebar_workspace_rename.clone() else {
            return;
        };
        let title = rename.draft.trim().to_string();
        if title.is_empty() {
            if let Some(open) = self.sidebar_workspace_rename.as_mut() {
                open.error = Some("The workspace name can’t be empty.".to_string());
            }
            return;
        }
        let folder = super::refresh::conversation_group(Some(&rename.path));
        let stored_title = if title == folder { String::new() } else { title };
        match self.workspace_over_path(&rename.path) {
            Some(workspace) => workspace.title = stored_title,
            None => self.sidebar_workspaces.push(SidebarWorkspace {
                path: rename.path.clone(),
                title: stored_title,
            }),
        }
        self.persist_workspaces(desktop);
        self.sidebar_workspace_rename = None;
    }

    /// Make `open` the one open workspace tray (or close every tray when it is
    /// `None`). The session rows' trays are put away too: one menu is open at a
    /// time, the same rule DSH's context-menu registry enforces.
    pub fn set_workspace_menu(&mut self, open: Option<&str>) {
        for (key, ui) in self.sidebar_group_ui.borrow().iter() {
            let mut ui = ui.borrow_mut();
            if Some(key.as_str()) != open {
                ui.close_menu();
            }
        }
        for card in self.agent_cards.values() {
            card.borrow_mut().close_menu();
        }
        if let Some(key) = open {
            let ui = self
                .sidebar_group_ui
                .borrow()
                .get(key)
                .cloned()
                .unwrap_or_default();
            ui.borrow_mut().menu_open = true;
            self.sidebar_group_ui
                .borrow_mut()
                .entry(key.to_string())
                .or_insert(ui);
        }
    }

    /// The group row's tray state cell for `section`, created on first use and
    /// shared with the element tree (the rows are rebuilt every frame).
    pub fn group_ui(&self, section: &str) -> Rc<RefCell<goble_ui::elements::conversation_list_item::SidebarGroupUi>> {
        self.sidebar_group_ui
            .borrow_mut()
            .entry(section.to_string())
            .or_default()
            .clone()
    }

    /// Every group section's tray state cell, keyed by section.
    pub fn group_uis(
        &self,
    ) -> std::collections::HashMap<
        String,
        Rc<RefCell<goble_ui::elements::conversation_list_item::SidebarGroupUi>>,
    > {
        self.sidebar_group_ui.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> UiState {
        UiState::mock()
    }

    /// The add flow adopts the picked directory: the row appears, its folder is
    /// the directory's last segment, and its display title is the folder until
    /// the user renames it.
    #[test]
    fn adding_a_workspace_appends_the_picked_directory() {
        let mut state = state();
        assert!(state.sidebar_workspaces.is_empty());
        let added = state
            .add_workspace("/Users/someone/Projects/goble/", None)
            .expect("the picked directory becomes a workspace");
        assert_eq!(added.path, "/Users/someone/Projects/goble");
        assert_eq!(added.folder(), "goble");
        assert_eq!(added.display_title(), "goble");
        assert_eq!(state.sidebar_workspaces.len(), 1);
    }

    /// Adding the same directory twice keeps one row.
    #[test]
    fn adding_the_same_workspace_twice_keeps_one_row() {
        let mut state = state();
        assert!(state.add_workspace("/tmp/alpha", None).is_some());
        assert!(state.add_workspace("/tmp/alpha", None).is_none());
        assert_eq!(state.sidebar_workspaces.len(), 1);
    }

    /// An empty pick (the picker was cancelled, or the platform has none) adds
    /// nothing.
    #[test]
    fn a_cancelled_pick_adds_no_workspace() {
        let mut state = state();
        assert!(state.add_workspace("   ", None).is_none());
        assert!(state.sidebar_workspaces.is_empty());
    }

    /// Renaming writes a display title that outlives the folder's own name; a
    /// rename back to the folder's name drops the override.
    #[test]
    fn renaming_a_workspace_stores_its_title() {
        let mut state = state();
        state.add_workspace("/tmp/alpha", None).expect("added");
        state.begin_workspace_rename("/tmp/alpha", "alpha");
        state.workspace_rename_draft("Alpha App".to_string());
        state.commit_workspace_rename(None);
        let workspace = state
            .workspace_for_folder("alpha")
            .expect("the stored workspace");
        assert_eq!(workspace.display_title(), "Alpha App");
        assert!(state.sidebar_workspace_rename.is_none());

        state.begin_workspace_rename("/tmp/alpha", "Alpha App");
        state.workspace_rename_draft("alpha".to_string());
        state.commit_workspace_rename(None);
        assert_eq!(
            state.workspace_for_folder("alpha").unwrap().title,
            "",
            "renaming back to the folder drops the override"
        );
    }

    /// A blank name keeps the dialog open with an error and writes nothing.
    #[test]
    fn a_blank_rename_is_refused() {
        let mut state = state();
        state.add_workspace("/tmp/alpha", None).expect("added");
        state.begin_workspace_rename("/tmp/alpha", "alpha");
        state.workspace_rename_draft("  ".to_string());
        state.commit_workspace_rename(None);
        let open = state
            .sidebar_workspace_rename
            .as_ref()
            .expect("the dialog stays open");
        assert!(open.error.is_some());
        assert_eq!(state.workspace_for_folder("alpha").unwrap().title, "");
    }

    /// Deleting drops the row (and only that row); renaming a folder the user
    /// never added adopts it into the durable list.
    #[test]
    fn deleting_and_adopting_workspaces() {
        let mut state = state();
        state.add_workspace("/tmp/alpha", None).expect("added");
        state.add_workspace("/tmp/beta", None).expect("added");
        state.delete_workspace("/tmp/alpha", None);
        assert!(state.workspace_for_folder("alpha").is_none());
        assert!(state.workspace_for_folder("beta").is_some());

        // A folder with sessions but no stored workspace: the rename adopts it.
        state.begin_workspace_rename("/tmp/gamma", "gamma");
        state.workspace_rename_draft("Gamma".to_string());
        state.commit_workspace_rename(None);
        assert_eq!(state.workspace_for_folder("gamma").unwrap().title, "Gamma");
    }

    /// "Show in Finder" is the real platform reveal, not a fake: on this test
    /// thread AppKit refuses to run, and the action reports that failure with
    /// the warning notice instead of pretending the folder opened. The
    /// workspace list is untouched (revealing is a read-only action).
    #[test]
    fn showing_a_workspace_in_finder_reports_the_platform_outcome() {
        let mut state = state();
        state.add_workspace("/tmp/alpha", None).expect("added");
        state.reveal_workspace_in_file_manager("/tmp/alpha");
        assert!(
            matches!(
                state.toast.as_ref().map(|notice| &notice.kind),
                Some(crate::state::ToastKind::RevealFailed)
            ),
            "the worker thread's refused AppKit call is reported: {:?}",
            state.toast
        );
        assert_eq!(state.sidebar_workspaces.len(), 1, "revealing changes no row");

        // A row with no directory has nothing to reveal, so it raises nothing.
        state.dismiss_toast();
        state.reveal_workspace_in_file_manager("   ");
        assert!(state.toast.is_none());
    }

    /// One workspace tray at a time: opening one closes the others (and the
    /// session rows' trays), which is what "an open menu" must mean app-wide.
    #[test]
    fn opening_a_workspace_tray_closes_every_other_menu() {
        let mut state = state();
        let first = state.group_ui("alpha");
        let second = state.group_ui("beta");
        state.set_workspace_menu(Some("alpha"));
        assert!(first.borrow().menu_open);
        assert!(!second.borrow().menu_open);
        state.set_workspace_menu(Some("beta"));
        assert!(!first.borrow().menu_open);
        assert!(second.borrow().menu_open);
        state.set_workspace_menu(None);
        assert!(!second.borrow().menu_open);
    }
}
