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
use std::path::Path;
use std::rc::Rc;

use goble_ui::color::ColorU;
use goble_ui::elements::{
    AppContext, Axis, Border, Button, ButtonVariant, ConstrainedBox, Container, CrossAxisAlignment,
    Diff, Divider, EdgeInsets, Element, Empty, Expanded, Fill, Flex, HoverRow, Icon, IconButton,
    LayoutContext, MainAxisAlignment, MainAxisSize, PaintContext, Point, PopupMenu, PopupMenuItem,
    PopupMenuPosition, Scrollable, SizeConstraint, Stack, Text, TextArea, TextInput, Tooltip,
    TooltipPosition,
};
use goble_ui::geometry::{rectf, vec2f, Vector2F};
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::{ColorToken, FontFamily, SpacingToken};

use super::chat;
use super::file_view::{DiffEditor, FileKeys};
use super::pickers::{DiffDetail, DiffFileDetail};
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
const FILE_HEADER_HEIGHT: f32 = 30.0;
/// The height of the review's controls bar under the pane's topbar.
const TOOLBAR_HEIGHT: f32 = 32.0;
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
            leading: Some(review_title(app, detail, bar_width)),
            ..chat::HeaderSlots::default()
        },
        _ => chat::HeaderSlots::default(),
    };
    let header = chat::build_agent_header_with(app, state, actions, pane_id, false, slots);

    // The second bar is painted with the header, above the cards: its commit
    // menu opens down over the file list, and a menu painted with the content
    // would be covered by the cards under it.
    let toolbar = match (detail, controls.as_ref()) {
        (Some(Some(_)), Some(controls)) => Some(review_controls(
            app,
            actions,
            pane_id,
            *controls.diff_sidebar.borrow(),
            compact,
            &CommitView {
                open: commit_open,
                menu_open: controls.diff_commit_menu_open.clone(),
                status: controls.diff_git_status.borrow().clone(),
            },
        )),
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
                .with_size(Vector2F::new(0.0, TOOLBAR_HEIGHT))
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
        column = column.with_child(Divider::horizontal().finish());
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
                Container::new(
                    body(
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
                    ),
                )
                .with_padding(EdgeInsets::new(sm, sm, sm, sm))
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
                    .with_padding(EdgeInsets::new(sm, sm, sm, sm))
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
        chrome = chrome.with_child(toolbar);
    }
    Stack::new()
        .with_children(vec![content])
        .with_overlay(chrome.finish(), Vector2F::zero())
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

/// The topbar's leading content: the directory this review covers, its branch
/// and its totals, in the order that fits the pane's current width.
fn review_title(app: &AppContext, detail: &DiffDetail, width: f32) -> Box<dyn Element> {
    let root = crate::state::display_path(&detail.root);
    let mut title = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0);
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
fn review_controls(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
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

    let bar = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(sidebar_button)
        .with_child(Expanded::new(Empty::new().with_size(Vector2F::zero()).finish()).finish())
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
            FontWeight::Regular,
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
) -> Box<dyn Element> {
    if detail.files.is_empty() {
        return empty_state(app, "No open changes");
    }
    // A selected file the working tree no longer has leaves every file in the
    // view rather than an empty pane.
    let selected = selected.filter(|path| detail.files.iter().any(|file| file.path == *path));

    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    if sidebar {
        row = row
            .with_child(file_list(
                app,
                actions,
                pane_id,
                detail,
                selected.as_deref(),
                list_scroll,
            ))
            .with_child(divider(app));
    }
    row = row.with_child(
        Expanded::new(diff_list(
            app, actions, pane_id, detail, selected, expanded, editing, focus, editors, hscroll,
            visible, active, scroll, revert,
        ))
        .finish(),
    );
    row.finish()
}

fn file_list(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    detail: &DiffDetail,
    selected: Option<&str>,
    scroll: Rc<RefCell<goble_ui::ScrollState>>,
) -> Box<dyn Element> {
    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(1.0);
    let on_diff = actions.on_diff_panel.clone();
    if selected.is_some() {
        column = column.with_child(
            HoverRow::new(
                Text::new("All changes")
                    .with_font_size(12.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_padding(EdgeInsets::new(6.0, 8.0, 6.0, 8.0))
            .with_on_click(move || (on_diff.borrow_mut())(pane_id, DiffPanelAction::Select(None)))
            .finish(),
        );
    }
    for file in &detail.files {
        let on_diff = actions.on_diff_panel.clone();
        let path = file.path.clone();
        column = column.with_child(
            HoverRow::new(file_row(app, file))
                .with_selected(selected == Some(file.path.as_str()))
                .with_padding(EdgeInsets::new(6.0, 8.0, 6.0, 8.0))
                .with_on_click(move || {
                    (on_diff.borrow_mut())(pane_id, DiffPanelAction::Select(Some(path.clone())))
                })
                .finish(),
        );
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
    .with_width(LIST_WIDTH)
    .finish()
}

/// One file-list row: the name over its directory, with the line counts at the
/// trailing edge.
fn file_row(app: &AppContext, file: &DiffFileDetail) -> Box<dyn Element> {
    let path = Path::new(&file.path);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&file.path);
    let dir = path.parent().and_then(|dir| dir.to_str()).unwrap_or("");
    let mut labels = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(1.0)
        .with_child(
            Text::new(name)
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        );
    if !dir.is_empty() {
        labels = labels.with_child(
            Text::new(dir)
                .with_font_size(10.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(Expanded::new(labels.finish()).finish())
        .with_child(file_counts(app, file.added, file.removed))
        .finish()
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
        if selected.as_deref().is_some_and(|path| path != file.path) {
            continue;
        }
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
        ));
        top = bottom;
    }
    if skipped > 0.0 {
        column = column.with_child(vertical_spacer(skipped));
    }
    *visible.borrow_mut() = shown;
    Scrollable::new(
        Container::new(column.finish())
            .with_padding(EdgeInsets::new(0.0, LIST_PAD_TOP, 0.0, 10.0))
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
) -> Box<dyn Element> {
    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(file_header(
            app,
            actions,
            pane_id,
            file,
            open,
            editing,
            editor,
            confirming_revert,
        ));
    if open {
        column = column.with_child(file_body(
            app, actions, pane_id, file, editing, editor, scroll, hscroll, focus, active,
            editor_top,
        ));
    }
    // Every file is its own box: the border is the file's bounds, and the gap
    // below it is the clear space before the next one.
    let boxed = Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_border(Border::all(1.0).with_border_fill(app.theme.color(ColorToken::Border)))
        .with_corner_radius(8.0)
        .finish();
    Container::new(boxed)
        .with_padding(EdgeInsets::new(0.0, 0.0, 0.0, FILE_BOX_GAP))
        .finish()
}

/// One file's header, laid out like warp-new's card: the fold chevron and path,
/// copy, the signed counts in their own box, and the link, revert and
/// open-in-editor controls at the trailing edge. The whole row is pinned to
/// [`FILE_HEADER_HEIGHT`], because the next file's editor is placed by it.
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
) -> Box<dyn Element> {
    let on_diff = actions.on_diff_panel.clone();
    let path = file.path.clone();
    let mut left = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(
            Icon::new(if open {
                "chevron-down"
            } else {
                "chevron-right"
            })
            .with_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        )
        // The path owns whatever room the controls leave: it stops before the
        // copy button instead of painting under it, and fades into the card at
        // the edge it was cut at.
        .with_child(
            Expanded::new(Box::new(PathLabel::new(
                file.path.clone(),
                app.theme.color(ColorToken::Text),
                app.theme.color(ColorToken::Surface),
            )))
            .finish(),
        );
    // An editor's buffer says whether its text has been written back; the
    // counts above describe the change itself.
    if editing {
        let (dirty, status) = match editor {
            Some(editor) => {
                let buffer = editor.buffer.borrow();
                (buffer.dirty(), buffer.status().map(str::to_string))
            }
            None => (false, None),
        };
        if let Some(mark) = if dirty {
            Some("unsaved")
        } else {
            status.as_deref()
        } {
            left = left.with_child(
                Text::new(mark)
                    .with_font_size(10.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        }
    }
    let toggle = HoverRow::new(left.finish())
        .with_padding(EdgeInsets::new(2.0, 4.0, 4.0, 4.0))
        .with_on_click(move || {
            (on_diff.borrow_mut())(pane_id, DiffPanelAction::SetFileOpen(path.clone(), !open))
        })
        .finish();

    let on_copy = actions.on_diff_panel.clone();
    let path = file.path.clone();
    let copy_button = icon_button(app, "copy", "Copy path", move || {
        (on_copy.borrow_mut())(pane_id, DiffPanelAction::CopyPaths(vec![path.clone()]))
    });

    // The diff is the body by default; this is the way to the file's text.
    let on_edit = actions.on_diff_panel.clone();
    let path = file.path.clone();
    let body_toggle = (!file.binary && !file.truncated).then(|| {
        text_button(app, if editing { "Diff" } else { "File" }, move || {
            (on_edit.borrow_mut())(
                pane_id,
                DiffPanelAction::SetFileEditing(path.clone(), !editing),
            )
        })
    });

    let on_link = actions.on_diff_panel.clone();
    let absolute = file.absolute_path.clone();
    let link_button = icon_button(app, "link", "Copy link", move || {
        (on_link.borrow_mut())(pane_id, DiffPanelAction::CopyLink(absolute.clone()))
    });
    let on_revert = actions.on_diff_panel.clone();
    let path = file.path.clone();
    let revert_button = icon_button(app, "undo", "Revert file", move || {
        (on_revert.borrow_mut())(pane_id, DiffPanelAction::ConfirmRevert(path.clone()))
    });
    let on_open = actions.on_diff_panel.clone();
    let absolute = file.absolute_path.clone();
    let external_button = icon_button(app, "maximize-01", "Open in editor", move || {
        (on_open.borrow_mut())(pane_id, DiffPanelAction::OpenExternal(absolute.clone()))
    });

    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(Expanded::new(toggle).finish())
        .with_child(copy_button)
        .with_child(stat_badge(app, file.added, file.removed));
    if confirming_revert {
        // A second press is the one that discards the change; the header asks
        // in place, so no press on a narrow icon can throw work away alone.
        let discard = Button::new(
            Text::new("Revert")
                .with_font_size(10.0)
                .with_theme_color(ColorToken::Error, app)
                .finish(),
        )
        .with_variant(ButtonVariant::Default)
        .with_border_color(app.theme.color(ColorToken::Error))
        .with_corner_radius(4.0)
        .with_on_click({
            let on_diff = actions.on_diff_panel.clone();
            let path = file.path.clone();
            move || (on_diff.borrow_mut())(pane_id, DiffPanelAction::RevertFile(path.clone()))
        })
        .finish();
        let cancel = text_button(app, "Cancel", {
            let on_diff = actions.on_diff_panel.clone();
            move || (on_diff.borrow_mut())(pane_id, DiffPanelAction::CancelRevert)
        });
        row = row
            .with_child(
                Text::new("Discard changes?")
                    .with_font_size(10.0)
                    .with_theme_color(ColorToken::Error, app)
                    .finish(),
            )
            .with_child(discard)
            .with_child(cancel);
    } else {
        if let Some(body_toggle) = body_toggle {
            row = row.with_child(body_toggle);
        }
        row = row
            .with_child(link_button)
            .with_child(revert_button)
            .with_child(external_button);
    }
    ConstrainedBox::new(row.finish())
        .with_height(FILE_HEADER_HEIGHT)
        .finish()
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
        // The review's worker colours the cards in view beside the frame; this
        // one draws whatever runs have already landed, so a large diff never
        // stalls the paint.
        let diff = Diff::from_prepared(Rc::clone(&file.hunks), file.prepared.clone())
            .with_fill_width(true)
            .with_viewport(scroll)
            .finish();
        let diff = Scrollable::new(diff, Axis::Horizontal)
            .with_state(horizontal_state(&hscroll, &file.absolute_path))
            .finish();
        Container::new(diff)
            .with_padding(EdgeInsets::new(8.0, DIFF_PAD_TOP, 8.0, DIFF_PAD_BOTTOM))
            .finish()
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
    let button = IconButton::new(
        Icon::new(icon)
            .with_size(13.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(vec2f(22.0, 22.0))
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
                    .with_font_size(10.0)
                    .with_theme_color(ColorToken::Success, app)
                    .finish(),
            )
            .with_child(
                Text::new("·")
                    .with_font_size(10.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(
                Text::new(format!("-{removed}"))
                    .with_font_size(10.0)
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
                .with_font_size(10.0)
                .with_theme_color(ColorToken::Success, app)
                .finish(),
        );
    }
    if removed > 0 {
        row = row.with_child(
            Text::new(format!("-{removed}"))
                .with_font_size(10.0)
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
        Container::new(Empty::new().finish())
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
