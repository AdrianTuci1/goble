//! The measurements DSH gives its diff surfaces, in one place.
//!
//! The review tab, the changed-files card and the shared file comparison are
//! rebuilt from the same DSH CSS modules, so their numbers live together rather
//! than being copied into each element. Every value is a logical pixel unless
//! it is named `*_EM`; a host that zooms the UI scales the rendered result, not
//! these constants.
//!
//! Sources: `ReviewTab.module.css`, `ChangedFiles.module.css` and
//! `FileDiff.module.css` in `ui-deliverables`, plus the shared token sheet for
//! the type ramp.

/// The DSH design measurements for a changed-files card, its review tab and the
/// file comparison those two surfaces share.
pub struct DiffSpec;

impl DiffSpec {
    // -----------------------------------------------------------------------
    // Review tab (`ReviewTab.module.css`)
    // -----------------------------------------------------------------------

    /// The review pane's header: DSH's fixed `height: 38px`.
    pub const REVIEW_HEADER_HEIGHT: f32 = 38.0;
    /// The header's horizontal padding: `0 6px 0 8px`.
    pub const REVIEW_HEADER_PAD_LEFT: f32 = 8.0;
    pub const REVIEW_HEADER_PAD_RIGHT: f32 = 6.0;
    /// The gap between the header's file selector, counts and tools.
    pub const REVIEW_HEADER_GAP: f32 = 6.0;
    /// The file selector's trigger: DSH's `height: 28px`.
    pub const REVIEW_SELECTOR_HEIGHT: f32 = 28.0;
    /// The trigger's text: 12 / 20.
    pub const REVIEW_SELECTOR_SIZE: f32 = 12.0;
    pub const REVIEW_SELECTOR_LINE_HEIGHT: f32 = 20.0;
    /// The trigger's chevron.
    pub const REVIEW_SELECTOR_CHEVRON: f32 = 12.0;
    /// A review tool button, and the glyph it centres.
    pub const REVIEW_TOOL_SIZE: f32 = 28.0;
    pub const REVIEW_TOOL_GLYPH: f32 = 15.0;
    /// The gap between two review tools: DSH's `gap: 2px`.
    pub const REVIEW_TOOL_GAP: f32 = 2.0;

    // -----------------------------------------------------------------------
    // Changed-files card (`ChangedFiles.module.css`)
    // -----------------------------------------------------------------------

    /// The card header's height: DSH's `height: 60px`.
    pub const CHANGED_FILES_HEADER_HEIGHT: f32 = 60.0;
    /// The header's inner padding: `8px 10px`.
    pub const CHANGED_FILES_HEADER_PAD_Y: f32 = 8.0;
    pub const CHANGED_FILES_HEADER_PAD_X: f32 = 10.0;
    /// The room between the tile and the title column: DSH's `gap: 10px`.
    pub const CHANGED_FILES_HEADER_GAP: f32 = 10.0;
    /// The leading tile: `40px × 40px`, radius 10.
    pub const CHANGED_FILES_TILE: f32 = 40.0;
    pub const CHANGED_FILES_TILE_RADIUS: f32 = 10.0;
    /// The glyph inside the tile.
    pub const CHANGED_FILES_TILE_GLYPH: f32 = 20.0;
    /// The card's title: 13 / 20 medium.
    pub const CHANGED_FILES_TITLE_SIZE: f32 = 13.0;
    pub const CHANGED_FILES_TITLE_LINE_HEIGHT: f32 = 20.0;
    /// The header's compact stat line: 10 / 16 mono.
    pub const CHANGED_FILES_STAT_SIZE: f32 = 10.0;
    pub const CHANGED_FILES_STAT_LINE_HEIGHT: f32 = 16.0;
    /// The gap between the added and deleted counts: DSH's `gap: 6px`.
    pub const CHANGED_FILES_STAT_GAP: f32 = 6.0;
    /// A file row's box: `padding: 7px 18px 7px 14px`.
    pub const CHANGED_FILES_ROW_PAD_TOP: f32 = 7.0;
    pub const CHANGED_FILES_ROW_PAD_RIGHT: f32 = 18.0;
    pub const CHANGED_FILES_ROW_PAD_BOTTOM: f32 = 7.0;
    pub const CHANGED_FILES_ROW_PAD_LEFT: f32 = 14.0;
    /// The row's minimum box, before the pointer-coarse rule raises it.
    pub const CHANGED_FILES_ROW_MIN_HEIGHT: f32 = 24.0;
    /// The row's path: 12 / 18.
    pub const CHANGED_FILES_PATH_SIZE: f32 = 12.0;
    pub const CHANGED_FILES_PATH_LINE_HEIGHT: f32 = 18.0;
    /// The row's counts: 11 / 18 mono.
    pub const CHANGED_FILES_COUNT_SIZE: f32 = 11.0;
    pub const CHANGED_FILES_COUNT_LINE_HEIGHT: f32 = 18.0;
    /// The room between the status glyph, path and counts: `gap: 10px`.
    pub const CHANGED_FILES_ROW_GAP: f32 = 10.0;
    /// The status glyph beside a row's path.
    pub const CHANGED_FILES_STATUS_GLYPH: f32 = 14.0;
    /// The fold row: `padding: 10px 18px 10px 14px`, 12 / 18.
    pub const CHANGED_FILES_FOLD_PAD_TOP: f32 = 10.0;
    pub const CHANGED_FILES_FOLD_PAD_RIGHT: f32 = 18.0;
    pub const CHANGED_FILES_FOLD_PAD_BOTTOM: f32 = 10.0;
    pub const CHANGED_FILES_FOLD_PAD_LEFT: f32 = 14.0;
    pub const CHANGED_FILES_FOLD_SIZE: f32 = 12.0;
    pub const CHANGED_FILES_FOLD_LINE_HEIGHT: f32 = 18.0;
    pub const CHANGED_FILES_FOLD_CHEVRON: f32 = 14.0;
    /// The number of rows shown before the fold: DSH's `COLLAPSED_ROWS = 4`.
    pub const CHANGED_FILES_FOLD: usize = 4;
    /// The card's corner radius: DSH's `--dsw-radius-lg` = 16 px.
    pub const CHANGED_FILES_RADIUS: f32 = 16.0;
    /// The card's hairline border and the rule under its header.
    pub const CHANGED_FILES_HAIRLINE: f32 = 0.5;

    // -----------------------------------------------------------------------
    // File comparison (`FileDiff.module.css`)
    // -----------------------------------------------------------------------

    /// A comparison line's box: DSH's 22 px row at the mono 12 px size.
    pub const DIFF_FONT_SIZE: f32 = 12.0;
    pub const DIFF_LINE_HEIGHT: f32 = 22.0;
    /// The line-height factor the text measurer takes (22 / 12).
    pub const DIFF_LINE_HEIGHT_FACTOR: f32 = Self::DIFF_LINE_HEIGHT / Self::DIFF_FONT_SIZE;
    /// DSH's `grid-template-columns: 3.5em 3.5em 1.2em minmax(0, 1fr)`.
    pub const DIFF_GUTTER_EM: f32 = 3.5;
    pub const DIFF_SIGN_EM: f32 = 1.2;
    /// The change marker: a 3 px inset rail at the left of the gutter.
    pub const DIFF_MARKER_WIDTH: f32 = 3.0;
    /// The gutter number's right padding.
    pub const DIFF_NUMBER_PAD_RIGHT: f32 = 8.0;
    /// The body's vertical padding: `8px 0 16px`.
    pub const DIFF_BODY_PAD_TOP: f32 = 8.0;
    pub const DIFF_BODY_PAD_BOTTOM: f32 = 16.0;
    /// The hunk header's padding and the gap under a hunk:
    /// `padding: 4px 16px` and `margin-bottom: 8px`.
    pub const DIFF_HUNK_HEADER_PAD_Y: f32 = 4.0;
    pub const DIFF_HUNK_HEADER_PAD_X: f32 = 16.0;
    pub const DIFF_HUNK_GAP: f32 = 8.0;
    /// The trailing room after a line's text: `.text { padding-right: 16px }`.
    pub const DIFF_TEXT_PAD_RIGHT: f32 = 16.0;
    /// The split view's divider between its two cells.
    pub const DIFF_SPLIT_DIVIDER: f32 = 0.5;

    /// The width of one line-number gutter at `font_size`.
    pub fn gutter_width(font_size: f32) -> f32 {
        Self::DIFF_GUTTER_EM * font_size
    }

    /// The width of the sign column at `font_size`.
    pub fn sign_width(font_size: f32) -> f32 {
        Self::DIFF_SIGN_EM * font_size
    }

    /// One comparison row's height at the default size.
    pub fn line_height() -> f32 {
        Self::DIFF_LINE_HEIGHT
    }
}
