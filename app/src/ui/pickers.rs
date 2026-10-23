//! The rich input's working-directory and git-branch menus.
//!
//! Both are the warp-new model: the menu is a window onto the real machine, and
//! a selection is a real shell command run in the pane's own pty — the
//! directory pill browses the filesystem (`cd`), the branch pill lists the
//! repository's local branches (`git checkout`). Nothing here mutates app
//! state: it reads the disk and the repository, and hands the app the labels to
//! draw and the ids to act on.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use goble_ui::elements::diff::{parse_unified_diff, DiffLine, DiffLineKind, Hunk, PreparedDiff};
use goble_ui::elements::{DiffFileStat, DiffSummary};

/// How long the diff control reuses a working-tree read. The control is on
/// screen all the time (unlike the picker menus, which are read only while
/// open), so this is the one place the read is throttled instead of gated.
const DIFF_CACHE_TTL: Duration = Duration::from_millis(1500);
/// A repository with a very large untracked tree would make the frame wait on
/// the disk; the control counts what it can and leaves the rest for the next
/// read.
const MAX_UNTRACKED_FILES: usize = 400;
/// The largest untracked file whose lines are counted. A larger file still
/// counts as changed, with zero counted lines.
const MAX_UNTRACKED_BYTES: u64 = 512 * 1024;
/// The bytes one refresh may read from untracked files in total. A tree with
/// hundreds of new files is common right after a branch switch; the read stays
/// bounded rather than walking all of it while a frame waits.
const MAX_UNTRACKED_TOTAL_BYTES: u64 = 2 * 1024 * 1024;
/// The most of a tracked diff a panel reads. A generated file can produce a
/// diff larger than the whole window; the panel keeps what fits in memory and
/// says the tail was cut.
const MAX_DIFF_DETAIL_BYTES: usize = 8 * 1024 * 1024;

/// One row of the working-directory menu: the label drawn, the absolute
/// directory it stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryRow {
    pub label: String,
    pub path: String,
}

/// One row of the branch menu: the branch name and whether it is checked out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchRow {
    pub name: String,
    pub current: bool,
}

/// The label the parent row carries, spelled out the way warp-new spells it.
pub const PARENT_LABEL: &str = ".. (Parent Directory)";

/// The directories directly under `cwd`: hidden entries skipped, alphabetical,
/// with the parent directory as the last row. Unreadable directories yield the
/// parent row alone rather than an error — a menu that cannot list is still a
/// menu that can go up.
pub fn directory_rows(cwd: &str) -> Vec<DirectoryRow> {
    let mut rows: Vec<DirectoryRow> = Vec::new();
    let dir = Path::new(cwd);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let is_dir = entry
                .file_type()
                .map(|kind| kind.is_dir())
                .unwrap_or(false);
            if !is_dir {
                continue;
            }
            let path = entry.path();
            rows.push(DirectoryRow {
                label: name,
                path: path.to_string_lossy().to_string(),
            });
        }
    }
    rows.sort_by(|a, b| {
        a.label
            .to_lowercase()
            .cmp(&b.label.to_lowercase())
            .then_with(|| a.label.cmp(&b.label))
    });
    if let Some(parent) = dir.parent() {
        rows.push(DirectoryRow {
            label: PARENT_LABEL.to_string(),
            path: parent.to_string_lossy().to_string(),
        });
    }
    rows
}

/// The local branches of the repository `cwd` sits in, most recently committed
/// first (warp-new's `git branch --sort=-committerdate`), with the checked-out
/// branch hoisted to the top so the menu opens on the state the pane is in.
/// Empty when `cwd` is not in a repository or git is unavailable.
pub fn branch_rows(cwd: &str) -> Vec<BranchRow> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(cwd)
        .arg("branch")
        .arg("--no-color")
        .arg("--sort=-committerdate")
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let (current, name) = match line.strip_prefix("* ") {
            Some(name) => (true, name),
            None => (false, line.trim_start_matches("  ")),
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        rows.push(BranchRow {
            name: name.to_string(),
            current,
        });
    }
    rows.sort_by_key(|row| !row.current);
    rows
}

/// The changed files in `cwd`'s working tree, tracked and untracked, with the
/// line counts the rich input's diff control draws. `None` outside a git
/// repository; `Some` with no files for a clean one.
///
/// Tracked files are read from `git diff --numstat HEAD`, which folds staged
/// and unstaged changes into one comparison against the last commit. A
/// repository with no commit yet falls back to its two staging areas. Untracked
/// files are not in any diff, so they are listed with `git ls-files` and their
/// lines counted directly.
pub fn diff_summary(cwd: &str) -> Option<DiffSummary> {
    let root = repo_root(cwd)?;
    let mut counts = BTreeMap::new();
    if let Some(output) = numstat(cwd, &["diff", "--numstat", "HEAD", "--"]) {
        collect_numstat(&mut counts, &output);
    } else {
        for args in [
            &["diff", "--numstat", "--"][..],
            &["diff", "--numstat", "--cached", "--"][..],
        ] {
            if let Some(output) = numstat(cwd, args) {
                collect_numstat(&mut counts, &output);
            }
        }
    }
    let mut files: Vec<DiffFileStat> = counts
        .into_iter()
        .map(|(path, (added, removed))| DiffFileStat::new(path, added, removed))
        .collect();
    files.extend(untracked_files(&root));
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Some(DiffSummary::new(files))
}

/// One changed file as the review panel draws it: its path, its line counts
/// and the parsed hunks of its unified diff.
#[derive(Clone, Debug, Default)]
pub struct DiffFileDetail {
    /// The path as git reports it, relative to the repository root.
    pub path: String,
    /// The path on disk, which "Open" hands to a file pane.
    pub absolute_path: String,
    pub added: u32,
    pub removed: u32,
    /// Shared, so the per-frame rebuild clones a pointer rather than every
    /// line of the file's diff.
    pub hunks: Rc<Vec<Hunk>>,
    /// The rows and syntax runs, built once when the diff is read.
    pub prepared: PreparedDiff,
    /// A binary file has no textual hunks; the entry is still listed so the
    /// file count matches the working tree.
    pub binary: bool,
    /// The file was past the read cap, so the panel shows its name and counts
    /// without the diff body.
    pub truncated: bool,
}

/// The working tree's changed files with their full diffs, plus the branch
/// they sit on. What the review pane draws.
#[derive(Clone, Debug, Default)]
pub struct DiffDetail {
    pub branch: String,
    pub root: String,
    pub files: Vec<DiffFileDetail>,
}

impl DiffDetail {
    pub fn files_changed(&self) -> usize {
        self.files.len()
    }

    pub fn added(&self) -> u64 {
        self.files.iter().map(|file| u64::from(file.added)).sum()
    }

    pub fn removed(&self) -> u64 {
        self.files.iter().map(|file| u64::from(file.removed)).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// A working-tree read in the form the background thread can build and send:
/// the same data as [`DiffDetail`], without the shared handles the UI adds.
/// Parsing runs here too, so a periodic refresh costs the frame nothing.
struct GitDiff {
    branch: String,
    root: String,
    files: Vec<GitDiffFile>,
}

#[derive(Default)]
struct GitDiffFile {
    path: String,
    absolute_path: String,
    added: u32,
    removed: u32,
    hunks: Vec<Hunk>,
    binary: bool,
    truncated: bool,
}

/// Read the working tree's changed files together with their unified diffs.
///
/// Tracked changes come from one `git diff HEAD` (or the two staging areas
/// when the repository has no commit yet), split into one section per file.
/// Untracked files are not in any diff, so each one is rendered as a new-file
/// hunk from its own text, under the same size caps the summary uses.
pub fn diff_detail(cwd: &str) -> Option<DiffDetail> {
    diff_detail_with(cwd, None)
}

/// The same read, carrying a file's prepared rows into the fresh read when its
/// hunks did not move — a periodic refresh then costs the cards on screen no
/// re-preparation.
fn diff_detail_with(cwd: &str, previous: Option<&DiffDetail>) -> Option<DiffDetail> {
    Some(assemble_detail(git_diff(cwd)?, previous))
}

fn git_diff(cwd: &str) -> Option<GitDiff> {
    let root = repo_root(cwd)?;
    let mut sections: BTreeMap<String, RawDiffSection> = BTreeMap::new();
    collect_tracked_sections(&root, &mut sections);

    let mut files: Vec<GitDiffFile> = sections
        .into_iter()
        .map(|(path, section)| {
            let hunks = parse_unified_diff(&section.text);
            let added = hunks.iter().map(Hunk::added).sum::<usize>() as u32;
            let removed = hunks.iter().map(Hunk::removed).sum::<usize>() as u32;
            GitDiffFile {
                absolute_path: root.join(&path).to_string_lossy().to_string(),
                path,
                added,
                removed,
                hunks,
                binary: section.binary,
                truncated: false,
            }
        })
        .collect();

    let tracked: std::collections::BTreeSet<String> =
        files.iter().map(|file| file.path.clone()).collect();
    for path in untracked_paths(&root) {
        if !tracked.contains(&path) {
            files.push(untracked_detail(&root, &path));
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));

    Some(GitDiff {
        branch: branch_name(&root),
        root: root.to_string_lossy().to_string(),
        files,
    })
}

/// Wrap a worker's read in the handles the review panel draws. A file whose
/// hunks are byte-for-byte the ones already prepared keeps its coloured runs,
/// so the periodic re-read costs no highlighting for the cards on screen.
fn assemble_detail(read: GitDiff, previous: Option<&DiffDetail>) -> DiffDetail {
    let files = read
        .files
        .into_iter()
        .map(|file| {
            let hunks = Rc::new(file.hunks);
            let prepared = previous
                .and_then(|previous| {
                    previous
                        .files
                        .iter()
                        .find(|old| old.path == file.path && old.hunks == hunks)
                })
                .map(|old| old.prepared.clone())
                .unwrap_or_else(|| PreparedDiff::rows_only(&hunks));
            DiffFileDetail {
                absolute_path: file.absolute_path,
                path: file.path,
                added: file.added,
                removed: file.removed,
                hunks,
                prepared,
                binary: file.binary,
                truncated: file.truncated,
            }
        })
        .collect();
    DiffDetail {
        branch: read.branch,
        root: read.root,
        files,
    }
}

/// One file's section of `git diff` output, before it is parsed.
#[derive(Default)]
struct RawDiffSection {
    text: String,
    binary: bool,
}

/// Read the tracked diff and split it into one section per file. The `HEAD`
/// comparison folds staged and unstaged changes together; without a commit
/// the two staging areas are read one after the other.
fn collect_tracked_sections(root: &Path, sections: &mut BTreeMap<String, RawDiffSection>) {
    let args = [
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--unified=3",
        "HEAD",
        "--",
    ];
    if let Some(output) = git_output(&root.to_string_lossy(), &args) {
        absorb_diff(&output.stdout, sections);
        return;
    }
    for args in [
        &["diff", "--no-color", "--no-ext-diff", "--unified=3", "--"][..],
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--unified=3",
            "--cached",
            "--",
        ][..],
    ] {
        if let Some(output) = git_output(&root.to_string_lossy(), args) {
            absorb_diff(&output.stdout, sections);
        }
    }
}

/// Split `git diff` output at its `diff --git` headers and merge sections that
/// name the same file (the no-`HEAD` path can produce two, one per staging
/// area).
fn absorb_diff(bytes: &[u8], sections: &mut BTreeMap<String, RawDiffSection>) {
    let bytes = &bytes[..bytes.len().min(MAX_DIFF_DETAIL_BYTES)];
    let text = String::from_utf8_lossy(bytes);
    let mut current: Option<(String, RawDiffSection)> = None;
    let finish = |current: &mut Option<(String, RawDiffSection)>,
                  sections: &mut BTreeMap<String, RawDiffSection>| {
        if let Some((path, section)) = current.take() {
            let entry = sections.entry(path).or_default();
            entry.text.push_str(&section.text);
            entry.binary |= section.binary;
        }
    };

    for line in text.lines() {
        if line.starts_with("diff --git ") {
            finish(&mut current, sections);
            current = Some((String::new(), RawDiffSection::default()));
        }
        let Some((path, section)) = current.as_mut() else {
            continue;
        };
        if path.is_empty() {
            if let Some(found) = diff_header_path(line, "+++ ")
                .or_else(|| diff_header_path(line, "--- "))
            {
                *path = found;
            }
        }
        if line.starts_with("Binary files ") || line == "GIT binary patch" {
            section.binary = true;
        }
        section.text.push_str(line);
        section.text.push('\n');
    }
    finish(&mut current, sections);
}

/// The file a `---`/`+++` header names, without git's `a/`/`b/` prefix.
fn diff_header_path(line: &str, prefix: &str) -> Option<String> {
    let value = line.strip_prefix(prefix)?.trim_end();
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value);
    let value = value
        .strip_prefix("b/")
        .or_else(|| value.strip_prefix("a/"))
        .unwrap_or(value);
    (!value.is_empty() && value != "/dev/null").then(|| value.to_string())
}

/// The checked-out branch, or empty for a repository with no commit.
fn branch_name(root: &Path) -> String {
    git_output(&root.to_string_lossy(), &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|branch| !branch.is_empty() && branch != "HEAD")
        .unwrap_or_default()
}

/// An untracked file as a new-file hunk. A binary or oversized file keeps its
/// name and is marked so the panel can say why it has no body.
fn untracked_detail(root: &Path, path: &str) -> GitDiffFile {
    let absolute = root.join(path);
    let mut file = GitDiffFile {
        path: path.to_string(),
        absolute_path: absolute.to_string_lossy().to_string(),
        ..GitDiffFile::default()
    };
    let Ok(metadata) = std::fs::metadata(&absolute) else {
        file.truncated = true;
        return file;
    };
    if !metadata.is_file() || metadata.len() > MAX_UNTRACKED_BYTES {
        file.truncated = true;
        return file;
    }
    let Ok(bytes) = std::fs::read(&absolute) else {
        file.truncated = true;
        return file;
    };
    if bytes.iter().take(8192).any(|byte| *byte == 0) {
        file.binary = true;
        return file;
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return file;
    }
    let count = lines.len() as u32;
    file.added = count;
    file.hunks = vec![Hunk {
        old_start: 0,
        old_count: 0,
        new_start: 1,
        new_count: count,
        section: String::new(),
        lines: lines
            .into_iter()
            .enumerate()
            .map(|(index, text)| DiffLine {
                kind: DiffLineKind::Added,
                old_line: None,
                new_line: Some(index as u32 + 1),
                text: text.to_string(),
            })
            .collect(),
    }];
    file
}

/// The repository root `cwd` sits in, or `None` outside a work tree.
fn repo_root(cwd: &str) -> Option<PathBuf> {
    if cwd.trim().is_empty() {
        return None;
    }
    let output = git_output(cwd, &["rev-parse", "--show-toplevel"])?;
    let text = String::from_utf8_lossy(&output.stdout);
    let root = text.trim();
    (!root.is_empty()).then(|| PathBuf::from(root))
}

/// `git diff --numstat` output, or `None` when git refused (no repository, no
/// `HEAD`, or no git at all).
fn numstat(cwd: &str, args: &[&str]) -> Option<String> {
    let output = git_output(cwd, args)?;
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Run git in `cwd` and return its output only when it succeeded.
fn git_output(cwd: &str, args: &[&str]) -> Option<std::process::Output> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .ok()?;
    output.status.success().then_some(output)
}

/// One row of `--numstat` output: `added\tremoved\tpath`, where a binary file
/// spells both counts as `-`. Renames carry `old => new` in the path, which is
/// what the menu reads.
fn collect_numstat(counts: &mut BTreeMap<String, (u32, u32)>, output: &str) {
    for line in output.lines() {
        let mut parts = line.splitn(3, '\t');
        let Some(added) = parts.next() else {
            continue;
        };
        let Some(removed) = parts.next() else {
            continue;
        };
        let Some(path) = parts.next() else {
            continue;
        };
        let path = path.trim_end_matches('\r').trim();
        if path.is_empty() {
            continue;
        }
        let added = added.parse::<u32>().unwrap_or(0);
        let removed = removed.parse::<u32>().unwrap_or(0);
        let entry = counts.entry(path.to_string()).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(added);
        entry.1 = entry.1.saturating_add(removed);
    }
}

/// The repository's untracked files, each counted as additions. The listing
/// runs at the root so it covers the whole tree, the way the tracked diff from
/// a subdirectory does; paths are root-relative, so reading one joins the root.
fn untracked_files(root: &Path) -> Vec<DiffFileStat> {
    let mut budget = MAX_UNTRACKED_TOTAL_BYTES;
    untracked_paths(root)
        .into_iter()
        .map(|path| {
            let added = count_lines(&root.join(&path), &mut budget);
            DiffFileStat::new(path, added, 0)
        })
        .collect()
}

/// The root-relative paths git reports as untracked, in git's order.
fn untracked_paths(root: &Path) -> Vec<String> {
    let root_arg = root.to_string_lossy();
    let Some(output) = git_output(
        &root_arg,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "--full-name",
            "-z",
        ],
    ) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for raw in output.stdout.split(|byte| *byte == 0) {
        if raw.is_empty() {
            continue;
        }
        if paths.len() >= MAX_UNTRACKED_FILES {
            break;
        }
        paths.push(String::from_utf8_lossy(raw).to_string());
    }
    paths
}

/// Count a file's lines for the untracked side of the summary. Binary files and
/// files past the size caps count as zero: the file is still shown, so the
/// control's file count stays true.
fn count_lines(path: &Path, budget: &mut u64) -> u32 {
    let Ok(metadata) = std::fs::metadata(path) else {
        return 0;
    };
    if !metadata.is_file() || metadata.len() > MAX_UNTRACKED_BYTES || metadata.len() > *budget {
        return 0;
    }
    *budget = budget.saturating_sub(metadata.len());
    let Ok(bytes) = std::fs::read(path) else {
        return 0;
    };
    if bytes.is_empty() || bytes.iter().take(8192).any(|byte| *byte == 0) {
        return 0;
    }
    let mut lines = bytes.iter().filter(|byte| **byte == b'\n').count() as u32;
    if !bytes.ends_with(b"\n") {
        lines = lines.saturating_add(1);
    }
    lines
}

/// The directory `path` lives in, for a shell command that has to run there.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// `cd` into `path`, as the shell would be told to do it: the directory pill
/// runs this in the pane's pty rather than setting a variable, so the shell's
/// own idea of where it is moves with it.
pub fn change_directory_command(path: &str) -> String {
    format!("cd {}", shell_quote(path))
}

/// Check `branch` out, the way warp-new's branch pill does.
pub fn checkout_branch_command(branch: &str) -> String {
    format!("git checkout {}", shell_quote(branch))
}

/// The rows both menus were last built from, kept per rebuild so an open menu
/// is not re-read from the disk on every frame while a closed one costs
/// nothing. Keyed by the directory they describe: a pane that moved lists its
/// new home the moment the menu opens again.
#[derive(Default)]
pub struct PickerCache {
    directories: Option<(String, Vec<DirectoryRow>)>,
    branches: Option<(String, Vec<BranchRow>)>,
    /// The working tree's changed files per directory, with when each was
    /// read. One slot would make two panes on different trees re-read each
    /// other's tree on every frame.
    diff: HashMap<String, (Instant, Option<DiffSummary>)>,
    /// The review pane's full diffs per directory: the pane's refresh
    /// generation and when the read happened. The panel is rebuilt every
    /// frame, so this is throttled like the summary; a generation bump reads
    /// at once instead of waiting for the next stale entry.
    detail: HashMap<String, (u64, Instant, Option<Rc<DiffDetail>>)>,
    /// The thread that refreshes the two reads beside the frame. A periodic
    /// `git diff` costs a window's worth of paint, so it runs here and the
    /// stale answer is served until the fresh one lands.
    reader: Option<DiffReader>,
    /// The directories whose summary is being read, so a stale entry asks the
    /// worker once rather than on every frame.
    summary_pending: HashSet<String>,
    /// The generation of the review read in flight, per directory.
    detail_pending: HashMap<String, u64>,
}

/// The most directories the two diff caches keep. A workspace has a handful of
/// panes and composers; past this the tree read longest ago is dropped.
const MAX_DIFF_DIRECTORIES: usize = 8;

/// The two reads a background thread refreshes.
enum ReadWork {
    Summary(String),
    Detail { cwd: String, generation: u64 },
}

enum ReadAnswer {
    Summary {
        cwd: String,
        summary: Option<DiffSummary>,
    },
    Detail {
        cwd: String,
        generation: u64,
        read: Option<GitDiff>,
    },
}

/// The thread that keeps the working-tree reads out of the frame. It reads in
/// the order asked and sends the answer back; the frame that asked serves its
/// last answer until this one lands.
struct DiffReader {
    jobs: Sender<ReadWork>,
    results: Receiver<ReadAnswer>,
}

impl DiffReader {
    fn spawn() -> Self {
        let (jobs, incoming) = mpsc::channel::<ReadWork>();
        let (answers, results) = mpsc::channel::<ReadAnswer>();
        std::thread::Builder::new()
            .name("goble-diff-reader".to_string())
            .spawn(move || {
                while let Ok(work) = incoming.recv() {
                    let answer = match work {
                        ReadWork::Summary(cwd) => ReadAnswer::Summary {
                            summary: diff_summary(&cwd),
                            cwd,
                        },
                        ReadWork::Detail { cwd, generation } => ReadAnswer::Detail {
                            read: git_diff(&cwd),
                            cwd,
                            generation,
                        },
                    };
                    if answers.send(answer).is_err() {
                        break;
                    }
                }
            })
            .expect("the diff reader thread must start");
        Self { jobs, results }
    }
}

impl PickerCache {
    /// The working-directory rows for `cwd`. Only an open menu pays for the
    /// listing, and the listing is dropped when the menu closes so it is fresh
    /// every time it is opened.
    pub fn directories(&mut self, cwd: &str, open: bool) -> Vec<DirectoryRow> {
        if !open {
            self.directories = None;
            return Vec::new();
        }
        if let Some((cached, rows)) = &self.directories {
            if cached == cwd {
                return rows.clone();
            }
        }
        let rows = directory_rows(cwd);
        self.directories = Some((cwd.to_string(), rows.clone()));
        rows
    }

    /// The branch rows for the repository `cwd` sits in, under the same rules.
    pub fn branches(&mut self, cwd: &str, open: bool) -> Vec<BranchRow> {
        if !open {
            self.branches = None;
            return Vec::new();
        }
        if let Some((cached, rows)) = &self.branches {
            if cached == cwd {
                return rows.clone();
            }
        }
        let rows = branch_rows(cwd);
        self.branches = Some((cwd.to_string(), rows.clone()));
        rows
    }

    /// The working tree's changed files for `cwd`. Unlike the pickers this is
    /// read while the control is visible, so the answer is reused for
    /// [`DIFF_CACHE_TTL`] instead of every frame. The first read is synchronous
    /// so the control opens with its counts; a periodic refresh runs on the
    /// reader thread and this keeps serving the last answer until it lands.
    pub fn diff(&mut self, cwd: &str) -> Option<DiffSummary> {
        self.collect_reads();
        if let Some((at, summary)) = self.diff.get(cwd) {
            let cached = summary.clone();
            if at.elapsed() < DIFF_CACHE_TTL {
                return cached;
            }
            if !self.summary_pending.contains(cwd) {
                self.request_summary(cwd);
            }
            return cached;
        }
        let summary = diff_summary(cwd);
        self.diff
            .insert(cwd.to_string(), (Instant::now(), summary.clone()));
        self.trim_summaries();
        summary
    }

    /// The review pane's diffs for `cwd`, read at most once per
    /// [`DIFF_CACHE_TTL`] or when `generation` changes.
    ///
    /// The first read and a generation bump are synchronous, so opening the
    /// pane shows the tree it asked for. The periodic refresh runs on the
    /// reader thread: the card list keeps drawing what it has, then the fresh
    /// read replaces it, and no frame pays for `git diff`.
    pub fn diff_detail(&mut self, cwd: &str, generation: u64) -> Option<Rc<DiffDetail>> {
        self.collect_reads();
        let cached = self
            .detail
            .get(cwd)
            .map(|(cached_generation, at, detail)| (*cached_generation, *at, detail.clone()));
        if let Some((cached_generation, at, detail)) = cached {
            if cached_generation == generation && at.elapsed() < DIFF_CACHE_TTL {
                return detail;
            }
            if cached_generation == generation {
                if self.detail_pending.get(cwd) != Some(&generation) {
                    self.request_detail(cwd, generation);
                }
                return detail;
            }
            // An explicit refresh, or a new generation: read it now, and drop
            // any periodic answer still in flight.
            self.detail_pending.remove(cwd);
            let fresh = diff_detail_with(cwd, detail.as_deref()).map(Rc::new);
            self.detail
                .insert(cwd.to_string(), (generation, Instant::now(), fresh.clone()));
            self.trim_details();
            return fresh;
        }
        let detail = diff_detail_with(cwd, None).map(Rc::new);
        self.detail.insert(
            cwd.to_string(),
            (generation, Instant::now(), detail.clone()),
        );
        self.trim_details();
        detail
    }

    /// Take every finished background read and put it where the frame looks.
    fn collect_reads(&mut self) {
        let mut landed = Vec::new();
        if let Some(reader) = &self.reader {
            while let Ok(answer) = reader.results.try_recv() {
                landed.push(answer);
            }
        }
        for answer in landed {
            match answer {
                ReadAnswer::Summary { cwd, summary } => {
                    self.summary_pending.remove(&cwd);
                    self.diff.insert(cwd, (Instant::now(), summary));
                    self.trim_summaries();
                }
                ReadAnswer::Detail {
                    cwd,
                    generation,
                    read,
                } => {
                    // An answer for a generation the pane moved past is stale.
                    if self.detail_pending.get(&cwd) != Some(&generation) {
                        continue;
                    }
                    self.detail_pending.remove(&cwd);
                    let previous = self
                        .detail
                        .get(&cwd)
                        .and_then(|(_, _, detail)| detail.clone());
                    let detail =
                        read.map(|read| Rc::new(assemble_detail(read, previous.as_deref())));
                    self.detail
                        .insert(cwd, (generation, Instant::now(), detail));
                    self.trim_details();
                }
            }
        }
    }

    fn request_summary(&mut self, cwd: &str) {
        self.summary_pending.insert(cwd.to_string());
        let reader = self.reader.get_or_insert_with(DiffReader::spawn);
        if reader
            .jobs
            .send(ReadWork::Summary(cwd.to_string()))
            .is_err()
        {
            self.reader = None;
            self.summary_pending.remove(cwd);
        }
    }

    fn request_detail(&mut self, cwd: &str, generation: u64) {
        self.detail_pending.insert(cwd.to_string(), generation);
        let reader = self.reader.get_or_insert_with(DiffReader::spawn);
        let work = ReadWork::Detail {
            cwd: cwd.to_string(),
            generation,
        };
        if reader.jobs.send(work).is_err() {
            self.reader = None;
            self.detail_pending.remove(cwd);
        }
    }

    fn trim_summaries(&mut self) {
        while self.diff.len() > MAX_DIFF_DIRECTORIES {
            let Some(oldest) = self
                .diff
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(path, _)| path.clone())
            else {
                break;
            };
            self.diff.remove(&oldest);
        }
    }

    fn trim_details(&mut self) {
        while self.detail.len() > MAX_DIFF_DIRECTORIES {
            let Some(oldest) = self
                .detail
                .iter()
                .min_by_key(|(_, (_, at, _))| *at)
                .map(|(path, _)| path.clone())
            else {
                break;
            };
            self.detail.remove(&oldest);
        }
    }
}

/// The absolute path a directory row points at, resolved against `cwd`.
pub fn resolve(cwd: &str, target: &str) -> String {
    if target.is_empty() {
        return cwd.to_string();
    }
    let candidate = if Path::new(target).is_absolute() {
        PathBuf::from(target)
    } else {
        Path::new(cwd).join(target)
    };
    candidate.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_menu_lists_its_subdirectories_and_the_parent_last() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("zeta")).unwrap();
        std::fs::create_dir(dir.path().join("Alpha")).unwrap();
        std::fs::create_dir(dir.path().join(".hidden")).unwrap();
        std::fs::write(dir.path().join("file.txt"), "not a directory").unwrap();

        let rows = directory_rows(&dir.path().to_string_lossy());
        let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["Alpha", "zeta", PARENT_LABEL],
            "directories only, case-insensitively sorted, parent last"
        );
        assert!(rows[0].path.ends_with("/Alpha"), "rows carry absolute paths");
    }

    #[test]
    fn a_directory_that_cannot_be_listed_still_offers_the_way_up() {
        let rows = directory_rows("/definitely/not/a/real/directory");
        assert_eq!(rows.len(), 1, "one row: {rows:?}");
        assert_eq!(rows[0].label, PARENT_LABEL);
    }

    #[test]
    fn the_parent_row_points_at_the_directory_above() {
        let rows = directory_rows("/Users/example/project");
        let parent = rows.last().expect("a parent row");
        assert_eq!(parent.path, "/Users/example");
        assert_eq!(resolve("/Users/example/project", &parent.path), "/Users/example");
    }

    #[test]
    fn the_selection_commands_are_shell_quoted() {
        assert_eq!(
            change_directory_command("/Users/me/My Project"),
            "cd '/Users/me/My Project'"
        );
        assert_eq!(checkout_branch_command("feature/x"), "git checkout 'feature/x'");
        assert_eq!(
            checkout_branch_command("it's"),
            r"git checkout 'it'\''s'",
            "a quote in a name cannot end the command"
        );
    }

    #[test]
    fn a_closed_menu_lists_nothing_and_is_not_cached() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("child")).unwrap();
        let cwd = dir.path().to_string_lossy().to_string();

        let mut cache = PickerCache::default();
        assert!(cache.directories(&cwd, false).is_empty(), "closed: no rows");

        let open = cache.directories(&cwd, true);
        assert!(open.iter().any(|row| row.label == "child"));

        // A directory created after the listing shows up the next time the menu
        // opens, because a closed menu drops the cache.
        std::fs::create_dir(dir.path().join("later")).unwrap();
        assert!(cache.directories(&cwd, false).is_empty());
        let reopened = cache.directories(&cwd, true);
        assert!(
            reopened.iter().any(|row| row.label == "later"),
            "the reopened menu is re-read: {reopened:?}"
        );
    }

    #[test]
    fn a_directory_outside_a_repository_has_no_branches() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(branch_rows(&dir.path().to_string_lossy()).is_empty());
    }

    #[test]
    fn numstat_rows_keep_paths_and_treat_binary_counts_as_zero() {
        let mut counts = BTreeMap::new();
        collect_numstat(
            &mut counts,
            "12\t3\tsrc/lib.rs\n-\t-\tassets/logo.png\n4\t1\told.rs => new.rs\n",
        );
        assert_eq!(counts.get("src/lib.rs"), Some(&(12, 3)));
        assert_eq!(counts.get("assets/logo.png"), Some(&(0, 0)));
        assert_eq!(counts.get("old.rs => new.rs"), Some(&(4, 1)));
    }

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
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
        std::fs::write(dir.path().join("tracked.txt"), "one\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "initial"]);
        dir
    }

    #[test]
    fn the_diff_summary_counts_tracked_and_untracked_files() {
        if !git_available() {
            return;
        }
        let repo = repository();
        let cwd = repo.path().to_string_lossy().to_string();
        assert!(
            diff_summary(&cwd).expect("a repository").is_empty(),
            "a fresh commit is clean"
        );

        std::fs::write(repo.path().join("tracked.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(repo.path().join("new.txt"), "a\nb").unwrap();

        let summary = diff_summary(&cwd).expect("a repository");
        let mut rows: Vec<(String, u32, u32)> = summary
            .files
            .iter()
            .map(|file| (file.path.clone(), file.added, file.removed))
            .collect();
        rows.sort();
        assert_eq!(
            rows,
            vec![
                ("new.txt".to_string(), 2, 0),
                ("tracked.txt".to_string(), 2, 0),
            ],
            "untracked lines are additions and tracked changes compare against HEAD"
        );
        assert_eq!(summary.files_changed(), 2);
        assert_eq!(summary.added(), 4);
        assert_eq!(summary.removed(), 0);
    }

    #[test]
    fn a_directory_outside_a_repository_has_no_diff() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(diff_summary(&dir.path().to_string_lossy()).is_none());
    }

    #[test]
    fn the_diff_detail_parses_tracked_changes_and_new_files() {
        if !git_available() {
            return;
        }
        let repo = repository();
        let cwd = repo.path().to_string_lossy().to_string();
        std::fs::write(repo.path().join("tracked.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(repo.path().join("new.txt"), "a\nb\n").unwrap();

        let detail = diff_detail(&cwd).expect("a repository");
        assert!(!detail.branch.is_empty(), "the pane names the branch");
        assert_eq!(detail.files_changed(), 2);
        assert_eq!(detail.added(), 4);
        assert_eq!(detail.removed(), 0);

        let paths: Vec<&str> = detail.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, vec!["new.txt", "tracked.txt"], "sorted by path");

        let tracked = &detail.files[1];
        assert_eq!((tracked.added, tracked.removed), (2, 0));
        assert!(!tracked.binary && !tracked.truncated);
        assert!(
            !tracked.hunks.is_empty(),
            "the change is parsed into hunks: {tracked:?}"
        );
        assert!(
            Path::new(&tracked.absolute_path).exists(),
            "Open hands the file pane a path on disk: {}",
            tracked.absolute_path
        );
        assert!(tracked.absolute_path.ends_with("tracked.txt"));

        let untracked = &detail.files[0];
        assert_eq!((untracked.added, untracked.removed), (2, 0));
        let hunk = untracked.hunks.first().expect("a synthesized new-file hunk");
        assert_eq!((hunk.old_start, hunk.new_start), (0, 1));
        assert!(
            hunk.lines
                .iter()
                .all(|line| line.kind == DiffLineKind::Added),
            "every line of a new file is an addition"
        );
    }

    #[test]
    fn a_binary_and_a_deleted_file_are_listed_without_textual_lines() {
        if !git_available() {
            return;
        }
        let repo = repository();
        let cwd = repo.path().to_string_lossy().to_string();
        std::fs::write(repo.path().join("image.bin"), [0u8, 1, 2, 3]).unwrap();
        std::fs::remove_file(repo.path().join("tracked.txt")).unwrap();

        let detail = diff_detail(&cwd).expect("a repository");
        let binary = detail
            .files
            .iter()
            .find(|file| file.path == "image.bin")
            .expect("the binary file is listed");
        assert!(binary.binary, "a NUL byte marks the file binary");
        assert!(binary.hunks.is_empty());

        let deleted = detail
            .files
            .iter()
            .find(|file| file.path == "tracked.txt")
            .expect("a deleted file keeps its pre-image path");
        assert_eq!((deleted.added, deleted.removed), (0, 1));
        assert!(!deleted.hunks.is_empty());
    }

    #[test]
    fn the_diff_detail_reads_again_only_when_the_generation_changes() {
        if !git_available() {
            return;
        }
        let repo = repository();
        let cwd = repo.path().to_string_lossy().to_string();

        let mut cache = PickerCache::default();
        assert!(
            cache.diff_detail(&cwd, 0).expect("repository").is_empty(),
            "a fresh commit is clean"
        );
        std::fs::write(repo.path().join("new.txt"), "line\n").unwrap();
        assert!(
            cache.diff_detail(&cwd, 0).expect("repository").is_empty(),
            "the same generation serves the cached read"
        );
        assert_eq!(
            cache
                .diff_detail(&cwd, 1)
                .expect("repository")
                .files_changed(),
            1,
            "a refresh reads the tree again"
        );
    }

    #[test]
    fn the_diff_header_path_strips_prefixes_and_ignores_dev_null() {
        assert_eq!(
            diff_header_path("+++ b/src/lib.rs", "+++ "),
            Some("src/lib.rs".to_string())
        );
        assert_eq!(
            diff_header_path("--- \"a/space name.rs\"", "--- "),
            Some("space name.rs".to_string())
        );
        assert_eq!(diff_header_path("+++ /dev/null", "+++ "), None);
        assert_eq!(diff_header_path("--- /dev/null", "--- "), None);
    }

    /// A periodic refresh runs on the reader thread: the stale answer is served
    /// first, so the frame never waits on `git diff`, and the fresh one lands a
    /// frame or two later.
    #[test]
    fn a_periodic_diff_refresh_is_read_beside_the_frame() {
        if !git_available() {
            return;
        }
        let repo = repository();
        let cwd = repo.path().to_string_lossy().to_string();
        let mut cache = PickerCache::default();
        assert!(cache.diff_detail(&cwd, 0).expect("repository").is_empty());

        std::fs::write(repo.path().join("new.txt"), "line\n").unwrap();
        // Age the entry past the TTL instead of sleeping the suite.
        if let Some((_, at, _)) = cache.detail.get_mut(&cwd) {
            *at = Instant::now() - DIFF_CACHE_TTL - Duration::from_millis(1);
        }
        assert!(
            cache.diff_detail(&cwd, 0).expect("repository").is_empty(),
            "the stale read is served while the worker refreshes"
        );

        let start = Instant::now();
        loop {
            let detail = cache.diff_detail(&cwd, 0).expect("repository");
            if detail.files_changed() == 1 {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "the worker's read lands"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Two review panes on different trees each keep their own read: one slot
    /// would make every frame re-read the other pane's tree.
    #[test]
    fn the_diff_detail_cache_keeps_each_directory() {
        if !git_available() {
            return;
        }
        let first = repository();
        let second = repository();
        let first_cwd = first.path().to_string_lossy().to_string();
        let second_cwd = second.path().to_string_lossy().to_string();
        std::fs::write(second.path().join("now.txt"), "line\n").unwrap();

        let mut cache = PickerCache::default();
        let first_read = cache.diff_detail(&first_cwd, 0).expect("repository");
        let second_read = cache.diff_detail(&second_cwd, 0).expect("repository");
        assert_eq!(second_read.files_changed(), 1);
        let first_again = cache.diff_detail(&first_cwd, 0).expect("repository");
        assert!(
            Rc::ptr_eq(&first_read, &first_again),
            "the first tree's read survives the second's"
        );
        let second_again = cache.diff_detail(&second_cwd, 0).expect("repository");
        assert!(
            Rc::ptr_eq(&second_read, &second_again),
            "and each pane keeps its own"
        );
    }

    #[test]
    fn the_diff_cache_is_keyed_by_the_directory() {
        if !git_available() {
            return;
        }
        let first = repository();
        let second = repository();
        let first_cwd = first.path().to_string_lossy().to_string();
        let second_cwd = second.path().to_string_lossy().to_string();
        std::fs::write(second.path().join("now.txt"), "line\n").unwrap();

        let mut cache = PickerCache::default();
        assert!(cache.diff(&first_cwd).expect("repository").is_empty());
        assert_eq!(
            cache.diff(&second_cwd).expect("repository").files_changed(),
            1,
            "a pane that moves to another directory reads it at once"
        );
        assert!(
            cache.diff(&first_cwd).expect("repository").is_empty(),
            "and coming back reads the first directory again"
        );
    }
}
