//! The review pane's own surfaces: the changed-file tree, the header's file
//! selector, the status facts each row carries, and the comparison body.
//!
//! The review pane was one flat list of diffs. This module gives it the surface
//! DSH's review tab has: a tree of the changed files with their type icons,
//! directories, `M`/`A`/`D` status and line counts; a selector in the header
//! that lists the same files and switches the comparison; and a comparison that
//! draws unified or side by side and can wrap long lines. Every view choice is
//! app-owned (see [`crate::state::DiffReviewState`]), because the element tree
//! is rebuilt every frame.
//!
//! Comparisons are drawn by the goble-ui [`FileDiffView`]. The app's own
//! [`ReviewDiff`] survives only as the fallback for a checkout built without
//! the `file-diff-view` feature (on by default), so the main review path has a
//! single painter for unified, split and wrapped bodies.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::rc::Rc;

use goble_ui::color::ColorU;
use goble_ui::elements::diff::{DiffLineKind, Hunk};
use goble_ui::elements::{
    file_icon_name, AppContext, Axis, Button, ButtonVariant, ConstrainedBox, Container,
    CrossAxisAlignment, EdgeInsets, Element, Empty, Expanded, Flex, HoverRow, Icon, LayoutContext,
    MainAxisSize, PaintContext, Point, PopupMenu, PopupMenuItem, PopupMenuPosition, ScrollState,
    Scrollable, SizeConstraint, Text, TextSpan, Tooltip, TooltipPosition,
};
use goble_ui::geometry::{rectf, vec2f, Vector2F};
use goble_ui::platform::text_atlas::{wrapped_lines, FontWeight};
use goble_ui::theme::{ColorToken, FontFamily};

use super::pickers::{DiffDetail, DiffFileDetail};
use super::{DiffPanelAction, UiActions};

#[cfg(feature = "file-diff-view")]
use goble_ui::elements::diff::{DiffLayout, FileDiffView};

/// The status a changed file carries in the tree: the three letters git's
/// short status uses for the cases a review lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewFileStatus {
    Modified,
    Added,
    Deleted,
}

impl ReviewFileStatus {
    pub fn letter(self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Deleted => "D",
        }
    }

    pub fn color(self) -> ColorToken {
        match self {
            Self::Modified => ColorToken::Warning,
            Self::Added => ColorToken::Success,
            Self::Deleted => ColorToken::Error,
        }
    }

    /// What the status means in a tooltip.
    pub fn label(self) -> &'static str {
        match self {
            Self::Modified => "Modified",
            Self::Added => "Added",
            Self::Deleted => "Deleted",
        }
    }
}

/// The status of one changed file, derived from the diff the working tree
/// read produced. A file whose every hunk starts at old line zero is new; one
/// whose every hunk ends at new line zero was deleted; everything else is a
/// modification. A binary entry has no hunks, so its counts stand in.
pub fn status_of(file: &DiffFileDetail) -> ReviewFileStatus {
    if file.hunks.is_empty() {
        return if file.added > 0 && file.removed == 0 {
            ReviewFileStatus::Added
        } else if file.added == 0 && file.removed > 0 {
            ReviewFileStatus::Deleted
        } else {
            ReviewFileStatus::Modified
        };
    }
    let new_file = file
        .hunks
        .iter()
        .all(|hunk| hunk.old_start == 0 && hunk.old_count == 0);
    if new_file {
        return ReviewFileStatus::Added;
    }
    let deleted = file
        .hunks
        .iter()
        .all(|hunk| hunk.new_start == 0 && hunk.new_count == 0);
    if deleted {
        return ReviewFileStatus::Deleted;
    }
    ReviewFileStatus::Modified
}

/// Whether a file can be opened, copied or edited: a deleted file has no text
/// on disk, so those controls are honestly disabled rather than failing after
/// the press.
pub fn file_exists(file: &DiffFileDetail) -> bool {
    status_of(file) != ReviewFileStatus::Deleted && Path::new(&file.absolute_path).exists()
}

/// One drawn row of the changed-file tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewTreeRow {
    pub path: String,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    /// The index into the review's file list for a file row; `None` for a
    /// directory.
    pub file: Option<usize>,
    pub folded: bool,
}

/// Flatten the changed files into the rows a tree draws: directories first,
/// then files, each alphabetical at every level, with a folded directory
/// hiding the rows below it. The directories are implied by the file paths,
/// so no extra read is needed to list them.
pub fn tree_rows(files: &[DiffFileDetail], folded: &BTreeSet<String>) -> Vec<ReviewTreeRow> {
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    for file in files {
        let parts: Vec<&str> = file.path.split('/').collect();
        for depth in 1..parts.len() {
            dirs.insert(parts[..depth].join("/"));
        }
    }
    let mut rows = Vec::new();
    append_level("", 0, files, &dirs, folded, &mut rows);
    rows
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("")
}

fn append_level(
    parent: &str,
    depth: usize,
    files: &[DiffFileDetail],
    dirs: &BTreeSet<String>,
    folded: &BTreeSet<String>,
    rows: &mut Vec<ReviewTreeRow>,
) {
    let mut child_dirs: Vec<&String> = dirs
        .iter()
        .filter(|dir| parent_of(dir.as_str()) == parent)
        .collect();
    child_dirs.sort_by_key(|dir| dir.to_lowercase());
    for dir in child_dirs {
        let is_folded = folded.contains(dir);
        rows.push(ReviewTreeRow {
            path: dir.clone(),
            name: dir.rsplit('/').next().unwrap_or(dir).to_string(),
            depth,
            is_dir: true,
            file: None,
            folded: is_folded,
        });
        if !is_folded {
            append_level(dir, depth + 1, files, dirs, folded, rows);
        }
    }
    let mut child_files: Vec<(usize, &DiffFileDetail)> = files
        .iter()
        .enumerate()
        .filter(|(_, file)| parent_of(&file.path) == parent)
        .collect();
    child_files.sort_by_key(|(_, file)| file.path.to_lowercase());
    for (index, file) in child_files {
        rows.push(ReviewTreeRow {
            path: file.path.clone(),
            name: file
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&file.path)
                .to_string(),
            depth,
            is_dir: false,
            file: Some(index),
            folded: false,
        });
    }
}

/// The changed-file tree in the review's side rail.
pub(crate) fn file_list(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    detail: &DiffDetail,
    selected: Option<&str>,
    folded: &BTreeSet<String>,
    scroll: Rc<RefCell<ScrollState>>,
) -> Box<dyn Element> {
    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(1.0);
    if selected.is_some() {
        let on_diff = actions.on_diff_panel.clone();
        column = column.with_child(
            HoverRow::new(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(6.0)
                    .with_child(
                        Icon::new("flat-list")
                            .with_size(13.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_child(
                        Text::new("All changes")
                            .with_font_size(12.0)
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .finish(),
            )
            .with_padding(EdgeInsets::new(6.0, 8.0, 6.0, 8.0))
            .with_corner_radius(4.0)
            .with_on_click(move || (on_diff.borrow_mut())(pane_id, DiffPanelAction::Select(None)))
            .finish(),
        );
    }
    for row in tree_rows(&detail.files, folded) {
        column = column.with_child(tree_row(
            app,
            actions,
            pane_id,
            &detail.files,
            &row,
            selected,
        ));
    }
    ConstrainedBox::new(
        Scrollable::new(
            Container::new(column.finish())
                .with_padding_right(8.0)
                .finish(),
            Axis::Vertical,
        )
        .with_state(scroll)
        .finish(),
    )
    .with_width(REVIEW_LIST_WIDTH)
    .finish()
}

/// The rail's width. A tree row carries the type icon, a name, a status letter
/// and the signed counts; this is the narrowest column that keeps all four
/// legible without eliding every deep path.
pub const REVIEW_LIST_WIDTH: f32 = 264.0;

fn tree_row(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    files: &[DiffFileDetail],
    row: &ReviewTreeRow,
    selected: Option<&str>,
) -> Box<dyn Element> {
    let mut line = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(0.0);
    if row.depth > 0 {
        line = line.with_child(fixed(row.depth as f32 * 12.0));
    }
    line = line.with_child(if row.is_dir {
        Container::new(
            Icon::new(if row.folded {
                "chevron-right"
            } else {
                "chevron-down"
            })
            .with_size(11.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        )
        .with_padding(EdgeInsets::uniform(2.5))
        .finish()
    } else {
        fixed(16.0)
    });
    line = line.with_child(fixed(4.0));
    line = line.with_child(
        Icon::new(if row.is_dir {
            if row.folded {
                "folder-closed"
            } else {
                "folder-open"
            }
        } else {
            file_icon_name(&row.name)
        })
        .with_size(14.0)
        .with_theme_color(ColorToken::Muted, app)
        .finish(),
    );
    line = line.with_child(fixed(6.0));
    line = line.with_child(
        Expanded::new(
            Text::new(row.name.clone())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
        )
        .finish(),
    );

    if let Some(index) = row.file {
        let file = &files[index];
        let status = status_of(file);
        line = line
            .with_child(
                Tooltip::new(
                    Text::new(status.letter())
                        .with_font_family(FontFamily::Mono)
                        .with_font_size(10.0)
                        .with_theme_color(status.color(), app)
                        .finish(),
                    status.label(),
                )
                .with_position(TooltipPosition::Below)
                .finish(),
            )
            .with_child(fixed(4.0))
            .with_child(file_counts(app, file.added, file.removed));
    }

    let inner = line.finish();
    if row.is_dir {
        let on_diff = actions.on_diff_panel.clone();
        let path = row.path.clone();
        return HoverRow::new(inner)
            .with_padding(EdgeInsets::new(4.0, 5.0, 4.0, 6.0))
            .with_corner_radius(4.0)
            .with_on_click(move || {
                (on_diff.borrow_mut())(pane_id, DiffPanelAction::ToggleDir(path.clone()))
            })
            .finish();
    }
    let on_diff = actions.on_diff_panel.clone();
    let path = row.path.clone();
    HoverRow::new(inner)
        .with_selected(selected == row.file.map(|index| files[index].path.as_str()))
        .with_padding(EdgeInsets::new(4.0, 5.0, 4.0, 6.0))
        .with_corner_radius(4.0)
        .with_on_click(move || {
            (on_diff.borrow_mut())(pane_id, DiffPanelAction::Select(Some(path.clone())));
        })
        .finish()
}

/// The review topbar's file selector: the selected file's name behind a
/// chevron, opening a menu of every changed file with its status and counts.
/// The menu owns its arrow keys and `Enter`, and its open flag and highlight
/// are the app's cells, so both survive the per-frame rebuild.
pub(crate) fn file_selector(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    detail: &DiffDetail,
    selected: Option<&str>,
    state: &crate::state::DiffReviewState,
) -> Box<dyn Element> {
    // The trigger names the picker, not the selected file: every card below
    // already names its path, and a long path here would crowd the header's
    // totals and controls. The menu shows the files with their status letters,
    // which is what the check and the keyboard move through.
    let trigger = Button::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(5.0)
            .with_child(
                Text::new(if detail.files.is_empty() {
                    "No files".to_string()
                } else {
                    "Files".to_string()
                })
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
            )
            .with_child(
                Icon::new("chevron-down")
                    .with_size(10.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .finish(),
    )
    .with_variant(ButtonVariant::Ghost)
    .with_corner_radius(4.0)
    .finish();
    let trigger = ConstrainedBox::new(trigger).with_max_width(240.0).finish();
    let items: Vec<PopupMenuItem> = detail
        .files
        .iter()
        .map(|file| {
            let status = status_of(file);
            let item = PopupMenuItem::new(format!("{}  {}", status.letter(), file.path))
                .with_icon(file_icon_name(&file.path))
                .with_diff(file.added, file.removed);
            if selected == Some(file.path.as_str()) {
                item.selected()
            } else {
                item
            }
        })
        .collect();
    let paths: Vec<String> = detail.files.iter().map(|file| file.path.clone()).collect();
    let on_diff = actions.on_diff_panel.clone();
    let open = state.selector_open.clone();
    let close = state.selector_open.clone();
    PopupMenu::new(trigger, items)
        .with_open(open)
        .with_position(PopupMenuPosition::Below)
        .with_highlight_index(state.selector_highlight.clone())
        .with_on_select(move |index| {
            *close.borrow_mut() = false;
            if let Some(path) = paths.get(index) {
                (on_diff.borrow_mut())(pane_id, DiffPanelAction::Select(Some(path.clone())));
            }
        })
        .finish()
}

fn file_counts(app: &AppContext, added: u32, removed: u32) -> Box<dyn Element> {
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(3.0)
        .with_child(
            Text::new(format!("+{added}"))
                .with_font_family(FontFamily::Mono)
                .with_font_size(9.0)
                .with_theme_color(ColorToken::Success, app)
                .finish(),
        )
        .with_child(
            Text::new(format!("-{removed}"))
                .with_font_family(FontFamily::Mono)
                .with_font_size(9.0)
                .with_theme_color(ColorToken::Error, app)
                .finish(),
        )
        .finish()
}

fn fixed(width: f32) -> Box<dyn Element> {
    Empty::new().with_size(vec2f(width, 0.0)).finish()
}

/// A comparison body for one file: the goble-ui [`FileDiffView`], with rows
/// and syntax runs prepared once at read time.
///
/// Every layout goes through the same painter — unified is the default, split
/// and wrap are the same view's choices — so the review cannot drift into two
/// different comparisons. [`FileDiffView`] paints the change bands from the
/// same [`ColorToken::DiffInsertBg`]/[`ColorToken::DiffDeleteBg`] pair the
/// app's fallback used, which is what keeps the colours continuous.
pub(crate) fn comparison(
    file: &DiffFileDetail,
    split: bool,
    wrap: bool,
    scroll: Rc<RefCell<ScrollState>>,
) -> Box<dyn Element> {
    #[cfg(feature = "file-diff-view")]
    {
        let view = FileDiffView::from_prepared(Rc::clone(&file.hunks), file.prepared.clone())
            .with_layout(if split {
                DiffLayout::Split
            } else {
                DiffLayout::Unified
            })
            .with_wrap(wrap)
            .with_viewport(Rc::clone(&scroll))
            .with_fill_width(true);
        return Box::new(view);
    }
    #[cfg(not(feature = "file-diff-view"))]
    {
        let (old, new) = file.prepared.highlighted_lines();
        Box::new(ReviewDiff::new(
            Rc::clone(&file.hunks),
            split,
            wrap,
            scroll,
            old,
            new,
        ))
    }
}

/// The status letter a file shows in the pane's topbar.
pub fn header_status(app: &AppContext, file: &DiffFileDetail) -> Box<dyn Element> {
    let status = status_of(file);
    Tooltip::new(
        Text::new(status.letter())
            .with_font_family(FontFamily::Mono)
            .with_font_size(10.0)
            .with_theme_color(status.color(), app)
            .finish(),
        status.label(),
    )
    .with_position(TooltipPosition::Below)
    .finish()
}

// ---------------------------------------------------------------------------
// The fallback comparison painter.
// ---------------------------------------------------------------------------

const DIFF_FONT_SIZE: f32 = 12.0;
const DIFF_LINE_HEIGHT: f32 = 1.35;
const DIFF_ROW_HEIGHT: f32 = DIFF_FONT_SIZE * DIFF_LINE_HEIGHT;
const DIFF_GUTTER: f32 = 34.0;
const DIFF_SIGN: f32 = 12.0;
const DIFF_PAD: f32 = 8.0;
const DIFF_RAIL: f32 = 3.0;

/// One logical line of a comparison, before wrapping splits it into visual
/// rows.
#[derive(Clone, Debug)]
struct LogicalCell {
    old_no: Option<u32>,
    new_no: Option<u32>,
    kind: DiffLineKind,
    text: String,
}

#[derive(Clone, Debug)]
enum LogicalRow {
    Header(String),
    Unified(LogicalCell),
    Split(Option<LogicalCell>, Option<LogicalCell>),
}

/// A logical row after wrapping: each cell's text is one or more chunks, and
/// every chunk is one fixed-height visual row.
struct VisualRow {
    header: Option<String>,
    left: Option<VisualCell>,
    right: Option<VisualCell>,
}

struct VisualCell {
    old_no: Option<u32>,
    new_no: Option<u32>,
    kind: DiffLineKind,
    chunks: Vec<String>,
}

impl VisualRow {
    fn height(&self) -> usize {
        self.left
            .as_ref()
            .map(|cell| cell.chunks.len())
            .unwrap_or(0)
            .max(
                self.right
                    .as_ref()
                    .map(|cell| cell.chunks.len())
                    .unwrap_or(0),
            )
            .max(1)
    }
}

/// The fallback unified/side-by-side comparison. It draws the parsed hunks
/// directly, with the theme's insert/delete bands and the two line-number
/// gutters, and wraps long lines when asked. The element is deliberately
/// simple: it is the bridge until `FileDiffView` is the one painter.
struct ReviewDiff {
    hunks: Rc<Vec<Hunk>>,
    split: bool,
    wrap: bool,
    viewport: Option<Rc<RefCell<ScrollState>>>,
    /// The coloured runs the read prepared, keyed by file line number: the old
    /// side and the new side. The worker installs them beside the frame; the
    /// painter falls back to the change's flat colour until it does.
    old_highlights: Rc<HashMap<u32, Vec<TextSpan>>>,
    new_highlights: Rc<HashMap<u32, Vec<TextSpan>>>,
    visual: Vec<VisualRow>,
    top: Vec<f32>,
    gutter_digits: u32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ReviewDiff {
    #[allow(clippy::too_many_arguments)]
    fn new(
        hunks: Rc<Vec<Hunk>>,
        split: bool,
        wrap: bool,
        viewport: Rc<RefCell<ScrollState>>,
        old_highlights: Rc<HashMap<u32, Vec<TextSpan>>>,
        new_highlights: Rc<HashMap<u32, Vec<TextSpan>>>,
    ) -> Self {
        Self {
            hunks,
            split,
            wrap,
            viewport: Some(viewport),
            old_highlights,
            new_highlights,
            visual: Vec::new(),
            top: Vec::new(),
            gutter_digits: 1,
            size: None,
            origin: None,
        }
    }

    fn logical_rows(&self) -> Vec<LogicalRow> {
        let mut rows = Vec::new();
        for hunk in self.hunks.iter() {
            rows.push(LogicalRow::Header(hunk.header()));
            if self.split {
                rows.extend(split_rows(hunk));
            } else {
                rows.extend(hunk.lines.iter().map(|line| {
                    LogicalRow::Unified(LogicalCell {
                        old_no: line.old_line,
                        new_no: line.new_line,
                        kind: line.kind,
                        text: line.text.clone(),
                    })
                }));
            }
        }
        rows
    }

    fn prepare(&mut self, width: f32) {
        let rows = self.logical_rows();
        self.gutter_digits = self
            .hunks
            .iter()
            .flat_map(|hunk| {
                [
                    hunk.old_start + hunk.old_count,
                    hunk.new_start + hunk.new_count,
                ]
            })
            .max()
            .map(digit_count)
            .unwrap_or(1)
            .max(1);
        let gutter = self.gutter_width();
        let text_width = if self.wrap && width.is_finite() {
            let columns = if self.split { 2.0 } else { 1.0 };
            ((width - DIFF_PAD * 2.0 - gutter * columns - DIFF_RAIL) / columns).max(24.0)
        } else {
            f32::INFINITY
        };
        self.visual = rows
            .into_iter()
            .map(|row| match row {
                LogicalRow::Header(header) => VisualRow {
                    header: Some(header),
                    left: None,
                    right: None,
                },
                LogicalRow::Unified(cell) => VisualRow {
                    header: None,
                    left: Some(wrap_cell(cell, text_width)),
                    right: None,
                },
                LogicalRow::Split(left, right) => VisualRow {
                    header: None,
                    left: left.map(|cell| wrap_cell(cell, text_width)),
                    right: right.map(|cell| wrap_cell(cell, text_width)),
                },
            })
            .collect();
        self.top.clear();
        let mut y = 0.0;
        for row in &self.visual {
            self.top.push(y);
            y += row.height() as f32 * DIFF_ROW_HEIGHT;
        }
        self.top.push(y);
    }

    fn gutter_width(&self) -> f32 {
        let digits = self.gutter_digits as f32;
        if self.split {
            digits * 7.0 + 12.0
        } else {
            digits * 14.0 + 12.0
        }
    }

    fn natural_width(&self) -> f32 {
        let mut longest = 0.0f32;
        for row in &self.visual {
            for cell in [row.left.as_ref(), row.right.as_ref()]
                .into_iter()
                .flatten()
            {
                for chunk in &cell.chunks {
                    longest = longest.max(measure_mono(chunk));
                }
            }
        }
        let columns = if self.split { 2.0 } else { 1.0 };
        DIFF_PAD * 2.0 + (self.gutter_width() + longest + DIFF_RAIL) * columns
    }

    fn row_bg(kind: DiffLineKind, app: &AppContext) -> ColorU {
        match kind {
            DiffLineKind::Added => app.theme.color(ColorToken::DiffInsertBg),
            DiffLineKind::Removed => app.theme.color(ColorToken::DiffDeleteBg),
            DiffLineKind::Context => app.theme.color(ColorToken::Surface),
        }
    }

    fn text_color(kind: DiffLineKind, app: &AppContext) -> ColorU {
        match kind {
            DiffLineKind::Added => app.theme.color(ColorToken::Success),
            DiffLineKind::Removed => app.theme.color(ColorToken::Error),
            DiffLineKind::Context => app.theme.color(ColorToken::Text),
        }
    }

    fn sign(kind: DiffLineKind) -> &'static str {
        match kind {
            DiffLineKind::Added => "+",
            DiffLineKind::Removed => "-",
            DiffLineKind::Context => " ",
        }
    }

    fn paint_cell(
        &self,
        renderer: &mut goble_ui::render::Renderer,
        app: &AppContext,
        cell: &VisualCell,
        origin: Vector2F,
        width: f32,
        chunk_start: usize,
        line: usize,
    ) {
        let y = origin.y + line as f32 * DIFF_ROW_HEIGHT;
        if cell.kind != DiffLineKind::Context {
            renderer.fill_rect(
                rectf(origin.x, y, width, DIFF_ROW_HEIGHT),
                Self::row_bg(cell.kind, app),
            );
            renderer.fill_rect(
                rectf(origin.x, y, DIFF_RAIL, DIFF_ROW_HEIGHT),
                Self::text_color(cell.kind, app),
            );
        }
        let gutter = self.gutter_width();
        let color = Self::text_color(cell.kind, app);
        let muted = app.theme.color(ColorToken::Muted);
        if self.split {
            if let Some(number) = cell.old_no {
                draw_number(
                    renderer,
                    origin.x + gutter - 12.0,
                    y,
                    number,
                    if cell.kind == DiffLineKind::Removed {
                        color
                    } else {
                        muted
                    },
                );
            }
            if let Some(number) = cell.new_no {
                draw_number(
                    renderer,
                    origin.x + gutter - 12.0,
                    y,
                    number,
                    if cell.kind == DiffLineKind::Added {
                        color
                    } else {
                        muted
                    },
                );
            }
        } else {
            if let Some(number) = cell.old_no {
                draw_number(
                    renderer,
                    origin.x + 24.0,
                    y,
                    number,
                    if cell.kind == DiffLineKind::Removed {
                        color
                    } else {
                        muted
                    },
                );
            }
            if let Some(number) = cell.new_no {
                draw_number(
                    renderer,
                    origin.x + gutter - 12.0,
                    y,
                    number,
                    if cell.kind == DiffLineKind::Added {
                        color
                    } else {
                        muted
                    },
                );
            }
        }
        let marker = Self::sign(cell.kind);
        if marker != " " {
            renderer.draw_text_with_font(
                vec2f(origin.x + gutter - 2.0, y),
                marker,
                DIFF_FONT_SIZE,
                color,
                DIFF_SIGN,
                DIFF_LINE_HEIGHT,
                FontWeight::Regular,
                FontFamily::Mono,
                false,
            );
        }
        let text_x = origin.x + gutter + DIFF_SIGN;
        // The prepared highlight is keyed by the line's number on its own
        // side; a wrapped line is drawn flat because its chunks no longer line
        // up with the runs the highlighter produced for the whole line.
        let runs = if cell.chunks.len() == 1 {
            match cell.kind {
                DiffLineKind::Removed => cell.old_no.and_then(|no| self.old_highlights.get(&no)),
                DiffLineKind::Added => cell.new_no.and_then(|no| self.new_highlights.get(&no)),
                DiffLineKind::Context => cell
                    .old_no
                    .and_then(|no| self.old_highlights.get(&no))
                    .or_else(|| cell.new_no.and_then(|no| self.new_highlights.get(&no))),
            }
        } else {
            None
        };
        if let Some(runs) = runs {
            let mut x = text_x;
            for run in runs {
                renderer.draw_text_with_font(
                    vec2f(x, y),
                    run.text.clone(),
                    DIFF_FONT_SIZE,
                    run.color,
                    f32::INFINITY,
                    DIFF_LINE_HEIGHT,
                    run.weight,
                    FontFamily::Mono,
                    run.italic,
                );
                x += measure_mono(&run.text);
            }
        } else if let Some(chunk) = cell.chunks.get(chunk_start) {
            renderer.draw_text_with_font(
                vec2f(text_x, y),
                chunk.clone(),
                DIFF_FONT_SIZE,
                color,
                f32::INFINITY,
                DIFF_LINE_HEIGHT,
                FontWeight::Regular,
                FontFamily::Mono,
                false,
            );
        }
    }
}

impl Element for ReviewDiff {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let width = if constraint.max.x.is_finite() {
            constraint.max.x.max(constraint.min.x)
        } else {
            0.0
        };
        self.prepare(if width > 0.0 { width } else { f32::INFINITY });
        let natural = self.natural_width();
        let width = if width > 0.0 {
            width
        } else {
            natural.max(constraint.min.x).min(constraint.max.x)
        };
        let height = self.top.last().copied().unwrap_or(0.0);
        let size = vec2f(width, height.max(constraint.min.y));
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let window = self
            .viewport
            .as_ref()
            .and_then(|state| state.borrow().painted_viewport());
        let first = window
            .map(|window| {
                let top = window.min_y() - origin.y;
                self.top.iter().rposition(|y| *y <= top).unwrap_or(0)
            })
            .unwrap_or(0);
        let last = window
            .map(|window| {
                let bottom = window.max_y() - origin.y;
                self.top
                    .iter()
                    .position(|y| *y >= bottom)
                    .unwrap_or(self.visual.len())
                    .min(self.visual.len())
            })
            .unwrap_or(self.visual.len());
        let half = (size.x - DIFF_PAD * 2.0) / 2.0;
        for index in first..last {
            let row = &self.visual[index];
            let top = origin.y + self.top[index];
            if let Some(header) = &row.header {
                renderer.fill_rect(
                    rectf(origin.x, top, size.x, DIFF_ROW_HEIGHT),
                    app.theme.color(ColorToken::SurfaceRaised),
                );
                renderer.draw_text_with_font(
                    vec2f(origin.x + DIFF_PAD, top),
                    header.clone(),
                    DIFF_FONT_SIZE,
                    app.theme.color(ColorToken::Muted),
                    f32::INFINITY,
                    DIFF_LINE_HEIGHT,
                    FontWeight::Regular,
                    FontFamily::Mono,
                    false,
                );
                continue;
            }
            if self.split {
                let left_origin = vec2f(origin.x + DIFF_PAD, top);
                let right_origin = vec2f(origin.x + DIFF_PAD + half, top);
                if let Some(cell) = &row.left {
                    for line in 0..cell.chunks.len().max(1) {
                        self.paint_cell(renderer, app, cell, left_origin, half, line, line);
                    }
                } else {
                    renderer.fill_rect(
                        rectf(
                            left_origin.x,
                            top,
                            half,
                            DIFF_ROW_HEIGHT * row.height() as f32,
                        ),
                        app.theme.color(ColorToken::SurfaceRaised),
                    );
                }
                if let Some(cell) = &row.right {
                    for line in 0..cell.chunks.len().max(1) {
                        self.paint_cell(renderer, app, cell, right_origin, half, line, line);
                    }
                } else {
                    renderer.fill_rect(
                        rectf(
                            right_origin.x,
                            top,
                            half,
                            DIFF_ROW_HEIGHT * row.height() as f32,
                        ),
                        app.theme.color(ColorToken::SurfaceRaised),
                    );
                }
                renderer.fill_rect(
                    rectf(
                        origin.x + DIFF_PAD + half - 1.0,
                        top,
                        1.0,
                        DIFF_ROW_HEIGHT * row.height() as f32,
                    ),
                    app.theme.color(ColorToken::Border),
                );
            } else if let Some(cell) = &row.left {
                let cell_origin = vec2f(origin.x + DIFF_PAD, top);
                let width = size.x - DIFF_PAD * 2.0;
                for line in 0..cell.chunks.len().max(1) {
                    self.paint_cell(renderer, app, cell, cell_origin, width, line, line);
                }
            }
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

fn split_rows(hunk: &Hunk) -> Vec<LogicalRow> {
    let mut rows = Vec::new();
    let mut dels: Vec<LogicalCell> = Vec::new();
    let mut adds: Vec<LogicalCell> = Vec::new();
    let flush =
        |dels: &mut Vec<LogicalCell>, adds: &mut Vec<LogicalCell>, rows: &mut Vec<LogicalRow>| {
            for index in 0..dels.len().max(adds.len()) {
                rows.push(LogicalRow::Split(
                    dels.get(index).cloned(),
                    adds.get(index).cloned(),
                ));
            }
            dels.clear();
            adds.clear();
        };
    for line in &hunk.lines {
        match line.kind {
            DiffLineKind::Removed => dels.push(LogicalCell {
                old_no: line.old_line,
                new_no: None,
                kind: DiffLineKind::Removed,
                text: line.text.clone(),
            }),
            DiffLineKind::Added => adds.push(LogicalCell {
                old_no: None,
                new_no: line.new_line,
                kind: DiffLineKind::Added,
                text: line.text.clone(),
            }),
            DiffLineKind::Context => {
                flush(&mut dels, &mut adds, &mut rows);
                rows.push(LogicalRow::Split(
                    Some(LogicalCell {
                        old_no: line.old_line,
                        new_no: None,
                        kind: DiffLineKind::Context,
                        text: line.text.clone(),
                    }),
                    Some(LogicalCell {
                        old_no: None,
                        new_no: line.new_line,
                        kind: DiffLineKind::Context,
                        text: line.text.clone(),
                    }),
                ));
            }
        }
    }
    flush(&mut dels, &mut adds, &mut rows);
    rows
}

fn wrap_cell(cell: LogicalCell, width: f32) -> VisualCell {
    let chunks = if width.is_finite() {
        let lines = wrapped_lines(
            &cell.text,
            DIFF_FONT_SIZE,
            DIFF_LINE_HEIGHT,
            width,
            FontWeight::Regular,
            FontFamily::Mono,
            false,
        );
        if lines.is_empty() {
            vec![String::new()]
        } else {
            lines.into_iter().map(|line| line.text).collect()
        }
    } else {
        vec![cell.text]
    };
    VisualCell {
        old_no: cell.old_no,
        new_no: cell.new_no,
        kind: cell.kind,
        chunks,
    }
}

fn draw_number(
    renderer: &mut goble_ui::render::Renderer,
    right_x: f32,
    y: f32,
    number: u32,
    color: ColorU,
) {
    let text = number.to_string();
    let width = measure_mono(&text);
    renderer.draw_text_with_font(
        vec2f(right_x - width, y),
        text,
        DIFF_FONT_SIZE,
        color,
        f32::INFINITY,
        DIFF_LINE_HEIGHT,
        FontWeight::Regular,
        FontFamily::Mono,
        false,
    );
}

fn measure_mono(text: &str) -> f32 {
    goble_ui::platform::text_atlas::measure_text_family(
        text,
        DIFF_FONT_SIZE,
        DIFF_LINE_HEIGHT,
        f32::INFINITY,
        FontWeight::Regular,
        FontFamily::Mono,
        false,
    )
    .x
}

fn digit_count(mut n: u32) -> u32 {
    let mut digits = 1;
    while n >= 10 {
        n /= 10;
        digits += 1;
    }
    digits
}

#[cfg(test)]
mod ui_tests {
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;
    use std::sync::Arc;

    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::{
        AppContext, Element, EventContext, LayoutContext, PaintContext, SizeConstraint,
    };
    use goble_ui::event::{DispatchedEvent, ModifiersState};
    use goble_ui::geometry::{vec2f, Vector2F};
    use goble_ui::render::{RenderCommand, Renderer};

    use crate::root_view::RootView;
    use crate::state::UiState;
    use crate::ui::{LayoutKind, Pane, PaneKind, Space};

    fn window() -> Vector2F {
        vec2f(1024.0, 768.0)
    }

    fn git_available() -> bool {
        std::process::Command::new("git")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn git(dir: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn repository() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "test@example.com"]);
        git(dir.path(), &["config", "user.name", "Test"]);
        std::fs::write(dir.path().join("tracked.txt"), "alpha\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "initial"]);
        dir
    }

    /// The real window with one pane of `kind` on `cwd`.
    fn root_with(
        kind: PaneKind,
    ) -> (
        RootView,
        Rc<RefCell<UiState>>,
        Arc<DesktopState>,
        tempfile::TempDir,
        AppContext,
    ) {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        );
        let app = AppContext::default();
        let view = RootView::new(&app, &desktop, None);
        let state = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.spaces = vec![Space::unnamed(Pane::Leaf { id: 7, kind })];
            s.active_space = 0;
            s.active_pane_id = 7;
        }
        (view, state, desktop, dir, app)
    }

    fn frame(root: &mut RootView, app: &AppContext) -> Vec<RenderCommand> {
        let _ = root.layout(SizeConstraint::loose(window()), &mut LayoutContext, app);
        let mut paint_ctx = PaintContext::new(Renderer::new());
        root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
        paint_ctx
            .renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    fn texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn click(root: &mut RootView, app: &AppContext, position: Vector2F) {
        let mut ctx = EventContext;
        let _ = root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position,
                button: 0,
            },
            &mut ctx,
            app,
        );
        let _ = frame(root, app);
        let _ = root.dispatch_event(
            &DispatchedEvent::MouseUp {
                position,
                button: 0,
            },
            &mut ctx,
            app,
        );
    }

    fn click_text(root: &mut RootView, app: &AppContext, text: &str) {
        let commands = frame(root, app);
        let origin = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    origin,
                    text: drawn,
                    ..
                } if drawn == text => Some(*origin),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the pane draws {text:?}: {:?}", texts(&commands)));
        click(root, app, origin + vec2f(4.0, 4.0));
    }

    fn click_icon(root: &mut RootView, app: &AppContext, name: &str) {
        let commands = frame(root, app);
        let origin = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    origin,
                    name: drawn,
                    ..
                } if drawn == name => Some(*origin),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the pane draws the {name:?} icon: {:?}", texts(&commands)));
        click(root, app, origin + vec2f(6.0, 6.0));
    }

    fn press(root: &mut RootView, app: &AppContext, key: &str) {
        let mut ctx = EventContext;
        let _ = root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: key.to_string(),
                modifiers: ModifiersState::none(),
            },
            &mut ctx,
            app,
        );
        let _ = frame(root, app);
    }

    /// The file tree and the comparison read the same selected path: a click
    /// narrows the view to that file, and Up/Down move the selection (and the
    /// comparison) without the list having to be clicked again.
    #[test]
    fn clicking_and_arrowing_select_the_file_the_comparison_shows() {
        if !git_available() {
            return;
        }
        let repo = repository();
        std::fs::write(repo.path().join("tracked.txt"), "alpha\nbeta\n").unwrap();
        std::fs::write(repo.path().join("new.txt"), "gamma\n").unwrap();
        let cwd = repo.path().to_string_lossy().to_string();
        let (mut root, state, _desktop, _dir, app) = root_with(PaneKind::Diff { cwd });

        // R: the Files Changed rail is on by default; select its row.
        state
            .borrow_mut()
            .select_diff_file(7, Some("new.txt".to_string()));
        assert_eq!(
            *state.borrow().pane_controls(7).diff_selected.borrow(),
            Some("new.txt".to_string()),
            "the row selection is the comparison's file"
        );
        let drawn = texts(&frame(&mut root, &app));
        assert!(drawn.iter().any(|text| text == "gamma"), "its diff shows");
        assert!(
            !drawn.iter().any(|text| text == "beta"),
            "the other file's diff is out of the view: {drawn:?}"
        );

        press(&mut root, &app, "ArrowDown");
        let selected = state
            .borrow()
            .pane_controls(7)
            .diff_selected
            .borrow()
            .clone();
        assert_eq!(
            selected,
            Some("tracked.txt".to_string()),
            "the arrow moves the same cell the click wrote"
        );
        assert!(
            texts(&frame(&mut root, &app))
                .iter()
                .any(|text| text == "beta"),
            "the comparison followed the keyboard"
        );
    }

    /// The toolbar toggles are real: the state flips and the bar's own label
    /// changes to the action it now offers.
    #[test]
    fn the_toolbar_switches_split_and_wrap() {
        if !git_available() {
            return;
        }
        let repo = repository();
        std::fs::write(repo.path().join("tracked.txt"), "alpha\nbeta\n").unwrap();
        let cwd = repo.path().to_string_lossy().to_string();
        let (mut root, state, _desktop, _dir, app) = root_with(PaneKind::Diff { cwd });

        click_text(&mut root, &app, "Split");
        assert!(state.borrow().diff_review(7).view.is_split());
        let drawn = texts(&frame(&mut root, &app));
        assert!(
            drawn.iter().any(|text| text == "Unified"),
            "the button now offers the other view: {drawn:?}"
        );

        click_text(&mut root, &app, "Wrap");
        assert!(state.borrow().diff_review(7).wrap);
        let drawn = texts(&frame(&mut root, &app));
        assert!(
            drawn.iter().any(|text| text == "Wrap"),
            "the shared ReviewTab keeps its wrap control on the bar: {drawn:?}"
        );
        click_text(&mut root, &app, "Wrap");
        assert!(
            !state.borrow().diff_review(7).wrap,
            "a second press switches wrapping back off"
        );
    }

    /// The file pane header's floating control runs the wired action: one file
    /// window is floated over the pane's own layout, without moving the space
    /// to the floating layout.
    #[test]
    fn the_file_pane_header_opens_the_space_as_floating() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("notes.md");
        std::fs::write(&path, "hello\n").expect("file");
        let path = path.to_string_lossy().to_string();
        let (mut root, state, _desktop, _dir, app) =
            root_with(PaneKind::File { path: path.clone() });

        let commands = frame(&mut root, &app);
        let origin = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon { origin, name, .. } if name == "fullscreen-enter" => {
                    Some(*origin)
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!("the file pane header draws its floating control: {commands:?}")
            });
        click(&mut root, &app, origin + vec2f(6.0, 6.0));

        let s = state.borrow();
        assert_eq!(
            s.spaces[s.active_space].layout,
            LayoutKind::Bsp,
            "the wired action floats one window; it does not move the whole space"
        );
        assert_eq!(
            s.spaces[s.active_space].floating.len(),
            1,
            "exactly one file window is floated over the layout"
        );
        assert!(
            s.spaces[s.active_space].floating[0].overlay,
            "the entry is the single-window overlay"
        );
        assert_eq!(
            s.spaces[s.active_space].leaf_kind(s.active_pane_id),
            Some(&PaneKind::File { path }),
            "the file pane the action opened is the active window"
        );
        let _ = _desktop;
    }
}

/// R: the narrow "Files Changed" rail on the RIGHT of the review surface. Its
/// rows are text/badge led (type letter, filename, abbreviated path), with no
/// per-row icon cluster; clicking a row selects that file in the stack. This is
/// the one file-list rail — the diff stack beside it is not a second list.
pub(crate) fn files_changed_rail(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    detail: &DiffDetail,
    selected: Option<&str>,
) -> Box<dyn Element> {
    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.0)
        .with_child(
            Container::new(
                Text::new("Files Changed")
                    .with_font_size(12.0)
                    .with_weight(FontWeight::Medium)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_padding(EdgeInsets::new(8.0, 10.0, 8.0, 10.0))
            .finish(),
        );
    // The rail's way back to the whole tree.
    {
        let on_diff = actions.on_diff_panel.clone();
        let all = HoverRow::new(
            Container::new(
                Text::new("All changes")
                    .with_font_size(12.0)
                    .with_theme_color(
                        if selected.is_none() {
                            ColorToken::Text
                        } else {
                            ColorToken::Muted
                        },
                        app,
                    )
                    .finish(),
            )
            .with_padding(EdgeInsets::new(6.0, 8.0, 6.0, 8.0))
            .with_corner_radius(6.0)
            .finish(),
        )
        .with_selected(selected.is_none())
        .with_on_click(move || (on_diff.borrow_mut())(pane_id, DiffPanelAction::Select(None)))
        .finish();
        column = column.with_child(all);
    }
    for file in &detail.files {
        let path = file.path.clone();
        let absolute = file.absolute_path.clone();
        let sel = selected == Some(file.path.as_str());
        let on_diff = actions.on_diff_panel.clone();
        let on_files = actions.on_open_file_side.clone();
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(7.0)
            .with_child(
                Text::new(type_badge(&file.path))
                    .with_font_family(FontFamily::Mono)
                    .with_font_size(10.0)
                    .with_weight(FontWeight::SemiBold)
                    .with_theme_color(badge_color(&file.path), app)
                    .finish(),
            )
            .with_child(
                Text::new(short_name(&file.path))
                    .with_font_family(FontFamily::System)
                    .with_font_size(12.0)
                    .with_weight(FontWeight::SemiBold)
                    .with_theme_color(ColorToken::Text, app)
                    .with_max_lines(1)
                    .finish(),
            )
            .with_child(
                Expanded::new(
                    Text::new(short_dir(&file.path))
                        .with_font_family(FontFamily::System)
                        .with_font_size(11.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .with_max_lines(1)
                        .finish(),
                )
                .finish(),
            )
            .finish();
        let row = HoverRow::new(
            Container::new(row)
                .with_padding(EdgeInsets::new(6.0, 8.0, 6.0, 8.0))
                .with_corner_radius(6.0)
                .finish(),
        )
        .with_selected(sel)
        .with_on_click(move || {
            (on_diff.borrow_mut())(pane_id, DiffPanelAction::Select(Some(path.clone())));
            (on_files.borrow_mut())(absolute.clone());
        })
        .finish();
        column = column.with_child(row);
    }
    ConstrainedBox::new(
        Container::new(Scrollable::new(column.finish(), Axis::Vertical).finish())
            .with_padding(EdgeInsets::new(4.0, 4.0, 4.0, 4.0))
            .finish(),
    )
    .with_width(220.0)
    .finish()
}

fn short_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn short_dir(path: &str) -> String {
    let parent = path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let parts: Vec<&str> = parent.split('/').filter(|part| !part.is_empty()).collect();
    let tail = parts[parts.len().saturating_sub(2)..].join("/");
    if tail.is_empty() {
        String::new()
    } else {
        format!("…/{tail}")
    }
}

/// The reference's file-type letter badge: h/c source, md, shell, JSON, YAML,
/// README. Other text files fall back to their extension's first letter.
pub(crate) fn type_badge(path: &str) -> String {
    let name = short_name(path);
    let lower = name.to_ascii_lowercase();
    if lower == "readme.md" {
        return "M+".to_string();
    }
    let ext = lower.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    match ext {
        "h" | "hpp" => "h".to_string(),
        "c" => "c".to_string(),
        "cc" | "cpp" => "c++".to_string(),
        "rs" => "R".to_string(),
        "ts" | "tsx" => "TS".to_string(),
        "js" | "jsx" => "JS".to_string(),
        "json" => "{}".to_string(),
        "yaml" | "yml" => "Y".to_string(),
        "sh" | "bash" | "zsh" => ">_".to_string(),
        "md" => "md".to_string(),
        other if !other.is_empty() => other.chars().take(2).collect(),
        _ => "•".to_string(),
    }
}

pub(crate) fn badge_color(path: &str) -> ColorToken {
    let ext = short_name(path)
        .to_ascii_lowercase()
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_string())
        .unwrap_or_default();
    match ext.as_str() {
        "h" | "hpp" | "c" | "cc" | "cpp" | "rs" | "ts" | "tsx" | "js" | "jsx" => ColorToken::Accent,
        "json" => ColorToken::Warning,
        "yaml" | "yml" | "sh" | "bash" | "zsh" => ColorToken::Success,
        _ => ColorToken::Muted,
    }
}
