//! The diff review pane: the working tree's changed files, their line counts
//! and their unified diffs.
//!
//! The pane is opened from the composer's diff control and belongs to one
//! repository directory (the leaf carries it, so the pane reopens on the same
//! tree). Its topbar carries the directory, branch and totals; the controls
//! that act on the tree — the show/hide-files toggle and the commit split —
//! are a second bar under it. The file list on the left selects one
//! file, and "All changes" puts every file back in the view. Each file's body
//! is its diff, whose changed rows wear the theme's insert and delete colours;
//! its header folds it, copies its path or link, asks before reverting it,
//! swaps in the whole file for editing, and opens it in the platform's editor.
//!
//! Every file is its own rounded card, laid out like warp-new's review panel:
//! the fold chevron and path, copy, the signed counts, then the link, revert and
//! open-in-editor controls. A card's body swipes sideways on its own while the
//! column around it carries the vertical scroll. Above the cards, Commit opens
//! a message bar, and the split button's menu carries Push and Create pull
//! request; those jobs run off the frame so a push never stalls the paint.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use goble_ui::color::ColorU;
use goble_ui::elements::{
    AppContext, Axis, Border, Button, ButtonVariant, ConstrainedBox, Container, CrossAxisAlignment,
    Divider, EdgeInsets, Element, Empty, Expanded, Fill, Flex, HoverRow, Icon, IconButton,
    KeyHandler, LayoutContext, MainAxisAlignment, MainAxisSize, PaintContext, Point, PopupMenu,
    PopupMenuItem, PopupMenuPosition, ReviewTab, ReviewTabFile, Scrollable, SizeConstraint, Stack,
    Text, TextArea, TextInput, Tooltip, TooltipPosition, TopbarButton,
};
use goble_ui::geometry::{rectf, vec2f, Vector2F};
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::{ColorToken, FontFamily, SpacingToken};

use super::chat;
use super::file_menu;
use super::file_view::{DiffEditor, FileKeys};
use super::pickers::{DiffDetail, DiffFileDetail};
use super::reviewer;
use super::{DiffPanelAction, UiActions, UiSnapshot};

/// Width of the file list. The panel is a full pane, so the list stays a side
/// rail; the diffs take every point left over.
const LIST_WIDTH: f32 = 248.0;
/// A file whose diff is longer than this opens folded, so a generated or
/// vendored change cannot bury every other file. The header still shows the
/// counts, and one click opens it.
const LARGE_DIFF_ROWS: usize = 600;
/// The text size of the header's own labels, and of a file's stats.
const HEADER_TEXT: f32 = 11.0;
const TOOLBAR_TEXT: f32 = 11.0;
/// The characters the topbar gives a directory and a branch before it elides
/// them: enough to name the tree on a full-width pane, short enough that a
/// narrow one keeps its controls on the same row.
const ROOT_CHARS: usize = 38;
/// The directory's cap in a split too narrow for the full path.
const NARROW_ROOT_CHARS: usize = 22;
const BRANCH_CHARS: usize = 20;
/// The pane widths at which the topbar adds the branch and the totals. The
/// directory always leads the bar; a narrow split shortens it and drops what
/// does not fit rather than crowding the pane's own controls.
const BRANCH_FROM: f32 = 320.0;
const STATS_FROM: f32 = 400.0;
/// Below this width the commit's label would crowd its chevron, so the commit
/// keeps its check and its tooltip names it.
const COMPACT_COMMIT: f32 = 280.0;
/// The fixed heights the review column is measured in. An in-place editor reads
/// the scroll region's offset against its own top (its rows are stacked with
/// every other file's in one region), so each file's top is the sum of the
/// blocks above it: the file header and whichever body it draws. These are the
/// heights those blocks are laid out at, and a test pins the editor's rows to
/// the same box the field uses.
/// DSH's changed-file card header: a 40pt file tile in a 60pt row.
const FILE_HEADER_HEIGHT: f32 = 31.0;
const FILE_TILE_SIZE: f32 = 40.0;
/// The height of the review's controls bar under the pane's topbar, and of the
/// hairline that separates the two. The bars are the pane's overlay chrome, so
/// the content column reserves their heights to start below them.
const TOOLBAR_HEIGHT: f32 = 32.0;
const TOOLBAR_SEPARATOR_HEIGHT: f32 = 1.0;
/// The clear space between two file boxes, measured into the next box's top.
const FILE_BOX_GAP: f32 = 8.0;
const LIST_PAD_TOP: f32 = 10.0;
/// How far above and below the viewport a file box is still built: enough for
/// a wheel's next frame, so scrolling into a box never shows a hole.
const VISIBLE_MARGIN: f32 = 320.0;
/// The height a review is assumed to fill on its first frame, before the
/// region has been measured. One frame later the region's own viewport wins.
const FIRST_FRAME_VIEWPORT: f32 = 760.0;
/// Files the review reads or colours in one frame. A review of many changed
/// files fills one screenful per frame instead of paying for the whole list at
/// once.
const MAX_VISIBLE_READS: usize = 4;
const DIFF_ROW_HEIGHT: f32 = 12.0 * 1.35;
const DIFF_PAD_TOP: f32 = 8.0;
const DIFF_PAD_BOTTOM: f32 = 8.0;
const EDITOR_LINE_HEIGHT: f32 = 17.0;
const EDITOR_PAD_TOP: f32 = 4.0;
const EDITOR_PAD_BOTTOM: f32 = 8.0;
const NOTE_HEIGHT: f32 = 28.0;
/// The digits the in-place editor pads its line numbers to, the same gutter the
/// file pane draws.
const GUTTER_DIGITS: usize = 5;
const EDITOR_LINE_HEIGHT_FACTOR: f32 = 1.35;

/// One horizontal offset per changed file, keyed by absolute path. A card's
/// code swipes on its own while the column keeps its vertical place.
type HorizontalOffsets = BTreeMap<String, Rc<RefCell<goble_ui::ScrollState>>>;

/// The review pane's surface: the pane's one topbar — carrying the review's
/// title, totals and controls — the file list and the diffs.
pub(crate) fn build_diff_pane(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    _cwd: &str,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let controls = state.pane_controls.get(&pane_id).cloned();
    let detail = state.pane_diffs.get(&pane_id);
    let read = detail.is_some();
    let commit_open = controls
        .as_ref()
        .is_some_and(|controls| *controls.diff_commit_open.borrow());

    // How much of the review's description fits on the topbar depends on the
    // split. The frame's build runs before its paint, so the pane's width is
    // the one recorded on the frame before; a fresh pane assumes a full one.
    let bar_width = state
        .pane_bounds_prev
        .get(&pane_id)
        .map(|bounds| bounds.width())
        .unwrap_or(1024.0);
    let compact = bar_width < COMPACT_COMMIT;

    // The pane's topbar describes the tree; the controls that act on it are a
    // second bar under it, the way warp-new stacks its review panel.
    let slots = match detail {
        Some(Some(detail)) => chat::HeaderSlots {
            leading: Some(review_title(
                app,
                actions,
                pane_id,
                detail,
                controls
                    .as_ref()
                    .and_then(|controls| controls.diff_selected.borrow().clone())
                    .as_deref(),
                bar_width,
            )),
            ..chat::HeaderSlots::default()
        },
        _ => chat::HeaderSlots::default(),
    };
    let header = chat::build_agent_header_with(app, state, actions, pane_id, None, slots);

    // The second bar is painted with the header, above the cards: its commit
    // menu opens down over the file list, and a menu painted with the content
    // would be covered by the cards under it.
    let toolbar = match (detail, controls.as_ref()) {
        (Some(Some(detail)), Some(controls)) => {
            let view = crate::state::review_for(&actions.review_views, pane_id);
            let selected = controls.diff_selected.borrow().clone();
            let expanded = controls.diff_expanded.borrow().clone();
            Some(review_controls(
                app,
                actions,
                pane_id,
                detail,
                selected.as_deref(),
                &expanded,
                &view,
                *controls.diff_sidebar.borrow(),
                compact,
                &CommitView {
                    open: commit_open,
                    menu_open: controls.diff_commit_menu_open.clone(),
                    status: controls.diff_git_status.borrow().clone(),
                },
            ))
        }
        _ => None,
    };

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            Empty::new()
                .with_size(Vector2F::new(0.0, super::shell::TOPBAR_HEIGHT))
                .finish(),
        );
    if toolbar.is_some() {
        column = column.with_child(
            Empty::new()
                .with_size(Vector2F::new(
                    0.0,
                    TOOLBAR_HEIGHT + TOOLBAR_SEPARATOR_HEIGHT,
                ))
                .finish(),
        );
    }

    if let (Some(Some(detail)), Some(controls)) = (detail, controls) {
        let selected = controls.diff_selected.borrow().clone();
        let expanded = controls.diff_expanded.borrow().clone();
        let editing = controls.diff_editing.borrow().clone();
        let focus = controls.diff_edit_focus.clone();
        let editors = state.pane_diff_editors.get(&pane_id);
        let hscroll = controls.diff_hscroll.clone();
        let visible = controls.diff_visible.clone();
        let sidebar = *controls.diff_sidebar.borrow();
        if commit_open {
            column = column.with_child(commit_bar(
                app,
                actions,
                pane_id,
                controls.diff_commit_message.clone(),
                controls.diff_commit_focus.clone(),
                controls.diff_git_status.clone(),
            ));
        }
        column = column.with_child(
            Expanded::new(
                Container::new(body(
                    app,
                    actions,
                    pane_id,
                    detail,
                    selected,
                    expanded,
                    editing,
                    focus,
                    editors,
                    hscroll,
                    visible,
                    sidebar,
                    state.active_pane_id == pane_id,
                    controls.diff_scroll.clone(),
                    controls.diff_list_scroll.clone(),
                    controls.diff_revert_confirm.clone(),
                    controls.diff_commit_focus.clone(),
                    (state.window_size.x.max(1200.0) * 0.55).max(300.0),
                    0.0,
                ))
                .finish(),
            )
            .finish(),
        );
    } else {
        // The first frame before the snapshot carries a read, or a directory
        // that is no longer inside a work tree.
        let message = if read {
            "No working tree here"
        } else {
            "Reading the changes…"
        };
        column = column.with_child(
            Expanded::new(
                Container::new(empty_state(app, message))
                    .with_padding(EdgeInsets::new(0.0, 0.0, 0.0, 0.0))
                    .finish(),
            )
            .finish(),
        );
    }

    let content = Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .finish();
    let mut chrome = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header);
    if let Some(toolbar) = toolbar {
        // A hairline on the seam: the bars sit flush, so the content column
        // reserves its point along with the bar's own height.
        chrome = chrome
            .with_child(Divider::horizontal().finish())
            .with_child(toolbar);
    }
    Stack::new()
        .with_children(vec![content])
        .with_overlay(chrome.finish(), Vector2F::zero())
        .finish()
}

/// Q: the review surface as a side-panel tab. It draws the ReviewTab header
/// (changed-file selector, counts, split/wrap) and the diff body — with its one
/// file-list rail — but no pane topbar, because the panel's tab strip already
/// owns that row. The data and controls are the active pane's, so Review needs
/// no split pane and no second file column.
pub(crate) fn build_review_tab(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let controls = state.pane_controls.get(&pane_id).cloned();
    let detail = state.pane_diffs.get(&pane_id);
    let read = detail.is_some();
    let commit_open = controls
        .as_ref()
        .is_some_and(|controls| *controls.diff_commit_open.borrow());
    // A narrow panel shows the diff alone: the ReviewTab selector in the
    // header still chooses files, and the one rail is the wide-layout list.
    // The side panel is the trailing share of the content area the sidebar
    // leaves, not of the whole window. Using the window width here made the
    // rail too wide and squeezed the file list.
    let area_width = (state.window_size.x
        - if state.sidebar_visible {
            state.sidebar_width
        } else {
            0.0
        })
    .max(240.0);
    let panel_width = if state.side_panel_maximized {
        area_width.max(900.0)
    } else {
        (area_width
            * crate::ui::build::resolved_side_split_ratio(state.side_split_ratio, area_width))
        .max(240.0)
    };
    let show_rail = panel_width >= 520.0;
    let compact = panel_width < 900.0;

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(build_review_topbar(app, state, actions, pane_id))
        .with_child(Divider::horizontal().finish());
    if let (Some(Some(detail)), Some(controls)) = (detail, controls.as_ref()) {
        if commit_open {
            column = column.with_child(commit_bar(
                app,
                actions,
                pane_id,
                controls.diff_commit_message.clone(),
                controls.diff_commit_focus.clone(),
                controls.diff_git_status.clone(),
            ));
        }
        let selected = controls.diff_selected.borrow().clone();
        let expanded = controls.diff_expanded.borrow().clone();
        let rail_width = if show_rail {
            (panel_width * 0.40).clamp(200.0, 420.0)
        } else {
            0.0
        };
        let editing = controls.diff_editing.borrow().clone();
        let editors = state.pane_diff_editors.get(&pane_id);
        column = column.with_child(
            Expanded::new(
                Container::new(body(
                    app,
                    actions,
                    pane_id,
                    detail,
                    selected,
                    expanded,
                    editing,
                    controls.diff_edit_focus.clone(),
                    editors,
                    controls.diff_hscroll.clone(),
                    controls.diff_visible.clone(),
                    show_rail,
                    true,
                    controls.diff_scroll.clone(),
                    controls.diff_list_scroll.clone(),
                    controls.diff_revert_confirm.clone(),
                    controls.diff_commit_focus.clone(),
                    (panel_width - rail_width - if show_rail { 1.0 } else { 0.0 }).max(200.0),
                    rail_width,
                ))
                .finish(),
            )
            .finish(),
        );
    } else {
        let message = if read {
            "No working tree here"
        } else {
            "Reading the changes…"
        };
        column = column.with_child(
            Expanded::new(
                Container::new(empty_state(app, message))
                    .with_padding(EdgeInsets::uniform(sm))
                    .finish(),
            )
            .finish(),
        );
    }
    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .finish()
}

/// The commit bar and split button as one frame sees them. The open flag and
/// the status are copies; the menu flag is the pane's own, because the popup
/// toggles it when a press lands outside the panel.
struct CommitView {
    open: bool,
    menu_open: Rc<RefCell<bool>>,
    status: Option<(String, bool)>,
}

/// 223: the review's own 1 pt-separated icon row, directly under the general
/// topbar. The general bar owns only the chips and window controls.
fn build_review_topbar(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
) -> Box<dyn Element> {
    let on_diff = actions.on_diff_panel.clone();
    let toggle_files = {
        let on_diff = on_diff.clone();
        move || (on_diff.borrow_mut())(pane_id, DiffPanelAction::ToggleSidebar)
    };
    let on_search = actions.on_spotlight.clone();
    let search = Tooltip::new(
        TopbarButton::new(
            Icon::new("search")
                .with_size(14.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_size(26.0)
        .with_corner_radius(7.0)
        .with_on_click(move || {
            (on_search.borrow_mut())(crate::ui::spotlight::SpotlightAction::Open)
        })
        .finish(),
        "Search",
    )
    .with_position(TooltipPosition::Below)
    .finish();
    let sidebar = Tooltip::new(
        TopbarButton::new(
            Icon::new("panel-left-outline")
                .with_size(14.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_size(26.0)
        .with_corner_radius(7.0)
        .with_on_click(toggle_files)
        .finish(),
        "Show/hide files",
    )
    .with_position(TooltipPosition::Below)
    .finish();
    let detail = state
        .pane_diffs
        .get(&pane_id)
        .and_then(|detail| detail.as_ref());
    let files = detail.map(|detail| detail.files.len()).unwrap_or(0);
    if files == 0 {
        // 595: empty review keeps only the magnifier and the sidebar toggle.
        return ConstrainedBox::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(2.0)
                .with_child(Expanded::new(Empty::new().finish()).finish())
                .with_child(search)
                .with_child(sidebar)
                .finish(),
        )
        .with_height(32.0)
        .finish();
    }
    let added: u64 = detail.map(|detail| detail.added()).unwrap_or(0);
    let removed: u64 = detail.map(|detail| detail.removed()).unwrap_or(0);
    let pill_menu = PopupMenu::new(
        HoverRow::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(4.0)
                .with_child(
                    Text::new("Uncommitted")
                        .with_font_size(12.0)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .with_child(
                    Icon::new("chevron-down")
                        .with_size(11.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .finish(),
        )
        .with_padding(EdgeInsets::new(7.0, 3.0, 7.0, 3.0))
        .with_corner_radius(7.0)
        .finish(),
        vec![
            PopupMenuItem::new("Uncommitted")
                .with_description("All working tree changes")
                .selected(),
            PopupMenuItem::new("Branch").with_description("Changes on this branch"),
            PopupMenuItem::new("Agent edit").with_description("Edits by the agent"),
        ],
    )
    .with_open(state.review_uncommitted_open.clone())
    .with_position(PopupMenuPosition::Below)
    .with_on_select(|_| {})
    .finish();
    let counts = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(5.0)
        .with_child(
            Text::new(format!("+{added}"))
                .with_font_family(FontFamily::Mono)
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Success, app)
                .finish(),
        )
        .with_child(
            Text::new(format!("−{removed}"))
                .with_font_family(FontFamily::Mono)
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Error, app)
                .finish(),
        )
        .finish();
    let commit = {
        let on_diff = on_diff.clone();
        Button::new(
            Text::new("Commit")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_variant(ButtonVariant::Primary)
        .with_on_click(move || (on_diff.borrow_mut())(pane_id, DiffPanelAction::ToggleCommit))
        .finish()
    };
    let commit_menu = PopupMenu::new(
        TopbarButton::new(
            Icon::new("chevron-down")
                .with_size(11.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_size(22.0)
        .with_corner_radius(6.0)
        .finish(),
        vec![
            PopupMenuItem::new("Amend").with_description("Amend the last commit"),
            PopupMenuItem::new("Push").with_description("Push the current branch"),
        ],
    )
    .with_open(state.review_commit_open.clone())
    .with_position(PopupMenuPosition::Below)
    .with_on_select(|_| {})
    .finish();
    let split = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(0.0)
        .with_child(commit)
        .with_child(
            ConstrainedBox::new(
                Container::new(Empty::new().finish())
                    .with_background(Fill::Solid(app.theme.color(ColorToken::Border)))
                    .finish(),
            )
            .with_width(1.0)
            .with_height(18.0)
            .finish(),
        )
        .with_child(commit_menu)
        .finish();
    let on_diff_kebab = on_diff.clone();
    let kebab = Tooltip::new(
        TopbarButton::new(
            Icon::new("dots-vertical")
                .with_size(14.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_size(26.0)
        .with_corner_radius(7.0)
        .with_on_click(move || {
            (on_diff_kebab.borrow_mut())(pane_id, DiffPanelAction::ToggleSidebar)
        })
        .finish(),
        "Review options",
    )
    .with_position(TooltipPosition::Below)
    .finish();
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(pill_menu)
        .with_child(counts)
        .with_child(Expanded::new(Empty::new().finish()).finish())
        .with_child(split)
        .with_child(kebab)
        .with_child(search)
        .with_child(sidebar)
        .finish();
    ConstrainedBox::new(row)
        .with_height(crate::ui::chrome::CHROME_HEIGHT)
        .finish()
}

/// The topbar's leading content: the directory this review covers, its branch
/// and its totals, in the order that fits the pane's current width.
fn review_title(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    detail: &DiffDetail,
    selected: Option<&str>,
    width: f32,
) -> Box<dyn Element> {
    let root = crate::state::display_path(&detail.root);
    let mut title = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0);
    // DSH's review header leads with the file selector: every changed file is
    // one press away, and the selected one's name is what the header says.
    if !detail.files.is_empty() {
        let view = crate::state::review_for(&actions.review_views, pane_id);
        title = title.with_child(reviewer::file_selector(
            app, actions, pane_id, detail, selected, &view,
        ));
    }
    if !root.is_empty() {
        let chars = if width < BRANCH_FROM {
            NARROW_ROOT_CHARS
        } else {
            ROOT_CHARS
        };
        title = title.with_child(
            Text::new(shortened(&root, chars, true))
                .with_font_size(HEADER_TEXT)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(1)
                .finish(),
        );
    }
    if !detail.branch.is_empty() && width >= BRANCH_FROM {
        title = title.with_child(
            Text::new(shortened(&detail.branch, BRANCH_CHARS, false))
                .with_font_size(HEADER_TEXT)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(1)
                .finish(),
        );
    }
    if width >= STATS_FROM {
        title = title.with_child(stats(
            app,
            detail.files_changed(),
            detail.added(),
            detail.removed(),
        ));
    }
    title.finish()
}

/// The review's second topbar: the file-list toggle at the leading edge and
/// the commit split at the trailing one, with room between them for the last
/// git job's word.
#[allow(clippy::too_many_arguments)]
fn review_controls(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    detail: &DiffDetail,
    selected: Option<&str>,
    expanded: &BTreeMap<String, bool>,
    view: &crate::state::DiffReviewState,
    sidebar: bool,
    compact: bool,
    commit: &CommitView,
) -> Box<dyn Element> {
    // The list toggle is the pane's one icon for the rail, the way warp-new's
    // is: the tooltip names it, so the bar stays a row of controls rather than
    // a sentence.
    let on_diff = actions.on_diff_panel.clone();
    let sidebar_tip = if sidebar { "Hide files" } else { "Show files" };
    let sidebar_button = icon_button(app, "layers-three-01", sidebar_tip, move || {
        (on_diff.borrow_mut())(pane_id, DiffPanelAction::ToggleSidebar);
    });

    // A finished job's word sits beside the button that started it: a commit
    // closes the bar, so the topbar is where its outcome has to be read.
    let mut trailing = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0);
    if !commit.open {
        if let Some((text, ok)) = commit.status.as_ref() {
            trailing = trailing.with_child(status_label(app, text, *ok));
        }
    }
    trailing = trailing.with_child(commit_split(
        app,
        actions,
        pane_id,
        commit.menu_open.clone(),
        compact,
    ));

    // DSH's own review-tab header: the changed-file selector, the selected
    // file's counts and the split/unified and wrap choices, composed from the
    // shared goble-ui element rather than a second ad-hoc copy of the row.
    let review_tab = review_tab_header(app, actions, pane_id, detail, selected, view);
    let all_open = detail.files.iter().all(|file| file_open(file, expanded));
    let on_fold = actions.on_diff_panel.clone();
    let fold_paths: Vec<String> = detail.files.iter().map(|file| file.path.clone()).collect();
    let fold_button = text_button(
        app,
        if all_open { "Fold all" } else { "Unfold all" },
        move || {
            for path in &fold_paths {
                (on_fold.borrow_mut())(
                    pane_id,
                    DiffPanelAction::SetFileOpen(path.clone(), !all_open),
                );
            }
        },
    );
    let _ = selected;
    let bar = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(sidebar_button)
        .with_child(Expanded::new(review_tab).finish())
        .with_child(fold_button)
        .with_child(trailing.finish())
        .finish();
    // The row under the pane's topbar belongs to the panel's own surface, so
    // it keeps the panel's black rather than the bar gray above it.
    ConstrainedBox::new(
        Container::new(bar)
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .with_padding(EdgeInsets::new(6.0, 0.0, 6.0, 0.0))
            .finish(),
    )
    .with_height(TOOLBAR_HEIGHT)
    .finish()
}

/// The review tab's header row, built from the shared goble-ui element: its
/// selector names the selected changed file, its counts are that file's, and
/// its split/wrap controls report through the same pane actions the rest of
/// the review uses.
fn review_tab_header(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    detail: &DiffDetail,
    selected: Option<&str>,
    view: &crate::state::DiffReviewState,
) -> Box<dyn Element> {
    let files: Vec<ReviewTabFile> = detail
        .files
        .iter()
        .map(|file| ReviewTabFile {
            path: file.path.clone(),
            added: u64::from(file.added),
            removed: u64::from(file.removed),
            status: reviewer::status_of(file)
                .letter()
                .chars()
                .next()
                .unwrap_or('M'),
        })
        .collect();
    let paths: Vec<String> = detail.files.iter().map(|file| file.path.clone()).collect();
    let absolute: Vec<String> = detail
        .files
        .iter()
        .map(|file| file.absolute_path.clone())
        .collect();
    let current = selected
        .and_then(|selected| detail.files.iter().position(|file| file.path == selected))
        .unwrap_or(0);
    let selected_absolute = absolute.get(current).cloned();
    let all_absolute = absolute.clone();

    let on_select = actions.on_diff_panel.clone();
    let select_paths = paths.clone();
    let on_split = actions.on_diff_panel.clone();
    let on_wrap = actions.on_diff_panel.clone();
    let on_open = actions.on_diff_panel.clone();
    let on_other = actions.on_diff_panel.clone();
    let open_path = selected_absolute.clone();
    let copy_paths = selected_absolute
        .map(|path| vec![path])
        .unwrap_or_else(|| all_absolute);

    ReviewTab::new(files)
        .with_selected(current)
        .with_split(view.view.is_split())
        .with_wrap(view.wrap)
        .with_selector_open(view.selector_open.clone())
        .with_selector_highlight(view.selector_highlight.clone())
        .with_on_select(move |index| {
            if let Some(path) = select_paths.get(index) {
                (on_select.borrow_mut())(pane_id, DiffPanelAction::Select(Some(path.clone())));
            }
        })
        .with_on_toggle_split(move || {
            (on_split.borrow_mut())(pane_id, DiffPanelAction::ToggleView);
        })
        .with_on_toggle_wrap(move || {
            (on_wrap.borrow_mut())(pane_id, DiffPanelAction::ToggleWrap);
        })
        .with_on_open_file(move || {
            if let Some(path) = open_path.clone() {
                (on_open.borrow_mut())(pane_id, DiffPanelAction::OpenFile(path));
            }
        })
        .with_on_other(move || {
            (on_other.borrow_mut())(pane_id, DiffPanelAction::CopyPaths(copy_paths.clone()));
        })
        .finish()
}

/// A label cut to at most `max` characters, ending in an ellipsis when it was
/// longer. A path keeps its tail — the last components are what name it — and
/// anything else keeps its head, so a long directory or branch never widens the
/// topbar past its row.
fn shortened(text: &str, max: usize, keep_tail: bool) -> String {
    let chars = text.chars().count();
    if chars <= max || max < 2 {
        return text.to_string();
    }
    if keep_tail {
        let tail: String = text.chars().skip(chars - (max - 1)).collect();
        format!("…{tail}")
    } else {
        let head: String = text.chars().take(max - 1).collect();
        format!("{head}…")
    }
}

/// A path label that keeps inside the box it is given and fades into the card's
/// surface at its right edge. The header's copy control follows the label, so
/// the label's box is exactly the room that control leaves: a deep path is cut
/// there and the fade says there is more of it.
struct PathLabel {
    text: String,
    font_size: f32,
    color: ColorU,
    fade: ColorU,
    natural: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

const PATH_FONT_SIZE: f32 = 13.0;
const PATH_LINE_HEIGHT: f32 = 1.2;
const PATH_FADE_WIDTH: f32 = 20.0;

impl PathLabel {
    fn new(text: String, color: ColorU, fade: ColorU) -> Self {
        Self {
            text,
            font_size: PATH_FONT_SIZE,
            color,
            fade,
            natural: 0.0,
            size: None,
            origin: None,
        }
    }
}

impl Element for PathLabel {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        self.natural = goble_ui::elements::text::measure_text(
            &self.text,
            self.font_size,
            PATH_LINE_HEIGHT,
            f32::INFINITY,
        )
        .x;
        let width = self
            .natural
            .max(constraint.min.x)
            .min(constraint.max.x)
            .max(0.0);
        let height = (self.font_size * PATH_LINE_HEIGHT)
            .max(constraint.min.y)
            .min(constraint.max.y);
        let size = vec2f(width, height);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        renderer.clip_rect(rectf(origin.x, origin.y, size.x, size.y));
        renderer.draw_text_with_font(
            origin,
            self.text.clone(),
            self.font_size,
            self.color,
            f32::INFINITY,
            PATH_LINE_HEIGHT,
            FontWeight::Medium,
            FontFamily::System,
            false,
        );
        renderer.pop_clip();
        if self.natural > size.x {
            let width = PATH_FADE_WIDTH.min(size.x);
            renderer.fill_rect_fade_right(
                rectf(origin.x + size.x - width, origin.y, width, size.y),
                self.fade,
                0.0,
            );
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

/// The primary Commit button and its menu: push and the pull request. The menu
/// is the split button's own chevron beside the primary, so the commit stays
/// one press and the two jobs that reach the network sit behind it.
fn commit_split(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    open: Rc<RefCell<bool>>,
    compact: bool,
) -> Box<dyn Element> {
    let trigger = Button::new(
        Icon::new("chevron-down")
            .with_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_variant(ButtonVariant::Default)
    .with_corner_radius(0.0)
    .finish();
    let items = vec![
        PopupMenuItem::new("Push").with_icon("arrow-up"),
        PopupMenuItem::new("Create pull request").with_icon("git-branch"),
    ];
    let on_select = actions.on_diff_panel.clone();
    let menu = PopupMenu::new(trigger, items)
        .with_open(open)
        .with_position(PopupMenuPosition::BelowEnd)
        .with_corner_radius(0.0)
        .with_on_select(move |index| {
            let action = match index {
                0 => DiffPanelAction::Push,
                1 => DiffPanelAction::CreatePr,
                _ => return,
            };
            (on_select.borrow_mut())(pane_id, action);
        })
        .finish();
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(2.0)
        .with_child(commit_button(app, actions, pane_id, compact))
        .with_child(menu)
        .finish()
}

/// The primary Commit control: the check, its label and the accent fill, square
/// like the rest of the review. The toolbar and the commit bar both draw it, so
/// a commit reads the same in either place. A compact toolbar drops the word
/// and keeps the check, with the tooltip naming it.
fn commit_button(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    compact: bool,
) -> Box<dyn Element> {
    let mut label = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(
            Icon::new("check")
                .with_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        );
    if !compact {
        label = label.with_child(
            Text::new("Commit")
                .with_font_size(TOOLBAR_TEXT)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
        );
    }
    let on_commit = actions.on_diff_panel.clone();
    let button = Button::new(label.finish())
        .with_variant(ButtonVariant::Primary)
        .with_corner_radius(0.0)
        .with_on_click(move || (on_commit.borrow_mut())(pane_id, DiffPanelAction::Commit))
        .finish();
    if compact {
        Tooltip::new(button, "Commit")
            .with_position(TooltipPosition::Above)
            .finish()
    } else {
        button
    }
}

/// The bar under the topbar: the message the commit and its split button's
/// jobs act on, the commit itself, and the last job's word.
fn commit_bar(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    message: Rc<RefCell<String>>,
    focus: Rc<RefCell<bool>>,
    status: Rc<RefCell<Option<(String, bool)>>>,
) -> Box<dyn Element> {
    let on_change = actions.on_diff_panel.clone();
    let on_submit = actions.on_diff_panel.clone();
    let on_focus = Rc::clone(&focus);
    let field = TextInput::new()
        .with_value(message.borrow().clone())
        .with_placeholder("Commit message")
        // The bar opens on its field; a press in the diff body takes the
        // keyboard away, so editing a file never types into the message too.
        .with_focused(*focus.borrow())
        .with_on_change(move |text| {
            (on_change.borrow_mut())(pane_id, DiffPanelAction::SetCommitMessage(text));
        })
        .with_on_focus_change(move |gained| *on_focus.borrow_mut() = gained)
        .with_on_submit(move || (on_submit.borrow_mut())(pane_id, DiffPanelAction::Commit))
        .finish();
    // The bar's commit is the topbar's own control, so the two read the same.
    let commit = commit_button(app, actions, pane_id, false);
    let close = icon_button(app, "close", "Close commit", {
        let on_close = actions.on_diff_panel.clone();
        move || (on_close.borrow_mut())(pane_id, DiffPanelAction::ToggleCommit)
    });

    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(Expanded::new(field).finish())
        .with_child(commit)
        .with_child(close);
    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(6.0)
        .with_child(row.finish());
    if let Some((text, ok)) = status.borrow().clone() {
        column = column.with_child(status_label(app, &text, ok));
    }
    Container::new(column.finish())
        .with_padding(EdgeInsets::new(0.0, 8.0, 8.0, 10.0))
        .finish()
}

/// A git job's last word, in the colour of its outcome. It stays one line and
/// takes at most the width of a menu, so a long git error cannot push the
/// topbar's own controls off the pane.
fn status_label(app: &AppContext, text: &str, ok: bool) -> Box<dyn Element> {
    let color = if ok {
        ColorToken::Success
    } else {
        ColorToken::Error
    };
    ConstrainedBox::new(
        Text::new(text)
            .with_font_size(10.0)
            .with_theme_color(color, app)
            .with_max_lines(1)
            .finish(),
    )
    .with_max_width(260.0)
    .finish()
}

/// The file list and the diffs side by side.
#[allow(clippy::too_many_arguments)]
fn body(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    detail: &DiffDetail,
    selected: Option<String>,
    expanded: BTreeMap<String, bool>,
    editing: BTreeMap<String, bool>,
    focus: Rc<RefCell<Option<String>>>,
    editors: Option<&std::collections::HashMap<String, Rc<DiffEditor>>>,
    hscroll: Rc<RefCell<HorizontalOffsets>>,
    visible: Rc<RefCell<Vec<String>>>,
    sidebar: bool,
    active: bool,
    scroll: Rc<RefCell<goble_ui::ScrollState>>,
    list_scroll: Rc<RefCell<goble_ui::ScrollState>>,
    revert: Rc<RefCell<Option<String>>>,
    commit_focus: Rc<RefCell<bool>>,
    content_width: f32,
    rail_width: f32,
) -> Box<dyn Element> {
    if detail.files.is_empty() {
        return empty_state(app, "No open changes");
    }
    // A selected file the working tree no longer has leaves every file in the
    // view rather than an empty pane.
    let selected = selected.filter(|path| detail.files.iter().any(|file| file.path == *path));
    let selected_path = selected.clone();

    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    let view = crate::state::review_for(&actions.review_views, pane_id);
    // R: the stacked diff is the left column; the single file-list rail is the
    // narrow "Files Changed" column on the RIGHT. The diff column is not a
    // second file list — each file opens inline under its own header.
    let keyboard_expanded = expanded.clone();
    let edit_focus = Rc::clone(&focus);
    row = row.with_child(
        Expanded::new(diff_list(
            app,
            actions,
            pane_id,
            detail,
            selected,
            expanded,
            editing,
            focus,
            editors,
            hscroll,
            visible,
            active,
            scroll,
            revert,
            &view,
            content_width,
        ))
        .finish(),
    );
    if sidebar {
        let _ = list_scroll;
        row = row
            .with_child(divider(app))
            .with_child(reviewer::files_changed_rail(
                app,
                actions,
                pane_id,
                detail,
                selected_path.as_deref(),
                rail_width,
            ));
    }
    let row = row.finish();
    if !active {
        return row;
    }
    // The review owns Up/Down/Enter for its file list while the pane is the
    // active one. A focused commit field, an in-place editor or the open file
    // selector keep their keys: those surfaces are where the caret is.
    let files: Vec<String> = detail.files.iter().map(|file| file.path.clone()).collect();
    let open_state: Vec<(String, bool)> = detail
        .files
        .iter()
        .map(|file| (file.path.clone(), file_open(file, &keyboard_expanded)))
        .collect();
    let selector_open = view.selector_open.clone();
    let on_diff = actions.on_diff_panel.clone();
    Box::new(KeyHandler::new(row, move |key: &str, modifiers| {
        if modifiers.command || modifiers.ctrl || modifiers.alt {
            return false;
        }
        if *commit_focus.borrow() || edit_focus.borrow().is_some() || *selector_open.borrow() {
            return false;
        }
        match key {
            "ArrowDown" | "ArrowUp" => {
                if files.is_empty() {
                    return true;
                }
                let current = selected_path
                    .as_ref()
                    .and_then(|path| files.iter().position(|file| file == path));
                let next = match (current, key) {
                    (Some(index), "ArrowDown") => (index + 1).min(files.len() - 1),
                    (Some(index), "ArrowUp") => index.saturating_sub(1),
                    (None, "ArrowDown") => 0,
                    (None, _) => files.len() - 1,
                    _ => return true,
                };
                (on_diff.borrow_mut())(pane_id, DiffPanelAction::Select(Some(files[next].clone())));
                true
            }
            "Enter" => {
                let Some(path) = selected_path.clone() else {
                    return true;
                };
                let open = open_state
                    .iter()
                    .find(|(file, _)| *file == path)
                    .map(|(_, open)| *open)
                    .unwrap_or(false);
                (on_diff.borrow_mut())(pane_id, DiffPanelAction::SetFileOpen(path, !open));
                true
            }
            _ => false,
        }
    }))
}

/// Every changed file, folded or open and — for the files opened for editing —
/// its whole text in place, in one scrollable column. A selected file narrows
/// the column to that one.
///
/// The column is one document: each file's body is stacked directly under the
/// previous one, and an editor reads the region's offset against its own top,
/// so a wheel at the end of a file carries on into the next.
#[allow(clippy::too_many_arguments)]
fn diff_list(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    detail: &DiffDetail,
    selected: Option<String>,
    expanded: BTreeMap<String, bool>,
    editing: BTreeMap<String, bool>,
    focus: Rc<RefCell<Option<String>>>,
    editors: Option<&std::collections::HashMap<String, Rc<DiffEditor>>>,
    hscroll: Rc<RefCell<HorizontalOffsets>>,
    visible: Rc<RefCell<Vec<String>>>,
    active: bool,
    scroll: Rc<RefCell<goble_ui::ScrollState>>,
    revert: Rc<RefCell<Option<String>>>,
    view: &crate::state::DiffReviewState,
    content_width: f32,
) -> Box<dyn Element> {
    let (offset, viewport) = {
        let scroll = scroll.borrow();
        (scroll.offset(), scroll.viewport())
    };
    let viewport = if viewport > 0.0 {
        viewport
    } else {
        FIRST_FRAME_VIEWPORT
    };
    let window_start = (offset - VISIBLE_MARGIN).max(0.0);
    let window_end = offset + viewport + VISIBLE_MARGIN;
    let mut shown: Vec<String> = Vec::new();
    // The list is as tall as its boxes: a `Max` axis would take the scroll
    // region's unbounded height and report an infinite content size, which
    // leaves the region without a bottom to clamp against.
    let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    let mut top = LIST_PAD_TOP;
    let mut skipped = 0.0f32;
    for file in &detail.files {
        let open = file_open(file, &expanded);
        let is_editing = file_shows_editor(file, &editing);
        let editor = editors.and_then(|editors| editors.get(&file.absolute_path));
        let height =
            FILE_HEADER_HEIGHT + body_height(file, open, is_editing, editor) + FILE_BOX_GAP;
        let bottom = top + height;
        if bottom <= window_start || top >= window_end {
            skipped += height;
            top = bottom;
            continue;
        }
        if skipped > 0.0 {
            column = column.with_child(vertical_spacer(skipped));
            skipped = 0.0;
        }
        if !file.binary && !file.truncated && shown.len() < MAX_VISIBLE_READS {
            shown.push(file.absolute_path.clone());
        }
        // An editor's first row sits under the file header and the body's top
        // padding; that is the top it reads the shared offset against.
        let editor_top = top + FILE_HEADER_HEIGHT + EDITOR_PAD_TOP;
        column = column.with_child(file_block(
            app,
            actions,
            pane_id,
            file,
            open,
            is_editing,
            editor,
            Rc::clone(&scroll),
            Rc::clone(&hscroll),
            Rc::clone(&focus),
            active,
            editor_top,
            revert.borrow().as_deref() == Some(file.path.as_str()),
            view,
            content_width,
            selected.as_deref() == Some(file.path.as_str()),
        ));
        top = bottom;
    }
    if skipped > 0.0 {
        column = column.with_child(vertical_spacer(skipped));
    }
    *visible.borrow_mut() = shown;
    Scrollable::new(
        Container::new(column.finish())
            .with_padding(EdgeInsets::new(0.0, LIST_PAD_TOP, 0.0, 0.0))
            .finish(),
        Axis::Vertical,
    )
    .with_state(scroll)
    .finish()
}

/// Whether a file's body is its in-place editor. The diff is the body by
/// default — the added and removed rows are what the pane is for — and the
/// header's toggle swaps the whole file in. A binary or truncated file has no
/// text to edit and never shows one.
///
/// The frame's read asks this too (`root_view::rebuild`), so what is read and
/// what is painted can never name different bodies.
pub(crate) fn file_shows_editor(file: &DiffFileDetail, editing: &BTreeMap<String, bool>) -> bool {
    if file.binary || file.truncated {
        return false;
    }
    editing.get(&file.path).copied().unwrap_or(false)
}

/// A run of file boxes the viewport skips, at exactly the height their headers
/// and bodies would take.
fn vertical_spacer(height: f32) -> Box<dyn Element> {
    Empty::new().with_size(Vector2F::new(0.0, height)).finish()
}

/// Whether a file's diff is drawn. A short diff opens by default; a long one
/// stays folded until the user opens it or asks for every file.
fn file_open(file: &DiffFileDetail, expanded: &BTreeMap<String, bool>) -> bool {
    expanded.get(&file.path).copied().unwrap_or_else(|| {
        !file.binary && !file.truncated && file.prepared.rows() <= LARGE_DIFF_ROWS
    })
}

/// The height of one file's body, which is what places the next file's editor
/// in the shared scroll region. It mirrors exactly what [`file_body`] lays out:
/// an editor is one row per line of its buffer, a diff one row per prepared
/// row, and every note is the one pinned height.
fn body_height(
    file: &DiffFileDetail,
    open: bool,
    editing: bool,
    editor: Option<&Rc<DiffEditor>>,
) -> f32 {
    if !open {
        return 0.0;
    }
    if editing {
        let Some(editor) = editor else {
            return NOTE_HEIGHT;
        };
        if !editor.buffer.borrow().editable() {
            return NOTE_HEIGHT;
        }
        let lines = editor.buffer.borrow().lines();
        return EDITOR_PAD_TOP + EDITOR_PAD_BOTTOM + lines as f32 * EDITOR_LINE_HEIGHT;
    }
    if file.binary || file.truncated || file.hunks.is_empty() {
        return NOTE_HEIGHT;
    }
    DIFF_PAD_TOP + DIFF_PAD_BOTTOM + file.prepared.rows() as f32 * DIFF_ROW_HEIGHT
}

#[allow(clippy::too_many_arguments)]
fn file_block(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    file: &DiffFileDetail,
    open: bool,
    editing: bool,
    editor: Option<&Rc<DiffEditor>>,
    scroll: Rc<RefCell<goble_ui::ScrollState>>,
    hscroll: Rc<RefCell<HorizontalOffsets>>,
    focus: Rc<RefCell<Option<String>>>,
    active: bool,
    editor_top: f32,
    confirming_revert: bool,
    view: &crate::state::DiffReviewState,
    content_width: f32,
    selected: bool,
) -> Box<dyn Element> {
    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(open_file_tab_on_hover(
            file_header(
                app,
                actions,
                pane_id,
                file,
                open,
                editing,
                editor,
                confirming_revert,
                content_width,
                selected,
            ),
            actions.on_diff_panel.clone(),
            pane_id,
            file.path.clone(),
        ))
        // 225: every filename row is followed by a 1 px rule at the card width.
        .with_child(Divider::horizontal().finish());
    if open {
        column = column.with_child(file_body(
            app,
            actions,
            pane_id,
            file,
            editing,
            editor,
            scroll,
            hscroll,
            focus,
            active,
            editor_top,
            view,
            content_width,
        ));
        column = column.with_child(Divider::horizontal().finish());
    }
    // 252: the expanded diff sits directly in the flat list — no card, border,
    // radius or inset around the row.
    let block = column.finish();
    let row: Box<dyn Element> = if content_width > 0.0 {
        ConstrainedBox::new(block)
            .with_width(content_width)
            .finish()
    } else {
        block
    };
    // The whole row is still the file's context-menu target.
    let on_file_menu = actions.on_file_menu.clone();
    let menu_path = file.absolute_path.clone();
    let menu_copy_path = file.path.clone();
    file_menu::context_click(
        row,
        Rc::new(RefCell::new(move |at| {
            (on_file_menu.borrow_mut())(menu_path.clone(), menu_copy_path.clone(), at);
        })),
    )
}

/// One file's flat list row: badge, bold name, left-truncated muted path,
/// counts, and the fold chevron at the trailing edge. No toolbar, no second
/// header, no per-file card.
#[allow(clippy::too_many_arguments)]
fn file_header(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    file: &DiffFileDetail,
    open: bool,
    editing: bool,
    editor: Option<&Rc<DiffEditor>>,
    confirming_revert: bool,
    content_width: f32,
    selected: bool,
) -> Box<dyn Element> {
    let on_diff = actions.on_diff_panel.clone();
    let path = file.path.clone();
    let select_path = path.clone();
    let (dir, name) = match file.path.rsplit_once('/') {
        Some((dir, name)) => (dir.to_string(), name.to_string()),
        None => (String::new(), file.path.clone()),
    };
    let dir = truncate_left(&dir, 26);
    let _ = (editing, editor, confirming_revert);

    let lead = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(10.0)
        .with_child(
            reviewer::FileTypeIcon::new(reviewer::file_type_icon(&file.path), 16.0).finish(),
        )
        .with_child(
            Text::new(name)
                .with_font_size(13.0)
                .with_weight(FontWeight::SemiBold)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
        )
        .with_child(
            Expanded::new(
                Text::new(dir)
                    .with_font_size(12.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .with_max_lines(1)
                    .finish(),
            )
            .finish(),
        )
        .finish();

    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0)
        .with_child(Expanded::new(lead).finish())
        .with_child(file_counts(app, file.added, file.removed))
        .with_child(
            Icon::new(if open {
                "chevron-down"
            } else {
                "chevron-right"
            })
            .with_size(13.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        )
        .finish();

    let header = HoverRow::new(
        ConstrainedBox::new(
            Container::new(row)
                .with_padding(EdgeInsets::new(0.0, 8.0, 0.0, 8.0))
                .finish(),
        )
        .with_width(content_width)
        .finish(),
    )
    .with_selected(selected)
    .with_on_click(move || {
        (on_diff.borrow_mut())(pane_id, DiffPanelAction::Select(Some(select_path.clone())));
        (on_diff.borrow_mut())(pane_id, DiffPanelAction::SetFileOpen(path.clone(), !open))
    })
    .finish();
    let header: Box<dyn Element> = if content_width > 0.0 {
        ConstrainedBox::new(header)
            .with_width(content_width)
            .with_height(FILE_HEADER_HEIGHT)
            .finish()
    } else {
        ConstrainedBox::new(header)
            .with_height(FILE_HEADER_HEIGHT)
            .finish()
    };
    header
}

/// 616: hovering a Review file header reveals/opens its dedicated file tab.
struct OpenFileTabOnHover {
    child: Box<dyn Element>,
    on_diff: Rc<RefCell<dyn FnMut(u64, DiffPanelAction)>>,
    pane_id: u64,
    path: String,
    inside: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

fn open_file_tab_on_hover(
    child: Box<dyn Element>,
    on_diff: Rc<RefCell<dyn FnMut(u64, DiffPanelAction)>>,
    pane_id: u64,
    path: String,
) -> Box<dyn Element> {
    Box::new(OpenFileTabOnHover {
        child,
        on_diff,
        pane_id,
        path,
        inside: false,
        size: None,
        origin: None,
    })
}

impl Element for OpenFileTabOnHover {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.child.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }
    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.child.paint(origin, ctx, app);
    }
    fn dispatch_event(
        &mut self,
        event: &goble_ui::event::DispatchedEvent,
        ctx: &mut goble_ui::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        if let (Some(origin), Some(size)) = (self.origin, self.size) {
            let position = match event {
                goble_ui::event::DispatchedEvent::MouseMove { position }
                | goble_ui::event::DispatchedEvent::MouseDown { position, .. }
                | goble_ui::event::DispatchedEvent::MouseUp { position, .. } => Some(*position),
                _ => None,
            };
            if let Some(position) = position {
                let inside = position.x >= origin.x()
                    && position.x <= origin.x() + size.x
                    && position.y >= origin.y()
                    && position.y <= origin.y() + size.y;
                let clicked = matches!(
                    event,
                    goble_ui::event::DispatchedEvent::MouseUp { button: 0, .. }
                );
                if inside && (!self.inside || clicked) {
                    (self.on_diff.borrow_mut())(
                        self.pane_id,
                        DiffPanelAction::OpenFileTab(self.path.clone()),
                    );
                }
                self.inside = inside;
            }
        }
        self.child.dispatch_event(event, ctx, app)
    }
    fn size(&self) -> Option<Vector2F> {
        self.size
    }
    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

/// Keep the tail of a directory path, with a leading ellipsis, the way the
/// reference shows the part nearest the filename.
fn truncate_left(path: &str, max_chars: usize) -> String {
    let count = path.chars().count();
    if count <= max_chars {
        return path.to_string();
    }
    let tail: String = path.chars().skip(count.saturating_sub(max_chars)).collect();
    format!("…{tail}")
}

/// One file's body: its in-place editor, its diff, or the note that says why
/// there is neither.
#[allow(clippy::too_many_arguments)]
fn file_body(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    file: &DiffFileDetail,
    editing: bool,
    editor: Option<&Rc<DiffEditor>>,
    scroll: Rc<RefCell<goble_ui::ScrollState>>,
    hscroll: Rc<RefCell<HorizontalOffsets>>,
    focus: Rc<RefCell<Option<String>>>,
    active: bool,
    editor_top: f32,
    view: &crate::state::DiffReviewState,
    content_width: f32,
) -> Box<dyn Element> {
    if editing {
        let Some(editor) = editor else {
            return note(app, "Reading the file…");
        };
        {
            let buffer = editor.buffer.borrow();
            if !buffer.editable() {
                let reason = buffer.read_only().unwrap_or("read only").to_string();
                return note(app, &format!("Not editable: {reason}"));
            }
        }
        let field = inline_editor(
            actions, pane_id, file, editor, scroll, focus, active, editor_top,
        );
        let field = Scrollable::new(field, Axis::Horizontal)
            .with_state(horizontal_state(&hscroll, &file.absolute_path))
            .finish();
        return Container::new(field)
            .with_padding(EdgeInsets::new(8.0, EDITOR_PAD_TOP, 8.0, EDITOR_PAD_BOTTOM))
            .finish();
    }
    if file.binary {
        note(app, "Binary file")
    } else if file.truncated {
        note(app, "File is too large to show")
    } else if file.hunks.is_empty() {
        note(app, "No textual changes")
    } else {
        // The comparison is the app's own unified/split painter until the
        // goble-ui `FileDiffView` feature lands; either way the view choice and
        // wrapping are the pane's, not the element's. A wrapped comparison is
        // laid out at the pane's width, so it needs no sideways region; an
        // unwrapped one keeps its long lines one swipe away.
        let diff = reviewer::comparison(
            file,
            view.view.is_split(),
            view.wrap,
            false,
            Rc::clone(&scroll),
        );
        // Y: in wrapped mode the add/del band spans the measured diff column;
        // unwrapped diffs keep their natural width so the sideways scroller
        // can move the whole box.
        // 506: the change bands span the whole list column, even when the
        // comparison is not wrapped; the width is the column's, not the
        // longest line's.
        let diff = ConstrainedBox::new(diff)
            .with_min_width(content_width)
            .finish();
        if view.wrap {
            diff
        } else {
            Scrollable::new(diff, Axis::Horizontal)
                .with_state(horizontal_state(&hscroll, &file.absolute_path))
                .finish()
        }
    }
}

/// The horizontal offset of one file's box, created the first frame the file
/// is laid out and kept for as long as the pane is open.
fn horizontal_state(
    states: &Rc<RefCell<HorizontalOffsets>>,
    path: &str,
) -> Rc<RefCell<goble_ui::ScrollState>> {
    states
        .borrow_mut()
        .entry(path.to_string())
        .or_insert_with(|| Rc::new(RefCell::new(goble_ui::ScrollState::smooth())))
        .clone()
}

/// The whole file as an editor inside the review list.
///
/// The text and its highlighted runs come from the file pane's own read
/// ([`super::file_view::read`] through the path-keyed cache), so the review
/// pane colours code with the one highlighter the rest of the app uses. The
/// field shares the list's scroll state and is told where its first row sits
/// (the `top` the column measured), so it draws only the rows in view while the
/// list scrolls from one file straight into the next. Cmd/Ctrl+S is the file
/// pane's own save, wrapped around the field.
#[allow(clippy::too_many_arguments)]
fn inline_editor(
    actions: &UiActions,
    pane_id: u64,
    file: &DiffFileDetail,
    editor: &DiffEditor,
    scroll: Rc<RefCell<goble_ui::ScrollState>>,
    focus: Rc<RefCell<Option<String>>>,
    active: bool,
    editor_top: f32,
) -> Box<dyn Element> {
    let path = file.path.clone();
    let (value, caret, anchor) = {
        let buffer = editor.buffer.borrow();
        (buffer.text_rc(), buffer.caret(), buffer.anchor())
    };
    // The runs belong to the text they were highlighted from, one set per line.
    // A line the edit moved or changed no longer spells its set, and the field
    // draws that line plain rather than in another line's colours — the same
    // mapping the file pane's editor uses. The mapping is built by the read
    // cache once per text version, not by this per-frame rebuild.
    let focused = active && focus.borrow().as_deref() == Some(path.as_str());
    let on_change = {
        let buffer = Rc::clone(&editor.buffer);
        move |text: String| buffer.borrow_mut().set_text(text)
    };
    let on_focus = {
        let focus = Rc::clone(&focus);
        let path = path.clone();
        let on_diff = actions.on_diff_panel.clone();
        move |gained: bool| {
            if gained {
                *focus.borrow_mut() = Some(path.clone());
                // A press in the editor is a press in this pane: the field
                // only takes keys while the pane is the active one.
                (on_diff.borrow_mut())(pane_id, DiffPanelAction::Focus);
            } else if focus.borrow().as_deref() == Some(path.as_str()) {
                *focus.borrow_mut() = None;
            }
        }
    };
    let field = TextArea::new()
        .with_shared_value(value)
        .with_multiline(true)
        .with_line_numbers(GUTTER_DIGITS)
        .with_line_height(EDITOR_LINE_HEIGHT_FACTOR)
        .with_min_height(0.0)
        .with_full_width(true)
        .with_focused(focused)
        .with_caret(caret)
        .with_anchor(anchor)
        .with_line_runs(editor.runs.clone())
        .with_scroll_state(scroll)
        .with_scroll_bias(editor_top)
        .with_on_change(on_change)
        .with_on_focus_change(on_focus)
        .finish();
    Box::new(FileKeys::new(Rc::clone(&editor.buffer), focused, field))
}

fn icon_button(
    app: &AppContext,
    icon: &'static str,
    tooltip: &'static str,
    on_click: impl FnMut() + 'static,
) -> Box<dyn Element> {
    icon_button_enabled(app, icon, tooltip, true, on_click)
}

/// The same icon control, drawn disabled when `enabled` is false: a press does
/// nothing and the tooltip still names the action, which is the honest state
/// for a file the working tree no longer has.
fn icon_button_enabled(
    app: &AppContext,
    icon: &'static str,
    tooltip: &'static str,
    enabled: bool,
    on_click: impl FnMut() + 'static,
) -> Box<dyn Element> {
    let button = IconButton::new(
        Icon::new(icon)
            .with_size(13.0)
            .with_theme_color(
                if enabled {
                    ColorToken::Muted
                } else {
                    ColorToken::Border
                },
                app,
            )
            .finish(),
    )
    .with_size(vec2f(22.0, 22.0))
    .with_disabled(!enabled)
    .with_on_click(on_click)
    .finish();
    Tooltip::new(button, tooltip)
        .with_position(TooltipPosition::Above)
        .finish()
}

fn text_button(
    app: &AppContext,
    label: &str,
    on_click: impl FnMut() + 'static,
) -> Box<dyn Element> {
    Button::new(
        Text::new(label)
            .with_font_size(TOOLBAR_TEXT)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_variant(ButtonVariant::Ghost)
    .with_corner_radius(0.0)
    .with_on_click(on_click)
    .finish()
}

fn stats(app: &AppContext, files: usize, added: u64, removed: u64) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(
            Text::new(format!(
                "{files} {}",
                if files == 1 { "file" } else { "files" }
            ))
            .with_font_size(HEADER_TEXT)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        );
    if added > 0 || removed > 0 {
        row = row.with_child(
            Text::new("•")
                .with_font_size(HEADER_TEXT)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    if added > 0 {
        row = row.with_child(
            Text::new(format!("+{added}"))
                .with_font_size(HEADER_TEXT)
                .with_theme_color(ColorToken::Success, app)
                .finish(),
        );
    }
    if removed > 0 {
        row = row.with_child(
            Text::new(format!("-{removed}"))
                .with_font_size(HEADER_TEXT)
                .with_theme_color(ColorToken::Error, app)
                .finish(),
        );
    }
    row.finish()
}

/// A file header's signed counts in their own box, the way warp-new's card
/// draws them: `+A · -D`, always both numbers.
fn stat_badge(app: &AppContext, added: u32, removed: u32) -> Box<dyn Element> {
    let removed_color = if removed > 0 {
        ColorToken::Error
    } else {
        ColorToken::Muted
    };
    Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(3.0)
            .with_child(
                Text::new(format!("+{added}"))
                    .with_font_family(FontFamily::Mono)
                    .with_font_size(10.0)
                    .with_line_height(16.0 / 10.0)
                    .with_theme_color(ColorToken::Success, app)
                    .finish(),
            )
            .with_child(
                Text::new("·")
                    .with_font_family(FontFamily::Mono)
                    .with_font_size(10.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(
                Text::new(format!("-{removed}"))
                    .with_font_family(FontFamily::Mono)
                    .with_font_size(10.0)
                    .with_line_height(16.0 / 10.0)
                    .with_theme_color(removed_color, app)
                    .finish(),
            )
            .finish(),
    )
    .with_padding(EdgeInsets::new(5.0, 2.0, 5.0, 2.0))
    .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
    .with_border(Border::all(1.0).with_border_fill(app.theme.color(ColorToken::Border)))
    .with_corner_radius(4.0)
    .finish()
}

/// A file header's own +A/-D counts, at the trailing edge of a row.
fn file_counts(app: &AppContext, added: u32, removed: u32) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0);
    if added > 0 {
        row = row.with_child(
            Text::new(format!("+{added}"))
                .with_font_family(FontFamily::Mono)
                .with_font_size(11.0)
                .with_line_height(18.0 / 11.0)
                .with_theme_color(ColorToken::Success, app)
                .finish(),
        );
    }
    if removed > 0 {
        row = row.with_child(
            Text::new(format!("-{removed}"))
                .with_font_family(FontFamily::Mono)
                .with_font_size(11.0)
                .with_line_height(18.0 / 11.0)
                .with_theme_color(ColorToken::Error, app)
                .finish(),
        );
    }
    if added == 0 && removed == 0 {
        row = row.with_child(
            Text::new("0")
                .with_font_size(10.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    row.finish()
}

fn note(app: &AppContext, text: &str) -> Box<dyn Element> {
    ConstrainedBox::new(
        Container::new(
            Text::new(text)
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_padding(EdgeInsets::new(4.0, 6.0, 8.0, 6.0))
        .finish(),
    )
    .with_height(NOTE_HEIGHT)
    .finish()
}

fn divider(app: &AppContext) -> Box<dyn Element> {
    ConstrainedBox::new(
        Container::new(
            Flex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .finish(),
        )
        .with_background(Fill::Solid(app.theme.color(ColorToken::Border)))
        .finish(),
    )
    .with_width(1.0)
    .finish()
}

/// The pane's zero state: what it will show once there are changes.
fn empty_state(app: &AppContext, title: &str) -> Box<dyn Element> {
    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::Center)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0)
        .with_child(
            Icon::new("code")
                .with_size(32.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Text::new(title)
                .with_font_size(15.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        );
    // The transient and the not-a-repository states need no promise about what
    // will appear; a clean tree does.
    if title != "Reading the changes…" && title != "No working tree here" {
        column = column.with_child(
            Text::new("Changes you or the agent make will appear here.")
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .finish()
}
