//! Environment domain state: a hierarchical tree of medium -> project ->
//! session, plus the selected leaf.
//!
//! Mirrors the plain [`crate::ui::MediaSnapshot`] but lives in the executable
//! so it is owned by the app. The tree's top level mirrors the harness's
//! [`MediumKind`] variants; each medium expands to the projects that carry
//! sessions on it, and each project expands to its sessions. Sessions/projects
//! come from the backend ([`DesktopState::list_projects`] /
//! [`DesktopState::list_sessions`]); they always come from the store, so there
//! is no mock fallback.

use goble_desktop_service::DesktopState;
use goble_harness_types::MediumKind;
use goble_persistence::{Project, Session};

use crate::ui::{MediaNode, MediaProject, MediaSession};

/// The top-level mediums, in display order: `(kind, id, label)`.
const MEDIUM_META: &[(MediumKind, &str, &str)] = &[
    (MediumKind::Local, "local", "Local"),
    (MediumKind::RemoteXrdp, "remote-xrdp", "Remote (xrdp)"),
];

/// The default project a medium's turns are scoped to before any session is
/// picked: `(medium_id, project_id)`.
const DEFAULT_PROJECTS: &[(&str, &str)] = &[("local", "default"), ("remote-xrdp", "remote")];

/// Map a medium id to the conversation `workspace_routing` value (`"local"` /
/// `"remote"`) so the sidebar can show the folders + conversations that belong
/// to the selected environment.
pub fn medium_routing(medium_id: &str) -> &'static str {
    match medium_id {
        "remote-xrdp" => "remote",
        _ => "local",
    }
}

fn default_project_for(medium_id: &str) -> String {
    DEFAULT_PROJECTS
        .iter()
        .find(|(mid, _)| *mid == medium_id)
        .map(|(_, pid)| (*pid).to_string())
        .unwrap_or_else(|| "default".to_string())
}

/// The expand/collapse key for a medium (top-level) branch node.
pub fn medium_key(medium_id: &str) -> String {
    format!("medium:{medium_id}")
}

/// The expand/collapse key for a project branch node under a medium.
pub fn project_key(medium_id: &str, project_id: &str) -> String {
    format!("project:{medium_id}:{project_id}")
}

/// Read the persisted custom mediums `(id, label)` from the store, if any. The
/// built-in Local/Remote mediums are never persisted; this returns only the
/// mediums a user added via the topbar "+" menu.
fn load_custom_mediums(desktop: Option<&DesktopState>) -> Vec<(String, String)> {
    let Some(d) = desktop else {
        return Vec::new();
    };
    let Some(json) = d.get_ui_mediums() else {
        return Vec::new();
    };
    serde_json::from_str(&json).unwrap_or_default()
}

/// Build the tree from the durable projects + sessions: group the sessions by
/// the medium they run on, then by the project they are scoped to. A session's
/// path is its project's directory (used as the pane's cwd when picked).
fn build_from_backend(projects: &[Project], sessions: &[Session]) -> Vec<MediaNode> {
    let project_directory = |pid: &str| -> String {
        projects
            .iter()
            .find(|p| p.project_id.0 == pid)
            .map(|p| p.directory.clone())
            .unwrap_or_default()
    };

    MEDIUM_META
        .iter()
        .map(|(kind, medium_id, label)| {
            let mut by_project: Vec<(String, Vec<MediaSession>)> = Vec::new();
            for s in sessions.iter().filter(|s| s.medium_id.0 == *medium_id) {
                let pid = if s.project_id.0.is_empty() {
                    "default"
                } else {
                    s.project_id.0.as_str()
                };
                let pid = pid.to_string();
                if let Some((_, list)) = by_project.iter_mut().find(|(p, _)| *p == pid) {
                    list.push(MediaSession {
                        id: s.session_id.0.clone(),
                        label: s.session_id.0.clone(),
                        path: project_directory(&pid),
                    });
                } else {
                    by_project.push((
                        pid.clone(),
                        vec![MediaSession {
                            id: s.session_id.0.clone(),
                            label: s.session_id.0.clone(),
                            path: project_directory(&pid),
                        }],
                    ));
                }
            }
            let projects = by_project
                .into_iter()
                .map(|(pid, sessions)| MediaProject {
                    id: pid.clone(),
                    label: pid.clone(),
                    sessions,
                })
                .collect();
            MediaNode {
                kind: *kind,
                id: (*medium_id).to_string(),
                label: (*label).to_string(),
                projects,
            }
        })
        .collect()
}

/// A small deterministic tree used only by the [`MediaState::mock`] test
/// fixture: each medium has its default project and one session on it.
fn build_mock() -> Vec<MediaNode> {
    MEDIUM_META
        .iter()
        .map(|(kind, medium_id, label)| {
            let pid = default_project_for(medium_id);
            MediaNode {
                kind: *kind,
                id: (*medium_id).to_string(),
                label: (*label).to_string(),
                projects: vec![MediaProject {
                    id: pid.clone(),
                    label: pid.clone(),
                    sessions: vec![MediaSession {
                        id: format!("s-{medium_id}"),
                        label: format!("s-{medium_id}"),
                        path: format!("/workspace/{pid}"),
                    }],
                }],
            }
        })
        .collect()
}

/// Whether a node key names a branch that exists in `nodes` (used to drop
/// expansions that no longer resolve after a refresh).
fn key_exists(nodes: &[MediaNode], key: &str) -> bool {
    if let Some(medium_id) = key.strip_prefix("medium:") {
        return nodes.iter().any(|m| m.id == medium_id);
    }
    if let Some(rest) = key.strip_prefix("project:") {
        let mut parts = rest.splitn(2, ':');
        let medium_id = parts.next().unwrap_or("");
        let project_id = parts.next().unwrap_or("");
        return nodes
            .iter()
            .any(|m| m.id == medium_id && m.projects.iter().any(|p| p.id == project_id));
    }
    false
}

#[derive(Clone)]
pub struct MediaState {
    /// The environment tree, in display order.
    pub mediums: Vec<MediaNode>,
    /// The id of the currently selected medium.
    pub selected_medium: String,
    /// The id of the currently selected project (under the selected medium).
    pub selected_project: String,
    /// The id of the currently selected session leaf, or empty when the user
    /// has not picked one yet (the turn then runs on the pane's own session).
    pub selected_session: String,
    /// Keys of the tree branches currently expanded (medium / project nodes).
    pub expanded: Vec<String>,
    /// User-added mediums `(id, label)`, persisted so a custom environment the
    /// user adds via the topbar "+" menu reappears across restarts. The
    /// built-in Local/Remote mediums are never stored here.
    pub custom_mediums: Vec<(String, String)>,
}

impl MediaState {
    /// Test fixture: a deterministic tree with `local` selected and expanded.
    ///
    /// Not used by the runtime — the app always builds from the real store via
    /// [`Self::from_desktop`]. Kept as an explicit fixture so tests can
    /// construct a known tree without a backend store.
    pub fn mock() -> Self {
        let mut state = Self {
            mediums: build_mock(),
            selected_medium: "local".to_string(),
            selected_project: default_project_for("local"),
            selected_session: String::new(),
            expanded: vec![medium_key("local")],
            custom_mediums: Vec::new(),
        };
        state.append_custom_mediums();
        state
    }

    /// Start from real backend data (sessions/projects grouped per medium),
    /// selecting `local` by default. User-added mediums persisted in the store
    /// are re-appended so they reappear across a restart.
    pub fn from_desktop(desktop: &DesktopState) -> Self {
        let mut state = Self {
            mediums: build_from_backend(&desktop.list_projects(), &desktop.list_sessions()),
            selected_medium: "local".to_string(),
            selected_project: default_project_for("local"),
            selected_session: String::new(),
            expanded: vec![medium_key("local")],
            custom_mediums: load_custom_mediums(Some(desktop)),
        };
        state.append_custom_mediums();
        state.reconcile_selection();
        state
    }

    /// Rebuild the tree from the backend store. A selection/expansion that no
    /// longer resolves is reset.
    pub fn refresh(&mut self, desktop: &DesktopState) {
        self.mediums = build_from_backend(&desktop.list_projects(), &desktop.list_sessions());
        self.custom_mediums = load_custom_mediums(Some(desktop));
        self.append_custom_mediums();
        self.expanded.retain(|key| key_exists(&self.mediums, key));
        self.reconcile_selection();
    }

    /// Reset the selection to the `local`/`default` defaults when the selected
    /// medium/project/session no longer exist in the tree.
    fn reconcile_selection(&mut self) {
        if !self.mediums.iter().any(|m| m.id == self.selected_medium) {
            self.selected_medium = "local".to_string();
            self.selected_project = default_project_for("local");
            self.selected_session.clear();
            return;
        }
        if !self.session_exists(&self.selected_medium, &self.selected_project, &self.selected_session)
            && !self.selected_session.is_empty()
        {
            self.selected_session.clear();
        }
    }

    /// Append a node for each persisted custom medium that is not already in the
    /// tree (a custom medium has no backend sessions, so it carries a single
    /// default project/session and routes to `local` by default).
    fn append_custom_mediums(&mut self) {
        for (id, label) in self.custom_mediums.clone() {
            if self.mediums.iter().any(|m| m.id == id) {
                continue;
            }
            self.mediums.push(MediaNode {
                kind: MediumKind::Vm,
                id: id.clone(),
                label,
                projects: vec![MediaProject {
                    id: "default".to_string(),
                    label: "default".to_string(),
                    sessions: vec![MediaSession {
                        id: format!("s-{id}"),
                        label: format!("s-{id}"),
                        path: format!("/workspace/{id}"),
                    }],
                }],
            });
        }
    }

    fn session_exists(&self, medium_id: &str, project_id: &str, session_id: &str) -> bool {
        self.mediums
            .iter()
            .find(|m| m.id == medium_id)
            .and_then(|m| m.projects.iter().find(|p| p.id == project_id))
            .and_then(|p| p.sessions.iter().find(|s| s.id == session_id))
            .is_some()
    }

    /// Expand/collapse a branch node by its key (see [`medium_key`] /
    /// [`project_key`]). Unknown keys are ignored.
    pub fn toggle(&mut self, key: &str) {
        if self.expanded.iter().any(|k| k == key) {
            self.expanded.retain(|k| k != key);
        } else {
            self.expanded.push(key.to_string());
        }
    }

    /// Whether a branch node (medium / project) is currently expanded.
    pub fn is_expanded(&self, key: &str) -> bool {
        self.expanded.iter().any(|k| k == key)
    }

    /// Select a session leaf, setting the active medium+project+session for the
    /// next turn and expanding the branch so the leaf stays visible. Returns
    /// whether the leaf was found (and thus the selection changed); an unknown
    /// leaf is ignored and leaves the selection untouched.
    pub fn select_session(&mut self, medium_id: &str, project_id: &str, session_id: &str) -> bool {
        if !self.session_exists(medium_id, project_id, session_id) {
            return false;
        }
        self.selected_medium = medium_id.to_string();
        self.selected_project = project_id.to_string();
        self.selected_session = session_id.to_string();
        let mk = medium_key(medium_id);
        if !self.expanded.iter().any(|k| *k == mk) {
            self.expanded.push(mk);
        }
        let pk = project_key(medium_id, project_id);
        if !self.expanded.iter().any(|k| *k == pk) {
            self.expanded.push(pk);
        }
        true
    }

    /// Select only a medium (used by the composer's harness pill): resets the
    /// project to that medium's default and clears the session so the next turn
    /// runs on the pane's own session. Unknown mediums are ignored.
    pub fn select_medium(&mut self, medium_id: &str) -> bool {
        if !self.mediums.iter().any(|m| m.id == medium_id) {
            return false;
        }
        self.selected_medium = medium_id.to_string();
        self.selected_project = default_project_for(medium_id);
        self.selected_session.clear();
        let mk = medium_key(medium_id);
        if !self.expanded.iter().any(|k| *k == mk) {
            self.expanded.push(mk);
        }
        true
    }

    /// Add a custom environment medium `(id, label)` to the tree and to the
    /// persisted custom list. Returns `false` when a medium with that id already
    /// exists (the caller should pick a distinct id). The new medium routes to
    /// `local` by default (`medium_routing` falls back to `"local"`).
    pub fn add_medium(&mut self, id: &str, label: &str) -> bool {
        if self.mediums.iter().any(|m| m.id == id) {
            return false;
        }
        self.mediums.push(MediaNode {
            kind: MediumKind::Vm,
            id: id.to_string(),
            label: label.to_string(),
            projects: vec![MediaProject {
                id: "default".to_string(),
                label: "default".to_string(),
                sessions: vec![MediaSession {
                    id: format!("s-{id}"),
                    label: format!("s-{id}"),
                    path: format!("/workspace/{id}"),
                }],
            }],
        });
        self.custom_mediums.push((id.to_string(), label.to_string()));
        true
    }

    /// The custom mediums `(id, label)` persisted so a custom environment the
    /// user added reappears after a restart.
    pub fn custom_mediums(&self) -> &[(String, String)] {
        &self.custom_mediums
    }

    /// The default project id a medium's turns are scoped to before any session
    /// is picked (used to scope a new space's cwd to the chosen environment).
    pub fn default_project_for_medium(&self, medium_id: &str) -> String {
        default_project_for(medium_id)
    }

    /// Where a session lives in the tree, as `(medium_id, project_id, session_id)`.
    /// Used to resolve the composer's working-directory pill to a session.
    pub fn session_location(&self, session_id: &str) -> Option<(String, String, String)> {
        for m in &self.mediums {
            for p in &m.projects {
                if p.sessions.iter().any(|s| s.id == session_id) {
                    return Some((m.id.clone(), p.id.clone(), session_id.to_string()));
                }
            }
        }
        None
    }

    /// The [`goble_harness_types::MediumId`] string carried on the turn.
    pub fn selected_medium_id(&self) -> &str {
        &self.selected_medium
    }

    /// The `project_id` the selected medium's turns are scoped to.
    pub fn selected_project_id(&self) -> &str {
        &self.selected_project
    }

    /// The session id picked in the tree, or empty when the user has not picked
    /// one (the turn then runs on the pane's own session/conversation).
    pub fn selected_session_id(&self) -> &str {
        &self.selected_session
    }

    /// The working directory of the selected session (its project's directory),
    /// used to set the active pane's cwd when a session is chosen. Empty when
    /// no session is selected.
    pub fn selected_session_path(&self) -> String {
        self.session_path(&self.selected_medium, &self.selected_project, &self.selected_session)
            .unwrap_or_default()
    }

    fn session_path(&self, medium_id: &str, project_id: &str, session_id: &str) -> Option<String> {
        self.mediums
            .iter()
            .find(|m| m.id == medium_id)?
            .projects
            .iter()
            .find(|p| p.id == project_id)?
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .map(|s| s.path.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_harness_types::{MediumId, ProjectId, SessionId};
    use goble_persistence::{Project, Session};

    /// A real `DesktopState` over an in-memory sqlite store + temp thread store.
    fn desktop() -> (std::sync::Arc<goble_desktop_service::DesktopState>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("create temp thread store dir");
        let state = goble_desktop_service::DesktopState::new(
            goble_core::store::Store::open_in_memory().expect("open in-memory store"),
            goble_desktop_service::ThreadStore::new(dir.path()).expect("open thread store"),
        );
        (state, dir)
    }

    fn project(id: &str, dir: &str) -> Project {
        Project {
            project_id: ProjectId::new(id),
            directory: dir.to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    fn session(id: &str, project: &str, medium: &str) -> Session {
        Session {
            session_id: SessionId::new(id),
            project_id: ProjectId::new(project),
            medium_id: MediumId::new(medium),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn default_selects_local_and_expands_it() {
        let state = MediaState::mock();
        assert_eq!(state.selected_medium_id(), "local");
        assert_eq!(state.selected_project_id(), "default");
        assert_eq!(state.mediums.len(), 2, "only Local + Remote environments");
        assert_eq!(state.selected_session_id(), "");
        assert!(state.is_expanded(&medium_key("local")));
    }

    #[test]
    fn toggle_expands_and_collapses_a_branch() {
        let mut state = MediaState::mock();
        assert!(!state.is_expanded(&medium_key("remote-xrdp")));

        state.toggle(&medium_key("remote-xrdp"));
        assert!(state.is_expanded(&medium_key("remote-xrdp")));

        // Selecting a session also auto-expands its branch.
        state.select_session("remote-xrdp", "remote", "s-remote-xrdp");
        assert!(state.is_expanded(&project_key("remote-xrdp", "remote")));

        state.toggle(&project_key("remote-xrdp", "remote"));
        assert!(!state.is_expanded(&project_key("remote-xrdp", "remote")));
    }

    #[test]
    fn select_session_sets_medium_project_and_session() {
        let mut state = MediaState::mock();
        state.select_session("remote-xrdp", "remote", "s-remote-xrdp");
        assert_eq!(state.selected_medium_id(), "remote-xrdp");
        assert_eq!(state.selected_project_id(), "remote");
        assert_eq!(state.selected_session_id(), "s-remote-xrdp");
        assert_eq!(state.selected_session_path(), "/workspace/remote");
    }

    #[test]
    fn select_session_ignores_unknown_leaf() {
        let mut state = MediaState::mock();
        state.select_session("remote-xrdp", "remote", "not-a-session");
        assert_eq!(state.selected_medium_id(), "local");
        assert_eq!(state.selected_session_id(), "");
    }

    #[test]
    fn select_medium_resets_project_and_session() {
        let mut state = MediaState::mock();
        state.select_session("remote-xrdp", "remote", "s-remote-xrdp");
        assert!(state.select_medium("remote-xrdp"));
        assert_eq!(state.selected_medium_id(), "remote-xrdp");
        assert_eq!(state.selected_session_id(), "");
        // Unknown mediums are ignored.
        assert!(!state.select_medium("vm"));
        assert_eq!(state.selected_medium_id(), "remote-xrdp");
    }

    #[test]
    fn session_location_finds_leaf_anywhere_in_tree() {
        let state = MediaState::mock();
        assert_eq!(
            state.session_location("s-remote-xrdp"),
            Some(("remote-xrdp".to_string(), "remote".to_string(), "s-remote-xrdp".to_string()))
        );
        assert_eq!(state.session_location("not-a-session"), None);
    }

    #[test]
    fn builds_tree_from_real_sessions_grouped_per_medium_and_project() {
        let projects = vec![
            project("alpha", "/workspace/alpha"),
            project("beta", "/workspace/beta"),
        ];
        let sessions = vec![
            session("s-a", "alpha", "local"),
            session("s-b", "", "local"),
            session("s-c", "alpha", "remote-xrdp"),
            session("s-d", "beta", "remote-xrdp"),
        ];
        let nodes = build_from_backend(&projects, &sessions);

        let local = nodes.iter().find(|m| m.id == "local").unwrap();
        assert_eq!(local.projects.len(), 2, "local groups alpha + default");
        let alpha = local.projects.iter().find(|p| p.id == "alpha").unwrap();
        assert_eq!(alpha.sessions.len(), 1);
        assert_eq!(alpha.sessions[0].id, "s-a");
        assert_eq!(alpha.sessions[0].path, "/workspace/alpha");
        let default = local.projects.iter().find(|p| p.id == "default").unwrap();
        assert_eq!(default.sessions[0].id, "s-b");

        let remote = nodes.iter().find(|m| m.id == "remote-xrdp").unwrap();
        assert_eq!(remote.projects.len(), 2);
        assert_eq!(remote.projects[0].sessions[0].id, "s-c");
        assert_eq!(remote.projects[1].sessions[0].id, "s-d");
    }

    #[test]
    fn refresh_rebuilds_from_store_not_mock() {
        let (desktop, _dir) = desktop();
        let mut state = MediaState::from_desktop(&desktop);
        // Refreshing always rebuilds from the store's projects + sessions; it
        // never substitutes a deterministic mock tree.
        state.refresh(&desktop);
        assert_eq!(state.mediums.len(), 2, "only Local + Remote environments");
        assert_eq!(state.selected_medium_id(), "local");
    }

    #[test]
    fn medium_routing_maps_remote_and_defaults_to_local() {
        assert_eq!(medium_routing("remote-xrdp"), "remote");
        assert_eq!(medium_routing("local"), "local");
        assert_eq!(medium_routing("vm"), "local");
        // A user-added medium routes to `local` by default.
        assert_eq!(medium_routing("staging-vps"), "local");
    }

    #[test]
    fn add_medium_appends_node_and_persists_custom_list() {
        let mut state = MediaState::mock();
        assert_eq!(state.mediums.len(), 2, "built-in Local + Remote only");
        assert!(state.add_medium("staging-vps", "Staging VPS"));
        assert_eq!(state.mediums.len(), 3);
        let node = state.mediums.iter().find(|m| m.id == "staging-vps").unwrap();
        assert_eq!(node.label, "Staging VPS");
        assert_eq!(node.projects.len(), 1, "custom medium has a default project");
        assert_eq!(
            state.custom_mediums(),
            &[("staging-vps".to_string(), "Staging VPS".to_string())]
        );
        // The new medium is selectable and resets the project to its default.
        assert!(state.select_medium("staging-vps"));
        assert_eq!(state.selected_medium_id(), "staging-vps");
        assert_eq!(state.selected_project_id(), "default");
        // A duplicate id is rejected.
        assert!(!state.add_medium("staging-vps", "Dupe"));
        assert_eq!(state.mediums.len(), 3);
    }

    #[test]
    fn refresh_reloads_persisted_custom_mediums() {
        let (desktop, _dir) = desktop();
        let mut state = MediaState::from_desktop(&desktop);
        // Persist a custom medium in the store; a refresh must re-append it.
        desktop
            .set_ui_mediums(r#"[["staging-vps","Staging VPS"]]"#)
            .expect("persist custom mediums");
        state.refresh(&desktop);
        assert!(state.mediums.iter().any(|m| m.id == "staging-vps"));
    }
}
