//! The sidebar's global search: what a query matches in the files under the
//! active pane's working directory.
//!
//! The search prefers ripgrep: it is a real recursive content search that
//! honours the machine's ignore files, and its output is parsed straight into
//! the rows the view draws. What it may not do is *require* ripgrep — an app
//! started from Finder inherits launchd's `PATH`, not the shell's, and a user
//! who has never installed it would get an error instead of a search. When no
//! `rg` binary can be found the search runs in-process over the same directory,
//! skipping the trees a search never wants (VCS metadata, build outputs,
//! dependency trees) and the files it cannot read.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use goble_ui::elements::{
    file_icon_name, AppContext, Axis, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets,
    Element, Empty, Expanded, Flex, HoverRow, Icon, MainAxisSize, Scrollable, SearchInput, Text,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::{SearchRow, UiActions, UiSnapshot};

/// Matched lines kept per file, so one chatty file cannot fill the band.
const LINES_PER_FILE: usize = 10;
/// Files kept, all told.
const FILES: usize = 200;
/// How long a matched line may be before ripgrep leaves it out: a minified line
/// is not something the band can show.
const MAX_LINE_BYTES: usize = 400;
/// How large a file the in-process search will read. A source file is far under
/// this; a bundle or a database is far over it and has nothing to show.
const MAX_FILE_BYTES: u64 = 1 << 20;
/// How many files the walk reads between two checks of whether the query it is
/// walking for has been superseded. Small enough that typing abandons the
/// previous query at once, large enough that the check costs nothing.
const CANCEL_CHECK_FILES: usize = 32;

/// Directories the in-process search never descends into. ripgrep skips these
/// through the ignore files; without it they are named here, because they hold
/// more files than everything the user cares about put together.
const SKIPPED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "target",
    "node_modules",
    ".venv",
    "venv",
    "dist",
    "build",
    ".next",
    ".cache",
    ".idea",
    ".vscode",
];

/// The `rg` binary to run, when the machine has one.
///
/// `PATH` is searched first, so a user who installed ripgrep somewhere their
/// terminal sees gets exactly that; then the directories Homebrew, Cargo and
/// the system put it in, which a Finder-launched app cannot see in its own
/// `PATH`.
fn ripgrep_binary() -> Option<PathBuf> {
    let from_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join("rg"))
            .find(|candidate| candidate.is_file())
    });
    from_path.or_else(|| {
        [
            "/opt/homebrew/bin/rg",
            "/usr/local/bin/rg",
            "/usr/bin/rg",
            "/opt/local/bin/rg",
        ]
        .iter()
        .map(PathBuf::from)
        .find(|candidate| candidate.is_file())
    })
}

/// One query for the worker to walk for.
struct Job {
    generation: u64,
    root: String,
    query: String,
}

/// What a walk produced: the rows to show, or the reason it has none.
struct Found {
    rows: Vec<SearchRow>,
    error: Option<String>,
}

/// A finished search the worker has published for the UI thread to collect.
pub(crate) struct SearchOutcome {
    generation: u64,
    pub rows: Vec<SearchRow>,
    pub error: Option<String>,
}

/// The search, moved off the UI thread.
///
/// A walk of the pane's directory reads every file under it, which is far too
/// slow to run between two of the user's keystrokes: doing it there freezes the
/// window for as long as the tree takes, and the bigger the tree the longer the
/// freeze. The field therefore hands each query here, and the one thread this
/// worker owns is the only thing that ever walks.
///
/// Typing is a stream of superseded queries, so the worker keeps only the newest
/// one: whatever is queued behind it is dropped, and a walk already running gives
/// up as soon as it notices a newer generation. The answer is published only
/// when it is still the newest — and the UI thread checks again on the way in —
/// so what the band draws always belongs to the query in the field.
pub struct SearchWorker {
    /// The channel to the walking thread, opened the first time a query is
    /// asked for so a state that never searches never starts one.
    jobs: RefCell<Option<Sender<Job>>>,
    /// The newest generation asked for, which the walk compares itself against.
    latest: Arc<AtomicU64>,
    /// The finished search waiting to be collected.
    done: Arc<Mutex<Option<SearchOutcome>>>,
    generation: Cell<u64>,
}

impl SearchWorker {
    pub(crate) fn new() -> Self {
        Self {
            jobs: RefCell::new(None),
            latest: Arc::new(AtomicU64::new(0)),
            done: Arc::new(Mutex::new(None)),
            generation: Cell::new(0),
        }
    }

    /// Ask for `query` under `root`, superseding whatever the worker was doing.
    pub(crate) fn search(&self, root: &str, query: &str) {
        let generation = self.supersede();
        let job = Job {
            generation,
            root: root.to_string(),
            query: query.to_string(),
        };
        let mut jobs = self.jobs.borrow_mut();
        if jobs.is_none() {
            *jobs = spawn_worker(Arc::clone(&self.latest), Arc::clone(&self.done));
        }
        match jobs.as_ref() {
            Some(sender) => {
                let _ = sender.send(job);
            }
            // A machine that refuses a thread still searches: the walk then
            // happens here, exactly as it did before the worker existed.
            None => publish(&self.done, generation, run_walk(root, query, &|| false)),
        }
    }

    /// Forget the query in flight: its answer is nobody's any more.
    pub(crate) fn cancel(&self) {
        self.supersede();
        if let Ok(mut done) = self.done.lock() {
            *done = None;
        }
    }

    /// The finished search, when the worker answered the newest query. Taking it
    /// leaves the slot empty.
    pub(crate) fn take(&self) -> Option<SearchOutcome> {
        let mut done = self.done.lock().ok()?;
        match done.as_ref() {
            Some(outcome) if outcome.generation == self.generation.get() => done.take(),
            // A superseded answer: it belongs to a query the field no longer
            // holds, so it is dropped rather than shown.
            _ => None,
        }
    }

    /// Move to the next generation, which is what tells a walking thread that
    /// its answer is no longer wanted.
    fn supersede(&self) -> u64 {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.latest.store(generation, Ordering::SeqCst);
        generation
    }
}

/// Start the walking thread. `None` when the machine refuses one, which leaves
/// the caller to walk on its own thread.
fn spawn_worker(latest: Arc<AtomicU64>, done: Arc<Mutex<Option<SearchOutcome>>>) -> Option<Sender<Job>> {
    let (jobs, inbox) = mpsc::channel::<Job>();
    std::thread::Builder::new()
        .name("global-search".to_string())
        .spawn(move || {
            while let Ok(mut job) = inbox.recv() {
                // Only the newest queued query is worth a walk: the ones behind
                // it are queries the user has already typed past.
                while let Ok(newer) = inbox.try_recv() {
                    job = newer;
                }
                latest.fetch_max(job.generation, Ordering::SeqCst);
                let cancelled = || latest.load(Ordering::SeqCst) != job.generation;
                let found = run_walk(&job.root, &job.query, &cancelled);
                // Superseded while it walked: the answer describes a query the
                // field has moved on from, so it is not published at all.
                if !cancelled() {
                    publish(&done, job.generation, found);
                }
            }
        })
        .ok()
        .map(|_| jobs)
}

/// Leave a finished walk where the UI thread will find it.
fn publish(done: &Mutex<Option<SearchOutcome>>, generation: u64, found: Option<Found>) {
    let Some(found) = found else { return };
    if let Ok(mut slot) = done.lock() {
        *slot = Some(SearchOutcome {
            generation,
            rows: found.rows,
            error: found.error,
        });
    }
}

fn run_walk(root: &str, query: &str, cancelled: &dyn Fn() -> bool) -> Option<Found> {
    match run_search(root, query, cancelled) {
        Ok(Some(rows)) => Some(Found { rows, error: None }),
        // The walk gave up on a superseded query: no answer, not an empty one.
        Ok(None) => None,
        Err(error) => Some(Found {
            rows: Vec::new(),
            error: Some(error),
        }),
    }
}

/// The files under `root` whose contents match `query`, flattened to rows.
///
/// `Err` carries why the search could not run at all — a root this app cannot
/// read — which is the one thing the view has to say instead of results.
pub fn search(root: &str, query: &str) -> Result<Vec<SearchRow>, String> {
    Ok(run_search(root, query, &|| false)?.unwrap_or_default())
}

/// The search itself. `None` is a walk that gave up because `cancelled` said the
/// query behind it had been superseded.
fn run_search(
    root: &str,
    query: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<Vec<SearchRow>>, String> {
    if root.is_empty() || query.trim().is_empty() {
        return Ok(Some(Vec::new()));
    }
    if !Path::new(root).is_dir() {
        return Err(format!("Global search cannot read {root}."));
    }
    match ripgrep_binary() {
        Some(rg) => search_with_ripgrep(&rg, root, query).map(Some),
        None => search_in_process(root, query, cancelled),
    }
}

fn search_with_ripgrep(rg: &Path, root: &str, query: &str) -> Result<Vec<SearchRow>, String> {
    let pattern = regex_escape(query);
    let output = Command::new(rg)
        .args([
            "--null",
            "--line-number",
            "--no-heading",
            "--color",
            "never",
            "--smart-case",
            "--max-count",
        ])
        .arg(LINES_PER_FILE.to_string())
        .arg("--max-columns")
        .arg(MAX_LINE_BYTES.to_string())
        .arg("-e")
        .arg(pattern)
        .arg("--")
        .arg(root)
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => return Err(format!("Global search could not run ripgrep: {error}")),
    };
    // ripgrep answers 1 for "searched, nothing matched", which is an answer and
    // not a failure.
    if !output.status.success() && output.status.code() != Some(1) {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if message.is_empty() {
            "Global search could not run.".to_string()
        } else {
            message
        });
    }

    // `--null` puts the path and the match on either side of a NUL, so a path
    // with a colon in it (or a match line with one) parses without guessing
    // where the path ends.
    let text = String::from_utf8_lossy(&output.stdout);
    let mut rows: Vec<SearchRow> = Vec::new();
    let mut current: Option<String> = None;
    let mut parts = text.split('\0');
    while let Some(path) = parts.next() {
        let Some(match_line) = parts.next() else {
            break;
        };
        let Some((number, body)) = match_line.trim_end_matches('\n').split_once(':') else {
            continue;
        };
        let Ok(number) = number.parse::<u32>() else {
            continue;
        };
        if current.as_deref() != Some(path) {
            if rows.iter().filter(|row| row.line.is_none()).count() >= FILES {
                break;
            }
            current = Some(path.to_string());
            rows.push(SearchRow {
                path: path.to_string(),
                name: file_name(path),
                line: None,
                text: String::new(),
            });
        }
        rows.push(SearchRow {
            path: path.to_string(),
            name: String::new(),
            line: Some(number),
            text: body.to_string(),
        });
    }
    Ok(rows)
}

/// The same search without ripgrep: the directory walked in-process, every text
/// file read once, the same per-file and total caps applied. `None` is a walk
/// that gave up because the query it was walking for was superseded.
fn search_in_process(
    root: &str,
    query: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<Vec<SearchRow>>, String> {
    let needle = query.trim();
    let case_sensitive = needle.chars().any(|ch| ch.is_uppercase());
    let needle_lower = needle.to_lowercase();
    let matches = |line: &str| {
        if case_sensitive {
            line.contains(needle)
        } else {
            line.to_lowercase().contains(&needle_lower)
        }
    };

    let mut rows: Vec<SearchRow> = Vec::new();
    let walk = walkdir::WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| !is_skipped(entry));
    let mut seen = 0usize;
    for entry in walk.filter_map(|entry| entry.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        if rows.iter().filter(|row| row.line.is_none()).count() >= FILES {
            break;
        }
        seen += 1;
        if seen % CANCEL_CHECK_FILES == 0 && cancelled() {
            return Ok(None);
        }
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else { continue };
        if metadata.len() > MAX_FILE_BYTES {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else { continue };
        // A NUL byte in the head means binary: ripgrep refuses those too, and
        // the band has nothing to show for one.
        if bytes.iter().take(1024).any(|byte| *byte == 0) {
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let path = path.to_string_lossy().to_string();
        // The file's own row is written with its first matched line, so a file
        // that matches nothing contributes nothing.
        let mut written = 0usize;
        let mut header = false;
        for (index, line) in text.lines().enumerate() {
            if written >= LINES_PER_FILE {
                break;
            }
            if !matches(line) || line.len() > MAX_LINE_BYTES {
                continue;
            }
            if !header {
                header = true;
                rows.push(SearchRow {
                    path: path.clone(),
                    name: file_name(&path),
                    line: None,
                    text: String::new(),
                });
            }
            written += 1;
            rows.push(SearchRow {
                path: path.clone(),
                name: String::new(),
                line: Some(index as u32 + 1),
                text: line.to_string(),
            });
        }
    }
    Ok(Some(rows))
}

/// Whether the walk keeps out of this entry: the named build/dependency trees,
/// and the hidden files and directories everywhere under the root (ripgrep
/// skips hidden entries by default, and so does the band's own listing).
fn is_skipped(entry: &walkdir::DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    let name = entry.file_name().to_string_lossy().to_string();
    if entry.file_type().is_dir() && SKIPPED_DIRS.contains(&name.as_str()) {
        return true;
    }
    name.starts_with('.')
}

/// Escape everything a regex would treat as syntax, so the query is a literal
/// string: a search for `fn main(` is a search for that text.
fn regex_escape(pattern: &str) -> String {
    let mut escaped = String::with_capacity(pattern.len());
    for character in pattern.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// The search view: the query field, then what it found — one row per file, one
/// row per matching line under it.
pub(crate) fn build_search(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let xs = app.theme.spacing_px(SpacingToken::Xs);

    let on_search_change = actions.on_global_search_change.clone();
    let on_search_focus = actions.on_global_search_focus_change.clone();
    let field = SearchInput::new()
        .with_value(state.global_search_query.clone())
        .with_focused(state.global_search_focused)
        .with_placeholder("Search in files")
        .with_compact(true)
        .with_icon(false)
        .with_extra_height(super::sidebar::SEARCH_EXTRA_HEIGHT)
        .with_on_change(move |value| (on_search_change.borrow_mut())(value))
        .with_on_focus_change(move |focused| (on_search_focus.borrow_mut())(focused))
        .finish();

    let files = state
        .global_search_rows
        .iter()
        .filter(|row| row.line.is_none())
        .count();
    let matches = state.global_search_rows.len() - files;
    let summary = if state.global_search_query.trim().is_empty() {
        "Search in files across this pane's directory.".to_string()
    } else if let Some(error) = state.global_search_error.clone() {
        error
    } else if !state.global_search_searched {
        "Searching…".to_string()
    } else if state.global_search_rows.is_empty() {
        "No results found.".to_string()
    } else {
        format!(
            "{matches} result{} in {files} file{}",
            if matches == 1 { "" } else { "s" },
            if files == 1 { "" } else { "s" },
        )
    };

    let mut list = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(1.0);
    for row in &state.global_search_rows {
        let mut line = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(4.0);
        match row.line {
            // A file's own row names it, with the icon of what it is.
            None => {
                line = line
                    .with_child(
                        Icon::new(file_icon_name(&row.name))
                            .with_size(14.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_child(
                        Text::new(row.name.clone())
                            .with_theme_color(ColorToken::Text, app)
                            .with_font_size(12.0)
                            .with_max_lines(1)
                            .finish(),
                    );
            }
            // A matched line, indented under it: the line number, then the text.
            Some(number) => {
                line = line
                    .with_child(
                        ConstrainedBox::new(Empty::new().finish())
                            .with_width(14.0)
                            .finish(),
                    )
                    .with_child(
                        Text::new(format!("{number}"))
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(11.0)
                            .finish(),
                    )
                    .with_child(
                        Text::new(row.text.clone())
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(11.0)
                            .with_max_lines(1)
                            .finish(),
                    );
            }
        }
        let path = row.path.clone();
        let on_open = actions.on_explorer_file_click.clone();
        list = list.with_child(
            HoverRow::new(line.finish())
                .with_padding(EdgeInsets::new(0.0, 2.0, 0.0, 2.0))
                .with_on_click(move || (on_open.borrow_mut())(path.clone()))
                .finish(),
        );
    }

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    column = column.with_child(field);
    column = column.with_child(
        Container::new(
            Text::new(summary)
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(2)
                .finish(),
        )
        .with_padding(EdgeInsets::new(xs, 0.0, xs, 0.0))
        .finish(),
    );
    column = column.with_child(
        Expanded::new(
            Scrollable::new(list.finish(), Axis::Vertical)
                .with_state(state.global_search_scroll.clone())
                .finish(),
        )
        .finish(),
    );
    column.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_is_a_literal_string() {
        assert_eq!(regex_escape("fn main("), r"fn main\(");
        assert_eq!(regex_escape("a.b*c"), r"a\.b\*c");
        assert_eq!(regex_escape("plain"), "plain");
    }

    /// Wait for the worker to publish, the way the frame loop does: collect
    /// until an answer appears, or give up.
    fn collect(worker: &SearchWorker) -> Option<SearchOutcome> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(outcome) = worker.take() {
                return Some(outcome);
            }
            if std::time::Instant::now() > deadline {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// Typing is a stream of queries and only the last one is the user's: the
    /// worker walks off the UI thread and answers with the newest query, never
    /// an earlier one it happened to start on.
    #[test]
    fn the_worker_answers_the_newest_query() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("alpha.txt"), "alpha needle\n").unwrap();
        std::fs::write(dir.path().join("beta.txt"), "beta needle\n").unwrap();
        let root = dir.path().to_string_lossy().to_string();

        let worker = SearchWorker::new();
        worker.search(&root, "alpha");
        worker.search(&root, "beta");
        let outcome = collect(&worker).expect("the worker answers");
        let names: Vec<String> = outcome
            .rows
            .iter()
            .filter(|row| row.line.is_none())
            .map(|row| row.name.clone())
            .collect();
        assert_eq!(
            names,
            vec!["beta.txt".to_string()],
            "the answer belongs to the newest query, not the one it started on"
        );
        assert!(outcome.error.is_none());
        assert!(
            worker.take().is_none(),
            "collecting the answer leaves the slot empty"
        );
    }

    /// Emptying the field abandons the walk in flight: its answer describes a
    /// query the field no longer holds, so it is never shown.
    #[test]
    fn a_cancelled_query_is_never_shown() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("alpha.txt"), "alpha needle\n").unwrap();
        let worker = SearchWorker::new();
        worker.search(&dir.path().to_string_lossy(), "alpha");
        worker.cancel();

        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
        while std::time::Instant::now() < deadline {
            assert!(
                worker.take().is_none(),
                "a cancelled query has nothing to show"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn the_search_prefers_a_ripgrep_binary_it_can_find() {
        // The machine this runs on may have none; what must hold either way is
        // that a resolved path really is a file.
        if let Some(rg) = ripgrep_binary() {
            assert!(rg.is_file(), "{rg:?} is a file");
            assert_eq!(rg.file_name().unwrap(), "rg");
        }
    }

    /// A search groups the matched lines under the file they are in, whichever
    /// engine answers: ripgrep when the machine has one, the in-process walk
    /// otherwise.
    #[test]
    fn a_search_groups_matches_under_the_file_they_are_in() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src").join("main.rs"),
            "fn main() {\n    // needle here\n}\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("notes.md"), "no match in here\n").unwrap();

        let rows = search(&dir.path().to_string_lossy(), "needle").expect("a search runs");
        assert_eq!(rows.len(), 2, "a file row and its matched line: {rows:?}");
        assert_eq!(rows[0].line, None, "the file comes first");
        assert!(rows[0].path.ends_with("src/main.rs"), "{:?}", rows[0]);
        assert_eq!(rows[0].name, "main.rs");
        assert_eq!(rows[1].line, Some(2));
        assert_eq!(
            rows[1].text, "    // needle here",
            "the matched line is kept as it is written, indentation included"
        );

        // A query with nothing behind it is an answer, not a failure.
        assert!(search(&dir.path().to_string_lossy(), "absent").expect("runs").is_empty());
        assert!(search(&dir.path().to_string_lossy(), "").expect("runs").is_empty());
    }

    /// The in-process engine is the one this machine actually runs, so it is
    /// tested on its own: the query is a literal string, its case is smart, and
    /// the trees and files a search never wants are left out.
    #[test]
    fn the_in_process_search_is_literal_smart_case_and_skips_the_noise() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(
            dir.path().join("app.rs"),
            "fn main() {\n    let value = Needle::new();\n    // needle here\n}\n",
        )
        .unwrap();
        let walk = |query: &str| {
            search_in_process(&dir.path().to_string_lossy(), query, &|| false)
                .expect("a walk runs")
                .expect("nothing cancelled it")
                .iter()
                .filter(|row| row.line.is_some())
                .count()
        };
        // Rust's regex syntax must not leak in: `main(` matches literally.
        assert_eq!(walk("main("), 1, "the query is a literal string");
        // Lowercase query: both cases match (ripgrep's --smart-case).
        assert_eq!(walk("needle"), 2, "a lowercase query matches either case");
        // An uppercase character makes it case-sensitive.
        let sensitive = search_in_process(&dir.path().to_string_lossy(), "Needle", &|| false)
            .expect("a walk runs")
            .expect("nothing cancelled it");
        let lines: Vec<u32> = sensitive.iter().filter_map(|row| row.line).collect();
        assert_eq!(lines, vec![2], "an uppercase query is case-sensitive");

        // Binaries, the build/dependency trees and hidden entries are skipped.
        let noisy = tempfile::tempdir().expect("temp dir");
        std::fs::write(noisy.path().join("real.txt"), "needle\n").unwrap();
        std::fs::write(noisy.path().join("blob.bin"), b"needle\0\x01\x02").unwrap();
        std::fs::create_dir(noisy.path().join("target")).unwrap();
        std::fs::write(noisy.path().join("target").join("out.txt"), "needle\n").unwrap();
        std::fs::create_dir(noisy.path().join(".git")).unwrap();
        std::fs::write(noisy.path().join(".git").join("config"), "needle\n").unwrap();
        let rows = search_in_process(&noisy.path().to_string_lossy(), "needle", &|| false)
            .expect("a walk runs")
            .expect("nothing cancelled it");
        assert_eq!(rows.len(), 2, "one file, one line: {rows:?}");
        assert!(rows[0].path.ends_with("real.txt"), "{:?}", rows[0]);
    }

    /// A root this app cannot read is the one failure the band reports; a
    /// missing ripgrep is no longer one of them.
    #[test]
    fn an_unreadable_root_is_the_reported_failure() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("not-here");
        let error = search(&missing.to_string_lossy(), "needle").expect_err("a failure");
        assert!(error.contains("cannot read"), "{error:?}");
        assert!(
            !error.contains("ripgrep"),
            "a missing ripgrep is not a search failure: {error:?}"
        );
    }
}
