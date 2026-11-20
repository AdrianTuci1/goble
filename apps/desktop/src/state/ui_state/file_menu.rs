//! The file row/card context menu's one open target.
//!
//! A file row or a changed-file card reports a right click as a path and the
//! point it landed on. The target lives in app state because the element tree
//! is rebuilt every frame; the UI hangs the shared [`goble_ui::elements::PopupMenu`]
//! from that point and calls back into the action layer when a row is taken.

use std::path::Path;

use super::*;

/// The one file context menu that is open, and the point it hangs from.
#[derive(Clone, Debug, PartialEq)]
pub struct FileMenu {
    /// The absolute path the menu acts on: opening it, floating it, and
    /// handing it to the platform's editor.
    pub path: String,
    /// The path "Copy path" writes: the absolute path for an explorer row, and
    /// the repository-relative path a changed-file card already shows.
    pub copy_path: String,
    /// Where the right click landed, in window coordinates.
    pub at: goble_ui::Vector2F,
    /// The real git backend to revert through, or `None` outside a work tree.
    /// Revert is offered only when this is `Some`.
    pub revert: Option<FileRevertTarget>,
}

/// The working tree a file context menu can revert in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRevertTarget {
    /// The repository root, which is where the git job runs.
    pub repo: String,
    /// The file's path relative to `repo`, as git wants it.
    pub git_path: String,
}

impl UiState {
    /// Open the context menu for `path` at the point the right click landed
    /// on. `copy_path` is what the Copy path row writes. Revert is resolved
    /// against the real git backend here, once per opening, rather than per
    /// frame.
    pub fn open_file_menu(
        &mut self,
        path: String,
        copy_path: String,
        at: goble_ui::Vector2F,
    ) {
        let revert = file_revert_target(&path);
        *self.file_menu.borrow_mut() = Some(FileMenu {
            path,
            copy_path,
            at,
            revert,
        });
    }

    /// Put the file context menu away (a row was taken, a press landed outside
    /// it, or Escape closed it).
    pub fn dismiss_file_menu(&mut self) {
        *self.file_menu.borrow_mut() = None;
    }
}

/// The repository root and relative path for `path`, if the real git backend
/// can resolve it. Outside a work tree the menu carries no Revert row.
fn file_revert_target(path: &str) -> Option<FileRevertTarget> {
    let path = Path::new(path);
    let directory = if path.is_dir() {
        path
    } else {
        path.parent()?
    };
    let root = crate::ui::pickers::repo_root(directory.to_str()?)?;
    let relative = path.strip_prefix(&root).ok()?.to_string_lossy().replace('\\', "/");
    Some(FileRevertTarget {
        repo: root.to_string_lossy().to_string(),
        git_path: relative,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_outside_a_work_tree_has_no_revert_backend() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("plain.txt");
        std::fs::write(&file, "text").expect("file");
        let target = file_revert_target(&file.to_string_lossy());
        assert!(
            target.is_none(),
            "no repository means no Revert row, rather than a fake one"
        );
    }

    #[test]
    fn a_file_inside_a_work_tree_resolves_its_root_and_git_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("repo");
        std::fs::create_dir_all(root.join("src")).expect("src");
        let initialized = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .expect("git is available");
        assert!(initialized.success(), "git init succeeds");
        let file = root.join("src").join("main.rs");
        std::fs::write(&file, "fn main() {}").expect("file");

        let target = file_revert_target(&file.to_string_lossy()).expect("a real backend");
        assert_eq!(target.repo, root.to_string_lossy());
        assert_eq!(target.git_path, "src/main.rs");
    }
}
