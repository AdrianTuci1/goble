//! The pane that shows one file: the text of the file the explorer or a search
//! result handed over, with a line-number gutter.
//!
//! Reading is the explorer's rule — a frame that changes nothing reads nothing.
//! A view keeps the file's lines in a cache beside the state and re-reads it only
//! when its size or modification time moved, so a file being written under the
//! view updates it without a read per frame. The highlighted runs are built in
//! that same cache entry, so a frame that changes nothing re-highlights nothing
//! either. A file view is a pane leaf like any other, so it sits beside the
//! terminal that opened it and comes back on the next launch with the same file
//! open.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::time::SystemTime;

use goble_ui::elements::{
    file_icon_name, AppContext, Axis, Code, Container, CrossAxisAlignment, EdgeInsets, Element,
    Empty, Expanded, Fill, Flex, Icon, MainAxisSize, Scrollable, Spacer, Stack, Text, Tooltip,
    TooltipPosition,
};
use goble_ui::geometry::Vector2F;
use goble_ui::syntax::HighlightedLine;
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

/// What a file view draws for one file: the body, and — when the file's own
/// type resolves — the body's lines already broken into the highlighted runs
/// the pane paints.
///
/// The runs live beside the body rather than inside it because a run carries a
/// click callback and so is neither `PartialEq` nor `Eq`, which [`FileBody`]
/// is; the two are still read and cached together (see [`FileCache`]).
#[derive(Clone)]
pub struct FileContent {
    body: FileBody,
    /// One entry per line the body shows, in the same order. `None` — a
    /// language that did not resolve, or a file that is not text at all —
    /// keeps the pane drawing its plain lines.
    runs: Option<Rc<Vec<HighlightedLine>>>,
}

/// A run carries a click callback, so the runs themselves are not printable;
/// how many sets there are is what a failing assertion needs to see.
impl std::fmt::Debug for FileContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileContent")
            .field("body", &self.body)
            .field("runs", &self.runs.as_ref().map(|runs| runs.len()))
            .finish()
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
    /// The body and its highlighted runs, read once under the one size +
    /// modification-time stamp: a frame that changes nothing re-reads and
    /// re-highlights nothing.
    content: FileContent,
}

impl FileCache {
    /// What every file the listed panes show draws, keyed by pane id. `wanted`
    /// is the file every open view points at, so the cache holds exactly the
    /// files on screen.
    pub fn read(&mut self, wanted: &[(u64, String)]) -> HashMap<u64, FileContent> {
        let mut contents = HashMap::new();
        for (pane_id, path) in wanted {
            contents.insert(*pane_id, self.content(path));
        }
        self.files
            .retain(|path, _| wanted.iter().any(|(_, wanted)| wanted == path));
        contents
    }

    fn content(&mut self, path: &str) -> FileContent {
        let stamp = fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.len(), metadata.modified().ok()));
        if let Some(cached) = self.files.get(path) {
            if Some((cached.len, cached.modified)) == stamp {
                return cached.content.clone();
            }
        }
        let content = read(path);
        if let Some((len, modified)) = stamp {
            self.files.insert(
                path.to_string(),
                Cached {
                    len,
                    modified,
                    content: content.clone(),
                },
            );
        }
        content
    }
}

/// Read one file into what its view draws, and nothing otherwise.
fn read(path: &str) -> FileContent {
    /// A file the view will not draw.
    fn none(body: FileBody) -> FileContent {
        FileContent { body, runs: None }
    }

    let Ok(metadata) = fs::metadata(path) else {
        return none(FileBody::Unreadable);
    };
    if !metadata.is_file() {
        return none(FileBody::Unreadable);
    }
    if metadata.len() > MAX_FILE_BYTES {
        return none(FileBody::TooLarge {
            bytes: metadata.len(),
        });
    }
    let Ok(bytes) = fs::read(path) else {
        return none(FileBody::Unreadable);
    };
    // A NUL near the start is the cheap test for a binary file: rendering PNGs
    // and object files as text helps nobody, and the alternative is a pane full
    // of replacement glyphs.
    if bytes.iter().take(8 * 1024).any(|byte| *byte == 0) {
        return none(FileBody::NotText);
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return none(FileBody::NotText);
    };
    let total = text.lines().count();
    let shown: Vec<String> = text
        .lines()
        .take(MAX_LINES)
        .map(|line| line.to_string())
        .collect();
    let runs = highlight(&shown, path);
    FileContent {
        body: FileBody::Lines {
            shown: Rc::new(shown),
            total,
        },
        runs,
    }
}

/// The lines this pane draws as the highlighted runs for the file's own type —
/// the same [`goble_ui::syntax::highlight`] every other surface colours code
/// with, resolved from the path itself. `None` when the language does not
/// resolve (an unknown extension, a file whose name has none), which is the
/// pane's cue to draw the text plain.
///
/// Only the lines the pane shows are handed over, so [`MAX_LINES`] bounds this
/// exactly as it bounds the body, and the whole thing runs inside [`FileCache`]
/// with the read rather than per frame.
fn highlight(lines: &[String], path: &str) -> Option<Rc<Vec<HighlightedLine>>> {
    goble_ui::syntax::highlight(&lines.join("\n"), path).map(Rc::new)
}

/// The pane's view of `path`: the pane's one topbar, the file's name and size,
/// and the file's lines, highlighted by the file's own type, under a
/// line-number gutter.
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
    let file = state.pane_files.get(&pane_id);
    let body = match file {
        Some(FileContent {
            body: FileBody::Lines { shown, total },
            runs,
        }) => lines_view(app, state, pane_id, Rc::clone(shown), runs.as_ref(), *total),
        Some(content) => note(app, &content.body.message()),
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
        .with_child(title_row(app, path, file.map(|file| &file.body)))
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
///
/// `runs` are the lines already highlighted for the file's type, one entry per
/// line, or `None` when the type did not resolve — a line without runs of its
/// own (or a file without any) is drawn plain.
fn lines_view(
    app: &AppContext,
    state: &UiSnapshot,
    pane_id: u64,
    lines: Rc<Vec<String>>,
    runs: Option<&Rc<Vec<HighlightedLine>>>,
    total: usize,
) -> Box<dyn Element> {
    let mut list = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    for (index, line) in lines.iter().enumerate() {
        let runs = runs
            .and_then(|runs| runs.get(index))
            .filter(|runs| !runs.is_empty());
        list = list.with_child(line_row(app, index + 1, line, runs));
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

/// One line: the padded number, then the line itself — its highlighted runs
/// when the file's type resolved, its plain text when it did not.
///
/// The runs are painted through the shared `Code` element, the same text model
/// the chat's code blocks and read excerpts use: the runs carry their own
/// colours, and it keeps the mono, unwrapped line this pane draws either way.
fn line_row(
    app: &AppContext,
    number: usize,
    line: &str,
    runs: Option<&HighlightedLine>,
) -> Box<dyn Element> {
    let text: Box<dyn Element> = match runs {
        // One line's runs are one entry of the lines `Code` paints.
        Some(runs) => Code::new(line.to_string())
            .with_font_size(LINE_FONT_SIZE)
            .with_line_height(LINE_HEIGHT)
            .with_highlighted_lines(vec![runs.clone()])
            .finish(),
        None => Text::new(line.to_string())
            .with_font_size(LINE_FONT_SIZE)
            .with_line_height(LINE_HEIGHT)
            .with_font_family(FontFamily::Mono)
            .with_theme_color(ColorToken::Text, app)
            .with_max_lines(1)
            .finish(),
    };
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
        .with_child(text)
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
        let contents = cache.read(&[(7, path.clone())]);
        match contents.get(&7).map(|content| &content.body) {
            Some(FileBody::Lines { shown, total }) => {
                assert_eq!(shown.as_slice(), ["fn main() {}", "", "let x = 1;"]);
                assert_eq!(*total, 3);
            }
            other => panic!("expected the file's lines, got {other:?}"),
        }
    }

    /// A frame that changes nothing reads nothing: the second read of an
    /// unchanged file is the cached body — and the cached runs, not a second
    /// highlight — while a file that changed is read again.
    #[test]
    fn an_unchanged_file_is_not_read_again_and_a_changed_one_is() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("notes.md");
        fs::write(&path, "one\n").expect("write");
        let path = path.to_string_lossy().to_string();
        let wanted = vec![(1, path.clone())];

        let mut cache = FileCache::default();
        let first = cache.read(&wanted);
        let cached = cache
            .files
            .get(&path)
            .expect("the file is cached")
            .content
            .clone();
        let second = cache.read(&wanted);
        assert_eq!(
            first.get(&1).map(|content| &content.body),
            second.get(&1).map(|content| &content.body),
            "the same file, the same body"
        );
        assert_eq!(
            second.get(&1).map(|content| &content.body),
            Some(&cached.body)
        );
        let runs = |contents: &HashMap<u64, FileContent>| {
            contents
                .get(&1)
                .and_then(|content| content.runs.as_ref())
                .expect("markdown resolves, so the lines carry runs")
                .clone()
        };
        assert!(
            Rc::ptr_eq(&runs(&first), &runs(&second)),
            "an unchanged file is not highlighted again: the cached runs are handed back"
        );

        // A different size re-reads it (the modification time can share a
        // timestamp with the previous write on a coarse clock).
        fs::write(&path, "one\ntwo\n").expect("write again");
        let third = cache.read(&wanted);
        match third.get(&1).map(|content| &content.body) {
            Some(FileBody::Lines { total, .. }) => assert_eq!(*total, 2, "the new text is read"),
            other => panic!("expected the new lines, got {other:?}"),
        }
        assert!(
            !Rc::ptr_eq(&runs(&first), &runs(&third)),
            "a file that changed is highlighted again"
        );

        // A view that is gone drops its file.
        cache.read(&[]);
        assert!(cache.files.is_empty(), "no open view keeps no file");
    }

    /// The pane's lines are coloured by the file's own type — every type the
    /// same one call resolves: a `config.toml` carries one set of runs per line
    /// it shows, and so do `.rs`, `.md` and `.json` — while a file whose type
    /// does not resolve carries none. The runs are read, and cached, with the
    /// lines.
    #[test]
    fn a_files_lines_carry_the_runs_of_its_own_type_or_none_at_all() {
        let dir = tempfile::tempdir().expect("temp dir");
        let names = [
            "config.toml",
            "main.rs",
            "notes.md",
            "data.json",
            "deploy.sh",
            "notes.zzz",
        ];
        let wanted: Vec<(u64, String)> = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let path = dir.path().join(name);
                fs::write(&path, "model = \"gpt-4o\"\napi_key = \"sk-test\"\n").expect("write");
                (index as u64, path.to_string_lossy().to_string())
            })
            .collect();

        let mut cache = FileCache::default();
        let contents = cache.read(&wanted);

        for (index, name) in names.iter().enumerate() {
            let content = contents
                .get(&(index as u64))
                .unwrap_or_else(|| panic!("{name} is read"));
            if *name == "notes.zzz" {
                assert!(
                    content.runs.is_none(),
                    "a type that does not resolve keeps the pane on plain lines"
                );
                continue;
            }
            let runs = content
                .runs
                .as_ref()
                .unwrap_or_else(|| panic!("{name} resolves its own language"));
            assert_eq!(runs.len(), 2, "{name}: one set of runs per line shown");
            if matches!(*name, "config.toml" | "main.rs") {
                // These two grammars do colour this text: the key, the `=` and
                // the string value are not one colour.
                let colours: std::collections::HashSet<_> =
                    runs.iter().flatten().map(|span| span.color).collect();
                assert!(
                    colours.len() > 1,
                    "{name}: a key and its string must not come out one colour: {colours:?}"
                );
            }
        }
    }

    #[test]
    fn a_binary_file_a_missing_path_and_a_directory_are_reported_not_drawn() {
        let dir = tempfile::tempdir().expect("temp dir");
        let binary = dir.path().join("logo.png");
        fs::write(&binary, [0x89, b'P', b'N', b'G', 0x00, 0x1a]).expect("write");

        let mut cache = FileCache::default();
        let body = |contents: HashMap<u64, FileContent>, pane: u64| {
            contents.get(&pane).map(|content| content.body.clone())
        };
        assert_eq!(
            body(cache.read(&[(1, binary.to_string_lossy().to_string())]), 1),
            Some(FileBody::NotText)
        );
        assert_eq!(
            body(
                cache.read(&[(2, dir.path().join("gone.rs").to_string_lossy().to_string())]),
                2
            ),
            Some(FileBody::Unreadable)
        );
        assert_eq!(
            body(
                cache.read(&[(3, dir.path().to_string_lossy().to_string())]),
                3
            ),
            Some(FileBody::Unreadable),
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
        let contents = cache.read(&[(4, path.to_string_lossy().to_string())]);
        match contents.get(&4).map(|content| &content.body) {
            Some(FileBody::Lines { shown, total }) => {
                assert_eq!(shown.len(), MAX_LINES);
                assert_eq!(*total, MAX_LINES + 25);
                assert_eq!(shown[0], "line 0");
            }
            other => panic!("expected lines, got {other:?}"),
        }
    }
}
