//! Unified-diff transcript row: the parsed model and the elements that draw it.
//!
//! One module per surface: the unified-diff data and its parser ([`model`]),
//! the inline transcript [`paint::Diff`], the reusable comparison
//! [`file_diff::FileDiffView`], and the DSH measurements both draw from
//! ([`spec`]). The re-exports here keep every public path this module had
//! before the split (`elements::diff::Diff`, `elements::diff::parse_unified_diff`,
//! ...).

mod file_diff;
mod model;
mod paint;
mod review_tab;
mod spec;
#[cfg(test)]
mod tests;

pub use file_diff::{DiffLayout, FileDiffView};
pub use model::{
    build_split_rows, parse_unified_diff, split_rows, DiffLine, DiffLineKind, DiffRow, DiffStats,
    Hunk, SplitDiffRow, SplitRow,
};
pub use paint::{Diff, DiffHighlightJob, DiffHighlightSide, DiffSide, PreparedDiff};
pub use review_tab::{ReviewHeader, ReviewTab, ReviewTabFile, ReviewTabHeader, ReviewTabState};
pub use spec::DiffSpec;
