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
//!
//! A frame that changes nothing still rebuilds the tree, and every line of a file
//! is an element of it, so the read-only body draws only the lines the pane's own
//! viewport can show and stands the rest in as spacers of the height they would
//! take. The read body keeps the whole file's scroll range, and a file of
//! [`MAX_LINES`] lines costs what a screenful of them costs.
//!
//! A file the read took whole is editable: the pane draws it as a multi-line
//! field over an editor buffer, and Cmd/Ctrl+S writes that buffer back to the
//! file. A file the read did not take whole stays read-only — writing the part
//! of it the pane holds over the rest would destroy the file.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::SystemTime;

use goble_ui::elements::diff::{DiffHighlightJob, PreparedDiff};
use goble_ui::elements::interactive::{contains, handle_mouse_event, InteractiveState};
use goble_ui::elements::{
    file_icon_name, AppContext, Axis, Button, ButtonVariant, Code, ConstrainedBox,
    Container, CrossAxisAlignment, Dialog, EdgeInsets, Element, Empty, EventContext, Expanded,
    Fill, Flex,
    Icon, KeyHandler, LayoutContext, LineRuns, MainAxisSize, PaintContext, Point, Scrollable,
    SizeConstraint, Spacer, Stack, Text, TextArea, Tooltip, TooltipPosition, TopbarButton,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, Vector2F};
use goble_ui::syntax::{HighlightedLine, HighlightedSpans};
use goble_ui::theme::{ColorToken, FontFamily, SpacingToken};

use super::chat;
use super::{UiActions, UiSnapshot};
use crate::ui::PaneKind;

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
/// One line's box: what a row of the body is laid out at, and so what a spacer
/// standing in for a line the pane is not drawing has to take. A highlighted
/// row's `Code` measures `ceil(LINE_FONT_SIZE * LINE_HEIGHT)` and a plain row's
/// `Text` measures that line box unrounded, so the plain body is held to this
/// height — one row of either kind measures exactly this, which a test pins.
const LINE_BOX: f32 = 17.0;
/// How many lines the read-only body draws before the pane has reported a
/// viewport to size itself against. The first frame has no measurement, so it
/// draws a screenful for the tallest pane; the frame after draws what fits.
const FIRST_FRAME_LINES: usize = 64;
/// The rows a window keeps beyond the viewport, so a pane scrolled to a line
/// that is not its first still fills, top and bottom.
const WINDOW_SLACK: usize = 4;

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
/// A file's size and modification time: the pair a re-read is keyed on, and
/// what an editor buffer compares the file it was read from against.
type FileStamp = (u64, Option<SystemTime>);

#[derive(Clone)]
pub struct FileContent {
    body: FileBody,
    /// One entry per line the body shows, in the same order. `None` — a
    /// language that did not resolve, or a file that is not text at all —
    /// keeps the pane drawing its plain lines.
    runs: Option<Rc<Vec<HighlightedLine>>>,
    /// The file's text, exactly as it was read: what an editor buffer starts
    /// from and what a save writes back. `None` when the read stopped at a cap,
    /// and for everything that is not text.
    text: Option<Rc<String>>,
    /// The file's size and modification time at the read that produced this
    /// content, which is what tells a buffer the file moved under it.
    stamp: Option<FileStamp>,
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

impl FileContent {
    /// The file's lines as the highlighted runs of its own type, one entry per
    /// line the body shows.
    pub(crate) fn runs(&self) -> Option<&Rc<Vec<HighlightedLine>>> {
        self.runs.as_ref()
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
///
/// The same pass settles each open view's editor buffer (see [`FileBuffer`]):
/// this is the one place that holds both the read that just happened and the
/// text the user is holding, which is what decides whether a file written from
/// outside replaces the buffer or is kept out of it.
#[derive(Default)]
pub(crate) struct FileCache {
    files: HashMap<String, FileContent>,
    /// Every buffer a pane holds, keyed by the pane and the file it edits: a
    /// pane can keep several files open as chips, and switching between them
    /// must not drop the text of the one it leaves.
    buffers: HashMap<(u64, String), Rc<RefCell<FileBuffer>>>,
}

impl FileCache {
    /// What every open view draws, keyed by pane id, for the tests that open
    /// one file per pane: each pane's answer is the last file named for it.
    #[cfg(test)]
    pub fn read(&mut self, wanted: &[(u64, String)]) -> HashMap<u64, FileContent> {
        let active = wanted.iter().fold(HashMap::new(), |mut active, (pane, path)| {
            active.insert(*pane, path.clone());
            active
        });
        self.read_open(wanted, &active)
    }

    /// Settle every open file against its read and answer the active file of
    /// each pane in `active`.
    pub fn read_open(
        &mut self,
        open: &[(u64, String)],
        active: &HashMap<u64, String>,
    ) -> HashMap<u64, FileContent> {
        let mut contents = HashMap::new();
        for (pane_id, path) in open {
            contents.insert((*pane_id, path.clone()), self.content(path));
        }
        self.files
            .retain(|path, _| open.iter().any(|(_, wanted)| wanted == path));
        self.settle(open, &contents);
        active
            .iter()
            .filter_map(|(pane_id, path)| {
                contents
                    .get(&(*pane_id, path.clone()))
                    .map(|content| (*pane_id, content.clone()))
            })
            .collect()
    }

    /// The editors of the open views, keyed by pane and file: shared handles,
    /// so the element tree reads the text, the caret and the markers the state
    /// machine below keeps.
    pub fn buffers(&self) -> HashMap<(u64, String), Rc<RefCell<FileBuffer>>> {
        self.buffers.clone()
    }

    fn content(&mut self, path: &str) -> FileContent {
        let stamp = fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.len(), metadata.modified().ok()));
        if let Some(cached) = self.files.get(path) {
            if stamp.is_some() && cached.stamp == stamp {
                return cached.clone();
            }
        }
        let content = read(path);
        if stamp.is_some() {
            self.files.insert(path.to_string(), content.clone());
        }
        content
    }

    /// Settle every open view's buffer against the read that just happened.
    ///
    /// A file that did not move leaves its buffer alone, edits included. A file
    /// that did is taken into the buffer when the buffer holds nothing unsaved,
    /// and kept out of it — with the pane saying so — when it does: a write from
    /// outside never discards an edit that was not saved. A file that can no
    /// longer be edited whole loses its buffer only while the buffer is clean,
    /// so the pane falls back to the read-only body.
    fn settle(
        &mut self,
        wanted: &[(u64, String)],
        contents: &HashMap<(u64, String), FileContent>,
    ) {
        self.buffers
            .retain(|(pane_id, path), _| wanted.iter().any(|(id, open)| id == pane_id && open == path));
        for (pane_id, path) in wanted {
            let Some(content) = contents.get(&(*pane_id, path.clone())) else {
                continue;
            };
            let Some(buffer) = self.buffers.get(&(*pane_id, path.clone())).cloned() else {
                self.open(*pane_id, path, content);
                continue;
            };
            // A write this buffer made is not a change from outside. The entry
            // it was compared against predates the write, so it is dropped: the
            // next frame re-reads and re-highlights what the save wrote.
            if buffer.borrow_mut().take_written() {
                self.files.remove(path);
                continue;
            }
            if buffer.borrow().stamp() == content.stamp {
                continue;
            }
            if buffer.borrow().dirty() {
                buffer.borrow_mut().note_external_change();
                continue;
            }
            if buffer.borrow().editable() && read_only_reason(content).is_none() {
                buffer.borrow_mut().reload(content);
                continue;
            }
            self.open(*pane_id, path, content);
        }
    }

    fn open(&mut self, pane_id: u64, path: &str, content: &FileContent) {
        self.buffers.insert(
            (pane_id, path.to_string()),
            Rc::new(RefCell::new(FileBuffer::open(path, content))),
        );
    }
}

/// One changed file the review pane has opened for editing: the read (its lines
/// and their highlighted runs) and the buffer the field edits.
pub struct DiffEditor {
    pub content: FileContent,
    pub buffer: Rc<RefCell<FileBuffer>>,
    /// The buffer's lines mapped to the runs of the text they were highlighted
    /// from. Built once per text version and shared by every frame, so a
    /// review that is not edited maps nothing.
    pub runs: Option<Rc<LineRuns>>,
}

impl std::fmt::Debug for DiffEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiffEditor")
            .field("content", &self.content)
            .field("buffer", &self.buffer.borrow())
            .finish()
    }
}

/// How many reads the review keeps between frames. A file scrolled out of view
/// stays here, so scrolling back or showing the file list again does not parse
/// it a second time; the least recently seen ones are dropped above this.
const MAX_REVIEW_FILES: usize = 48;

/// The changed files the review pane has opened for editing.
///
/// It is the same read the file pane does, minus the syntax pass:
/// [`read_without_highlight`] takes the text, and a worker thread colours it
/// with the shared highlighter beside the frame, so opening a review of large
/// files does not freeze the window on syntect. It is keyed by path rather than
/// by pane, since a review pane shows many files. Only the files the user
/// opened are read, their stamps decide whether a frame reads anything at all,
/// and a buffer survives the per-frame rebuild so an edit is not lost while it
/// is unsaved.
#[derive(Default)]
pub(crate) struct DiffFileCache {
    files: HashMap<String, FileContent>,
    buffers: HashMap<String, Rc<RefCell<FileBuffer>>>,
    /// The run mapping for each path's live text, keyed by the text it was
    /// built from: a frame with no edit reuses it, and an edit rebuilds it once.
    runs: HashMap<String, (Rc<String>, Option<Rc<LineRuns>>)>,
    /// The text each path's colouring was requested for, with the token that
    /// names that request. It is what keeps one file to one request while the
    /// worker colours it, and what tells a late answer from a current one.
    versions: HashMap<String, (Rc<String>, u64)>,
    /// The runs the worker returned, with the text they were built from. The
    /// frame after they land builds the line mapping from them; the frame
    /// before draws the text plain.
    highlighted: HashMap<String, (Rc<String>, Rc<Vec<HighlightedLine>>)>,
    /// The token of the request in flight, per path.
    pending: HashMap<String, u64>,
    /// The cards the worker is colouring, keyed by absolute path.
    diff_pending: HashMap<String, PendingDiff>,
    /// The colouring thread, started by the first file the review opens. It
    /// lives here so closing every review closes it too.
    highlighter: Option<HighlightWorker>,
    next_token: u64,
    /// The order files were last in view, least recent first. It bounds the
    /// cache by what the review moved past rather than by what one frame shows.
    order: Vec<String>,
}

/// One file's colouring request, in a form the worker thread can take.
struct HighlightJob {
    path: String,
    token: u64,
    text: String,
    language: String,
}

/// A diff card's colouring request. The card's sides are already joined on the
/// frame that saw it; the worker runs the parser.
struct DiffJob {
    path: String,
    token: u64,
    language: String,
    job: DiffHighlightJob,
}

/// The work the one highlighter thread accepts: a file for the review's
/// in-place editor, or a card's changed lines.
enum HighlightWork {
    File(HighlightJob),
    Diff(DiffJob),
}

/// A coloured file on its way back to the frame that asked for it. A token
/// that no longer matches the path's latest request is stale and is dropped.
struct HighlightResult {
    path: String,
    token: u64,
    spans: Option<HighlightedSpans>,
}

/// A coloured card, one answer per side of its job.
struct DiffResult {
    path: String,
    token: u64,
    answers: Vec<Option<HighlightedSpans>>,
}

enum HighlightAnswer {
    File(HighlightResult),
    Diff(DiffResult),
}

/// The thread that colours what the review opened, so a large file or diff
/// never spends its highlight inside a frame. Jobs are coloured in the order
/// they arrive; the UI drops an answer whose text moved on.
struct HighlightWorker {
    jobs: Sender<HighlightWork>,
    results: Receiver<HighlightAnswer>,
}

impl HighlightWorker {
    fn spawn() -> Self {
        let (jobs, incoming) = mpsc::channel::<HighlightWork>();
        let (answers, results) = mpsc::channel::<HighlightAnswer>();
        std::thread::Builder::new()
            .name("goble-highlight".to_string())
            .spawn(move || {
                while let Ok(work) = incoming.recv() {
                    let answer = match work {
                        HighlightWork::File(job) => {
                            let spans = goble_ui::syntax::highlight_spans(&job.text, &job.language);
                            HighlightAnswer::File(HighlightResult {
                                path: job.path,
                                token: job.token,
                                spans,
                            })
                        }
                        HighlightWork::Diff(job) => {
                            let answers = job
                                .job
                                .sides
                                .iter()
                                .map(|side| {
                                    goble_ui::syntax::highlight_spans(&side.text, &job.language)
                                })
                                .collect();
                            HighlightAnswer::Diff(DiffResult {
                                path: job.path,
                                token: job.token,
                                answers,
                            })
                        }
                    };
                    if answers.send(answer).is_err() {
                        break;
                    }
                }
            })
            .expect("the highlighter thread must start");
        Self { jobs, results }
    }
}

/// A card whose colouring is in flight: what to install the answer into, and
/// the job whose side order the answer follows.
struct PendingDiff {
    token: u64,
    prepared: PreparedDiff,
    job: DiffHighlightJob,
}

impl DiffFileCache {
    /// The editors for the review panes' files in view, keyed by pane id and
    /// absolute path. `wanted` is what the last frame laid out; the reads stay
    /// cached for a while after they leave the view, so scrolling back costs no
    /// disk work.
    pub fn read(
        &mut self,
        wanted: &[(u64, String)],
    ) -> HashMap<u64, HashMap<String, Rc<DiffEditor>>> {
        let keep: std::collections::HashSet<String> =
            wanted.iter().map(|(_, path)| path.clone()).collect();
        self.collect();
        for path in &keep {
            self.touch(path);
            let content = self.content(path);
            self.settle(path, &content);
            self.request_highlight(path, &content);
        }
        let cached: Vec<(String, FileContent)> = self
            .files
            .iter()
            .map(|(path, content)| (path.clone(), content.clone()))
            .collect();
        let mut all: Vec<(String, FileContent, Rc<RefCell<FileBuffer>>, Option<Rc<LineRuns>>)> =
            Vec::new();
        for (path, content) in cached {
            let Some(buffer) = self.buffers.get(&path).cloned() else {
                continue;
            };
            let runs = self.line_runs(&path, &content, &buffer);
            all.push((path, content, buffer, runs));
        }
        // Every pane that laid a file out gets the whole cache: the box that
        // just left the viewport — or the one the file list filtered out — is
        // already read, so showing it again does not wait on the disk.
        let mut panes: Vec<u64> = wanted.iter().map(|(pane_id, _)| *pane_id).collect();
        panes.sort_unstable();
        panes.dedup();
        let mut editors: HashMap<u64, HashMap<String, Rc<DiffEditor>>> = HashMap::new();
        for pane_id in panes {
            let files = all
                .iter()
                .map(|(path, content, buffer, runs)| {
                    (
                        path.clone(),
                        Rc::new(DiffEditor {
                            content: content.clone(),
                            buffer: Rc::clone(buffer),
                            runs: runs.clone(),
                        }),
                    )
                })
                .collect();
            editors.insert(pane_id, files);
        }
        self.evict(&keep);
        editors
    }

    /// Mark `path` as the most recently seen read.
    fn touch(&mut self, path: &str) {
        if let Some(at) = self.order.iter().position(|seen| seen == path) {
            self.order.remove(at);
        }
        self.order.push(path.to_string());
    }

    /// Drop the least recently seen reads above the cap. A file in view, or a
    /// buffer with an edit that was never saved, is never dropped: the first
    /// would be read again at once, and the second is the user's work.
    fn evict(&mut self, wanted: &std::collections::HashSet<String>) {
        if self.files.len() <= MAX_REVIEW_FILES {
            return;
        }
        let mut index = 0;
        while self.files.len() > MAX_REVIEW_FILES && index < self.order.len() {
            let path = self.order[index].clone();
            let dirty = self
                .buffers
                .get(&path)
                .is_some_and(|buffer| buffer.borrow().dirty());
            if !wanted.contains(&path) && !dirty {
                self.order.remove(index);
                self.files.remove(&path);
                self.buffers.remove(&path);
                self.runs.remove(&path);
                self.versions.remove(&path);
                self.highlighted.remove(&path);
                self.pending.remove(&path);
                self.diff_pending.remove(&path);
            } else {
                index += 1;
            }
        }
    }

    /// The runs the buffer's lines are drawn with, mapped once per text it
    /// holds: a frame that changed nothing finds the mapping by identity.
    fn line_runs(
        &mut self,
        path: &str,
        content: &FileContent,
        buffer: &Rc<RefCell<FileBuffer>>,
    ) -> Option<Rc<LineRuns>> {
        let (text, lines) = {
            let buffer = buffer.borrow();
            (buffer.text_rc(), buffer.lines())
        };
        if let Some((cached, runs)) = self.runs.get(path) {
            if Rc::ptr_eq(cached, &text) {
                return runs.clone();
            }
        }
        // The worker's runs when they belong to the text this read produced;
        // the read's own only for a cache filled before the worker existed.
        let source = self
            .highlighted
            .get(path)
            .filter(|(cached, _)| {
                content
                    .text
                    .as_ref()
                    .is_some_and(|text| Rc::ptr_eq(cached, text))
            })
            .map(|(_, source)| Rc::clone(source))
            .or_else(|| content.runs().cloned());
        let runs = source.map(|source| {
            let count = source.len();
            let sources = (0..lines)
                .map(|index| (index < count).then_some(index))
                .collect();
            Rc::new(LineRuns {
                lines: Rc::clone(&source),
                sources,
            })
        });
        self.runs
            .insert(path.to_string(), (Rc::clone(&text), runs.clone()));
        runs
    }

    fn content(&mut self, path: &str) -> FileContent {
        let stamp = fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.len(), metadata.modified().ok()));
        if let Some(cached) = self.files.get(path) {
            if stamp.is_some() && cached.stamp == stamp {
                return cached.clone();
            }
        }
        let content = read_without_highlight(path);
        if stamp.is_some() {
            self.files.insert(path.to_string(), content.clone());
        }
        content
    }

    /// Take every answer the worker has finished. An answer for a text that
    /// moved on, or for a path that left the cache, is dropped; a current one
    /// becomes the path's runs and drops the plain mapping built in the frames
    /// before it landed, so the next frame maps the colour in.
    fn collect(&mut self) {
        let mut landed = Vec::new();
        if let Some(worker) = &self.highlighter {
            while let Ok(result) = worker.results.try_recv() {
                landed.push(result);
            }
        }
        for answer in landed {
            match answer {
                HighlightAnswer::File(result) => {
                    if self.pending.get(&result.path) != Some(&result.token) {
                        continue;
                    }
                    self.pending.remove(&result.path);
                    let Some(spans) = result.spans else {
                        continue;
                    };
                    let Some((text, _)) = self.versions.get(&result.path) else {
                        continue;
                    };
                    let lines = Rc::new(goble_ui::syntax::spans_to_lines(&spans));
                    self.highlighted
                        .insert(result.path.clone(), (Rc::clone(text), lines));
                    self.runs.remove(&result.path);
                }
                HighlightAnswer::Diff(result) => {
                    let Some(pending) = self.diff_pending.get(&result.path) else {
                        continue;
                    };
                    if pending.token != result.token {
                        continue;
                    }
                    let Some(pending) = self.diff_pending.remove(&result.path) else {
                        continue;
                    };
                    pending
                        .prepared
                        .install_highlights(&pending.job, &result.answers);
                }
            }
        }
    }

    /// Ask the worker to colour the changed lines of every card in view. The
    /// rows are joined on this frame and the parser runs beside it, so a card
    /// that scrolls in is flat for a beat and then coloured, never frozen.
    pub fn highlight_diffs(
        &mut self,
        wanted: &[(u64, String)],
        details: &HashMap<u64, Option<Rc<super::pickers::DiffDetail>>>,
    ) {
        self.collect();
        for (pane_id, path) in wanted {
            let Some(Some(detail)) = details.get(pane_id) else {
                continue;
            };
            let Some(file) = detail.files.iter().find(|file| &file.absolute_path == path) else {
                continue;
            };
            if self.diff_pending.contains_key(path) {
                continue;
            }
            let Some(job) = file.prepared.highlight_job() else {
                continue;
            };
            self.next_token += 1;
            let token = self.next_token;
            self.diff_pending.insert(
                path.clone(),
                PendingDiff {
                    token,
                    prepared: file.prepared.clone(),
                    job: job.clone(),
                },
            );
            let worker = self.highlighter.get_or_insert_with(HighlightWorker::spawn);
            let work = HighlightWork::Diff(DiffJob {
                path: path.clone(),
                token,
                language: path.clone(),
                job,
            });
            if worker.jobs.send(work).is_err() {
                self.highlighter = None;
                self.diff_pending.remove(path);
            }
        }
        // A card that left the view no longer needs its answer; dropping the
        // entry lets the late answer fall on the floor and bounds the work
        // held between frames.
        self.diff_pending
            .retain(|path, _| wanted.iter().any(|(_, wanted_path)| wanted_path == path));
    }

    /// Ask the worker to colour `path`'s text, once per text version. A file
    /// that is not editable whole has no text to colour and stays plain.
    fn request_highlight(&mut self, path: &str, content: &FileContent) {
        let Some(text) = content.text.clone() else {
            return;
        };
        if self
            .versions
            .get(path)
            .is_some_and(|(requested, _)| Rc::ptr_eq(requested, &text))
        {
            return;
        }
        self.next_token += 1;
        let token = self.next_token;
        self.versions
            .insert(path.to_string(), (Rc::clone(&text), token));
        self.pending.insert(path.to_string(), token);
        let worker = self.highlighter.get_or_insert_with(HighlightWorker::spawn);
        let job = HighlightWork::File(HighlightJob {
            path: path.to_string(),
            token,
            text: (*text).clone(),
            language: path.to_string(),
        });
        if worker.jobs.send(job).is_err() {
            // The thread died: forget it and the request, so the next frame
            // starts a fresh one instead of waiting forever.
            self.highlighter = None;
            self.pending.remove(path);
            self.versions.remove(path);
        }
    }

    /// Settle `path`'s buffer against the read that just happened, with the
    /// same rule the file pane uses: a save of this buffer drops the pre-save
    /// read, a file that did not move leaves the buffer alone, a file that did
    /// move refreshes only a clean buffer, and an unsaved edit is never thrown
    /// away.
    fn settle(&mut self, path: &str, content: &FileContent) -> Rc<RefCell<FileBuffer>> {
        let Some(buffer) = self.buffers.get(path).cloned() else {
            let buffer = Rc::new(RefCell::new(FileBuffer::open(path, content)));
            self.buffers.insert(path.to_string(), Rc::clone(&buffer));
            return buffer;
        };
        if buffer.borrow_mut().take_written() {
            self.files.remove(path);
            return buffer;
        }
        if buffer.borrow().stamp() == content.stamp {
            return buffer;
        }
        if buffer.borrow().dirty() {
            buffer.borrow_mut().note_external_change();
            return buffer;
        }
        if buffer.borrow().editable() && read_only_reason(content).is_none() {
            buffer.borrow_mut().reload(content);
            return buffer;
        }
        let replacement = Rc::new(RefCell::new(FileBuffer::open(path, content)));
        self.buffers
            .insert(path.to_string(), Rc::clone(&replacement));
        replacement
    }
}

/// Read one file into what its view draws, and nothing otherwise.
pub(crate) fn read(path: &str) -> FileContent {
    read_inner(path, true)
}

/// The same read without the syntax pass. The review pane's cache takes this
/// form and hands the text to a background highlighter, so a card that scrolls
/// into view never waits on syntect inside the frame.
pub(crate) fn read_without_highlight(path: &str) -> FileContent {
    read_inner(path, false)
}

fn read_inner(path: &str, highlight: bool) -> FileContent {
    /// A file the view will not draw.
    fn none(body: FileBody, stamp: Option<FileStamp>) -> FileContent {
        FileContent {
            body,
            runs: None,
            text: None,
            stamp,
        }
    }

    let Ok(metadata) = fs::metadata(path) else {
        return none(FileBody::Unreadable, None);
    };
    let stamp = Some((metadata.len(), metadata.modified().ok()));
    if !metadata.is_file() {
        return none(FileBody::Unreadable, stamp);
    }
    if metadata.len() > MAX_FILE_BYTES {
        return none(
            FileBody::TooLarge {
                bytes: metadata.len(),
            },
            stamp,
        );
    }
    let Ok(bytes) = fs::read(path) else {
        return none(FileBody::Unreadable, stamp);
    };
    // A NUL near the start is the cheap test for a binary file: rendering PNGs
    // and object files as text helps nobody, and the alternative is a pane full
    // of replacement glyphs.
    if bytes.iter().take(8 * 1024).any(|byte| *byte == 0) {
        return none(FileBody::NotText, stamp);
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return none(FileBody::NotText, stamp);
    };
    let total = text.lines().count();
    let shown: Vec<String> = text
        .lines()
        .take(MAX_LINES)
        .map(|line| line.to_string())
        .collect();
    // The whole text is kept only when the read took all of it: an editor
    // buffer over the part of a file the pane shows would save that part back
    // over the rest of it.
    let whole = total == shown.len();
    let runs = highlight.then(|| highlight_lines(&shown, path)).flatten();
    FileContent {
        body: FileBody::Lines {
            shown: Rc::new(shown),
            total,
        },
        runs,
        text: whole.then(|| Rc::new(text)),
        stamp,
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
fn highlight_lines(lines: &[String], path: &str) -> Option<Rc<Vec<HighlightedLine>>> {
    goble_ui::syntax::highlight(&lines.join("\n"), path).map(Rc::new)
}

/// Why a file that was read cannot be written back, or `None` when it can.
///
/// A read that stopped at either cap leaves the pane holding part of the file,
/// so saving it would carry that part over the rest: those files — and the ones
/// that are not text, or are not readable at all — are read-only, and this is
/// the one place that decides it. The pane says the reason beside the file's
/// name, and [`FileBuffer::save`] refuses on it, so the refusal is a rule of the
/// state machine rather than a button the view chose not to draw.
fn read_only_reason(content: &FileContent) -> Option<&'static str> {
    match &content.body {
        FileBody::Lines { shown, total } if *total <= shown.len() => None,
        FileBody::Lines { .. } => Some("too long to edit"),
        FileBody::TooLarge { .. } => Some("too large to edit"),
        FileBody::NotText => Some("not text, so not editable"),
        FileBody::Unreadable => Some("not readable, so not editable"),
    }
}

/// One file pane's editing state: the text the pane holds, the bytes it is
/// compared against, and everything the pane must refuse to do with it.
///
/// The buffer outlives the frame — the element tree is rebuilt every frame and
/// would lose the text with it — so it lives in the read cache and reaches the
/// tree through the snapshot. The caret and the selection anchor live here too,
/// shared with the element the way the composer shares its own.
pub struct FileBuffer {
    path: String,
    /// The text the pane edits, and the text the file holds — saved verbatim by
    /// `save`, and what `dirty` compares the live text against: an edit that is
    /// typed and then typed back out is not an unsaved change. The live text is
    /// shared with the field rather than copied into it every frame.
    text: Rc<String>,
    saved: String,
    /// Whether the live text differs from the file's. Kept beside the two
    /// strings because the review cache asks every buffer on every frame, and
    /// comparing whole files there would grow with the review.
    dirty: bool,
    /// How many lines the live text has, counted when it changes: the review
    /// header and the block's height ask for it every frame.
    lines: usize,
    /// The file's size and modification time at the last read or write, which
    /// is what tells this buffer the file moved under it.
    stamp: Option<FileStamp>,
    /// Why this buffer must not be written back, when it must not.
    read_only: Option<&'static str>,
    /// Whether the file was written since the last settle, so the cache entry
    /// read before the write is dropped and the file re-read.
    written: bool,
    caret: Rc<RefCell<usize>>,
    anchor: Rc<RefCell<Option<usize>>>,
    /// The last failure, drawn in the pane until the next save. A write that
    /// failed must leave a mark: a save that silently did nothing is worse than
    /// one that says it could not.
    error: Option<String>,
    /// What the pane says about the save that worked, and about a file that
    /// changed on disk under an unsaved edit.
    status: Option<String>,
    note: Option<String>,
}

/// A buffer can hold a whole file, so a failing assertion prints what the
/// buffer knows about its text rather than the text itself.
impl std::fmt::Debug for FileBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileBuffer")
            .field("path", &self.path)
            .field("bytes", &self.text.len())
            .field("dirty", &self.dirty())
            .field("read_only", &self.read_only)
            .field("error", &self.error)
            .field("note", &self.note)
            .finish()
    }
}

impl FileBuffer {
    /// The buffer for a file the pane just read.
    ///
    /// Every pane that shows a file gets one: the read-only ones carry the
    /// reason instead of an editable text, so a caller cannot save a file the
    /// read did not take whole by looking at the pane rather than at the buffer.
    pub fn open(path: &str, content: &FileContent) -> Self {
        let read_only = read_only_reason(content);
        let text = match (read_only.is_none(), content.text.as_deref()) {
            (true, Some(text)) => text.to_string(),
            _ => String::new(),
        };
        let saved = text.clone();
        let lines = text.matches('\n').count() + 1;
        Self {
            path: path.to_string(),
            saved,
            text: Rc::new(text),
            dirty: false,
            lines,
            stamp: content.stamp,
            read_only,
            written: false,
            caret: Rc::new(RefCell::new(0)),
            anchor: Rc::new(RefCell::new(None)),
            error: None,
            status: None,
            note: None,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// The text the pane draws and saves. Whole: the buffer never holds part of
    /// a file, because a part is not something that can be written back.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The live text as the field takes it, without copying it.
    pub fn text_rc(&self) -> Rc<String> {
        Rc::clone(&self.text)
    }

    /// How many rows the text has, counted when it changed.
    pub fn lines(&self) -> usize {
        self.lines
    }

    pub fn caret(&self) -> Rc<RefCell<usize>> {
        Rc::clone(&self.caret)
    }

    pub fn anchor(&self) -> Rc<RefCell<Option<usize>>> {
        Rc::clone(&self.anchor)
    }

    pub fn stamp(&self) -> Option<FileStamp> {
        self.stamp
    }

    /// Whether the buffer holds text the file does not. An edit typed and typed
    /// back out is not one: the file's own bytes are the comparison.
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    pub fn editable(&self) -> bool {
        self.read_only.is_none()
    }

    /// Why the pane cannot edit this file, when it cannot.
    pub fn read_only(&self) -> Option<&str> {
        self.read_only
    }

    /// What the pane says about the buffer beside the file's name: how much text
    /// the buffer holds, whether any of it is unsaved, and what the last save
    /// did. Why a file cannot be edited at all is the pane's own line — see
    /// [`Self::read_only`].
    pub fn detail(&self) -> String {
        let mut parts = vec![format!("{} lines", self.lines)];
        if self.dirty() {
            parts.push("unsaved changes".to_string());
        }
        if let Some(status) = &self.status {
            parts.push(status.clone());
        }
        parts.join(" · ")
    }

    /// The text the field reports after a key: the buffer's own, whole.
    pub fn set_text(&mut self, text: String) {
        self.lines = text.matches('\n').count() + 1;
        self.text = Rc::new(text);
        // Typed back to the file's own text, it is not a change at all, and
        // the comparison runs here rather than on every frame that asks.
        self.dirty = *self.text != self.saved;
        // The last save's report describes the text this keystroke just
        // replaced; the dirty marker says where the buffer stands now.
        self.status = None;
    }

    /// Write the buffer to its file. Refuses on a read-only buffer — the caps
    /// are a rule of this state machine, not of the view that draws no editor —
    /// and on any write that fails, carrying the reason back so the pane can
    /// show it. The bytes written are the buffer's own, whole.
    pub fn save(&mut self) -> Result<(), String> {
        if let Some(reason) = self.read_only {
            return Err(self.fail(format!("Not saved: this file is {reason}.")));
        }
        if let Err(error) = fs::write(&self.path, self.text.as_bytes()) {
            return Err(self.fail(format!("Not saved: {error}")));
        }
        self.saved = (*self.text).clone();
        self.dirty = false;
        // The write is this buffer's own doing, so the cache entry that predates
        // it is dropped rather than read as a change from outside; the stamp is
        // the file's, as it is now.
        self.written = true;
        self.stamp = fs::metadata(&self.path)
            .ok()
            .map(|metadata| (metadata.len(), metadata.modified().ok()));
        self.error = None;
        self.status = Some("saved".to_string());
        self.note = None;
        Ok(())
    }

    fn fail(&mut self, message: String) -> String {
        self.status = None;
        self.error = Some(message.clone());
        message
    }

    /// Take a fresh read of the file, keeping where the caret is: a clean
    /// buffer is the pane's view of the file, so the pane shows the file as it
    /// now is.
    fn reload(&mut self, content: &FileContent) {
        let Some(text) = content.text.as_deref() else {
            return;
        };
        self.lines = text.matches('\n').count() + 1;
        self.text = Rc::new(text.to_string());
        self.saved = text.to_string();
        self.dirty = false;
        self.stamp = content.stamp;
        self.error = None;
        self.status = None;
        self.note = None;
        let length = self.text.chars().count();
        let caret = (*self.caret.borrow()).min(length);
        *self.caret.borrow_mut() = caret;
        *self.anchor.borrow_mut() = None;
    }

    /// The file was written from outside while this buffer held an edit: the
    /// edit is kept, and the pane says why the text it shows is not the file's.
    fn note_external_change(&mut self) {
        self.note = Some("changed on disk — your unsaved changes are kept".to_string());
    }

    /// Whether the file was written since the last settle, clearing the mark.
    fn take_written(&mut self) -> bool {
        std::mem::take(&mut self.written)
    }
}

/// The pane's view of `path`: the file's name and size in the pane's own
/// topbar, and the file's lines, highlighted by the file's own type, under a
/// line-number gutter.
pub(crate) fn build_file_view(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    path: &str,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let file = state.pane_files.get(&pane_id);
    let editor = state
        .file_buffers
        .get(&(pane_id, path.to_string()))
        .cloned();
    let editing = editor
        .as_ref()
        .filter(|buffer| buffer.borrow().editable())
        .cloned();
    let body = match &editing {
        Some(buffer) => editor_view(
            state,
            pane_id,
            buffer,
            file.and_then(|file| file.runs.as_ref()),
        ),
        None => match file {
            Some(FileContent {
                body: FileBody::Lines { shown, total },
                runs,
                ..
            }) => lines_view(app, state, pane_id, Rc::clone(shown), runs.as_ref(), *total),
            Some(content) => note(app, &content.body.message()),
            // The body is read while the snapshot is built, so a pane with no
            // body yet is a frame old at most.
            None => note(app, "Reading the file…"),
        },
    };

    let detail = match &editing {
        // The editing pane's own count and marks — what the buffer holds now,
        // not the lines the file was read as. A read-only pane keeps the read's
        // own detail (how much of the file it is showing).
        Some(buffer) => Some(buffer.borrow().detail()),
        None => file.map(|file| file.body.detail()),
    };
    // Why the pane will not edit the file, said beside its name. Only a file
    // the read cut short at a cap needs it: the bodies that were never readable
    // already say what they are, and their titles read as they always have.
    let read_only = match (&editing, &editor, file.map(|file| &file.body)) {
        (None, Some(buffer), Some(FileBody::Lines { .. })) => {
            buffer.borrow().read_only().map(str::to_string)
        }
        _ => None,
    };
    // A save that failed, and a file that was written under an unsaved edit,
    // are said in the pane rather than in a log: both leave the text on screen
    // differing from the file, which the pane must not let read as agreement.
    let mut marks: Vec<(String, ColorToken)> = Vec::new();
    if let Some(buffer) = &editor {
        let buffer = buffer.borrow();
        if let Some(message) = buffer.error() {
            marks.push((message.to_string(), ColorToken::Error));
        }
        if let Some(message) = buffer.note() {
            marks.push((message.to_string(), ColorToken::Warning));
        }
    }

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm)
        // The topbar floats over the content, so the body starts under it.
        .with_child(
            Empty::new()
                .with_size(Vector2F::new(0.0, super::shell::TOPBAR_HEIGHT))
                .finish(),
        );
    for (message, color) in marks {
        column = column.with_child(band(app, &message, color));
    }
    let column = column
        .with_child(Expanded::new(body).finish())
        .finish();
    let content = Container::new(column)
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .with_padding(EdgeInsets::new(sm, sm, sm, sm))
        .finish();
    // The file is named in the pane's own topbar, the row every pane wears,
    // rather than in a title row under it. Every file the pane holds open gets
    // a chip, so one pane can switch between files without splitting.
    let title = file_title(
        app,
        state,
        actions,
        pane_id,
        path,
        detail.as_deref(),
        read_only.as_deref(),
    );
    // The pane wears the same topbar as its siblings (the close button and the
    // app's own tray), so a pane is closed the same way whatever it hosts.
    let header = chat::build_agent_header_with(
        app,
        state,
        actions,
        pane_id,
        false,
        chat::HeaderSlots {
            leading: Some(title),
            trailing: None,
        },
    );
    let pane = Stack::new()
        .with_children(vec![content])
        .with_overlay(header, Vector2F::zero())
        .finish();
    match editor {
        // Cmd/Ctrl+S belongs to the pane rather than to the text field in it,
        // so the shortcut is answered above the body.
        Some(buffer) => Box::new(FileKeys::new(
            buffer,
            state.active_pane_id == pane_id,
            pane,
        )),
        None => pane,
    }
}

/// The "save before closing?" dialog a dirty file view gets when its pane is
/// closed while auto-save is off.
///
/// The three answers are the only ways out: save and close, discard and close,
/// or keep the pane (Cancel, `Escape`, or a backdrop click). It only paints and
/// takes events while a close is pending.
pub(crate) fn build_close_prompt(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let open = state.pending_close_pane.is_some() || state.pending_close_file.is_some();
    let name = state
        .pending_close_file
        .as_ref()
        .map(|(_, path)| file_name(path))
        .or_else(|| {
            state.pending_close_pane.and_then(|pane_id| {
                // The prompt names the file the pane is showing; the buffers
                // are the fallback for a pane whose kind is already gone.
                state
                    .spaces
                    .iter()
                    .find_map(|space| match space.leaf_kind(pane_id) {
                        Some(PaneKind::File { path }) => Some(path.clone()),
                        _ => None,
                    })
                    .or_else(|| {
                        state
                            .file_buffers
                            .keys()
                            .find(|(id, _)| *id == pane_id)
                            .map(|(_, path)| path.clone())
                    })
                    .map(|path| file_name(&path))
            })
        })
        .unwrap_or_else(|| "This file".to_string());

    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);
    let save = actions.on_close_unsaved_save.clone();
    let discard = actions.on_close_unsaved_discard.clone();
    let cancel = actions.on_close_unsaved_cancel.clone();
    let cancel_on_close = actions.on_close_unsaved_cancel.clone();
    let cancel_on_escape = actions.on_close_unsaved_cancel.clone();

    let content = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm)
        .with_child(
            Text::new("Save changes before closing?")
                .with_font_size(13.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(
            Text::new(format!("{name} has unsaved changes."))
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(Spacer::new().finish())
                .with_child(
                    Button::new(Text::new("Cancel").finish())
                        .with_variant(ButtonVariant::Ghost)
                        .with_on_click(move || (cancel.borrow_mut())())
                        .finish(),
                )
                .with_child(
                    Button::new(Text::new("Don't save").finish())
                        .with_variant(ButtonVariant::Outline)
                        .with_on_click(move || (discard.borrow_mut())())
                        .finish(),
                )
                .with_child(
                    Button::new(Text::new("Save and close").finish())
                        .with_variant(ButtonVariant::Primary)
                        .with_on_click(move || (save.borrow_mut())())
                        .finish(),
                )
                .finish(),
        );
    let panel = Container::new(content.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_padding(EdgeInsets::uniform(md))
        .with_corner_radius(app.theme.radius_px())
        .finish();
    let panel = KeyHandler::new(panel, move |key: &str, _modifiers| {
        if key == "Escape" {
            (cancel_on_escape.borrow_mut())();
            return true;
        }
        false
    })
    .finish();

    Dialog::new(panel)
        .with_open(open)
        .with_on_close(move || (cancel_on_close.borrow_mut())())
        .finish()
}

/// The last component of a path: what a pane and its tab call the file, and the
/// stem [`file_icon_name`] reads its type from. A path with no component of its
/// own (a root, or one that ends in `..`) is its own name.
pub(crate) fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// The files the pane holds open, named in its topbar. One file keeps the plain
/// title it always had; two or more become chips, the one the pane shows filled
/// in and the rest outlined, each a click away. `detail` is what the buffer (or
/// the read body) says about the shown file, `read_only` why the pane will not
/// edit it.
fn file_title(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    path: &str,
    detail: Option<&str>,
    read_only: Option<&str>,
) -> Box<dyn Element> {
    let detail = match (detail, read_only) {
        (Some(detail), Some(reason)) => Some(format!("{detail} · {reason}")),
        (Some(detail), None) => Some(detail.to_string()),
        (None, reason) => reason.map(str::to_string),
    };
    // A pane restored from a saved layout has no chip list yet; the file it
    // shows is the one it has open.
    let mut files = state
        .pane_open_files
        .get(&pane_id)
        .cloned()
        .unwrap_or_default();
    if !files.iter().any(|file| file == path) {
        files.push(path.to_string());
    }
    if files.len() > 1 {
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.0);
        for file in &files {
            row = row.with_child(file_chip(
                app,
                actions,
                pane_id,
                file,
                file == path,
            ));
        }
        if let Some(detail) = detail {
            row = row.with_child(
                Text::new(detail)
                    .with_font_size(11.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .with_max_lines(1)
                    .finish(),
            );
        }
        return row.with_child(Spacer::new().finish()).finish();
    }

    let name = file_name(path);
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
        );
    row = row.with_child(Spacer::new().finish());
    if let Some(detail) = detail {
        row = row.with_child(
            Text::new(detail)
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    row.finish()
}

/// One file in a pane's chip row. The file the pane shows is filled in and
/// inert; the others are outlined and switch the pane to that file when
/// clicked. The full path stays on the chip's own hover.
fn file_chip(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    path: &str,
    active: bool,
) -> Box<dyn Element> {
    let name = file_name(path);
    let content = Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(
            Icon::new(file_icon_name(&name))
                .with_size(16.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Tooltip::new(
                Text::new(name)
                    .with_font_size(13.0)
                    .with_theme_color(
                        if active {
                            ColorToken::Text
                        } else {
                            ColorToken::Muted
                        },
                        app,
                    )
                    .with_max_lines(1)
                    .finish(),
                path.to_string(),
            )
            .with_position(TooltipPosition::Below)
            .finish(),
        )
        .finish();
    let on_select = actions.on_file_chip_select.clone();
    let selected = path.to_string();
    let on_close = actions.on_file_chip_close.clone();
    let closed = path.to_string();
    let close: Box<dyn Element> = TopbarButton::new(
        Icon::new("x")
            .with_size(10.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(16.0)
    .with_on_click(move || (on_close.borrow_mut())(pane_id, closed.clone()))
    .finish();
    Box::new(FileChip {
        child: content,
        close,
        active,
        on_select: Rc::new(RefCell::new(move || {
            (on_select.borrow_mut())(pane_id, selected.clone())
        })),
        state: InteractiveState::default(),
        size: None,
        origin: None,
    })
}

/// One file in a pane's topbar, drawn the way the general topbar draws its
/// workspace tabs: a square, frameless cell the height of the bar. The file the
/// pane shows carries the raised fill; the others are plain until the pointer
/// is over them, and every chip shows its close "x" on hover so a file can be
/// closed without closing the pane.
struct FileChip {
    child: Box<dyn Element>,
    close: Box<dyn Element>,
    active: bool,
    on_select: Rc<RefCell<dyn FnMut()>>,
    state: InteractiveState,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl FileChip {
    /// The row the chip fills: the same height as the pane header's controls,
    /// so the bar keeps `TOPBAR_HEIGHT` around it.
    fn height() -> f32 {
        chat::HEADER_CONTROL_HEIGHT
    }

    /// The inset before the file's icon and after its close control, the same
    /// the workspace tabs use.
    fn h_pad() -> f32 {
        7.0
    }

    fn close_gap() -> f32 {
        4.0
    }

    fn inner(constraint: SizeConstraint, close_width: f32) -> SizeConstraint {
        let room =
            (constraint.max.x - Self::h_pad() * 2.0 - Self::close_gap() - close_width).max(0.0);
        SizeConstraint::new(Vector2F::zero(), Vector2F::new(room, Self::height()))
    }

    fn width(intrinsic: f32, constraint: SizeConstraint) -> f32 {
        intrinsic.max(constraint.min.x).min(constraint.max.x)
    }

    fn close_origin(origin: Vector2F, size: Vector2F, close_size: Vector2F) -> Vector2F {
        Vector2F::new(
            origin.x + (size.x - Self::h_pad() - close_size.x).max(Self::h_pad()),
            origin.y + (size.y - close_size.y).max(0.0) / 2.0,
        )
    }

    fn bounds(&self) -> Option<goble_ui::geometry::RectF> {
        let origin = self.origin?;
        let size = self.size?;
        Some(rectf(origin.x(), origin.y(), size.x, size.y))
    }
}

impl Element for FileChip {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // The close control's slot is reserved whether it is drawn or not, so a
        // chip does not change width under the pointer that reveals its "x".
        let close_size = self.close.layout(
            SizeConstraint::loose(Vector2F::new(Self::height(), Self::height())),
            ctx,
            app,
        );
        let child_size = self
            .child
            .layout(Self::inner(constraint, close_size.x), ctx, app);
        let intrinsic = child_size.x + Self::close_gap() + close_size.x + Self::h_pad() * 2.0;
        let size = Vector2F::new(
            Self::width(intrinsic, constraint),
            Self::height().min(constraint.max.y),
        );
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let rect = rectf(origin.x, origin.y, size.x, size.y);
        let hovered = ctx.hovered(rect);
        if let Some(renderer) = ctx.renderer.as_mut() {
            // Square fills and no stroke, exactly as the workspace tabs: the
            // pane bar behind the chips is the only frame they need.
            if self.active {
                renderer.fill_rect(rect, app.theme.color(ColorToken::SurfaceRaised));
            } else if hovered {
                renderer.fill_rect(rect, app.theme.color(ColorToken::Hover));
            }
        }
        let child_size = self.child.size().unwrap_or(Vector2F::zero());
        let close_size = self.close.size().unwrap_or(Vector2F::zero());
        self.child.paint(
            Vector2F::new(
                origin.x + Self::h_pad(),
                origin.y + (size.y - child_size.y).max(0.0) / 2.0,
            ),
            ctx,
            app,
        );
        if hovered {
            self.close
                .paint(Self::close_origin(origin, size, close_size), ctx, app);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        let over = match event {
            DispatchedEvent::MouseDown { position, .. }
            | DispatchedEvent::MouseUp { position, .. }
            | DispatchedEvent::MouseMove { position } => {
                self.bounds().is_some_and(|bounds| contains(bounds, *position))
            }
            _ => false,
        };
        // The close "x" is hit-tested first, and only while the chip shows it:
        // a click on the name still selects the file.
        if over && self.close.dispatch_event(event, ctx, app) {
            return true;
        }
        let Some(bounds) = self.bounds() else {
            return false;
        };
        let cb = Rc::clone(&self.on_select);
        let mut on_click = move || (cb.borrow_mut())();
        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut on_click)
    }
}

/// A line of the pane's own, above the file: what a save did, or that the file
/// moved under an unsaved edit.
fn band(app: &AppContext, text: &str, color: ColorToken) -> Box<dyn Element> {
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    Container::new(
        Text::new(text.to_string())
            .with_font_size(11.0)
            .with_theme_color(color, app)
            .finish(),
    )
    .with_padding(EdgeInsets::new(0.0, xs, 0.0, xs))
    .finish()
}

/// The pane's editable body: the buffer's text as a multi-line field over the
/// same line-number gutter the read-only view draws, with the caret and the
/// syntax runs the read resolved for the file's own type.
///
/// The text is the buffer's own, not the read's lines: an edit is what the pane
/// shows, and a frame that shows the file instead would throw it away on screen
/// while the buffer still held it.
///
/// The field holds the keyboard exactly while its pane is the active one, the
/// gate the pane's Cmd/Ctrl+S already reads: a file opened from the explorer is
/// the active pane from its first frame, so the caret is in the file and a
/// keystroke lands in the buffer without a press first — a file pane beside a
/// terminal takes nothing while the terminal is the active pane.
fn editor_view(
    state: &UiSnapshot,
    pane_id: u64,
    buffer: &Rc<RefCell<FileBuffer>>,
    runs: Option<&Rc<Vec<HighlightedLine>>>,
) -> Box<dyn Element> {
    let (value, caret, anchor) = {
        let buffer = buffer.borrow();
        (buffer.text().to_string(), buffer.caret(), buffer.anchor())
    };
    // The runs belong to the text they were highlighted from, one set per line
    // of the file as it was read. The buffer's lines are mapped onto them in
    // order; a line the buffer moved or changed no longer spells its set, and
    // the field draws that line plain rather than in another line's colours.
    let runs = runs.map(|runs| {
        let sources = (0..value.split('\n').count())
            .map(|index| (index < runs.len()).then_some(index))
            .collect();
        Rc::new(LineRuns {
            lines: Rc::clone(runs),
            sources,
        })
    });
    let on_change = {
        let buffer = Rc::clone(buffer);
        move |text: String| buffer.borrow_mut().set_text(text)
    };
    let field = TextArea::new()
        .with_value(value)
        .with_multiline(true)
        .with_line_numbers(GUTTER_DIGITS)
        .with_line_height(LINE_HEIGHT)
        .with_min_height(0.0)
        // A press on the empty tail of a line is a press in the file.
        .with_full_width(true)
        // The pane is the keyboard's owner only while it is the active one: a
        // file pane beside a terminal must not take the keys the terminal needs.
        .with_focused(state.active_pane_id == pane_id)
        .with_caret(caret)
        .with_anchor(anchor)
        .with_line_runs(runs)
        .with_on_change(on_change);
    let scroll = state.file_scroll.get(&pane_id).cloned().unwrap_or_default();
    // The field is handed the same scroll state the region around it is: a
    // buffer of as many lines as the pane will open is laid out a screenful at a
    // time, and the caret is brought into view when it moves off that window.
    let field = field.with_scroll_state(Rc::clone(&scroll));
    // Lines are not folded: the horizontal region lets a long one run past the
    // pane, and the vertical one scrolls the rows.
    let hscroll = state
        .file_hscroll
        .get(&pane_id)
        .cloned()
        .unwrap_or_default();
    // The horizontal region is the outer one: it measures the text at its
    // intrinsic width while the vertical region inside it still knows the
    // viewport's height, so the wheel scrolls sideways anywhere in the body.
    let field = Scrollable::new(field.finish(), Axis::Vertical)
        .with_state(scroll)
        .finish();
    Scrollable::new(field, Axis::Horizontal)
        .with_state(hscroll)
        .finish()
}

/// The pane's keyboard: the text field in the body answers the editing keys
/// itself, and this answers the one gesture that belongs to the pane rather
/// than to the text — Cmd/Ctrl+S, which writes the buffer back to the file it
/// was read from. Everything else is the body's, unchanged.
///
/// The review pane wraps its inline editors in the same element, so a save
/// means the same thing — and is refused for the same reasons — whichever
/// surface the file is edited on.
pub(crate) struct FileKeys {
    buffer: Rc<RefCell<FileBuffer>>,
    /// Whether this pane is the active one. Only the active pane answers the
    /// chord, so a file pane beside a terminal never takes it from the terminal.
    focused: bool,
    child: Box<dyn Element>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl FileKeys {
    pub(crate) fn new(
        buffer: Rc<RefCell<FileBuffer>>,
        focused: bool,
        child: Box<dyn Element>,
    ) -> Self {
        Self {
            buffer,
            focused,
            child,
            size: None,
            origin: None,
        }
    }
}

impl Element for FileKeys {
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

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        if let DispatchedEvent::KeyDown { key, modifiers } = event {
            // Save: the buffer's bytes, whole, to the file it came from. A
            // refusal (a capped file, a write that failed) stays on the buffer
            // and the pane says it — never a panic, never a silent no-op.
            if self.focused
                && (modifiers.command || modifiers.ctrl)
                && !modifiers.alt
                && !modifiers.shift
                && key.eq_ignore_ascii_case("s")
            {
                let _ = self.buffer.borrow_mut().save();
                return true;
            }
        }
        self.child.dispatch_event(event, ctx, app)
    }
}

/// The lines of `pane_id`'s view the frame draws: the ones its own scroll
/// viewport can show, and no more.
///
/// Every line of the body is an element of the frame and the tree is rebuilt
/// every frame, so a body that draws all [`MAX_LINES`] of a long file costs a
/// full layout of every one of them on every frame, whatever changed — the
/// cache saves the read and the highlight and nothing else. The pane's own
/// viewport is what bounds it. The lines outside the window are not dropped:
/// [`lines_view`] stands them in with spacers of the same height, so the offset
/// the pane keeps still means the same line and the scroll range is still the
/// whole file's.
///
/// The pane has no viewport to read before its first layout, so that frame draws
/// [`FIRST_FRAME_LINES`] lines plus the window's slack; the frame after draws
/// what fits.
fn drawn_window(state: &UiSnapshot, pane_id: u64, lines: usize) -> std::ops::Range<usize> {
    let (offset, viewport) = match state.file_scroll.get(&pane_id) {
        Some(scroll) => {
            let scroll = scroll.borrow();
            (scroll.offset(), scroll.viewport())
        }
        None => (0.0, 0.0),
    };
    let first = ((offset / LINE_BOX).floor().max(0.0) as usize).min(lines.saturating_sub(1));
    let rows = if viewport > 0.0 {
        (viewport / LINE_BOX).ceil() as usize
    } else {
        FIRST_FRAME_LINES
    };
    first..(first + rows + WINDOW_SLACK).min(lines)
}

/// The room `lines` rows take in the body without any of them being drawn, so
/// the content the viewport measures is the whole file's height.
fn filler(lines: usize) -> Box<dyn Element> {
    Empty::new()
        .with_size(Vector2F::new(0.0, lines as f32 * LINE_BOX))
        .finish()
}

/// The file's lines: one row per line the pane can show, a padded line number
/// then the text, in the pane's own scroll offset so a long file scrolls and
/// keeps its place. The lines above and below the window are spacers of their
/// own height, so what the body measures — and so what the pane scrolls
/// through — is the whole file.
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
    let window = drawn_window(state, pane_id, lines.len());
    let mut list = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        // Start, not Stretch: the horizontal region measures the widest line,
        // so a stretched column would take the unbounded width it is offered
        // and there would be nothing to scroll.
        .with_cross_axis_alignment(CrossAxisAlignment::Start);
    if window.start > 0 {
        list = list.with_child(filler(window.start));
    }
    for index in window.clone() {
        let line = &lines[index];
        let runs = runs
            .and_then(|runs| runs.get(index))
            .filter(|runs| !runs.is_empty());
        list = list.with_child(line_row(app, index + 1, line, runs));
    }
    if window.end < lines.len() {
        list = list.with_child(filler(lines.len() - window.end));
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
    let hscroll = state
        .file_hscroll
        .get(&pane_id)
        .cloned()
        .unwrap_or_default();
    // The horizontal region is the outer one so it can measure the widest line
    // while the vertical region inside it keeps the body's viewport height.
    let list = Scrollable::new(list.finish(), Axis::Vertical)
        .with_state(scroll)
        .finish();
    Scrollable::new(list, Axis::Horizontal)
        .with_state(hscroll)
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
    // `Code` rounds a line box up to a whole pixel and `Text` does not, so a
    // plain line would advance a fraction short of a highlighted one and walk
    // the drawn rows off the fillers they are counted in. The body is held to
    // the one line box the pane counts in, whatever it is drawn with.
    let text: Box<dyn Element> = ConstrainedBox::new(text).with_height(LINE_BOX).finish();
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

    /// The review pane's in-place editors reuse this module's read and
    /// highlight, and a dirty buffer survives closing the editor: "Done" puts
    /// the diff back without throwing an unsaved edit away.
    #[test]
    fn a_review_editors_text_is_parsed_once_and_an_unsaved_edit_survives_done() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("main.rs");
        fs::write(&path, "fn main() {}\n").expect("write");
        let path = path.to_string_lossy().to_string();

        let mut cache = DiffFileCache::default();
        let first = cache.read(&[(1, path.clone())]);
        let editor = first
            .get(&1)
            .and_then(|editors| editors.get(&path))
            .expect("the file is read");
        assert!(editor.buffer.borrow().editable());
        assert!(
            editor.content.runs().is_none(),
            "the read itself takes no syntax pass"
        );
        // The worker colours the file beside the frame, so the runs arrive a
        // read or two later; the mapping then carries them.
        let mut coloured = None;
        for _ in 0..500 {
            let ready = cache.read(&[(1, path.clone())]);
            let ready = ready
                .get(&1)
                .and_then(|editors| editors.get(&path))
                .expect("the file is read");
            if let Some(runs) = &ready.runs {
                coloured = Some(Rc::clone(runs));
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let runs = coloured.expect("the worker colours the file's own type");
        assert!(!runs.lines.is_empty(), "a Rust file has coloured runs");

        editor
            .buffer
            .borrow_mut()
            .set_text("fn main() { done(); }\n".to_string());

        // A second read of an unchanged file hands back the cached runs.
        let second = cache.read(&[(1, path.clone())]);
        let cached = second
            .get(&1)
            .and_then(|editors| editors.get(&path))
            .expect("the file is read again");
        assert!(
            cached
                .runs
                .as_ref()
                .is_some_and(|again| Rc::ptr_eq(&again.lines, &runs.lines)),
            "an unchanged file is not parsed and highlighted again"
        );

        // Closing the editor (`Done`) leaves no wanted files, but the edit is
        // still unsaved, so it stays and is shown again when the file reopens.
        assert!(cache.read(&[]).is_empty());
        let reopened = cache.read(&[(1, path.clone())]);
        let editor = reopened
            .get(&1)
            .and_then(|editors| editors.get(&path))
            .expect("the file is still cached");
        assert_eq!(
            editor.buffer.borrow().text(),
            "fn main() { done(); }\n",
            "the unsaved edit is kept"
        );
        assert!(editor.buffer.borrow().dirty());
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

    /// The buffer the pane at `pane` holds for a single file.
    fn buffer_at(cache: &FileCache, pane: u64) -> Rc<RefCell<FileBuffer>> {
        cache
            .buffers()
            .into_iter()
            .find(|((id, _), _)| *id == pane)
            .map(|(_, buffer)| buffer)
            .unwrap_or_else(|| panic!("pane {pane} has a buffer"))
    }

    /// A file that was read whole gets an editable buffer: an edit marks it
    /// unsaved, a save writes the buffer's own text — the exact bytes, no part
    /// of it — and the mark clears.
    #[test]
    fn a_buffer_edits_its_file_and_saves_the_exact_bytes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("main.rs");
        fs::write(&path, "fn main() {}\n").expect("write");
        let path = path.to_string_lossy().to_string();

        let mut cache = FileCache::default();
        cache.read(&[(7, path.clone())]);
        let buffer = buffer_at(&cache, 7);
        assert!(buffer.borrow().editable(), "a file read whole is editable");
        assert_eq!(
            buffer.borrow().text(),
            "fn main() {}\n",
            "the buffer starts at the file's own bytes"
        );
        assert!(!buffer.borrow().dirty(), "nothing is unsaved yet");

        let edited = "fn main() {\n    done();\n}\n";
        buffer.borrow_mut().set_text(edited.to_string());
        assert!(buffer.borrow().dirty(), "an edit is unsaved");
        assert!(
            buffer.borrow().detail().contains("unsaved changes"),
            "and the pane is told so: {}",
            buffer.borrow().detail()
        );
        assert_eq!(
            fs::read_to_string(&path).expect("read"),
            "fn main() {}\n",
            "typing writes nothing to the file"
        );

        buffer.borrow_mut().save().expect("the save writes");
        assert!(!buffer.borrow().dirty(), "a save clears the dirty mark");
        assert_eq!(buffer.borrow().status(), Some("saved"));
        assert_eq!(
            fs::read_to_string(&path).expect("read"),
            edited,
            "the file holds the buffer's bytes, whole"
        );

        // Typing the file's own bytes back out is not an unsaved change.
        buffer.borrow_mut().set_text(edited.to_string());
        assert!(!buffer.borrow().dirty(), "the file's own text is not a change");
    }

    /// A file the read did not take whole stays read-only, and the refusal is
    /// the state machine's rather than the view's: `save` writes nothing and
    /// says why.
    #[test]
    fn a_capped_or_uneditable_file_refuses_to_save_and_writes_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let long = dir.path().join("long.txt");
        let text: String = (0..MAX_LINES + 25).map(|i| format!("line {i}\n")).collect();
        fs::write(&long, &text).expect("write");
        let long = long.to_string_lossy().to_string();

        // Sparse, so the test does not write four megabytes to prove the cap.
        let huge = dir.path().join("huge.bin");
        fs::File::create(&huge)
            .expect("create")
            .set_len(MAX_FILE_BYTES + 1)
            .expect("grow");
        let huge = huge.to_string_lossy().to_string();

        let not_text = dir.path().join("logo.png");
        fs::write(&not_text, [0x89, b'P', b'N', b'G', 0x00, 0x1a]).expect("write");
        let not_text = not_text.to_string_lossy().to_string();

        let mut cache = FileCache::default();
        cache.read(&[
            (1, long.clone()),
            (2, huge.clone()),
            (3, not_text.clone()),
        ]);

        for (pane, path, reason, bytes) in [
            (1, &long, "too long to edit", text.len() as u64),
            (2, &huge, "too large to edit", MAX_FILE_BYTES + 1),
            (3, &not_text, "not text, so not editable", 6),
        ] {
            let buffer = buffer_at(&cache, pane);
            assert!(!buffer.borrow().editable(), "pane {pane} is read-only");
            assert_eq!(buffer.borrow().read_only(), Some(reason));
            // The buffer is handed text it must not write, so the refusal is
            // shown to be the state machine's and not the pane's drawing.
            buffer.borrow_mut().set_text("overwritten".to_string());
            let refused = buffer
                .borrow_mut()
                .save()
                .expect_err("a read-only file must not be written");
            assert!(refused.contains(reason), "{refused}");
            assert_eq!(
                fs::metadata(path).expect("the file is still there").len(),
                bytes,
                "{path}: the refused save wrote nothing"
            );
        }
        assert_eq!(
            fs::read(&long).expect("read"),
            text.as_bytes(),
            "the capped file is exactly as it was"
        );
    }

    /// The caps are boundaries, not roundings: a file of exactly `MAX_LINES`
    /// lines was read whole, so it is editable — and its buffer holds the file's
    /// own bytes, the trailing newline included.
    #[test]
    fn a_file_at_the_line_cap_is_still_editable() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("exact.txt");
        let text: String = (0..MAX_LINES).map(|i| format!("line {i}\n")).collect();
        fs::write(&path, &text).expect("write");

        let mut cache = FileCache::default();
        cache.read(&[(1, path.to_string_lossy().to_string())]);
        let buffer = buffer_at(&cache, 1);
        assert!(
            buffer.borrow().editable(),
            "the file was read whole, so it is editable"
        );
        assert_eq!(
            buffer.borrow().text(),
            text,
            "and the buffer holds the file's own bytes"
        );
    }

    /// A file written from outside the app: a clean buffer takes the new text,
    /// while a buffer with unsaved edits keeps them and the pane says the file
    /// moved under it.
    #[test]
    fn an_external_write_keeps_an_unsaved_edit_and_refreshes_a_clean_buffer() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("notes.txt");
        fs::write(&path, "one\n").expect("write");
        let path = path.to_string_lossy().to_string();
        let wanted = vec![(3, path.clone())];

        let mut cache = FileCache::default();
        cache.read(&wanted);
        assert_eq!(buffer_at(&cache, 3).borrow().text(), "one\n");

        // Clean: the pane shows the file as it now is.
        fs::write(&path, "one\ntwo\n").expect("write outside the app");
        cache.read(&wanted);
        assert_eq!(
            buffer_at(&cache, 3).borrow().text(),
            "one\ntwo\n",
            "a clean buffer takes the file's new text"
        );
        assert!(!buffer_at(&cache, 3).borrow().dirty());
        assert_eq!(buffer_at(&cache, 3).borrow().note(), None);

        // Dirty: the edit is kept, and the pane says why it differs.
        buffer_at(&cache, 3)
            .borrow_mut()
            .set_text("mine\n".to_string());
        fs::write(&path, "one\ntwo\nthree\n").expect("write outside the app");
        cache.read(&wanted);
        assert_eq!(
            buffer_at(&cache, 3).borrow().text(),
            "mine\n",
            "an unsaved edit is never discarded by a write from outside"
        );
        assert!(buffer_at(&cache, 3).borrow().dirty());
        assert!(
            buffer_at(&cache, 3).borrow().note().is_some(),
            "and the pane says the file moved under it"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("read"),
            "one\ntwo\nthree\n",
            "reading never writes the file"
        );
    }

    /// A save is this buffer's own write, not a change from outside: the cache
    /// entry read before it is dropped, so the next frame reads — and
    /// re-highlights — what the save wrote, and the pane does not report the
    /// file as changed under the edits it just saved.
    #[test]
    fn a_saved_file_is_read_again_rather_than_reported_as_changed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("main.rs");
        // The same length either way, so only the entry's own drop can make the
        // read that follows see the write.
        fs::write(&path, "let a = 1;\n").expect("write");
        let path = path.to_string_lossy().to_string();
        let wanted = vec![(5, path.clone())];

        let mut cache = FileCache::default();
        cache.read(&wanted);
        buffer_at(&cache, 5)
            .borrow_mut()
            .set_text("let b = 2;\n".to_string());
        buffer_at(&cache, 5)
            .borrow_mut()
            .save()
            .expect("the save writes");

        // The frame after the save drops the entry predating it; the frame after
        // that re-reads the file the save wrote.
        cache.read(&wanted);
        let contents = cache.read(&wanted);
        match contents.get(&5).map(|content| &content.body) {
            Some(FileBody::Lines { shown, .. }) => {
                assert_eq!(shown.as_slice(), ["let b = 2;"], "the save is what is on disk")
            }
            other => panic!("expected the saved line, got {other:?}"),
        }
        assert!(!buffer_at(&cache, 5).borrow().dirty());
        assert_eq!(buffer_at(&cache, 5).borrow().note(), None);
    }

    /// The pane's own chord belongs to the active pane: a background file pane
    /// leaves Cmd+S to whatever has the keyboard, and never writes its file from
    /// it.
    #[test]
    fn only_the_active_pane_answers_the_save_chord() {
        use goble_ui::event::ModifiersState;

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("main.rs");
        fs::write(&path, "fn main() {}\n").expect("write");
        let path = path.to_string_lossy().to_string();

        let mut cache = FileCache::default();
        cache.read(&[(9, path.clone())]);
        let buffer = buffer_at(&cache, 9);
        buffer.borrow_mut().set_text("changed\n".to_string());

        let app = AppContext::default();
        let mut ctx = EventContext;
        let chord = DispatchedEvent::KeyDown {
            key: "s".to_string(),
            modifiers: ModifiersState {
                command: true,
                ..Default::default()
            },
        };

        let mut background = FileKeys::new(Rc::clone(&buffer), false, Empty::new().finish());
        assert!(
            !background.dispatch_event(&chord, &mut ctx, &app),
            "a background pane leaves the chord to the app"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("read"),
            "fn main() {}\n",
            "and writes nothing"
        );

        let mut active = FileKeys::new(Rc::clone(&buffer), true, Empty::new().finish());
        assert!(
            active.dispatch_event(&chord, &mut ctx, &app),
            "the active pane answers it"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("read"),
            "changed\n",
            "and the file holds the buffer"
        );
    }

    /// A file pane is typeable from the frame it opens in, with no press first.
    ///
    /// The explorer opens a file by splitting a pane off the one that asked and
    /// making the new pane the space's active one, and the editor field's focus
    /// follows exactly that gate — the same one the pane's Cmd/Ctrl+S reads.
    /// While the field was focused only by a press inside it, a pane that had
    /// just been opened took no keystroke at all: its first Cmd/Ctrl+S "saved" a
    /// buffer nobody had typed into, and the file kept its old bytes.
    #[test]
    fn a_freshly_opened_file_pane_takes_typing_and_saves_it() {
        use crate::root_view::RootView;
        use crate::ui::{PaneKind, SplitDir};
        use goble_core::store::Store;
        use goble_desktop_service::{DesktopState, ThreadStore};
        use goble_ui::elements::{EventContext, LayoutContext, PaintContext, SizeConstraint};
        use goble_ui::event::ModifiersState;
        use goble_ui::geometry::vec2f;
        use goble_ui::render::{RenderCommand, Renderer};
        use std::sync::Arc as StdArc;

        let store_dir = tempfile::tempdir().expect("thread store dir");
        let desktop = StdArc::new(DesktopState::new(
            Store::open_in_memory().expect("store"),
            ThreadStore::new(store_dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("notes.txt");
        let original = "alpha\nbeta\n";
        fs::write(&path, original).expect("write");
        let path = path.to_string_lossy().to_string();

        let view = RootView::new(&app, &desktop, None);
        {
            let state = view.state_rc();
            let mut state = state.borrow_mut();
            state.show_workspace_choice = false;
            state.show_llm_key_banner = false;
            state.show_onboarding_tip = false;
            state.right_sidebar_open = false;
            let (space, pane) = (state.active_space, state.active_pane_id);
            let mut next = state.next_pane_id;
            let new_id = state.spaces[space]
                .split_with_kind(
                    pane,
                    SplitDir::Horizontal,
                    &mut next,
                    PaneKind::File { path: path.clone() },
                )
                .expect("a file pane splits off the pane that asked for it");
            state.next_pane_id = next;
            state.active_pane_id = new_id;
            state.sync_active_view();
        }
        let mut root: Box<dyn Element> = Box::new(view);
        let frame = |root: &mut Box<dyn Element>| -> Vec<RenderCommand> {
            let _ = root.layout(
                SizeConstraint::loose(vec2f(1024.0, 768.0)),
                &mut LayoutContext,
                &app,
            );
            let mut ctx = PaintContext::new(Renderer::new());
            root.paint(vec2f(0.0, 0.0), &mut ctx, &app);
            ctx.renderer
                .take()
                .map(|renderer| renderer.commands().to_vec())
                .unwrap_or_default()
        };
        let _ = frame(&mut root);

        // No press anywhere: the pane is the space's active pane, which is all
        // the field's focus and the save chord read.
        let mut ctx = EventContext;
        assert!(
            root.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: "!".to_string(),
                    modifiers: ModifiersState::default(),
                },
                &mut ctx,
                &app,
            ),
            "the pane the explorer just opened takes a keystroke"
        );
        let commands = frame(&mut root);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, .. } if text.contains('!')
            )),
            "and the character is in the file the pane is showing: {commands:?}"
        );

        assert!(
            root.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: "s".to_string(),
                    modifiers: ModifiersState {
                        command: true,
                        ..Default::default()
                    },
                },
                &mut ctx,
                &app,
            ),
            "Cmd+S is the pane's own chord"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("read"),
            "!alpha\nbeta\n",
            "and the save writes the buffer the pane was typing into, whole"
        );

        // A save the pane made is not a change from outside: the frame after it
        // drops the read that predates the write, and the frame after that
        // re-reads what was written, so the pane never calls its own save a
        // change under the edit it just made.
        let _ = frame(&mut root);
        let after_save = frame(&mut root);
        assert!(
            !after_save.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, .. } if text.contains("changed on disk")
            )),
            "the pane does not report its own save as a write from outside: {after_save:?}"
        );
    }

    /// Every line of the body is an element of the frame and the tree is rebuilt
    /// every frame, so a body that draws a long file in full lays out every line
    /// of it on every frame, whatever changed. The read-only body draws the
    /// lines the pane's own viewport can show: the first frame, which has no
    /// viewport measured yet, draws a screenful, and the frame after draws
    /// exactly what fits plus the window's slack. The lines it leaves out are
    /// spacers of their own height, so the body still measures the whole file
    /// and the pane still scrolls through all of it.
    ///
    /// Both of the body's row kinds are measured, on a pane tall enough that a
    /// row of the wrong pitch would fall short of the bottom of it: `.txt`
    /// resolves to Plain Text and draws a line as runs, `.zzz` resolves to
    /// nothing and draws it plain.
    #[test]
    fn a_long_file_draws_only_the_lines_its_viewport_shows() {
        let app = AppContext::default();
        let dir = tempfile::tempdir().expect("temp dir");
        let lines = MAX_LINES + 1_000;

        for name in ["long.txt", "long.zzz"] {
            let path = dir.path().join(name);
            let text: String = (0..lines)
                .map(|i| format!("let value_{i} = compute({i});\n"))
                .collect();
            fs::write(&path, &text).expect("write");
            let path = path.to_string_lossy().to_string();

            let (drawn, pitch, last_bottom, body_bottom, viewport, max_offset) =
                long_file_body(&app, &path);

            assert_eq!(
                drawn,
                (viewport / LINE_BOX).ceil() as usize + WINDOW_SLACK,
                "{name}: the frames with a viewport draw its own lines and the window's \
                 slack, {} rows in {viewport} points",
                (viewport / LINE_BOX).ceil() as usize + WINDOW_SLACK
            );
            assert_eq!(
                pitch, LINE_BOX,
                "{name}: a drawn row advances by one line box"
            );
            assert!(
                last_bottom >= body_bottom,
                "{name}: the rows the pane draws reach the bottom of its viewport: \
                 {last_bottom} against {body_bottom}"
            );
            assert!(
                max_offset >= MAX_LINES as f32 * LINE_BOX - viewport,
                "{name}: the lines the pane is not drawing still hold their height, so \
                 the body scrolls through every line the read took: {max_offset} against \
                 {} points of file",
                MAX_LINES as f32 * LINE_BOX - viewport
            );
        }
    }

    /// A file pane showing `path`, drawn at a window tall enough that the rows
    /// its body draws have to reach the bottom of it. Returns the second frame's
    /// commands — the first has no viewport to size a window against — and what
    /// the pane's own region measured.
    fn pane_frames(
        app: &AppContext,
        path: &str,
    ) -> (Vec<goble_ui::render::RenderCommand>, f32, f32) {
        use crate::root_view::RootView;
        use crate::ui::PaneKind;
        use goble_core::store::Store;
        use goble_desktop_service::{DesktopState, ThreadStore};
        use goble_ui::elements::{LayoutContext, PaintContext, SizeConstraint};
        use goble_ui::geometry::vec2f;
        use goble_ui::render::{RenderCommand, Renderer};
        use std::sync::Arc as StdArc;

        let store_dir = tempfile::tempdir().expect("thread store dir");
        let desktop = StdArc::new(DesktopState::new(
            Store::open_in_memory().expect("store"),
            ThreadStore::new(store_dir.path()).expect("thread store"),
        ));
        let view = RootView::new(app, &desktop, None);
        let pane_id = {
            let state = view.state_rc();
            let mut state = state.borrow_mut();
            state.show_workspace_choice = false;
            state.show_llm_key_banner = false;
            state.show_onboarding_tip = false;
            state.right_sidebar_open = false;
            let (space, pane) = (state.active_space, state.active_pane_id);
            state.spaces[space].set_leaf_kind(
                pane,
                PaneKind::File {
                    path: path.to_string(),
                },
            );
            pane
        };
        let state = view.state_rc();
        let mut root: Box<dyn Element> = Box::new(view);
        let mut frame = || -> Vec<RenderCommand> {
            let _ = root.layout(
                SizeConstraint::loose(vec2f(1024.0, 2400.0)),
                &mut LayoutContext,
                app,
            );
            let mut ctx = PaintContext::new(Renderer::new());
            root.paint(vec2f(0.0, 0.0), &mut ctx, app);
            ctx.renderer
                .take()
                .map(|renderer| renderer.commands().to_vec())
                .unwrap_or_default()
        };
        // The first frame has no viewport to size the window against; the one
        // after draws what the pane can show.
        let _ = frame();
        let commands = frame();

        let scroll = state
            .borrow()
            .file_scroll
            .get(&pane_id)
            .cloned()
            .expect("the pane's viewport was measured");
        let (viewport, max_offset) = {
            let scroll = scroll.borrow();
            (scroll.viewport(), scroll.max_offset())
        };
        assert!(viewport > 0.0, "the pane measured a viewport");
        (commands, viewport, max_offset)
    }

    /// The line numbers the pane drew in its gutter, in order.
    fn gutter_numbers(commands: &[goble_ui::render::RenderCommand]) -> Vec<usize> {
        use goble_ui::render::RenderCommand;

        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => {
                    // The gutter is right-aligned, so a small number carries
                    // leading spaces that `parse` would reject.
                    let padded = text.strip_suffix(' ')?;
                    (padded.len() == GUTTER_DIGITS
                        && padded.trim().chars().all(|ch| ch.is_ascii_digit()))
                    .then(|| padded.trim().parse().ok())?
                }
                _ => None,
            })
            .collect()
    }

    /// A file pane showing `path`, at a window tall enough that the rows it
    /// draws have to reach the bottom of it. Returns what the read-only body
    /// drew — how many lines, the pitch its rows advance by, and where the last
    /// of them ends — the bottom of the region they are clipped to, and what
    /// that region measured.
    fn long_file_body(
        app: &AppContext,
        path: &str,
    ) -> (usize, f32, f32, f32, f32, f32) {
        use goble_ui::geometry::PointF;
        use goble_ui::render::RenderCommand;

        let (commands, viewport, max_offset) = pane_frames(app, path);

        let origins: Vec<Vector2F> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text.starts_with("let value_") => {
                    Some(*origin)
                }
                _ => None,
            })
            .collect();
        let pitch = origins[1].y - origins[0].y;
        let last_bottom = origins.last().expect("the file's lines are drawn").y + pitch;
        let body = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::ClipRect(rect)
                    if rect.contains(PointF::new(origins[0].x, origins[0].y)) =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the body is clipped to the pane's viewport");
        (
            origins.len(),
            pitch,
            last_bottom,
            body.max_y(),
            viewport,
            max_offset,
        )
    }

    /// The editing body is windowed the way the read-only one is: a file the
    /// pane opens for editing draws the rows its own viewport can show, not the
    /// whole buffer, while the buffer still stands at its full height. Without
    /// the window a long file costs a layout of every one of its lines on every
    /// frame, which is what the read-only body already avoids.
    #[test]
    fn an_editable_file_draws_only_the_lines_its_viewport_shows() {
        let app = AppContext::default();
        let dir = tempfile::tempdir().expect("temp dir");
        let lines = MAX_LINES - 100;
        let path = dir.path().join("editable.rs");
        let text: String = (0..lines)
            .map(|i| format!("let value_{i} = compute({i});\n"))
            .collect();
        fs::write(&path, &text).expect("write");
        let path = path.to_string_lossy().to_string();
        assert!(
            read_only_reason(&read(&path)).is_none(),
            "the pane opens this file for editing"
        );

        let (commands, viewport, max_offset) = pane_frames(&app, &path);
        let drawn = gutter_numbers(&commands);
        let rows = (viewport / LINE_BOX).ceil() as usize;
        assert_eq!(
            drawn.len(),
            rows + WINDOW_SLACK,
            "the editing pane draws its own rows and the window's slack, {rows} \
             rows in {viewport} points"
        );
        assert_eq!(drawn[0], 1, "and opens at the buffer's first line");
        assert!(
            *drawn.last().expect("rows are drawn") < lines,
            "the buffer's last line is outside the window: {} rows drawn",
            drawn.len()
        );
        assert_eq!(
            max_offset,
            // The buffer holds the file verbatim, newline at its end and all,
            // so it has a last line past the last newline.
            (lines + 1) as f32 * LINE_BOX - viewport,
            "the rows outside the window stand in at their own height, so the \
             buffer still scrolls through all of itself"
        );
    }

    /// Every row of the read-only body is one [`LINE_BOX`] tall, whether its line
    /// was highlighted or drawn plain. The body's spacers and its offset→line
    /// arithmetic are counted in that box, so a plain row that advanced a
    /// fraction short of a highlighted one would walk the drawn rows off the
    /// fillers they stand on and leave the bottom of a tall pane blank.
    #[test]
    fn a_plain_row_and_a_highlighted_row_are_one_line_box_tall() {
        let app = AppContext::default();
        let line = "let value = compute(1);";
        let runs = highlight_lines(&[line.to_string()], "a.rs").expect("rust resolves");
        let constraint = SizeConstraint::loose(Vector2F::new(600.0, 400.0));
        let mut ctx = LayoutContext;

        let mut plain = line_row(&app, 1, line, None);
        let mut highlighted = line_row(&app, 1, line, Some(&runs[0]));
        assert_eq!(
            plain.layout(constraint, &mut ctx, &app).y,
            LINE_BOX,
            "a line drawn plain takes one line box"
        );
        assert_eq!(
            highlighted.layout(constraint, &mut ctx, &app).y,
            LINE_BOX,
            "and a highlighted line takes the same one"
        );
    }

    /// A window with one file pane showing `path`, made the active pane. The
    /// fixture directory is returned so the store outlives the test.
    fn file_pane(
        app: &AppContext,
        path: &str,
    ) -> (
        Box<dyn Element>,
        Rc<RefCell<crate::state::UiState>>,
        std::sync::Arc<goble_desktop_service::DesktopState>,
        u64,
        tempfile::TempDir,
    ) {
        use crate::root_view::RootView;
        use crate::ui::{PaneKind, SplitDir};
        use goble_core::store::Store;
        use goble_desktop_service::{DesktopState, ThreadStore};

        let store_dir = tempfile::tempdir().expect("thread store dir");
        // `DesktopState::new` already hands back a shared handle.
        let desktop = DesktopState::new(
            Store::open_in_memory().expect("store"),
            ThreadStore::new(store_dir.path()).expect("thread store"),
        );
        let view = RootView::new(app, &desktop, None);
        let state = view.state_rc();
        let pane_id = {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.show_onboarding_tip = false;
            s.right_sidebar_open = false;
            let (space, pane) = (s.active_space, s.active_pane_id);
            // The file pane is split off the original one: a space keeps its
            // last pane, and closing the file pane has to collapse onto it.
            let mut next = s.next_pane_id;
            let file_pane = s.spaces[space]
                .split_with_kind(
                    pane,
                    SplitDir::Vertical,
                    &mut next,
                    PaneKind::File {
                        path: path.to_string(),
                    },
                )
                .expect("the file pane splits off the pane that opened it");
            s.next_pane_id = next;
            s.active_pane_id = file_pane;
            s.sync_active_view();
            file_pane
        };
        (
            Box::new(view),
            state,
            desktop,
            pane_id,
            store_dir,
        )
    }

    fn draw(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<goble_ui::render::RenderCommand> {
        draw_at(root, app, None)
    }

    /// Draw with the pointer at `cursor` when given, so paint-time hover state
    /// can be asserted.
    fn draw_at(
        root: &mut Box<dyn Element>,
        app: &AppContext,
        cursor: Option<Vector2F>,
    ) -> Vec<goble_ui::render::RenderCommand> {
        use goble_ui::elements::PaintContext;
        use goble_ui::render::Renderer;

        let _ = root.layout(
            SizeConstraint::loose(Vector2F::new(1024.0, 768.0)),
            &mut LayoutContext,
            app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        if let Some(cursor) = cursor {
            ctx.cursor_position = cursor;
            ctx.cursor_inside = true;
        }
        root.paint(Vector2F::zero(), &mut ctx, app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    fn key_in(
        root: &mut Box<dyn Element>,
        app: &AppContext,
        key: &str,
        modifiers: goble_ui::event::ModifiersState,
    ) -> bool {
        use goble_ui::event::DispatchedEvent;

        let mut ctx = EventContext;
        root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: key.to_string(),
                modifiers,
            },
            &mut ctx,
            app,
        )
    }

    /// The file is named in the pane's own topbar. It adds no tab to the
    /// general bar, and no title row under the topbar either.
    #[test]
    fn a_file_is_named_in_the_pane_topbar_and_not_in_the_general_one() {
        use crate::ui::shell::TOPBAR_HEIGHT;
        use goble_ui::render::RenderCommand;

        let app = AppContext::default();
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("notes.txt");
        fs::write(&path, "alpha\nbeta\n").expect("write");
        let path = path.to_string_lossy().to_string();

        let (mut root, state, _desktop, pane_id, _store) = file_pane(&app, &path);
        let _ = draw(&mut root, &app);
        let commands = draw(&mut root, &app);
        let pane_top = state
            .borrow()
            .pane_bounds
            .borrow()
            .get(&pane_id)
            .map(|rect| rect.min_y())
            .expect("the pane was measured");
        let name = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "notes.txt" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the pane names its file");
        assert!(
            name.y >= pane_top && name.y < pane_top + TOPBAR_HEIGHT,
            "the name is in the pane's topbar, not in a row under it: {name:?} against {pane_top}"
        );
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, origin, .. }
                    if text == "notes.txt" && origin.y < TOPBAR_HEIGHT
            )),
            "a file adds no tab to the general bar: {commands:?}"
        );
    }

    /// Several files open in one pane become a row of chips in that pane's
    /// topbar: the file it shows is filled in, the others outlined and a click
    /// away. Switching files keeps the pane and its other files.
    #[test]
    fn several_open_files_draw_each_name_as_a_chip() {
        use crate::ui::shell::TOPBAR_HEIGHT;
        use crate::ui::PaneKind;
        use goble_ui::event::BUTTON_PRIMARY;
        use goble_ui::geometry::PointF;
        use goble_ui::render::RenderCommand;
        use goble_ui::theme::ColorToken;

        let app = AppContext::default();
        let dir = tempfile::tempdir().expect("temp dir");
        let first = dir.path().join("alpha.txt");
        let second = dir.path().join("beta.txt");
        fs::write(&first, "alpha\n").expect("write");
        fs::write(&second, "beta\n").expect("write");
        let first = first.to_string_lossy().to_string();
        let second = second.to_string_lossy().to_string();

        let (mut root, state, _desktop, pane_id, _store) = file_pane(&app, &first);
        assert!(
            state.borrow_mut().show_file_in_pane(pane_id, second.clone()),
            "the pane takes a second file"
        );
        let _ = draw(&mut root, &app);
        let commands = draw(&mut root, &app);
        let pane_top = state
            .borrow()
            .pane_bounds
            .borrow()
            .get(&pane_id)
            .map(|rect| rect.min_y())
            .expect("the pane was measured");
        let origin_of = |name: &str| {
            commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::DrawText { text, origin, .. } if text == name => Some(*origin),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("the pane names {name}: {commands:?}"))
        };

        for (name, text_color) in [
            ("alpha.txt", ColorToken::Muted),
            ("beta.txt", ColorToken::Text),
        ] {
            let origin = origin_of(name);
            assert!(
                origin.y >= pane_top && origin.y < pane_top + TOPBAR_HEIGHT,
                "{name} stays in the pane's topbar: {origin:?} against {pane_top}"
            );
            assert!(
                commands.iter().any(|command| matches!(
                    command,
                    RenderCommand::DrawText { text, color, .. }
                        if text == name && *color == app.theme.color(text_color)
                )),
                "{name} is drawn in its chip's own colour"
            );
        }

        // The shown file wears the tab look, not a plaque: a square raised cell
        // the height of the bar, with no padding or rounded frame of its own.
        let raised = app.theme.color(ColorToken::SurfaceRaised);
        let beta = origin_of("beta.txt");
        let beta_point = PointF::new(beta.x, beta.y);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { rect, color, corner_radius }
                    if *color == raised && *corner_radius == 0.0 && rect.contains(beta_point)
            )),
            "the shown file sits on a square tab fill: {commands:?}"
        );
        let alpha = origin_of("alpha.txt");
        let alpha_point = PointF::new(alpha.x, alpha.y);
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { rect, corner_radius, .. }
                    if *corner_radius > 0.0
                        && rect.height() <= chat::HEADER_CONTROL_HEIGHT + 4.0
                        && rect.contains(alpha_point)
            )),
            "a file chip has no rounded frame: {commands:?}"
        );

        // The other chip is plain until the pointer is over it: then it takes
        // the tab strip's own square hover fill, and shows its close "x".
        let hovered = draw_at(&mut root, &app, Some(Vector2F::new(alpha.x, alpha.y)));
        let alpha_chip = hovered
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::Hover)
                    && *corner_radius == 0.0
                    && rect.contains(alpha_point) =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("the chip takes a hover fill: {hovered:?}"));
        assert!(
            hovered.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, origin, .. }
                    if name == "x-close"
                        && origin.x > alpha_chip.min_x()
                        && origin.x < alpha_chip.max_x()
            )),
            "a hovered chip shows its close control: {hovered:?}"
        );

        // A click on the chip shows that file in the same pane, without a
        // second pane and without moving the chip.
        let at = Vector2F::new(alpha.x, alpha.y);
        let mut ctx = EventContext::default();
        root.dispatch_event(&DispatchedEvent::MouseMove { position: at }, &mut ctx, &app);
        root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: at,
                button: BUTTON_PRIMARY,
            },
            &mut ctx,
            &app,
        );
        root.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: at,
                button: BUTTON_PRIMARY,
            },
            &mut ctx,
            &app,
        );
        {
            let s = state.borrow();
            let space = s.active_space;
            assert!(
                matches!(
                    s.spaces[space].leaf_kind(pane_id),
                    Some(PaneKind::File { path }) if path == &first
                ),
                "the chip switches the pane to its file"
            );
            assert_eq!(
                s.pane_open_files.get(&pane_id).map(Vec::as_slice),
                Some([first.clone(), second.clone()].as_slice()),
                "the chip row keeps its order when a file is shown"
            );
        }

        // The hovered chip's "x" closes that one file: the pane stays, with
        // the file it was showing.
        let beta_hover = draw_at(&mut root, &app, Some(Vector2F::new(beta.x, beta.y)));
        let beta_chip = beta_hover
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::Hover)
                    && *corner_radius == 0.0
                    && rect.contains(beta_point) =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the other chip takes a hover fill");
        let close_at = Vector2F::new(
            beta_chip.max_x() - 7.0 - 8.0,
            (beta_chip.min_y() + beta_chip.max_y()) / 2.0,
        );
        root.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: close_at,
            },
            &mut ctx,
            &app,
        );
        root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: close_at,
                button: BUTTON_PRIMARY,
            },
            &mut ctx,
            &app,
        );
        root.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: close_at,
                button: BUTTON_PRIMARY,
            },
            &mut ctx,
            &app,
        );
        let s = state.borrow();
        let space = s.active_space;
        assert!(
            matches!(
                s.spaces[space].leaf_kind(pane_id),
                Some(PaneKind::File { path }) if path == &first
            ),
            "closing one chip leaves the pane showing its other file"
        );
        assert_eq!(
            s.pane_open_files.get(&pane_id).map(Vec::as_slice),
            Some([first].as_slice()),
            "only the closed file leaves the chip row"
        );
    }

    /// A chip's close asks about that file alone: the pane and its other files
    /// stay, and Cancel (Escape) keeps even the dirty one open.
    #[test]
    fn closing_a_dirty_file_chip_asks_only_about_that_file() {
        use crate::ui::PaneKind;
        use goble_ui::event::{ModifiersState, BUTTON_PRIMARY};
        use goble_ui::geometry::PointF;
        use goble_ui::render::RenderCommand;
        use goble_ui::theme::ColorToken;

        let app = AppContext::default();
        let dir = tempfile::tempdir().expect("temp dir");
        let first = dir.path().join("alpha.txt");
        let second = dir.path().join("beta.txt");
        fs::write(&first, "alpha\n").expect("write");
        fs::write(&second, "beta\n").expect("write");
        let first = first.to_string_lossy().to_string();
        let second = second.to_string_lossy().to_string();

        let (mut root, state, desktop, pane_id, _store) = file_pane(&app, &first);
        assert!(state.borrow_mut().show_file_in_pane(pane_id, second.clone()));
        let _ = draw(&mut root, &app);
        let _ = draw(&mut root, &app);
        assert!(key_in(&mut root, &app, "!", ModifiersState::default()));
        let commands = draw(&mut root, &app);
        assert!(
            state
                .borrow()
                .file_buffers
                .get(&(pane_id, second.clone()))
                .is_some_and(|buffer| buffer.borrow().dirty()),
            "the shown file has an unsaved edit"
        );

        // The shown file's square tab fill gives the chip its bounds; the close
        // control sits at the chip's trailing edge.
        let origin = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "beta.txt" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the pane names the second file");
        let chip = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == app.theme.color(ColorToken::SurfaceRaised)
                        && rect.contains(PointF::new(origin.x, origin.y)) =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the shown file sits on its chip");
        let close_at = Vector2F::new(
            chip.max_x() - 7.0 - 8.0,
            (chip.min_y() + chip.max_y()) / 2.0,
        );
        // Hovering the chip is what draws its close control; a control that
        // was never painted has no bounds to hit.
        let _ = draw_at(&mut root, &app, Some(Vector2F::new(origin.x, origin.y)));
        let mut ctx = EventContext::default();
        root.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: close_at,
            },
            &mut ctx,
            &app,
        );
        root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: close_at,
                button: BUTTON_PRIMARY,
            },
            &mut ctx,
            &app,
        );
        root.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: close_at,
                button: BUTTON_PRIMARY,
            },
            &mut ctx,
            &app,
        );
        assert_eq!(
            state.borrow().pending_close_file,
            Some((pane_id, second.clone())),
            "the dirty chip asks before it closes"
        );
        assert!(
            state.borrow().space_index_of_pane(pane_id).is_some(),
            "the pane stays while the question is up"
        );
        assert_eq!(
            state
                .borrow()
                .pane_open_files
                .get(&pane_id)
                .cloned()
                .unwrap_or_default(),
            vec![first.clone(), second.clone()],
            "no chip leaves before the answer"
        );

        // Escape is Cancel: the chip and its edit stay.
        let commands = draw(&mut root, &app);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, .. } if text == "Save changes before closing?"
            )),
            "the chip's close draws the save question: {commands:?}"
        );
        assert!(key_in(
            &mut root,
            &app,
            "Escape",
            ModifiersState::default()
        ));
        assert_eq!(state.borrow().pending_close_file, None);
        assert_eq!(
            state
                .borrow()
                .pane_open_files
                .get(&pane_id)
                .cloned()
                .unwrap_or_default(),
            vec![first.clone(), second.clone()]
        );

        // Ask again and save: only that file is written and dropped.
        state
            .borrow_mut()
            .request_close_file(pane_id, second.clone(), Some(&desktop));
        state
            .borrow_mut()
            .close_pending_file_save(Some(&desktop));
        assert_eq!(
            fs::read_to_string(&second).expect("read"),
            "!beta\n",
            "the chip's close wrote its buffer"
        );
        let s = state.borrow();
        let space = s.active_space;
        assert!(
            s.space_index_of_pane(pane_id).is_some(),
            "the pane stays with the file that is left"
        );
        assert!(
            matches!(
                s.spaces[space].leaf_kind(pane_id),
                Some(PaneKind::File { path }) if path == &first
            ),
            "the remaining file takes the pane"
        );
        assert_eq!(
            s.pane_open_files.get(&pane_id).map(Vec::as_slice),
            Some([first].as_slice())
        );
    }

    /// The file body does not fold its lines: the horizontal region takes the
    /// wheel's sideways travel to the end of the longest line, while the
    /// vertical region keeps the rows.
    #[test]
    fn a_file_pane_scrolls_sideways_to_the_end_of_a_long_line() {
        use goble_ui::event::DispatchedEvent;
        use goble_ui::render::Renderer;

        let app = AppContext::default();
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("wide.txt");
        let wide = "x".repeat(400);
        fs::write(&path, format!("{wide}\nshort\n")).expect("write");
        let path = path.to_string_lossy().to_string();

        let (mut root, state, _desktop, pane_id, _store) = file_pane(&app, &path);
        let _ = draw(&mut root, &app);
        let _ = draw(&mut root, &app);
        let hscroll = state
            .borrow()
            .file_hscroll
            .get(&pane_id)
            .cloned()
            .expect("the pane's horizontal region was measured");
        assert!(
            hscroll.borrow().max_offset() > 0.0,
            "the 400-character line runs past the pane"
        );

        // The pointer is over the body; a sideways wheel step is the horizontal
        // region's, because the vertical one has no travel of its own to use.
        let at = {
            let state = state.borrow();
            let bounds = state.pane_bounds.borrow();
            let pane = bounds.get(&pane_id).copied().expect("the pane was measured");
            Vector2F::new(
                pane.min_x() + pane.width() * 0.5,
                pane.min_y() + pane.height() * 0.5,
            )
        };
        let mut paint = PaintContext::new(Renderer::new());
        paint.cursor_position = at;
        paint.cursor_inside = true;
        root.paint(Vector2F::zero(), &mut paint, &app);
        let mut ctx = EventContext;
        root.dispatch_event(&DispatchedEvent::MouseMove { position: at }, &mut ctx, &app);
        assert!(root.dispatch_event(
            &DispatchedEvent::Scroll {
                delta: Vector2F::new(-20.0, 0.0),
                precise: true,
            },
            &mut ctx,
            &app,
        ));
        assert!(
            hscroll.borrow().offset() > 0.0,
            "the body followed the sideways wheel"
        );
    }

    /// Closing a dirty file pane asks first; `Escape` keeps the pane, and
    /// "Save and close" writes the buffer before the pane goes.
    #[test]
    fn closing_a_dirty_file_pane_asks_then_saves() {
        use goble_ui::event::ModifiersState;
        use goble_ui::render::RenderCommand;

        let app = AppContext::default();
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("notes.txt");
        fs::write(&path, "alpha\nbeta\n").expect("write");
        let path = path.to_string_lossy().to_string();

        let (mut root, state, desktop, pane_id, _store) = file_pane(&app, &path);
        let _ = draw(&mut root, &app);
        assert!(key_in(
            &mut root,
            &app,
            "!",
            ModifiersState::default()
        ));
        let _ = draw(&mut root, &app);
        assert!(
            state
                .borrow()
                .file_buffers
                .iter()
                .any(|((pane, _), buffer)| *pane == pane_id && buffer.borrow().dirty()),
            "the typed character is an unsaved edit"
        );

        let command_w = ModifiersState {
            command: true,
            ..Default::default()
        };
        assert!(key_in(&mut root, &app, "w", command_w));
        assert_eq!(state.borrow().pending_close_pane, Some(pane_id));
        assert!(
            state.borrow().space_index_of_pane(pane_id).is_some(),
            "the pane is still open while the question is up"
        );
        let commands = draw(&mut root, &app);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, .. } if text == "Save changes before closing?"
            )),
            "the prompt is drawn: {commands:?}"
        );

        // `Escape` is Cancel: the pane stays, and so does the edit.
        assert!(key_in(
            &mut root,
            &app,
            "Escape",
            ModifiersState::default()
        ));
        assert_eq!(state.borrow().pending_close_pane, None);
        assert!(state.borrow().space_index_of_pane(pane_id).is_some());

        // Ask again and save: the file holds the edit, and the pane is gone.
        assert!(key_in(&mut root, &app, "w", command_w));
        state
            .borrow_mut()
            .close_pending_pane_save(Some(&desktop));
        assert_eq!(
            fs::read_to_string(&path).expect("read"),
            "!alpha\nbeta\n",
            "the pending close wrote the buffer"
        );
        assert!(state.borrow().space_index_of_pane(pane_id).is_none());
    }

    /// With "Auto-save on close" on, the same close saves and goes without a
    /// question.
    #[test]
    fn auto_save_on_close_writes_the_file_and_closes_without_a_prompt() {
        use goble_ui::event::ModifiersState;

        let app = AppContext::default();
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("notes.txt");
        fs::write(&path, "alpha\nbeta\n").expect("write");
        let path = path.to_string_lossy().to_string();

        let (mut root, state, _desktop, pane_id, _store) = file_pane(&app, &path);
        let _ = draw(&mut root, &app);
        assert!(key_in(
            &mut root,
            &app,
            "!",
            ModifiersState::default()
        ));
        let _ = draw(&mut root, &app);
        state.borrow_mut().auto_save_on_close = true;

        let command_w = ModifiersState {
            command: true,
            ..Default::default()
        };
        assert!(key_in(&mut root, &app, "w", command_w));
        assert_eq!(
            state.borrow().pending_close_pane,
            None,
            "no question is asked"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("read"),
            "!alpha\nbeta\n",
            "the close wrote the buffer"
        );
        assert!(state.borrow().space_index_of_pane(pane_id).is_none());
    }
}
