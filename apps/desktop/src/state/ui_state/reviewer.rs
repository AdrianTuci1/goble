//! The review pane's view choices, and opening a file in the floating layout.
//!
//! The review pane is rebuilt every frame, so its own choices — unified or
//! split, wrapping, which directories of the changed-file tree are folded, and
//! whether the header's file selector is open — live here rather than on the
//! element tree. One [`DiffReviewState`] per pane means two reviews open at once
//! never share a choice, the way DSH keeps the view and wrap choices per review
//! tab.

use super::*;

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

/// How a review draws its comparison.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiffViewMode {
    /// One column with both line-number gutters.
    #[default]
    Unified,
    /// Two columns, the old side beside the new one.
    Split,
}

impl DiffViewMode {
    pub fn toggled(self) -> Self {
        match self {
            Self::Unified => Self::Split,
            Self::Split => Self::Unified,
        }
    }

    pub fn is_split(self) -> bool {
        matches!(self, Self::Split)
    }
}

/// One review pane's view choices.
///
/// The selector's open flag and keyboard highlight are shared handles because
/// [`goble_ui::elements::PopupMenu`] owns them for the frame; keeping the same
/// `Rc`s across rebuilds is what lets the menu stay open and its arrow keys
/// move a highlight between frames.
#[derive(Clone, Debug)]
pub struct DiffReviewState {
    pub view: DiffViewMode,
    pub wrap: bool,
    /// Whether the header's changed-file selector is open.
    pub selector_open: Rc<RefCell<bool>>,
    /// The selector row under the keyboard, shared with the popup.
    pub selector_highlight: Rc<RefCell<Option<usize>>>,
    /// The changed-file tree's folded directories, by display path. Empty means
    /// every directory is open, which is how the review starts.
    pub folded_dirs: BTreeSet<String>,
}

impl Default for DiffReviewState {
    fn default() -> Self {
        Self {
            view: DiffViewMode::Unified,
            wrap: false,
            selector_open: Rc::new(RefCell::new(false)),
            selector_highlight: Rc::new(RefCell::new(None)),
            folded_dirs: BTreeSet::new(),
        }
    }
}

impl DiffReviewState {
    /// The DSH review tab's default: side by side, not wrapped, with every
    /// changed file's directory open.
    pub fn split_default() -> Self {
        Self {
            view: DiffViewMode::Split,
            ..Self::default()
        }
    }

    /// Fold or unfold one directory of the tree; returns whether it is now
    /// folded.
    pub fn toggle_dir(&mut self, path: &str) -> bool {
        if self.folded_dirs.remove(path) {
            false
        } else {
            self.folded_dirs.insert(path.to_string());
            true
        }
    }

    pub fn dir_folded(&self, path: &str) -> bool {
        self.folded_dirs.contains(path)
    }
}

impl UiState {
    /// The review choices for `pane_id`, created on first use.
    pub fn diff_review(&self, pane_id: u64) -> DiffReviewState {
        self.diff_reviews
            .borrow()
            .get(&pane_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Show one file's comparison in a review pane, or every file when `file`
    /// is `None`. The selected path is the one both the tree and the header
    /// selector read, so a click, a key and a menu row all move the same cell;
    /// a selection also takes the caret out of an in-place editor and returns
    /// the column to the top of the newly selected comparison.
    pub fn select_diff_file(&mut self, pane_id: u64, file: Option<String>) {
        let controls = self.pane_controls_mut(pane_id);
        *controls.diff_selected.borrow_mut() = file;
        *controls.diff_edit_focus.borrow_mut() = None;
        controls.diff_scroll.borrow_mut().reset();
    }

    /// Flip one review pane's comparison between unified and split.
    pub fn toggle_diff_view(&mut self, pane_id: u64) {
        let mut reviews = self.diff_reviews.borrow_mut();
        let state = reviews.entry(pane_id).or_default();
        state.view = state.view.toggled();
    }

    /// Turn line wrapping on or off for one review pane.
    pub fn toggle_diff_wrap(&mut self, pane_id: u64) {
        let mut reviews = self.diff_reviews.borrow_mut();
        let state = reviews.entry(pane_id).or_default();
        state.wrap = !state.wrap;
    }

    /// Fold or unfold one directory of a review's changed-file tree.
    pub fn toggle_diff_dir(&mut self, pane_id: u64, path: &str) {
        let mut reviews = self.diff_reviews.borrow_mut();
        reviews.entry(pane_id).or_default().toggle_dir(path);
    }

    /// The active space's layout toggled between the docked BSP tree and the
    /// free-floating layout. This is the "docked" side of opening a file in a
    /// floating panel: a floating space can be put back into its docked panes
    /// without losing them.
    pub fn toggle_floating_layout(&mut self, desktop: Option<&DesktopState>) -> LayoutKind {
        let index = self.active_space;
        let current = self
            .spaces
            .get(index)
            .map(|space| space.layout)
            .unwrap_or(LayoutKind::Bsp);
        let next = if current == LayoutKind::Floating {
            LayoutKind::Bsp
        } else {
            LayoutKind::Floating
        };
        self.set_space_layout(index, next, desktop);
        next
    }

    /// Open `file_path` in a file pane and arrange the active space's panes as
    /// floating windows.
    ///
    /// This is DSH's "Open in floating panel": the file pane is opened through
    /// the same path an explorer click uses — beside the active pane, or as a
    /// chip when a file pane already has the focus — and the active space then
    /// switches to [`LayoutKind::Floating`], where every leaf (the file pane
    /// included) gets a window of its own. Returns whether a pane was opened or
    /// focused.
    pub fn open_file_in_floating(
        &mut self,
        file_path: String,
        desktop: Option<&DesktopState>,
    ) -> bool {
        if self.active_pane_shows_file() {
            let active = self.active_pane_id;
            if !self.show_file_in_pane(active, file_path) {
                return false;
            }
        } else {
            let kind = PaneKind::File {
                path: file_path.clone(),
            };
            let index = self.active_space;
            let Some(space) = self.spaces.get_mut(index) else {
                return false;
            };
            let Some(new_id) = space.split_with_kind(
                self.active_pane_id,
                crate::ui::SplitDir::Horizontal,
                &mut self.next_pane_id,
                kind,
            ) else {
                return false;
            };
            self.pane_open_files.insert(new_id, vec![file_path]);
            self.focus_pane(new_id);
            self.pane_hover
                .entry(new_id)
                .or_insert_with(|| Rc::new(RefCell::new(false)));
        }
        self.sync_active_view();
        let index = self.active_space;
        self.set_space_layout(index, LayoutKind::Floating, desktop);
        true
    }
}

/// Whether a pane's review state exists yet. Used by the UI to read a default
/// without creating an entry during a frame.
pub fn review_for(
    reviews: &RefCell<HashMap<u64, DiffReviewState>>,
    pane_id: u64,
) -> DiffReviewState {
    reviews
        .borrow()
        .get(&pane_id)
        .cloned()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::pickers::DiffFileDetail;
    use crate::ui::reviewer::{status_of, tree_rows, ReviewFileStatus};

    /// A selected file is one cell: the tree, the header selector and the key
    /// handler all write it, and every comparison reads it. Selecting "all
    /// changes" clears it again.
    #[test]
    fn selecting_a_file_writes_the_one_cell_the_reviewer_reads() {
        let mut state = UiState::mock();
        state.select_diff_file(7, Some("src/main.rs".to_string()));
        assert_eq!(
            *state.pane_controls(7).diff_selected.borrow(),
            Some("src/main.rs".to_string())
        );
        state.select_diff_file(7, None);
        assert_eq!(*state.pane_controls(7).diff_selected.borrow(), None);
    }

    /// The review starts unified and unwrapped; both toggles are per pane, so
    /// choosing split in one review never changes another.
    #[test]
    fn split_and_wrap_toggle_per_review_pane() {
        let mut state = UiState::mock();
        assert_eq!(state.diff_review(7).view, DiffViewMode::Unified);
        assert!(!state.diff_review(7).wrap);
        state.toggle_diff_view(7);
        state.toggle_diff_wrap(7);
        assert_eq!(state.diff_review(7).view, DiffViewMode::Split);
        assert!(state.diff_review(7).wrap);
        assert_eq!(
            state.diff_review(8).view,
            DiffViewMode::Unified,
            "another review keeps its own choice"
        );
        state.toggle_diff_view(7);
        state.toggle_diff_wrap(7);
        assert_eq!(state.diff_review(7).view, DiffViewMode::Unified);
        assert!(!state.diff_review(7).wrap);
    }

    /// "Open in floating panel" opens the file as a pane and then arranges the
    /// active space as floating windows; the docked toggle puts the same panes
    /// back into the BSP tree without losing them.
    #[test]
    fn opening_a_file_in_floating_opens_the_pane_and_floats_the_space() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("src").join("main.rs");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(&path, "fn main() {}").expect("file");
        let path = path.to_string_lossy().to_string();

        let mut state = UiState::mock();
        state.spaces = vec![Space::unnamed(Pane::Leaf {
            id: 7,
            kind: PaneKind::Chat,
        })];
        state.active_space = 0;
        state.active_pane_id = 7;
        state.next_pane_id = 8;

        assert!(
            state.open_file_in_floating(path.clone(), None),
            "the file pane opens"
        );
        assert_eq!(
            state.spaces[0].layout,
            LayoutKind::Floating,
            "the active space is arranged as floating windows"
        );
        assert!(
            state.spaces[0].visible_leaf_ids().contains(&7),
            "the pane that was already there stays in the space"
        );
        let active = state.active_pane_id;
        assert!(
            matches!(
                state.spaces[0].leaf_kind(active),
                Some(PaneKind::File { path: shown }) if *shown == path
            ),
            "the new window shows the file that was opened"
        );

        assert_eq!(
            state.toggle_floating_layout(None),
            LayoutKind::Bsp,
            "the docked toggle puts the panes back in the tree"
        );
        assert_eq!(state.spaces[0].layout, LayoutKind::Bsp);
        assert!(
            state.spaces[0].visible_leaf_ids().contains(&7),
            "the original pane survives the round trip"
        );
        assert!(
            state
                .spaces[0]
                .visible_leaf_ids()
                .iter()
                .any(|id| *id == active),
            "the file pane survives the round trip"
        );
    }

    /// The changed-file tree groups paths into directories, and folding one
    /// hides exactly the rows below it.
    #[test]
    fn the_changed_file_tree_nests_directories_and_folds_them() {
        let files = vec![
            DiffFileDetail {
                path: "src/ui/main.rs".to_string(),
                ..DiffFileDetail::default()
            },
            DiffFileDetail {
                path: "src/lib.rs".to_string(),
                ..DiffFileDetail::default()
            },
            DiffFileDetail {
                path: "README.md".to_string(),
                ..DiffFileDetail::default()
            },
        ];
        let rows = tree_rows(&files, &BTreeSet::new());
        let shown: Vec<(&str, usize)> = rows
            .iter()
            .map(|row| (row.name.as_str(), row.depth))
            .collect();
        assert_eq!(
            shown,
            vec![
                ("src", 0),
                ("ui", 1),
                ("main.rs", 2),
                ("lib.rs", 1),
                ("README.md", 0),
            ],
            "directories sort before files and nest by path"
        );

        let folded: BTreeSet<String> = ["src".to_string()].into_iter().collect();
        let rows = tree_rows(&files, &folded);
        let shown: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(shown, vec!["src", "README.md"], "the folded subtree is hidden");
    }

    /// A file with no old lines is added, one with no new lines is deleted, and
    /// everything else is modified.
    #[test]
    fn a_file_status_is_read_from_its_hunks() {
        use goble_ui::elements::diff::parse_unified_diff;

        let added = DiffFileDetail {
            path: "new.rs".to_string(),
            hunks: Rc::new(parse_unified_diff("@@ -0,0 +1,2 @@\n+a\n+b\n")),
            added: 2,
            ..DiffFileDetail::default()
        };
        assert_eq!(status_of(&added), ReviewFileStatus::Added);

        let deleted = DiffFileDetail {
            path: "old.rs".to_string(),
            hunks: Rc::new(parse_unified_diff("@@ -1,2 +0,0 @@\n-a\n-b\n")),
            removed: 2,
            ..DiffFileDetail::default()
        };
        assert_eq!(status_of(&deleted), ReviewFileStatus::Deleted);

        let modified = DiffFileDetail {
            path: "main.rs".to_string(),
            hunks: Rc::new(parse_unified_diff("@@ -1 +1 @@\n-a\n+b\n")),
            added: 1,
            removed: 1,
            ..DiffFileDetail::default()
        };
        assert_eq!(status_of(&modified), ReviewFileStatus::Modified);
    }
}
