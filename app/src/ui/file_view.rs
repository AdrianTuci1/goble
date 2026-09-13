//! The pane that shows one file: the text of the file the explorer or a search
//! result handed over, with a line-number gutter.
//!
//! Reading is the explorer's rule — a frame that changes nothing reads nothing.
//! A view keeps the file's lines in a cache beside the state and re-reads it only
//! when its size or modification time moved, so a file being written under the
//! view updates it without a read per frame. A file view is a pane leaf like any
//! other, so it sits beside the terminal that opened it and comes back on the
//! next launch with the same file open.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::time::SystemTime;

use goble_ui::elements::{
    file_icon_name, AppContext, Axis, Container, CrossAxisAlignment, EdgeInsets, Element, Empty,
    Expanded, Fill, Flex, Icon, MainAxisSize, Scrollable, Spacer, Stack, Text, Tooltip,
    TooltipPosition,
};
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, FontFamily, SpacingToken};

use super::chat;
use super::{UiActions, UiSnapshot};

/// The largest file a view opens, in bytes. Opening a file is a read of the
/// machine into the element tree, so it has a ceiling; past it the pane says how
/// big the file is instead of hanging the frame.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// The most lines a view draws. Every line is an element in the frame, and a
/// handful of files past this is more than a pane can show usefully; the view
/// says how many lines it left out.
const MAX_LINES: usize = 2_000;
/// How many monospace digits the line-number gutter reserves. The number is
/// padded to it, so the columns line up under a monospace face.
const GUTTER_DIGITS: usize = 5;
/// The size the file's own lines are drawn at, and their line box.
const LINE_FONT_SIZE: f32 = 12.0;
const LINE_HEIGHT: f32 = 1.35;

/// What a file view draws for one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileBody {
    /// The file's text: the lines the view draws, and how many lines the file
    /// has in total (a file past [`MAX_LINES`] has more than it shows).
    Lines {
        shown: Rc<Vec<String>>,
        total: usize,
    },
    /// A file past [`MAX_FILE_BYTES`], with its size.
    TooLarge { bytes: u64 },
    /// Bytes that are not text: a UTF-8 read that failed, or a NUL byte.
    NotText,
    /// The path is not a readable file at all.
    Unreadable,
}

impl FileBody {
    /// The one-line description the view's title shows beside the name.
    fn detail(&self) -> String {
        match self {
            FileBody::Lines { shown, total } => {
                if *total > shown.len() {
                    format!("{} lines · showing the first {}", total, shown.len())
                } else {
                    format!("{} lines", total)
                }
            }
            FileBody::TooLarge { bytes } => bytes_label(*bytes),
            FileBody::NotText => "not text".to_string(),
            FileBody::Unreadable => "not readable".to_string(),
        }
    }

    /// What the pane says instead of the file when there are no lines to draw.
    fn message(&self) -> String {
        match self {
            FileBody::TooLarge { bytes } => {
                format!("This file is {} — too large to show.", bytes_label(*bytes))
            }
            FileBody::NotText => "This file is not text.".to_string(),
            FileBody::Unreadable => "This file cannot be read.".to_string(),
            FileBody::Lines { .. } => String::new(),
        }
    }
}

/// A size as the title spells it: whole kilobytes and megabytes, because the
/// exact byte count of a file this view refuses to open is noise.
fn bytes_label(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let kb = bytes as f64 / KB;
    if kb < KB {
        format!("{kb:.0} KB")
    } else {
        format!("{:.1} MB", kb / KB)
    }
}

/// The files the open views have read, so a frame that changes nothing reads no
/// file. A file whose size or modification time moved is read again; the cache
/// drops every file no view shows any more, so closing a view frees it.
#[derive(Default)]
pub(crate) struct FileCache {
    files: HashMap<String, Cached>,
}

struct Cached {
    len: u64,
    modified: Option<SystemTime>,
    body: FileBody,
}

impl FileCache {
    /// The body of every file the listed panes show, keyed by pane id. `wanted`
    /// is the file every open view points at, so the cache holds exactly the
    /// files on screen.
    pub fn read(&mut self, wanted: &[(u64, String)]) -> HashMap<u64, FileBody> {
        let mut bodies = HashMap::new();
        for (pane_id, path) in wanted {
            bodies.insert(*pane_id, self.body(path));
        }
        self.files
            .retain(|path, _| wanted.iter().any(|(_, wanted)| wanted == path));
        bodies
    }

    fn body(&mut self, path: &str) -> FileBody {
        let stamp = fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.len(), metadata.modified().ok()));
        if let Some(cached) = self.files.get(path) {
            if Some((cached.len, cached.modified)) == stamp {
                return cached.body.clone();
            }
        }
        let body = read(path);
        if let Some((len, modified)) = stamp {
            self.files.insert(
                path.to_string(),
                Cached {
                    len,
                    modified,
                    body: body.clone(),
                },
            );
        }
        body
    }
}

/// Read one file into the body a view draws.
fn read(path: &str) -> FileBody {
    let Ok(metadata) = fs::metadata(path) else {
        return FileBody::Unreadable;
    };
    if !metadata.is_file() {
        return FileBody::Unreadable;
    }
    if metadata.len() > MAX_FILE_BYTES {
        return FileBody::TooLarge {
            bytes: metadata.len(),
        };
    }
    let Ok(bytes) = fs::read(path) else {
        return FileBody::Unreadable;
    };
    // A NUL near the start is the cheap test for a binary file: rendering PNGs
    // and object files as text helps nobody, and the alternative is a pane full
    // of replacement glyphs.
    if bytes.iter().take(8 * 1024).any(|byte| *byte == 0) {
        return FileBody::NotText;
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return FileBody::NotText;
    };
    let total = text.lines().count();
    let shown = text
        .lines()
        .take(MAX_LINES)
        .map(|line| line.to_string())
        .collect();
    FileBody::Lines {
        shown: Rc::new(shown),
        total,
    }
}

/// The pane's view of `path`: the pane's one topbar, the file's name and size,
/// and the file's lines under a line-number gutter.
pub(crate) fn build_file_view(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    path: &str,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    // The pane wears the same topbar as its siblings (the close button and the
    // app's own tray), so a pane is closed the same way whatever it hosts.
    let header = chat::build_agent_header(app, state, actions, pane_id, false);
    let body = match state.pane_files.get(&pane_id) {
        Some(FileBody::Lines { shown, total }) => {
            lines_view(app, state, pane_id, Rc::clone(shown), *total)
        }
        Some(body) => note(app, &body.message()),
        // The body is read while the snapshot is built, so a pane with no body
        // yet is a frame old at most.
        None => note(app, "Reading the file…"),
    };

    let column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm)
        // The topbar floats over the content, so the body starts under it.
        .with_child(
            Empty::new()
                .with_size(Vector2F::new(0.0, super::shell::TOPBAR_HEIGHT))
                .finish(),
        )
        .with_child(title_row(app, path, state.pane_files.get(&pane_id)))
        .with_child(Expanded::new(body).finish())
        .finish();
    let content = Container::new(column)
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .with_padding(EdgeInsets::new(sm, sm, sm, sm))
        .finish();
    Stack::new()
        .with_children(vec![content])
        .with_overlay(header, Vector2F::zero())
        .finish()
}

/// The file's name and how much of it is shown, with the full path on the name's
/// own hover.
fn title_row(app: &AppContext, path: &str, body: Option<&FileBody>) -> Box<dyn Element> {
    let name = Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());
    let name_text = Text::new(name.clone())
        .with_font_size(13.0)
        .with_theme_color(ColorToken::Text, app)
        .with_max_lines(1)
        .finish();
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(
            Icon::new(file_icon_name(&name))
                .with_size(16.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Tooltip::new(name_text, path.to_string())
                .with_position(TooltipPosition::Below)
                .finish(),
        )
        .with_child(Spacer::new().finish());
    if let Some(body) = body {
        row = row.with_child(
            Text::new(body.detail())
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    row.finish()
}

/// The file's lines: one row per line, a padded line number then the text, in
/// the pane's own scroll offset so a long file scrolls and keeps its place.
fn lines_view(
    app: &AppContext,
    state: &UiSnapshot,
    pane_id: u64,
    lines: Rc<Vec<String>>,
    total: usize,
) -> Box<dyn Element> {
    let mut list = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    for (index, line) in lines.iter().enumerate() {
        list = list.with_child(line_row(app, index + 1, line));
    }
    if total > lines.len() {
        list = list.with_child(note(
            app,
            &format!("… {} more lines", total - lines.len()),
        ));
    }
    // A file view opens at its first line rather than tailing, so its offset is
    // the pane's plain scroll state (not the terminal's following one).
    let scroll = state
        .file_scroll
        .get(&pane_id)
        .cloned()
        .unwrap_or_default();
    Scrollable::new(list.finish(), Axis::Vertical)
        .with_state(scroll)
        .finish()
}

fn line_row(app: &AppContext, number: usize, line: &str) -> Box<dyn Element> {
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Text::new(format!("{:>width$} ", number, width = GUTTER_DIGITS))
                .with_font_size(LINE_FONT_SIZE)
                .with_line_height(LINE_HEIGHT)
                .with_font_family(FontFamily::Mono)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(1)
                .finish(),
        )
        .with_child(
            Text::new(line.to_string())
                .with_font_size(LINE_FONT_SIZE)
                .with_line_height(LINE_HEIGHT)
                .with_font_family(FontFamily::Mono)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
        )
        .finish()
}

/// A muted line saying why the file is not drawn.
fn note(app: &AppContext, text: &str) -> Box<dyn Element> {
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    Container::new(
        Text::new(text.to_string())
            .with_font_size(11.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(xs))
    .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_read_into_its_lines_and_counted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("main.rs");
        fs::write(&path, "fn main() {}\n\nlet x = 1;\n").expect("write");
        let path = path.to_string_lossy().to_string();

        let mut cache = FileCache::default();
        let bodies = cache.read(&[(7, path.clone())]);
        match bodies.get(&7) {
            Some(FileBody::Lines { shown, total }) => {
                assert_eq!(shown.as_slice(), ["fn main() {}", "", "let x = 1;"]);
                assert_eq!(*total, 3);
            }
            other => panic!("expected the file's lines, got {other:?}"),
        }
    }

    /// A frame that changes nothing reads nothing: the second read of an
    /// unchanged file is the cached body, and a file that changed is read again.
    #[test]
    fn an_unchanged_file_is_not_read_again_and_a_changed_one_is() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("notes.md");
        fs::write(&path, "one\n").expect("write");
        let path = path.to_string_lossy().to_string();
        let wanted = vec![(1, path.clone())];

        let mut cache = FileCache::default();
        let first = cache.read(&wanted);
        let cached = cache.files.get(&path).expect("the file is cached").body.clone();
        let second = cache.read(&wanted);
        assert_eq!(first.get(&1), second.get(&1), "the same file, the same body");
        assert_eq!(second.get(&1), Some(&cached));

        // A different size re-reads it (the modification time can share a
        // timestamp with the previous write on a coarse clock).
        fs::write(&path, "one\ntwo\n").expect("write again");
        let third = cache.read(&wanted);
        match third.get(&1) {
            Some(FileBody::Lines { total, .. }) => assert_eq!(*total, 2, "the new text is read"),
            other => panic!("expected the new lines, got {other:?}"),
        }

        // A view that is gone drops its file.
        cache.read(&[]);
        assert!(cache.files.is_empty(), "no open view keeps no file");
    }

    #[test]
    fn a_binary_file_a_missing_path_and_a_directory_are_reported_not_drawn() {
        let dir = tempfile::tempdir().expect("temp dir");
        let binary = dir.path().join("logo.png");
        fs::write(&binary, [0x89, b'P', b'N', b'G', 0x00, 0x1a]).expect("write");

        let mut cache = FileCache::default();
        assert_eq!(
            cache.read(&[(1, binary.to_string_lossy().to_string())]).get(&1),
            Some(&FileBody::NotText)
        );
        assert_eq!(
            cache
                .read(&[(2, dir.path().join("gone.rs").to_string_lossy().to_string())])
                .get(&2),
            Some(&FileBody::Unreadable)
        );
        assert_eq!(
            cache.read(&[(3, dir.path().to_string_lossy().to_string())]).get(&3),
            Some(&FileBody::Unreadable),
            "a directory is not a file to open"
        );
    }

    /// Only the first `MAX_LINES` lines are drawn, and the total is still the
    /// file's own count, so the view can say what it left out.
    #[test]
    fn a_long_file_is_drawn_up_to_the_cap_and_counted_in_full() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("long.txt");
        let text: String = (0..MAX_LINES + 25).map(|i| format!("line {i}\n")).collect();
        fs::write(&path, text).expect("write");

        let mut cache = FileCache::default();
        let bodies = cache.read(&[(4, path.to_string_lossy().to_string())]);
        match bodies.get(&4) {
            Some(FileBody::Lines { shown, total }) => {
                assert_eq!(shown.len(), MAX_LINES);
                assert_eq!(*total, MAX_LINES + 25);
                assert_eq!(shown[0], "line 0");
            }
            other => panic!("expected lines, got {other:?}"),
        }
    }
}
