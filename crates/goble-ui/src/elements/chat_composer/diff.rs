//! The working tree's diff, as the rich input's diff control reads it.
//!
//! One row per changed file and the totals the control draws over it. The host
//! fills it from the repository (`app/src/ui/pickers.rs`); the composer only
//! draws it, so a pane with no repository has no summary and draws no control.

/// One changed file: its path in the repository and the lines it gained and
/// lost against `HEAD`. An untracked file counts all of its lines as additions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffFileStat {
    pub path: String,
    pub added: u32,
    pub removed: u32,
}

impl DiffFileStat {
    pub fn new(path: impl Into<String>, added: u32, removed: u32) -> Self {
        Self {
            path: path.into(),
            added,
            removed,
        }
    }
}

/// Every changed file in a pane's working tree, with the totals the control
/// shows. A clean tree has no files and draws no control, the way warp-new's
/// diff chip is only shown while there is something to show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffSummary {
    pub files: Vec<DiffFileStat>,
}

impl DiffSummary {
    pub fn new(files: Vec<DiffFileStat>) -> Self {
        Self { files }
    }

    pub fn files_changed(&self) -> usize {
        self.files.len()
    }

    pub fn added(&self) -> u64 {
        self.files.iter().map(|file| u64::from(file.added)).sum()
    }

    pub fn removed(&self) -> u64 {
        self.files.iter().map(|file| u64::from(file.removed)).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_sum_every_changed_file() {
        let summary = DiffSummary::new(vec![
            DiffFileStat::new("src/lib.rs", 12, 3),
            DiffFileStat::new("README.md", 4, 0),
        ]);
        assert_eq!(summary.files_changed(), 2);
        assert_eq!(summary.added(), 16);
        assert_eq!(summary.removed(), 3);
        assert!(!summary.is_empty());
        assert!(DiffSummary::default().is_empty());
    }
}
