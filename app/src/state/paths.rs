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

/// One initial space with a single chat leaf.
pub(crate) fn default_spaces() -> Vec<Space> {
    vec![Space::new("Space 1", Pane::Leaf { id: 1, kind: PaneKind::Chat })]
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
fn home_directory() -> Option<String> {
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

/// Best-effort git branch of `dir`, read from `.git/HEAD`. Handles a normal
/// `.git` directory and a `.git` *file* (worktree/submodule) whose first line
/// is `gitdir: <path>`. Returns empty when the directory is not a git repo.
pub(crate) fn current_branch(dir: &str) -> String {
    if let Ok(content) = std::fs::read_to_string(std::path::Path::new(dir).join(".git/HEAD")) {
        return parse_branch_head(&content);
    }
    // `.git` may be a file pointing at the real gitdir (worktrees, submodules).
    if let Ok(gitfile) = std::fs::read_to_string(std::path::Path::new(dir).join(".git")) {
        if let Some(gitdir) = gitfile
            .lines()
            .find_map(|l| l.trim().strip_prefix("gitdir:"))
        {
            let head = std::path::Path::new(gitdir.trim()).join("HEAD");
            if let Ok(content) = std::fs::read_to_string(&head) {
                return parse_branch_head(&content);
            }
        }
    }
    String::new()
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
