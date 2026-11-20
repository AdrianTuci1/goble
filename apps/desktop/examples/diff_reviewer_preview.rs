//! Headless visual verification of the diff reviewer deliverables.
//!
//! The comparison bodies are the product's reusable [`FileDiffView`]. The
//! changed-files card is the shipped `ChatView` card. The review shell around
//! the comparison (the 38pt selector header and the changed-file rail) is
//! composed from the same public `goble-ui` elements, because the app's review
//! tab is a `pub(crate)` widget and has no exported constructor yet.
//!
//! Usage: cargo run -p goble-app --example diff_reviewer_preview
//! Output: /tmp/goble-work/out/diff-reviewer/*.png
//!
//! Once per theme:
//! - `changed-files-card-{folded,expanded}` — the shipped transcript card.
//! - `file-reviewer-list` — the rail with selection and A/M/D statuses.
//! - `filediff-{unified,split}-hunk` — both product comparison layouts.
//! - `filediff-{unified,split}-{add-only,del-only}` — one-sided changes.
//! - `filediff-empty` / `reviewer-clean` — no hunks, and a clean tree.
//! - `filediff-selector-open` — the header's file menu open.
//! - `filediff-wrap-{off,on}` — a long line clipped, then wrapped.
//! - `floating-file-panel` — the file card + comparison in a floating frame.

#[path = "../../../crates/goble-ui/examples/composer_preview/raster.rs"]
mod raster;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use goble_core::harness::ToolCallStatus;
use goble_ui::elements::chat_content::ToolCall;
use goble_ui::elements::diff::{DiffLine, DiffLineKind, Hunk};
use goble_ui::elements::{
    AppContext, Axis, Border, ConstrainedBox, Container, CrossAxisAlignment, DiffLayout,
    DiffSpec, Divider, EdgeInsets, Element, Empty, Expanded, FileDiffView, Fill, Flex, HoverRow,
    Icon, MainAxisAlignment, MainAxisSize, PopupMenu, PopupMenuItem, PopupMenuPosition, ScrollState,
    Scrollable, Stack, Text,
};
use goble_ui::geometry::vec2f;
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::{ColorToken, FontFamily, Theme};
use goble_ui::{ChatFragment, ChatMessage, ChatRole, ChatView};

const OUT_DIR: &str = "/tmp/goble-work/out/diff-reviewer";
const RAIL_WIDTH: f32 = 248.0;
const HEADER_HEIGHT: f32 = DiffSpec::REVIEW_HEADER_HEIGHT;

// ---------------------------------------------------------------------------
// Changed-file data
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct FileItem {
    path: &'static str,
    added: u32,
    removed: u32,
    status: char,
}

const FILES: [FileItem; 7] = [
    FileItem { path: "README.md", added: 36, removed: 0, status: 'M' },
    FileItem { path: "docs/diff-reviewer.md", added: 72, removed: 0, status: 'A' },
    FileItem { path: "src/panes.rs", added: 18, removed: 4, status: 'M' },
    FileItem { path: "src/legacy_review.rs", added: 0, removed: 36, status: 'D' },
    FileItem { path: "assets/diff-split.svg", added: 12, removed: 0, status: 'A' },
    FileItem { path: "Cargo.toml", added: 2, removed: 1, status: 'M' },
    FileItem { path: "src/ui/diff_panel.rs", added: 0, removed: 0, status: 'M' },
];

fn total(files: &[FileItem]) -> (u32, u32) {
    files
        .iter()
        .fold((0, 0), |(a, d), file| (a + file.added, d + file.removed))
}

// ---------------------------------------------------------------------------
// Diff data
// ---------------------------------------------------------------------------

fn line(kind: DiffLineKind, old: Option<u32>, new: Option<u32>, text: &str) -> DiffLine {
    DiffLine {
        kind,
        old_line: old,
        new_line: new,
        text: text.to_string(),
    }
}

fn hunk(lines: Vec<DiffLine>) -> Hunk {
    Hunk {
        old_start: 40,
        old_count: 7,
        new_start: 40,
        new_count: 9,
        section: "fn build()".to_string(),
        lines,
    }
}

/// The ordinary review hunk: context, a deletion, additions, a second pair,
/// and the closing context.
fn review_hunk() -> Hunk {
    hunk(vec![
        line(DiffLineKind::Context, Some(40), Some(40), "    let panes = build_panes(state);"),
        line(DiffLineKind::Context, Some(41), Some(41), "    for pane in panes {"),
        line(DiffLineKind::Removed, Some(42), None, "        draw_old(pane);"),
        line(DiffLineKind::Added, None, Some(42), "        draw_new(pane);"),
        line(DiffLineKind::Added, None, Some(43), "        mark_visible(&pane);"),
        line(DiffLineKind::Context, Some(43), Some(44), "    }"),
        line(DiffLineKind::Removed, Some(44), None, "    legacy_footer();"),
        line(DiffLineKind::Added, None, Some(45), "    draw_reviewer(state);"),
        line(DiffLineKind::Context, Some(45), Some(46), "    layout_panes(&panes);"),
        line(DiffLineKind::Added, None, Some(47), "    publish_review(&panes);"),
        line(DiffLineKind::Context, Some(46), Some(48), "}"),
    ])
}

fn add_hunk() -> Hunk {
    Hunk {
        old_start: 0,
        old_count: 0,
        new_start: 1,
        new_count: 5,
        section: String::new(),
        lines: vec![
            line(DiffLineKind::Added, None, Some(1), "# Diff reviewer"),
            line(DiffLineKind::Added, None, Some(2), ""),
            line(DiffLineKind::Added, None, Some(3), "The review tab draws a file rail, a selector"),
            line(DiffLineKind::Added, None, Some(4), "and a unified or split comparison."),
            line(DiffLineKind::Added, None, Some(5), ""),
        ],
    }
}

fn del_hunk() -> Hunk {
    Hunk {
        old_start: 1,
        old_count: 4,
        new_start: 0,
        new_count: 0,
        section: String::new(),
        lines: vec![
            line(DiffLineKind::Removed, Some(1), None, "//! The old review surface."),
            line(DiffLineKind::Removed, Some(2), None, "//!"),
            line(DiffLineKind::Removed, Some(3), None, "//! One file, no selector, no statuses."),
            line(DiffLineKind::Removed, Some(4), None, "pub fn legacy_review() {}"),
        ],
    }
}

/// A line long enough to leave the comparison body, so wrap off/on differ in
/// the same frame.
fn long_hunk() -> Hunk {
    hunk(vec![
        line(DiffLineKind::Context, Some(40), Some(40), "fn build_reviewer(state: &ReviewState) -> Reviewer {"),
        line(
            DiffLineKind::Removed,
            Some(41),
            None,
            "    Reviewer::new(state).with_selection(state.selected).with_split(state.split)",
        ),
        line(
            DiffLineKind::Added,
            None,
            Some(41),
            "    Reviewer::new(state, options).with_selection(state.selected).with_statuses(state.statuses).with_split(state.split)",
        ),
        line(DiffLineKind::Context, Some(42), Some(42), "}"),
    ])
}

// ---------------------------------------------------------------------------
// Small building blocks
// ---------------------------------------------------------------------------

fn color(app: &AppContext, token: ColorToken) -> goble_ui::color::ColorU {
    app.theme.color(token)
}

fn mono(app: &AppContext, text: impl Into<String>, size: f32, token: ColorToken) -> Box<dyn Element> {
    Text::new(text)
        .with_font_family(FontFamily::Mono)
        .with_font_size(size)
        .with_line_height(18.0 / size)
        .with_theme_color(token, app)
        .finish()
}

fn label(app: &AppContext, text: impl Into<String>, size: f32, weight: FontWeight, token: ColorToken) -> Box<dyn Element> {
    Text::new(text)
        .with_font_size(size)
        .with_line_height(20.0 / size)
        .with_weight(weight)
        .with_theme_color(token, app)
        .finish()
}

fn status_color(status: char) -> ColorToken {
    match status {
        'A' => ColorToken::Success,
        'D' => ColorToken::Error,
        _ => ColorToken::Warning,
    }
}

fn counts(app: &AppContext, added: u32, removed: u32, size: f32) -> Box<dyn Element> {
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(mono(app, format!("+{added}"), size, ColorToken::Success))
        .with_child(mono(app, format!("-{removed}"), size, ColorToken::Error))
        .finish()
}

fn icon(app: &AppContext, name: &'static str, size: f32, token: ColorToken) -> Box<dyn Element> {
    Icon::new(name)
        .with_size(size)
        .with_theme_color(token, app)
        .finish()
}

fn hairline() -> Box<dyn Element> {
    Divider::horizontal().finish()
}

fn vertical_divider() -> Box<dyn Element> {
    Divider::vertical().finish()
}

fn note_row(app: &AppContext, text: &str) -> Box<dyn Element> {
    Container::new(
        Text::new(text.to_string())
            .with_font_size(11.0)
            .with_line_height(18.0 / 11.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_padding(EdgeInsets::new(8.0, 14.0, 8.0, 10.0))
    .finish()
}

/// The product comparison inside its scroll viewport, with DSH's optional
/// one-line note above the hunks.
fn diff_body(
    app: &AppContext,
    hunks: Vec<Hunk>,
    split: bool,
    wrap: bool,
    note: Option<&str>,
    language: Option<&str>,
) -> Box<dyn Element> {
    let scroll = Rc::new(RefCell::new(ScrollState::default()));
    let mut view = FileDiffView::new(hunks)
        .with_layout(if split { DiffLayout::Split } else { DiffLayout::Unified })
        .with_wrap(wrap)
        .with_viewport(Rc::clone(&scroll))
        .with_fill_width(true);
    if let Some(language) = language {
        view = view.with_language(language);
    }
    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    if let Some(note) = note {
        column = column.with_child(note_row(app, note));
    }
    column = column.with_child(Box::new(view));
    Scrollable::new(column.finish(), Axis::Vertical)
        .with_state(scroll)
        .finish()
}

// ---------------------------------------------------------------------------
// Review shell
// ---------------------------------------------------------------------------

fn file_row(app: &AppContext, file: &FileItem, selected: bool) -> Box<dyn Element> {
    let status = file.status;
    let (name, dir) = file
        .path
        .rsplit_once('/')
        .map(|(dir, name)| (name.to_string(), dir.to_string()))
        .unwrap_or_else(|| (file.path.to_string(), String::new()));
    let mut titles = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(1.0)
        .with_child(
            Text::new(name)
                .with_font_size(12.0)
                .with_line_height(18.0 / 12.0)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
        );
    if !dir.is_empty() {
        titles = titles.with_child(
            Text::new(dir)
                .with_font_size(10.0)
                .with_line_height(15.0 / 10.0)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(1)
                .finish(),
        );
    }
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(7.0)
        .with_child(icon(app, "file", 14.0, ColorToken::Muted))
        .with_child(mono(app, status.to_string(), 10.0, status_color(status)))
        .with_child(Expanded::new(titles.finish()).finish())
        .with_child(counts(app, file.added, file.removed, 10.0))
        .finish();
    HoverRow::new(row)
        .with_selected(selected)
        .with_corner_radius(6.0)
        .with_padding(EdgeInsets::new(6.0, 8.0, 6.0, 8.0))
        .finish()
}

fn file_rail(app: &AppContext, files: &[FileItem], selected: Option<usize>) -> Box<dyn Element> {
    let (added, removed) = total(files);
    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    let header = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0)
        .with_child(label(app, format!("Changes · {}", files.len()), 11.0, FontWeight::Medium, ColorToken::Muted))
        .with_child(Expanded::new(Empty::new().finish()).finish())
        .with_child(counts(app, added, removed, 10.0))
        .finish();
    column = column.with_child(
        ConstrainedBox::new(
            Container::new(header)
                .with_padding(EdgeInsets::new(0.0, 12.0, 0.0, 10.0))
                .finish(),
        )
        .with_height(HEADER_HEIGHT)
        .finish(),
    );
    column = column.with_child(hairline());
    if files.is_empty() {
        column = column.with_child(
            Container::new(
                Text::new("No changed files")
                    .with_font_size(11.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_padding(EdgeInsets::new(10.0, 12.0, 10.0, 10.0))
            .finish(),
        );
    }
    if !files.is_empty() {
        column = column.with_child(
            HoverRow::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(6.0)
                    .with_child(icon(app, "flat-list", 13.0, ColorToken::Muted))
                    .with_child(label(app, "All changes", 11.0, FontWeight::Regular, ColorToken::Muted))
                    .finish(),
            )
            .with_padding(EdgeInsets::new(6.0, 10.0, 6.0, 10.0))
            .finish(),
        );
    }
    for (index, file) in files.iter().enumerate() {
        column = column.with_child(
            Container::new(file_row(app, file, selected == Some(index)))
                .with_padding(EdgeInsets::new(0.0, 8.0, 0.0, 8.0))
                .finish(),
        );
    }
    column = column.with_child(
        Container::new(
            Text::new("M modified · A added · D deleted")
                .with_font_size(9.0)
                .with_line_height(14.0 / 9.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_padding(EdgeInsets::new(8.0, 12.0, 8.0, 10.0))
        .finish(),
    );
    ConstrainedBox::new(
        Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .finish(),
    )
    .with_width(RAIL_WIDTH)
    .finish()
}

fn selector_button(app: &AppContext, path: &str, open: bool) -> Box<dyn Element> {
    let row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(5.0)
        .with_child(icon(app, "file", 13.0, ColorToken::Muted))
        .with_child(
            Text::new(path.to_string())
                .with_font_size(12.0)
                .with_line_height(20.0 / 12.0)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
        )
        .with_child(icon(app, "chevron-down", 12.0, ColorToken::Muted))
        .finish();
    ConstrainedBox::new(
        Container::new(row)
            .with_padding(EdgeInsets::new(0.0, 8.0, 0.0, 6.0))
            .with_corner_radius(6.0)
            .with_background(Fill::Solid(color(
                app,
                if open { ColorToken::Hover } else { ColorToken::Bg },
            )))
            .finish(),
    )
    .with_height(DiffSpec::REVIEW_SELECTOR_HEIGHT)
    .finish()
}

fn tool_glyph(app: &AppContext, glyph: &'static str, active: bool) -> Box<dyn Element> {
    Container::new(icon(
        app,
        glyph,
        DiffSpec::REVIEW_TOOL_GLYPH,
        if active { ColorToken::Text } else { ColorToken::Muted },
    ))
    .with_padding(EdgeInsets::uniform(6.5))
    .with_corner_radius(6.0)
    .with_background(Fill::Solid(color(
        app,
        if active { ColorToken::Hover } else { ColorToken::Bg },
    )))
    .finish()
}

fn tool_text(app: &AppContext, text: &str, active: bool) -> Box<dyn Element> {
    Container::new(
        Text::new(text.to_string())
            .with_font_size(10.0)
            .with_line_height(16.0 / 10.0)
            .with_theme_color(if active { ColorToken::Text } else { ColorToken::Muted }, app)
            .finish(),
    )
    .with_padding(EdgeInsets::new(5.0, 7.0, 5.0, 7.0))
    .with_corner_radius(6.0)
    .with_background(Fill::Solid(color(
        app,
        if active { ColorToken::Hover } else { ColorToken::Bg },
    )))
    .finish()
}

fn tool_wrap(app: &AppContext, wrap: bool) -> Box<dyn Element> {
    Container::new(
        Text::new(if wrap { "Wrap on" } else { "Wrap" })
            .with_font_size(10.0)
            .with_line_height(16.0 / 10.0)
            .with_theme_color(if wrap { ColorToken::Text } else { ColorToken::Muted }, app)
            .finish(),
    )
    .with_padding(EdgeInsets::new(5.0, 7.0, 5.0, 7.0))
    .with_corner_radius(6.0)
    .with_background(Fill::Solid(color(
        app,
        if wrap { ColorToken::Hover } else { ColorToken::Bg },
    )))
    .finish()
}

fn review_header(
    app: &AppContext,
    file: Option<&FileItem>,
    split: bool,
    wrap: bool,
    menu_open: bool,
) -> Box<dyn Element> {
    let (added, removed) = file.map(|file| (file.added, file.removed)).unwrap_or((0, 0));
    let mut left = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(DiffSpec::REVIEW_HEADER_GAP);
    if let Some(file) = file {
        left = left.with_child(selector_button(app, file.path, menu_open));
        left = left.with_child(counts(app, added, removed, 11.0));
    } else {
        left = left.with_child(
            Container::new(label(app, "Review", 13.0, FontWeight::Medium, ColorToken::Text))
                .with_padding_left(10.0)
                .finish(),
        );
    }
    left = left.with_child(Expanded::new(Empty::new().finish()).finish());
    left = left.with_child(tool_text(app, "Split", split));
    left = left.with_child(tool_wrap(app, wrap));
    left = left.with_child(tool_glyph(app, "maximize-01", false));
    let row = left
        .with_child(icon(app, "dots-horizontal", 15.0, ColorToken::Muted))
        .finish();
    ConstrainedBox::new(
        Container::new(row)
            .with_padding(EdgeInsets::new(6.0, 0.0, 6.0, 8.0))
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .finish(),
    )
    .with_height(HEADER_HEIGHT)
    .finish()
}

fn reviewer_shell(
    app: &AppContext,
    files: &[FileItem],
    selected: Option<usize>,
    split: bool,
    wrap: bool,
    body: Box<dyn Element>,
    menu_open: bool,
) -> Box<dyn Element> {
    let file = selected.and_then(|index| files.get(index));
    let main = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(review_header(app, file, split, wrap, menu_open))
        .with_child(hairline())
        .with_child(Expanded::new(body).finish())
        .finish();
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(file_rail(app, files, selected))
        .with_child(vertical_divider())
        .with_child(Expanded::new(main).finish())
        .finish();
    Container::new(row)
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .finish()
}

fn empty_state(
    app: &AppContext,
    glyph: &'static str,
    token: ColorToken,
    title: &str,
    body: &str,
) -> Box<dyn Element> {
    Container::new(
        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_child(icon(app, glyph, 26.0, token))
            .with_child(
                Container::new(
                    Text::new(title.to_string())
                        .with_font_size(13.0)
                        .with_line_height(20.0 / 13.0)
                        .with_weight(FontWeight::Medium)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .with_padding(EdgeInsets::new(10.0, 0.0, 0.0, 0.0))
                .finish(),
            )
            .with_child(
                Container::new(
                    Text::new(body.to_string())
                        .with_font_size(11.0)
                        .with_line_height(18.0 / 11.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_padding(EdgeInsets::new(4.0, 0.0, 0.0, 0.0))
                .finish(),
            )
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
    .finish()
}

// ---------------------------------------------------------------------------
// Scenes
// ---------------------------------------------------------------------------

fn changed_files_card_scene(_app: &AppContext, open: bool) -> Box<dyn Element> {
    let message = ChatMessage::from_markdown(
        ChatRole::Assistant,
        "The review surface now covers **unified and split** comparisons, a changed-file rail, and a wrap toggle.",
    )
    .with_tool_calls(card_calls());
    ChatView::new()
        .without_composer()
        .with_messages(vec![
            ChatMessage::new(ChatRole::User, vec![ChatFragment::text("Review the diff surface.")]),
            message,
        ])
        .with_changed_files_open(Rc::new(RefCell::new(open)))
        .with_on_review_changes(|| {})
        .finish()
}

fn card_calls() -> Vec<ToolCall> {
    let files = [
        ("README.md", 36, 0, 'M'),
        ("docs/diff-reviewer.md", 72, 0, 'A'),
        ("src/panes.rs", 18, 4, 'M'),
        ("src/legacy_review.rs", 0, 36, 'D'),
        ("assets/diff-split.svg", 12, 0, 'A'),
        ("Cargo.toml", 2, 1, 'M'),
    ];
    files
        .iter()
        .enumerate()
        .map(|(index, (path, added, removed, status))| ToolCall {
            id: format!("card-{index}"),
            name: "edit_file".to_string(),
            arguments: format!(r#"{{"path":"{path}"}}"#),
            status: ToolCallStatus::Finished,
            result: Some("ok".to_string()),
            diff: Some(card_diff(path, *added, *removed, *status)),
        })
        .collect()
}

fn card_diff(path: &str, added: u32, removed: u32, status: char) -> String {
    let mut diff = String::new();
    match status {
        'A' => {
            diff.push_str("--- /dev/null\n");
            diff.push_str(&format!("+++ b/{path}\n"));
            diff.push_str(&format!("@@ -0,0 +1,{added} @@\n"));
            for line in 0..added {
                diff.push_str(&format!("+review line {line}\n"));
            }
        }
        'D' => {
            diff.push_str(&format!("--- a/{path}\n"));
            diff.push_str("+++ /dev/null\n");
            diff.push_str(&format!("@@ -1,{removed} +0,0 @@\n"));
            for line in 0..removed {
                diff.push_str(&format!("-review line {line}\n"));
            }
        }
        _ => {
            diff.push_str(&format!("--- a/{path}\n"));
            diff.push_str(&format!("+++ b/{path}\n"));
            diff.push_str(&format!("@@ -1,{} +1,{} @@\n", removed + 1, added + 1));
            diff.push_str(" review line 0\n");
            for line in 0..removed {
                diff.push_str(&format!("-old line {line}\n"));
            }
            for line in 0..added {
                diff.push_str(&format!("+new line {line}\n"));
            }
        }
    }
    diff
}

fn reviewer_list_scene(app: &AppContext) -> Box<dyn Element> {
    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(label(app, "Changed files", 14.0, FontWeight::Medium, ColorToken::Text))
                    .with_child(Expanded::new(Empty::new().finish()).finish())
                    .with_child(counts(app, 140, 41, 11.0))
                    .finish(),
            )
            .with_padding(EdgeInsets::new(0.0, 14.0, 14.0, 14.0))
            .finish(),
        )
        .with_child(hairline());
    for (index, file) in FILES.iter().enumerate() {
        column = column.with_child(
            Container::new(file_row(app, file, index == 2))
                .with_padding(EdgeInsets::new(0.0, 8.0, 0.0, 8.0))
                .finish(),
        );
    }
    column = column.with_child(
        Container::new(
            Text::new("M modified · A added · D deleted")
                .with_font_size(9.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_padding(EdgeInsets::new(8.0, 12.0, 8.0, 12.0))
        .finish(),
    );
    Container::new(
        Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
            .with_corner_radius(12.0)
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(16.0))
    .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
    .finish()
}

fn hunk_scene(app: &AppContext, split: bool) -> Box<dyn Element> {
    reviewer_shell(
        app,
        &FILES,
        Some(2),
        split,
        false,
        diff_body(app, vec![review_hunk()], split, false, None, None),
        false,
    )
}

fn one_sided_scene(app: &AppContext, add: bool, split: bool) -> Box<dyn Element> {
    let hunks = if add { vec![add_hunk()] } else { vec![del_hunk()] };
    let selected = if add { 1 } else { 3 };
    let note = if add {
        if split {
            "New file · split requested; the current comparison leaves the old half empty."
        } else {
            "New file"
        }
    } else if split {
        "Deleted file · split requested; the current comparison leaves the new half empty."
    } else {
        "Deleted file"
    };
    reviewer_shell(
        app,
        &FILES,
        Some(selected),
        split,
        false,
        diff_body(app, hunks, split, false, Some(note), None),
        false,
    )
}

fn empty_diff_scene(app: &AppContext) -> Box<dyn Element> {
    let body = empty_state(
        app,
        "check",
        ColorToken::Success,
        "No changes in this file",
        "The file matches the turn's start.",
    );
    reviewer_shell(app, &FILES, Some(6), false, false, body, false)
}

fn clean_scene(app: &AppContext) -> Box<dyn Element> {
    let body = empty_state(
        app,
        "check",
        ColorToken::Success,
        "Working tree clean",
        "No open changes to review.",
    );
    reviewer_shell(app, &[], None, false, false, body, false)
}

fn selector_open_scene(app: &AppContext) -> Box<dyn Element> {
    let shell = reviewer_shell(
        app,
        &FILES,
        Some(2),
        false,
        false,
        diff_body(app, vec![review_hunk()], false, false, None, None),
        true,
    );
    let items: Vec<PopupMenuItem> = FILES
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let item = PopupMenuItem::new(format!("{}  {}", file.status, file.path))
                .with_icon("file")
                .with_diff(file.added, file.removed);
            if index == 2 { item.selected() } else { item }
        })
        .collect();
    let menu = PopupMenu::new(selector_button(app, FILES[2].path, true), items)
        .with_open(Rc::new(RefCell::new(true)))
        .with_selected_color(ColorToken::Text)
        .with_max_width(360.0)
        .with_position(PopupMenuPosition::Below)
        .with_panel_offset(vec2f(0.0, 4.0));
    Stack::new()
        .with_children(vec![shell])
        // The rail is 248pt plus its 1pt divider; the selector begins after the
        // header's 8pt inset. The overlay carries the identical trigger, so the
        // base one is covered and the real menu panel paints over the body.
        .with_overlay(Box::new(menu), vec2f(RAIL_WIDTH + 1.0 + 8.0, 0.0))
        .finish()
}

fn wrap_scene(app: &AppContext, wrap: bool) -> Box<dyn Element> {
    reviewer_shell(
        app,
        &FILES,
        Some(2),
        false,
        wrap,
        diff_body(app, vec![long_hunk()], false, wrap, None, None),
        false,
    )
}

fn highlight_gap_scene(app: &AppContext) -> Box<dyn Element> {
    reviewer_shell(
        app,
        &FILES,
        Some(2),
        false,
        false,
        diff_body(
            app,
            vec![review_hunk()],
            false,
            false,
            Some("Highlighting comparison · note the missing spaces around syntax runs."),
            Some("rust"),
        ),
        false,
    )
}

fn floating_file_panel(app: &AppContext) -> Box<dyn Element> {
    let backdrop = Container::new(
        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(label(app, "Review session", 18.0, FontWeight::Medium, ColorToken::Muted))
            .with_child(
                Container::new(
                    Text::new("The diff reviewer can float over the conversation without taking the pane.")
                        .with_font_size(12.0)
                        .with_line_height(20.0 / 12.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_padding(EdgeInsets::new(10.0, 0.0, 0.0, 0.0))
                .finish(),
            )
            .with_child(
                Container::new(
                    Text::new("Select a changed file to inspect its comparison, then close the panel to return to the turn.")
                        .with_font_size(12.0)
                        .with_line_height(20.0 / 12.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_padding(EdgeInsets::new(8.0, 0.0, 0.0, 0.0))
                .finish(),
            )
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(48.0))
    .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
    .finish();

    let header = ConstrainedBox::new(
        Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(10.0)
                .with_child(
                    Container::new(icon(app, "file-rust", 20.0, ColorToken::Muted))
                        .with_padding(EdgeInsets::uniform(10.0))
                        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                        .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
                        .with_corner_radius(10.0)
                        .finish(),
                )
                .with_child(
                    Flex::column()
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(2.0)
                        .with_child(
                            Text::new("src/panes.rs")
                                .with_font_size(13.0)
                                .with_weight(FontWeight::Medium)
                                .with_theme_color(ColorToken::Text, app)
                                .finish(),
                        )
                        .with_child(counts(app, 18, 4, 10.0))
                        .finish(),
                )
                .with_child(Expanded::new(Empty::new().finish()).finish())
                .with_child(tool_glyph(app, "copy", false))
                .with_child(tool_glyph(app, "maximize-01", false))
                .with_child(tool_glyph(app, "close", false))
                .finish(),
        )
        .with_padding(EdgeInsets::new(8.0, 10.0, 8.0, 10.0))
        .finish(),
    )
    .with_height(DiffSpec::CHANGED_FILES_HEADER_HEIGHT)
    .finish();

    let panel = Container::new(
        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(header)
            .with_child(hairline())
            .with_child(Expanded::new(diff_body(
                app,
                vec![review_hunk()],
                false,
                false,
                None,
                None,
            ))
            .finish())
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
    .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
    .with_corner_radius(14.0)
    .finish();
    let panel = ConstrainedBox::new(panel).with_width(650.0).with_height(440.0).finish();

    Stack::new()
        .with_children(vec![backdrop])
        .with_overlay(
            Container::new(Empty::new().with_size(vec2f(650.0, 440.0)).finish())
                .with_background(Fill::Solid(goble_ui::color::ColorU::new(0, 0, 0, 14)))
                .with_corner_radius(16.0)
                .finish(),
            vec2f(482.0, 188.0),
        )
        .with_overlay(
            Container::new(Empty::new().with_size(vec2f(650.0, 440.0)).finish())
                .with_background(Fill::Solid(goble_ui::color::ColorU::new(0, 0, 0, 12)))
                .with_corner_radius(16.0)
                .finish(),
            vec2f(482.0, 196.0),
        )
        .with_overlay(
            Container::new(Empty::new().with_size(vec2f(650.0, 440.0)).finish())
                .with_background(Fill::Solid(goble_ui::color::ColorU::new(0, 0, 0, 8)))
                .with_corner_radius(16.0)
                .finish(),
            vec2f(482.0, 204.0),
        )
        .with_overlay(panel, vec2f(482.0, 180.0))
        .finish()
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> anyhow::Result<()> {
    let dir = PathBuf::from(OUT_DIR);
    std::fs::create_dir_all(&dir)?;
    let assets_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/goble-ui/assets")
        .canonicalize()?;
    let assets = raster::PreviewAssets::load(&assets_dir)?;

    for (theme_label, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        let app = AppContext {
            theme,
            ..AppContext::default()
        };
        let mut scenes: Vec<(&str, Box<dyn Element>, f32, f32)> = vec![
            ("changed-files-card-folded", changed_files_card_scene(&app, false), 900.0, 560.0),
            ("changed-files-card-expanded", changed_files_card_scene(&app, true), 900.0, 560.0),
            ("file-reviewer-list", reviewer_list_scene(&app), 380.0, 540.0),
            ("filediff-unified-hunk", hunk_scene(&app, false), 1100.0, 680.0),
            ("filediff-split-hunk", hunk_scene(&app, true), 1100.0, 680.0),
            ("filediff-unified-add-only", one_sided_scene(&app, true, false), 1100.0, 560.0),
            ("filediff-unified-del-only", one_sided_scene(&app, false, false), 1100.0, 560.0),
            ("filediff-split-add-only", one_sided_scene(&app, true, true), 1100.0, 560.0),
            ("filediff-split-del-only", one_sided_scene(&app, false, true), 1100.0, 560.0),
            ("filediff-empty", empty_diff_scene(&app), 1100.0, 640.0),
            ("reviewer-clean", clean_scene(&app), 1100.0, 680.0),
            ("filediff-selector-open", selector_open_scene(&app), 1100.0, 720.0),
            ("filediff-wrap-off", wrap_scene(&app, false), 1100.0, 560.0),
            ("filediff-wrap-on", wrap_scene(&app, true), 1100.0, 560.0),
            ("filediff-highlight-gap", highlight_gap_scene(&app), 1100.0, 560.0),
            ("floating-file-panel", floating_file_panel(&app), 1180.0, 760.0),
        ];
        for (name, root, width, height) in &mut scenes {
            let path = dir.join(format!("{name}-{theme_label}.png"));
            raster::render_element_to_png(&assets, &app, root, *width, *height, 2.0, None, &path)?;
        }
        println!("{theme_label}: {} scenes written to {}", scenes.len(), dir.display());
    }
    println!(
        "note: comparison bodies are the product FileDiffView; the changed-files card is the \
         shipped ChatView card. The review shell and floating frame are composed from public \
         elements because the app's review tab is still pub(crate)."
    );
    Ok(())
}
