use super::*;


/// The subset of [`UiState`] pane layout persisted across app restarts.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct PersistedPaneState {
    pub(crate) spaces: Vec<Space>,
    pub(crate) active_space: usize,
    pub(crate) active_pane_id: u64,
    /// Per-pane conversation/draft/cwd. `#[serde(default)]` so pane layouts
    /// saved before per-pane sessions existed still parse.
    #[serde(default)]
    pub(crate) pane_sessions: HashMap<u64, PaneSession>,
}

/// Collect every pane id (leaves and splits) in a pane tree.
pub(crate) fn collect_pane_ids(pane: &Pane, out: &mut Vec<u64>) {
    match pane {
        Pane::Leaf { id, .. } => out.push(*id),
        Pane::Split {
            id, first, second, ..
        } => {
            out.push(*id);
            collect_pane_ids(first, out);
            collect_pane_ids(second, out);
        }
    }
}

/// Collect the ids of every leaf pane (chat or terminal) in a pane tree, so
/// each leaf gets the per-session entry it needs.
pub(crate) fn collect_leaf_pane_ids(pane: &Pane, out: &mut Vec<u64>) {
    match pane {
        Pane::Leaf { id, .. } => out.push(*id),
        Pane::Split { first, second, .. } => {
            collect_leaf_pane_ids(first, out);
            collect_leaf_pane_ids(second, out);
        }
    }
}

/// Ensure every leaf pane has a [`PaneSession`] entry so it is an independent
/// session — a chat pane needs a transcript/cwd, and a terminal pane needs a
/// cwd to spawn its shell. A pane that is not yet bound keeps an empty
/// conversation id and lazily follows the selected conversation (this is how
/// the initial single pane tracks the sidebar); panes created by splitting /
/// adding a space are bound to a distinct conversation in
/// [`UiState::bind_pane_new_conversation`].
pub(crate) fn ensure_all_chat_pane_sessions(state: &mut UiState, _desktop: Option<&DesktopState>) {
    let mut leaf_ids = Vec::new();
    for space in &state.spaces {
        collect_leaf_pane_ids(&space.root, &mut leaf_ids);
    }
    let default_path = current_dir_display();
    for id in leaf_ids {
        state.pane_sessions.entry(id).or_insert_with(|| PaneSession {
            conversation_id: String::new(),
            draft: String::new(),
            path: default_path.clone(),
        });
    }
}

/// Resolve a pane's initial working directory from a project's directory when
/// known, else fall back to the process cwd (a sane default).
pub fn default_pane_path(project_id: &str, desktop: Option<&DesktopState>) -> String {
    if let Some(desktop) = desktop {
        for p in desktop.list_projects() {
            if p.project_id.0 == project_id {
                return p.directory;
            }
        }
    }
    current_dir_display()
}

/// One initial space with a single chat leaf. Nobody has named it, so its tab
/// label is derived from what it holds (an agent with no conversation subject
/// yet reads [`crate::state::NEW_AGENT_TAB_LABEL`]).
pub(crate) fn default_spaces() -> Vec<Space> {
    vec![Space::unnamed(Pane::Leaf { id: 1, kind: PaneKind::Chat })]
}

/// The current working directory, used as the composer's path label.
pub(crate) fn current_dir_display() -> String {
    std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "/".to_string())
}

/// A path as the UI labels it: the home directory collapses to `~`, the way
/// warp-new shows a working directory, so the pill and the shell prompt stay
/// readable instead of repeating the user's whole path.
///
/// Display only: a `~` string is not a usable working directory, so the paths
/// that are actually run, spawned or read keep their real form and this is
/// applied where the string is drawn.
pub fn display_path(path: &str) -> String {
    let Some(home) = home_directory() else {
        return path.to_string();
    };
    match path.strip_prefix(home.as_str()) {
        Some("") => "~".to_string(),
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

/// The user's home directory, from the environment (the process may run without
/// a passwd lookup; `HOME` on Unix, `USERPROFILE` on Windows).
pub(crate) fn home_directory() -> Option<String> {
    for key in ["HOME", "USERPROFILE"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim_end_matches('/');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Best-effort git branch of `dir`, read from the `.git/HEAD` of the repository
/// `dir` sits in. The search walks up from `dir`, the way git itself resolves the
/// repository for a subdirectory, so a shell in a repository's subdirectory still
/// names the branch. Returns empty when no directory above is a repository.
pub(crate) fn current_branch(dir: &str) -> String {
    let mut candidate = Some(std::path::Path::new(dir));
    while let Some(path) = candidate {
        if let Some(branch) = branch_at(path) {
            return branch;
        }
        candidate = path.parent();
    }
    String::new()
}

/// The branch the repository rooted at `path` reports, or `None` when `path` is
/// not a repository root. Handles a normal `.git` directory and a `.git` *file*
/// (worktree/submodule) whose first line is `gitdir: <path>`.
fn branch_at(path: &std::path::Path) -> Option<String> {
    if let Ok(content) = std::fs::read_to_string(path.join(".git/HEAD")) {
        return Some(parse_branch_head(&content));
    }
    let gitfile = std::fs::read_to_string(path.join(".git")).ok()?;
    let gitdir = gitfile
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))?;
    let head = std::path::Path::new(gitdir.trim()).join("HEAD");
    std::fs::read_to_string(head)
        .ok()
        .map(|content| parse_branch_head(&content))
}

pub(crate) fn parse_branch_head(content: &str) -> String {
    let line = content.lines().next().unwrap_or("").trim();
    if let Some(name) = line.strip_prefix("ref: refs/heads/") {
        name.to_string()
    } else if line.len() >= 40 {
        "detached".to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory inside a repository reports the repository's branch: git
    /// resolves the repository from a subdirectory, and so does the pill, so a
    /// shell that `cd`s into a crate or a folder still names the branch.
    #[test]
    fn a_directory_inside_a_repository_reports_its_branch() {
        let repo = tempfile::tempdir().expect("temp repo");
        std::fs::create_dir(repo.path().join(".git")).unwrap();
        std::fs::write(repo.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let nested = repo.path().join("crates").join("ui");
        std::fs::create_dir_all(&nested).unwrap();

        assert_eq!(current_branch(&repo.path().to_string_lossy()), "main");
        assert_eq!(current_branch(&nested.to_string_lossy()), "main");
        assert_eq!(
            current_branch("/definitely/not/a/repository/at/all"),
            "",
            "the walk stops at the filesystem root"
        );
    }
}
