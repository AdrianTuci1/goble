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
    /// Per-pane SSH bindings (the saved host and the resolved session).
    /// `#[serde(default)]` so a layout written before saved hosts still parses.
    #[serde(default)]
    pub(crate) pane_ssh: HashMap<u64, SshPaneBinding>,
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

/// Collect the ids of the chat leaves in a pane tree. Only a chat pane has a
/// conversation of its own at creation; a terminal or file pane gains one when
/// its harness starts a turn.
pub(crate) fn collect_chat_leaf_pane_ids(pane: &Pane, out: &mut Vec<u64>) {
    match pane {
        Pane::Leaf {
            id,
            kind: PaneKind::Chat,
        } => out.push(*id),
        Pane::Leaf { .. } => {}
        Pane::Split { first, second, .. } => {
            collect_chat_leaf_pane_ids(first, out);
            collect_chat_leaf_pane_ids(second, out);
        }
    }
}

/// Ensure every leaf pane has a [`PaneSession`] entry so it is an independent
/// session — a chat pane needs a transcript/cwd, and a terminal pane needs a
/// cwd to spawn its shell. Every chat leaf is also bound to a conversation of
/// its own, the initial pane included: a pane that starts a turn must be
/// resolvable by that conversation when its `chat:*` events arrive, whatever
/// pane has focus by then. A restored pane keeps the conversation it persisted;
/// only an empty binding is given a new one.
pub(crate) fn ensure_all_chat_pane_sessions(state: &mut UiState, desktop: Option<&DesktopState>) {
    let mut leaf_ids = Vec::new();
    let mut chat_ids = Vec::new();
    for space in &state.spaces {
        collect_leaf_pane_ids(&space.root, &mut leaf_ids);
        collect_chat_leaf_pane_ids(&space.root, &mut chat_ids);
    }
    let default_path = current_dir_display();
    for id in leaf_ids {
        state.pane_sessions.entry(id).or_insert_with(|| PaneSession {
            conversation_id: String::new(),
            draft: String::new(),
            path: default_path.clone(),
            daemon_session_id: String::new(),
        });
    }
    let mut created = false;
    let mut adopted_selection = false;
    for id in chat_ids {
        if state.pane_owns_conversation(id) {
            continue;
        }
        // The initial pane keeps tracking the sidebar: the conversation on
        // screen becomes its own binding. A second unbound chat pane gets a
        // distinct conversation instead of sharing that one.
        if !adopted_selection && id == state.active_pane_id {
            if let Some(selected) = state.selected_id.clone().filter(|id| !id.is_empty()) {
                state.record_pane_conversation(id, &selected);
                adopted_selection = true;
                continue;
            }
        }
        state.bind_pane_new_conversation(id, desktop);
        created = true;
    }
    // A conversation created here is a real one in the store, so the sidebar
    // lists it now rather than only after the first turn's update event.
    if created {
        if let Some(desktop) = desktop {
            state.refresh_conversations(desktop);
        }
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
/// warp-new shows a working directory, so the pill, the tab and the tooltip
/// stay readable instead of repeating the user's whole path.
///
/// Display only: a `~` string is not a usable working directory, so the paths
/// that are actually run, spawned or read keep their real form and this is
/// applied where the string is drawn.
pub fn display_path(path: &str) -> String {
    collapse_home(path, home_directory().as_deref())
}

/// The real path behind a label [`display_path`] produced: a leading `~`
/// becomes the home directory again, and every other path is returned as it
/// came. Used where a saved conversation's directory has to be spawned or read.
pub fn expand_path(path: &str) -> String {
    let Some(home) = home_directory() else {
        return path.to_string();
    };
    if path == "~" {
        return home;
    }
    match path.strip_prefix("~/") {
        Some(rest) => format!("{}/{}", home.trim_end_matches('/'), rest),
        None => path.to_string(),
    }
}

/// [`display_path`] with the home directory passed in, so the collapse is
/// testable without changing the process environment.
///
/// A trailing slash names the same directory, so it is dropped before the
/// comparison (`/Users/me/` reads as `~` the way `/Users/me` does); the
/// filesystem root keeps its one slash.
fn collapse_home(path: &str, home: Option<&str>) -> String {
    let trimmed = path.trim_end_matches('/');
    let path = if trimmed.is_empty() { path } else { trimmed };
    let Some(home) = home
        .map(|home| home.trim_end_matches('/'))
        .filter(|home| !home.is_empty())
    else {
        return path.to_string();
    };
    match path.strip_prefix(home) {
        Some("") => "~".to_string(),
        // Only a component boundary collapses: `/Users/melanie` is not under
        // `/Users/me`, so it keeps its absolute form.
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

    /// The home directory itself is the one a label spells `~`.
    #[test]
    fn the_home_directory_collapses_to_a_tilde() {
        assert_eq!(collapse_home("/Users/me", Some("/Users/me")), "~");
        assert_eq!(
            collapse_home("/Users/me", Some("/Users/me/")),
            "~",
            "a home directory that ends in a slash is the same directory"
        );
    }

    /// A directory under home keeps its whole tail, read from the `~`.
    #[test]
    fn a_directory_under_home_reads_from_the_tilde() {
        assert_eq!(
            collapse_home("/Users/me/Projects/goble", Some("/Users/me")),
            "~/Projects/goble"
        );
    }

    /// A path that shares home's prefix without a boundary is not under it:
    /// `/Users/melanie` must not come back as `~lanie`.
    #[test]
    fn a_sibling_that_shares_the_home_prefix_stays_absolute() {
        assert_eq!(
            collapse_home("/Users/melanie/Projects", Some("/Users/me")),
            "/Users/melanie/Projects"
        );
    }

    /// Every path outside home is left alone, home known or not.
    #[test]
    fn a_path_outside_home_is_unchanged() {
        assert_eq!(collapse_home("/opt/goble", Some("/Users/me")), "/opt/goble");
        assert_eq!(collapse_home("/Users/me", None), "/Users/me");
        assert_eq!(
            collapse_home("/Users/me/Projects", Some("")),
            "/Users/me/Projects"
        );
    }

    /// A trailing slash names the same directory, so it collapses too; the
    /// filesystem root keeps its own.
    #[test]
    fn a_trailing_slash_collapses() {
        assert_eq!(collapse_home("/Users/me/", Some("/Users/me")), "~");
        assert_eq!(
            collapse_home("/Users/me/Projects/goble/", Some("/Users/me")),
            "~/Projects/goble"
        );
        assert_eq!(collapse_home("/", Some("/Users/me")), "/");
    }

    /// An empty path is empty, not `~` and not a panic.
    #[test]
    fn an_empty_path_stays_empty() {
        assert_eq!(collapse_home("", Some("/Users/me")), "");
        assert_eq!(display_path(""), "");
    }

    /// The real environment's home directory collapses the same way its
    /// children do, so a label built from the process's own home reads `~`.
    #[test]
    fn display_path_collapses_the_real_home_directory() {
        let Some(home) = home_directory() else {
            return;
        };
        assert_eq!(display_path(&home), "~");
        assert_eq!(
            display_path(&format!("{home}/Projects/goble")),
            "~/Projects/goble"
        );
        assert_eq!(
            display_path("/definitely/not/under/home"),
            "/definitely/not/under/home"
        );
    }
}
