//! The rich input's working-directory and git-branch menus.
//!
//! Both are the warp-new model: the menu is a window onto the real machine, and
//! a selection is a real shell command run in the pane's own pty — the
//! directory pill browses the filesystem (`cd`), the branch pill lists the
//! repository's local branches (`git checkout`). Nothing here mutates app
//! state: it reads the disk and the repository, and hands the app the labels to
//! draw and the ids to act on.

use std::path::{Path, PathBuf};
use std::process::Command;

/// One row of the working-directory menu: the label drawn, the absolute
/// directory it stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryRow {
    pub label: String,
    pub path: String,
}

/// One row of the branch menu: the branch name and whether it is checked out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchRow {
    pub name: String,
    pub current: bool,
}

/// The label the parent row carries, spelled out the way warp-new spells it.
pub const PARENT_LABEL: &str = ".. (Parent Directory)";

/// The directories directly under `cwd`: hidden entries skipped, alphabetical,
/// with the parent directory as the last row. Unreadable directories yield the
/// parent row alone rather than an error — a menu that cannot list is still a
/// menu that can go up.
pub fn directory_rows(cwd: &str) -> Vec<DirectoryRow> {
    let mut rows: Vec<DirectoryRow> = Vec::new();
    let dir = Path::new(cwd);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let is_dir = entry
                .file_type()
                .map(|kind| kind.is_dir())
                .unwrap_or(false);
            if !is_dir {
                continue;
            }
            let path = entry.path();
            rows.push(DirectoryRow {
                label: name,
                path: path.to_string_lossy().to_string(),
            });
        }
    }
    rows.sort_by(|a, b| {
        a.label
            .to_lowercase()
            .cmp(&b.label.to_lowercase())
            .then_with(|| a.label.cmp(&b.label))
    });
    if let Some(parent) = dir.parent() {
        rows.push(DirectoryRow {
            label: PARENT_LABEL.to_string(),
            path: parent.to_string_lossy().to_string(),
        });
    }
    rows
}

/// The local branches of the repository `cwd` sits in, most recently committed
/// first (warp-new's `git branch --sort=-committerdate`), with the checked-out
/// branch hoisted to the top so the menu opens on the state the pane is in.
/// Empty when `cwd` is not in a repository or git is unavailable.
pub fn branch_rows(cwd: &str) -> Vec<BranchRow> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(cwd)
        .arg("branch")
        .arg("--no-color")
        .arg("--sort=-committerdate")
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let (current, name) = match line.strip_prefix("* ") {
            Some(name) => (true, name),
            None => (false, line.trim_start_matches("  ")),
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        rows.push(BranchRow {
            name: name.to_string(),
            current,
        });
    }
    rows.sort_by_key(|row| !row.current);
    rows
}

/// The directory `path` lives in, for a shell command that has to run there.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// `cd` into `path`, as the shell would be told to do it: the directory pill
/// runs this in the pane's pty rather than setting a variable, so the shell's
/// own idea of where it is moves with it.
pub fn change_directory_command(path: &str) -> String {
    format!("cd {}", shell_quote(path))
}

/// Check `branch` out, the way warp-new's branch pill does.
pub fn checkout_branch_command(branch: &str) -> String {
    format!("git checkout {}", shell_quote(branch))
}

/// The rows both menus were last built from, kept per rebuild so an open menu
/// is not re-read from the disk on every frame while a closed one costs
/// nothing. Keyed by the directory they describe: a pane that moved lists its
/// new home the moment the menu opens again.
#[derive(Default)]
pub struct PickerCache {
    directories: Option<(String, Vec<DirectoryRow>)>,
    branches: Option<(String, Vec<BranchRow>)>,
}

impl PickerCache {
    /// The working-directory rows for `cwd`. Only an open menu pays for the
    /// listing, and the listing is dropped when the menu closes so it is fresh
    /// every time it is opened.
    pub fn directories(&mut self, cwd: &str, open: bool) -> Vec<DirectoryRow> {
        if !open {
            self.directories = None;
            return Vec::new();
        }
        if let Some((cached, rows)) = &self.directories {
            if cached == cwd {
                return rows.clone();
            }
        }
        let rows = directory_rows(cwd);
        self.directories = Some((cwd.to_string(), rows.clone()));
        rows
    }

    /// The branch rows for the repository `cwd` sits in, under the same rules.
    pub fn branches(&mut self, cwd: &str, open: bool) -> Vec<BranchRow> {
        if !open {
            self.branches = None;
            return Vec::new();
        }
        if let Some((cached, rows)) = &self.branches {
            if cached == cwd {
                return rows.clone();
            }
        }
        let rows = branch_rows(cwd);
        self.branches = Some((cwd.to_string(), rows.clone()));
        rows
    }
}

/// The absolute path a directory row points at, resolved against `cwd`.
pub fn resolve(cwd: &str, target: &str) -> String {
    if target.is_empty() {
        return cwd.to_string();
    }
    let candidate = if Path::new(target).is_absolute() {
        PathBuf::from(target)
    } else {
        Path::new(cwd).join(target)
    };
    candidate.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_menu_lists_its_subdirectories_and_the_parent_last() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("zeta")).unwrap();
        std::fs::create_dir(dir.path().join("Alpha")).unwrap();
        std::fs::create_dir(dir.path().join(".hidden")).unwrap();
        std::fs::write(dir.path().join("file.txt"), "not a directory").unwrap();

        let rows = directory_rows(&dir.path().to_string_lossy());
        let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["Alpha", "zeta", PARENT_LABEL],
            "directories only, case-insensitively sorted, parent last"
        );
        assert!(rows[0].path.ends_with("/Alpha"), "rows carry absolute paths");
    }

    #[test]
    fn a_directory_that_cannot_be_listed_still_offers_the_way_up() {
        let rows = directory_rows("/definitely/not/a/real/directory");
        assert_eq!(rows.len(), 1, "one row: {rows:?}");
        assert_eq!(rows[0].label, PARENT_LABEL);
    }

    #[test]
    fn the_parent_row_points_at_the_directory_above() {
        let rows = directory_rows("/Users/example/project");
        let parent = rows.last().expect("a parent row");
        assert_eq!(parent.path, "/Users/example");
        assert_eq!(resolve("/Users/example/project", &parent.path), "/Users/example");
    }

    #[test]
    fn the_selection_commands_are_shell_quoted() {
        assert_eq!(
            change_directory_command("/Users/me/My Project"),
            "cd '/Users/me/My Project'"
        );
        assert_eq!(checkout_branch_command("feature/x"), "git checkout 'feature/x'");
        assert_eq!(
            checkout_branch_command("it's"),
            r"git checkout 'it'\''s'",
            "a quote in a name cannot end the command"
        );
    }

    #[test]
    fn a_closed_menu_lists_nothing_and_is_not_cached() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("child")).unwrap();
        let cwd = dir.path().to_string_lossy().to_string();

        let mut cache = PickerCache::default();
        assert!(cache.directories(&cwd, false).is_empty(), "closed: no rows");

        let open = cache.directories(&cwd, true);
        assert!(open.iter().any(|row| row.label == "child"));

        // A directory created after the listing shows up the next time the menu
        // opens, because a closed menu drops the cache.
        std::fs::create_dir(dir.path().join("later")).unwrap();
        assert!(cache.directories(&cwd, false).is_empty());
        let reopened = cache.directories(&cwd, true);
        assert!(
            reopened.iter().any(|row| row.label == "later"),
            "the reopened menu is re-read: {reopened:?}"
        );
    }

    #[test]
    fn a_directory_outside_a_repository_has_no_branches() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(branch_rows(&dir.path().to_string_lossy()).is_empty());
    }
}
