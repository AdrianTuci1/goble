//! Stable identity for a workspace that runs on a remote sandbox.
//!
//! A workspace is a project, not the application and not the machine. Its
//! identity is derived from the durable project id, so the same project keeps
//! the same sandbox name across restarts and provider sessions. The host name
//! is display metadata: it tells two machines apart without becoming part of
//! the key.
//!
//! The sandbox name is what a provider sees (`goble-<slug>-<key>`), the
//! sandbox path is where the checkout lives (`/workspace/<slug>`), and the
//! prompt name is the human label (`<slug>@<host>`). Every pane that works on
//! the workspace attaches to this one sandbox and therefore shares its
//! filesystem.

use sha2::{Digest, Sha256};

/// Prefix every sandbox name carries, so a provider dashboard can find them.
const SANDBOX_PREFIX: &str = "goble";
/// Provider names are capped at 63 characters; the key leaves room for a slug.
const MAX_SANDBOX_NAME: usize = 63;
/// How much of the SHA-256 digest the sandbox name carries, in hex characters.
const KEY_HEX_LEN: usize = 12;

/// A project's remote identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceIdentity {
    /// The durable project id used as the key.
    pub project_id: String,
    /// The project's short, filesystem-safe name (`goble`).
    pub slug: String,
    /// The machine the workspace was opened on, without `.local`.
    pub host: String,
    /// The stable digest of the project id; one workspace, one key.
    pub key: String,
    /// The name a provider creates the sandbox under.
    pub sandbox_name: String,
    /// Where the checkout lives inside the sandbox.
    pub sandbox_path: String,
    /// The label a shell prompt or a settings row shows.
    pub prompt_name: String,
}

impl WorkspaceIdentity {
    /// Build the identity for `project_id` checked out from `directory`, opened
    /// on `host`. A missing project id falls back to the directory, and a
    /// missing directory falls back to `workspace`, so every project has a
    /// usable name instead of an empty one.
    pub fn new(project_id: &str, directory: &str, host: &str) -> Self {
        let project_id = project_id.trim();
        let directory = directory.trim();

        let base_name = directory
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("workspace");
        let slug = slugify(base_name);
        let key_source = if project_id.is_empty() {
            directory
        } else {
            project_id
        };
        let key = digest(key_source);
        let host = normalize_host(host);

        let room = MAX_SANDBOX_NAME.saturating_sub(SANDBOX_PREFIX.len() + 1 + KEY_HEX_LEN + 1);
        let slug = truncate_slug(&slug, room);
        let sandbox_name = format!("{SANDBOX_PREFIX}-{slug}-{key}");

        Self {
            project_id: project_id.to_string(),
            slug: slug.clone(),
            host: host.clone(),
            key,
            sandbox_name,
            sandbox_path: format!("/workspace/{slug}"),
            prompt_name: format!("{slug}@{host}"),
        }
    }

    /// The local machine's name, as a person would say it: `Adrians-MacBook-Pro`.
    /// `HOSTNAME` is often unset on macOS, so the system `hostname` command is
    /// the fallback before a generic label.
    pub fn local_host() -> String {
        for variable in ["HOSTNAME", "COMPUTERNAME"] {
            if let Ok(value) = std::env::var(variable) {
                let normalized = normalize_host(&value);
                if !normalized.is_empty() && normalized != "local" {
                    return normalized;
                }
            }
        }
        std::process::Command::new("hostname")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|host| normalize_host(&host))
            .filter(|host| !host.is_empty())
            .unwrap_or_else(|| "local".to_string())
    }

    /// The identity of a project on this machine.
    pub fn for_local_project(project_id: &str, directory: &str) -> Self {
        Self::new(project_id, directory, &Self::local_host())
    }
}

/// Lowercase, `[a-z0-9-]`, no leading or trailing hyphen. A name that has no
/// usable characters becomes `workspace`.
fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_hyphen = false;
    for character in name.chars() {
        let character = character.to_ascii_lowercase();
        if character.is_ascii_alphanumeric() {
            slug.push(character);
            last_hyphen = false;
        } else if !last_hyphen && !slug.is_empty() {
            slug.push('-');
            last_hyphen = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "workspace".to_string()
    } else {
        slug
    }
}

/// Cut a slug to `max` characters without leaving a trailing hyphen.
fn truncate_slug(slug: &str, max: usize) -> String {
    let mut truncated: String = slug.chars().take(max).collect();
    while truncated.ends_with('-') {
        truncated.pop();
    }
    if truncated.is_empty() {
        "workspace".to_string()
    } else {
        truncated
    }
}

/// The first [`KEY_HEX_LEN`] hex characters of SHA-256. Twelve hex characters
/// are 48 bits: enough to keep names distinct without making them unreadable.
fn digest(source: &str) -> String {
    let digest = Sha256::digest(source.as_bytes());
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        .chars()
        .take(KEY_HEX_LEN)
        .collect()
}

/// A host label: lowercase, no `.local` suffix, spaces and apostrophes folded
/// into hyphens. An empty host becomes `local`.
fn normalize_host(host: &str) -> String {
    let host = host.trim().to_ascii_lowercase();
    let host = host.trim_end_matches('.');
    let host = host.strip_suffix(".local").unwrap_or(host);
    let host = host.strip_suffix(".lan").unwrap_or(host);
    if host.is_empty() {
        return "local".to_string();
    }
    let slug = slugify(host);
    slug
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_gets_one_stable_sandbox_name_per_key() {
        let first = WorkspaceIdentity::new(
            "projects/goble",
            "/Users/me/Projects/goble",
            "Adrians-MacBook-Pro.local",
        );
        let second =
            WorkspaceIdentity::new("projects/goble", "/Users/me/Projects/goble", "Other-Mac");
        assert_eq!(first.key, second.key);
        assert_eq!(first.sandbox_name, second.sandbox_name);
        assert_eq!(first.sandbox_name, "goble-goble-2fbfef8fa780");
        assert_eq!(first.sandbox_path, "/workspace/goble");
        assert_eq!(first.prompt_name, "goble@adrians-macbook-pro");
        assert_eq!(second.prompt_name, "goble@other-mac");
    }

    #[test]
    fn the_project_id_is_the_key_and_the_directory_is_the_label() {
        let identity = WorkspaceIdentity::new("project-42", "/srv/work/My Project", "studio");
        assert_eq!(identity.project_id, "project-42");
        assert_eq!(identity.slug, "my-project");
        assert_eq!(identity.sandbox_path, "/workspace/my-project");
        assert_eq!(identity.prompt_name, "my-project@studio");
    }

    #[test]
    fn a_missing_project_id_falls_back_to_the_directory() {
        let identity = WorkspaceIdentity::new("", "/srv/work/goble/", "studio");
        assert_eq!(identity.project_id, "");
        assert_eq!(identity.slug, "goble");
        assert_eq!(identity.prompt_name, "goble@studio");
    }

    #[test]
    fn empty_names_still_produce_a_usable_workspace() {
        let identity = WorkspaceIdentity::new("", "", "");
        assert_eq!(identity.slug, "workspace");
        assert_eq!(identity.host, "local");
        assert_eq!(identity.sandbox_name, "goble-workspace-e3b0c44298fc");
        assert_eq!(identity.sandbox_path, "/workspace/workspace");
        assert_eq!(identity.prompt_name, "workspace@local");
    }

    #[test]
    fn awkward_names_are_slugged_and_capped() {
        let short = WorkspaceIdentity::new("p", "My Goble!!", "Adrian’s MacBook Pro");
        assert_eq!(short.prompt_name, "my-goble@adrian-s-macbook-pro");

        let long = WorkspaceIdentity::new(
            "p",
            "A very long project name with punctuation !!! and spaces",
            "studio",
        );
        assert!(long.sandbox_name.len() <= MAX_SANDBOX_NAME);
        assert!(!long.sandbox_name.ends_with('-'));
        assert!(long
            .sandbox_name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
        assert!(long.slug.len() <= MAX_SANDBOX_NAME);
    }

    #[test]
    fn host_names_lose_their_local_suffix() {
        assert_eq!(
            normalize_host("Adrians-MacBook-Pro.local"),
            "adrians-macbook-pro"
        );
        assert_eq!(
            normalize_host("Adrians-MacBook-Pro.LAN."),
            "adrians-macbook-pro"
        );
        assert_eq!(
            normalize_host("Adrian’s MacBook Pro"),
            "adrian-s-macbook-pro"
        );
        assert_eq!(normalize_host(""), "local");
    }

    #[test]
    fn slugs_never_start_or_end_with_a_hyphen() {
        assert_eq!(slugify("--Goble!!"), "goble");
        assert_eq!(slugify("a  b"), "a-b");
        assert_eq!(truncate_slug("abcdef-", 6), "abcdef");
    }
}
