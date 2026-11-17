//! Projects domain state: sessions grouped per project + "what's running".
//!
//! Mirrors the plain [`crate::ui::ProjectsSnapshot`] but lives in the executable
//! so it is owned by the app. Sessions carry the project/medium identity of the
//! work (the reversibility substrate's `Session` entity); sessions with no
//! project fall back to the `default` project. The per-project status is
//! derived from the tasks scheduled/run against the project's sessions.

use std::collections::{BTreeMap, HashMap};

use goble_desktop_service::DesktopState;
use goble_persistence::{Project, Session, Task};

use crate::ui::{ProjectEntry, ProjectSessionEntry};

/// Status label used when a project has no live work.
const IDLE: &str = "idle";

#[derive(Clone)]
pub struct ProjectsState {
    pub projects: Vec<ProjectEntry>,
}

impl ProjectsState {
    /// Start from real backend data. Falls back to an empty list when the
    /// store has no sessions yet.
    pub fn from_desktop(desktop: &DesktopState) -> Self {
        let mut state = Self {
            projects: Vec::new(),
        };
        state.refresh(desktop);
        state
    }

    /// Reload sessions grouped per project and the per-project status.
    pub fn refresh(&mut self, desktop: &DesktopState) {
        let projects = desktop.list_projects();
        let sessions = desktop.list_sessions();
        let tasks = desktop.list_tasks();
        self.projects = group_projects(&projects, &sessions, &tasks);
    }

    /// Test fixture: a running, a scheduled and an idle project. Used only by
    /// tests (gated `#[cfg(test)]`); the app always builds from the real store
    /// via [`Self::from_desktop`].
    #[cfg(test)]
    pub fn mock() -> Self {
        Self {
            projects: vec![
                ProjectEntry {
                    project_id: "default".to_string(),
                    directory: "/workspace/goble".to_string(),
                    status: "running".to_string(),
                    sessions: vec![ProjectSessionEntry {
                        session_id: "s-1".to_string(),
                        medium: "local".to_string(),
                        created_at: "just now".to_string(),
                        status: "running".to_string(),
                    }],
                },
                ProjectEntry {
                    project_id: "notebooks".to_string(),
                    directory: "/workspace/notebooks".to_string(),
                    status: "scheduled".to_string(),
                    sessions: vec![ProjectSessionEntry {
                        session_id: "s-2".to_string(),
                        medium: "remote".to_string(),
                        created_at: "10 min ago".to_string(),
                        status: "scheduled".to_string(),
                    }],
                },
                ProjectEntry {
                    project_id: "archive".to_string(),
                    directory: "/workspace/archive".to_string(),
                    status: IDLE.to_string(),
                    sessions: Vec::new(),
                },
            ],
        }
    }
}

/// Group sessions per project and derive a per-project status from the tasks
/// scheduled/run against those sessions.
///
/// Sessions with no project identity land under the `default` project; a
/// session with no medium falls back to `local`.
fn group_projects(
    projects: &[Project],
    sessions: &[Session],
    tasks: &[Task],
) -> Vec<ProjectEntry> {
    // project_id -> (directory, sessions)
    let mut by_project: BTreeMap<String, (String, Vec<ProjectSessionEntry>)> = BTreeMap::new();
    for p in projects {
        by_project
            .entry(p.project_id.0.clone())
            .or_insert_with(|| (p.directory.clone(), Vec::new()));
    }

    // session_id -> its most "live" task status (running > scheduled/pending).
    let mut task_status: HashMap<String, String> = HashMap::new();
    for task in tasks {
        let slot = task_status
            .entry(task.session_id.0.clone())
            .or_insert_with(|| task.status.clone());
        if task.status == "running" || (task.status == "scheduled" && *slot != "running") {
            *slot = task.status.clone();
        }
    }

    for s in sessions {
        let project_id = s.project_id.0.as_str();
        let project = if project_id.is_empty() {
            "default"
        } else {
            project_id
        };
        let medium = if s.medium_id.0.is_empty() {
            "local"
        } else {
            s.medium_id.0.as_str()
        };
        let status = task_status
            .get(&s.session_id.0)
            .cloned()
            .unwrap_or_else(|| IDLE.to_string());
        by_project
            .entry(project.to_string())
            .or_insert_with(|| (String::new(), Vec::new()))
            .1
            .push(ProjectSessionEntry {
                session_id: s.session_id.0.clone(),
                medium: medium.to_string(),
                created_at: s.created_at.clone(),
                status,
            });
    }

    by_project
        .into_iter()
        .map(|(project_id, (directory, sessions))| {
            let status = project_status(&sessions);
            ProjectEntry {
                project_id,
                directory,
                status,
                sessions,
            }
        })
        .collect()
}

/// Collapse a project's sessions to a single "what's running" status.
fn project_status(sessions: &[ProjectSessionEntry]) -> String {
    if sessions.iter().any(|s| s.status == "running") {
        "running".to_string()
    } else if sessions
        .iter()
        .any(|s| s.status == "scheduled" || s.status == "pending")
    {
        "scheduled".to_string()
    } else {
        IDLE.to_string()
    }
}

#[cfg(test)]
mod tests {
    use goble_harness_types::{MediumId, ProjectId, SessionId};

    use super::*;

    fn session(id: &str, project: &str, medium: &str) -> Session {
        Session {
            session_id: SessionId::new(id),
            project_id: ProjectId::new(project),
            medium_id: MediumId::new(medium),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    fn task(session_id: &str, status: &str) -> Task {
        Task {
            task_id: format!("t-{session_id}"),
            session_id: SessionId::new(session_id),
            trigger: "manual".to_string(),
            status: status.to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn sessions_group_per_project_and_fall_back_to_default() {
        // A session with no project identity lands under `default`.
        let sessions = vec![
            session("s1", "alpha", "local"),
            session("s2", "", "remote"),
        ];
        let projects = vec![Project {
            project_id: ProjectId::new("alpha"),
            directory: "/workspace/alpha".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }];
        let entries = group_projects(&projects, &sessions, &[]);

        let alpha = entries.iter().find(|p| p.project_id == "alpha").unwrap();
        assert_eq!(alpha.directory, "/workspace/alpha");
        assert_eq!(alpha.sessions.len(), 1);
        assert_eq!(alpha.sessions[0].medium, "local");

        let default = entries.iter().find(|p| p.project_id == "default").unwrap();
        assert_eq!(default.directory, "");
        assert_eq!(default.sessions.len(), 1);
        assert_eq!(default.sessions[0].medium, "remote");
    }

    #[test]
    fn project_status_derived_from_tasks() {
        let sessions = vec![session("s1", "alpha", "local")];
        // A scheduled task on the project's session -> scheduled.
        let scheduled = group_projects(
            &[Project {
                project_id: ProjectId::new("alpha"),
                directory: "/w".to_string(),
                created_at: "2026-01-01T00:00:00Z".to_string(),
            }],
            &sessions,
            &[task("s1", "scheduled")],
        );
        assert_eq!(scheduled[0].status, "scheduled");

        // A running task wins over a scheduled one.
        let running = group_projects(
            &[Project {
                project_id: ProjectId::new("alpha"),
                directory: "/w".to_string(),
                created_at: "2026-01-01T00:00:00Z".to_string(),
            }],
            &sessions,
            &[task("s1", "scheduled"), task("s1", "running")],
        );
        assert_eq!(running[0].status, "running");

        // No tasks -> idle.
        let idle = group_projects(
            &[Project {
                project_id: ProjectId::new("alpha"),
                directory: "/w".to_string(),
                created_at: "2026-01-01T00:00:00Z".to_string(),
            }],
            &sessions,
            &[],
        );
        assert_eq!(idle[0].status, "idle");
    }
}
