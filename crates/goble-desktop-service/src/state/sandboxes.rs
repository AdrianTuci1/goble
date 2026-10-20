//! The sandbox a remote project runs in.
//!
//! One binding per project: every pane that opens the project attaches to the
//! same sandbox and shares its filesystem. The stable name comes from
//! [`WorkspaceIdentity`], so a provider can find the same sandbox again after a
//! restart; the host name is only part of the display label.

use goble_core::workspace_identity::WorkspaceIdentity;
use goble_persistence::{Project, ProjectSandbox};

/// Build the durable binding for `project` on `provider`, opened from `host`.
///
/// The binding starts without a provider handle: the caller creates the sandbox
/// and fills `sandbox_id` once the provider has accepted the name.
pub fn binding_for_project(project: &Project, provider: &str, host: &str) -> ProjectSandbox {
    let identity = WorkspaceIdentity::new(&project.project_id.0, &project.directory, host);
    ProjectSandbox {
        project_id: project.project_id.clone(),
        provider: provider.to_string(),
        sandbox_name: identity.sandbox_name,
        sandbox_id: None,
        workspace_path: identity.sandbox_path,
        updated_at: chrono::Utc::now().to_rfc3339(),
    }
}

/// The binding for the same project on this machine.
pub fn binding_for_local_project(project: &Project, provider: &str) -> ProjectSandbox {
    binding_for_project(project, provider, &WorkspaceIdentity::local_host())
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_harness_types::ProjectId;

    fn project() -> Project {
        Project {
            project_id: ProjectId::new("projects/goble"),
            directory: "/Users/adrian.tucicovencogmail.com/Projects/goble".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn a_project_binds_one_stable_named_sandbox() {
        let first = binding_for_project(&project(), "modal", "Adrians-MacBook-Pro");
        let second = binding_for_project(&project(), "modal", "Other-Mac");

        assert_eq!(first.sandbox_name, second.sandbox_name);
        assert_eq!(first.sandbox_name, "goble-goble-2fbfef8fa780");
        assert_eq!(first.workspace_path, "/workspace/goble");
        assert_eq!(first.sandbox_id, None);
        assert!(!first.updated_at.is_empty());
    }

    #[test]
    fn the_provider_is_part_of_the_binding() {
        let binding = binding_for_project(&project(), "e2b", "studio");
        assert_eq!(binding.provider, "e2b");
        assert_eq!(binding.project_id, ProjectId::new("projects/goble"));
    }
}
