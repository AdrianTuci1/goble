//! Unified-diff transcript row: the parsed model and the element that draws it.
//!
//! One module per surface: the unified-diff data and its parser ([`model`]) and
//! the [`paint::Diff`] element that measures, highlights and draws it. The
//! re-exports here keep every public path this module had before the split
//! (`elements::diff::Diff`, `elements::diff::parse_unified_diff`, ...).

mod model;
mod paint;
#[cfg(test)]
mod tests;

pub use model::{parse_unified_diff, DiffLine, DiffLineKind, DiffRow, DiffStats, Hunk};
pub use paint::Diff;
