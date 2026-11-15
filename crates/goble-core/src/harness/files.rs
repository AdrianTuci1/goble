use std::path::PathBuf;

use anyhow::{Context as _, Result};

use super::{unified_diff, CommandRunner, ToolOutcome};

pub(super) fn read_file(
    args: &serde_json::Value,
    workspace_dir: &std::path::Path,
) -> Result<String> {
    let path = args["path"].as_str().context("path is required")?;
    let path = resolve_read_path(path, workspace_dir)?;
    std::fs::read_to_string(&path).with_context(|| format!("failed to read {path:?}"))
}

/// The file a read opens: an absolute path is the one the caller named, so a
/// repository the user pointed at outside the workspace can be read the way a
/// command already reads it. Writes keep the stricter [`resolve_path`]: the
/// workspace guard there is what stops a stray write from landing anywhere on
/// the machine.
fn resolve_read_path(path: &str, workspace_dir: &std::path::Path) -> Result<PathBuf> {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        return Ok(canonicalize_loose(&p));
    }
    resolve_path(path, workspace_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A read reaches a file the workspace does not contain when the caller
    /// names it by absolute path: the agent used to refuse it and fall back to
    /// `cat`, which reads the same file one tool later.
    #[test]
    fn an_absolute_read_reaches_a_file_outside_the_workspace() {
        let workspace = std::env::temp_dir().join("goble-read-workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let outside = std::env::temp_dir().join("goble-read-outside.txt");
        std::fs::write(&outside, "outside the workspace").unwrap();

        let text = read_file(
            &serde_json::json!({ "path": outside.to_string_lossy() }),
            &workspace,
        )
        .expect("an absolute read outside the workspace");

        assert_eq!(text, "outside the workspace");
        std::fs::remove_file(&outside).ok();
    }
}

pub(super) fn write_file(
    args: &serde_json::Value,
    workspace_dir: &std::path::Path,
) -> Result<ToolOutcome> {
    let path = args["path"].as_str().context("path is required")?;
    let content = args["content"].as_str().context("content is required")?;
    let path = resolve_path(path, workspace_dir)?;
    // A write is the one place both versions exist: the file's previous text
    // and the new one. The diff is taken here so the row can draw what the
    // write added after a restart. A file that exists but cannot be read as
    // text (a binary) is no pre-image, so no diff is claimed for it.
    let previous = match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(_) if path.exists() => None,
        Err(_) => Some(String::new()),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content).with_context(|| format!("failed to write {path:?}"))?;
    let diff = previous.map(|previous| unified_diff(&previous, content));
    Ok(ToolOutcome::with_diff(format!("wrote {path:?}"), diff))
}

pub(super) fn edit_file(
    args: &serde_json::Value,
    workspace_dir: &std::path::Path,
) -> Result<ToolOutcome> {
    let path = args["path"].as_str().context("path is required")?;
    let old_text = args["old_text"].as_str().context("old_text is required")?;
    let new_text = args["new_text"].as_str().context("new_text is required")?;
    let path = resolve_path(path, workspace_dir)?;
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("failed to read {path:?}"))?;
    let replaced = content.replacen(old_text, new_text, 1);
    if replaced == content {
        anyhow::bail!("old_text not found in file");
    }
    std::fs::write(&path, replaced.as_str())
        .with_context(|| format!("failed to write {path:?}"))?;
    // Diffed against the whole file, so the hunk carries its real line
    // numbers and the context around the replacement.
    let diff = unified_diff(&content, &replaced);
    Ok(ToolOutcome::with_diff(
        format!("edited {path:?}"),
        Some(diff),
    ))
}

pub(super) fn delete_file(
    args: &serde_json::Value,
    workspace_dir: &std::path::Path,
) -> Result<ToolOutcome> {
    let path = args["path"].as_str().context("path is required")?;
    let path = resolve_path(path, workspace_dir)?;
    // The removed text is the diff that is left: the file is gone, so nothing
    // else can reconstruct what the deletion took away.
    let removed = std::fs::read_to_string(&path).ok();
    std::fs::remove_file(&path).with_context(|| format!("failed to delete {path:?}"))?;
    let diff = removed.map(|removed| unified_diff(&removed, ""));
    Ok(ToolOutcome::with_diff(format!("deleted {path:?}"), diff))
}

pub(super) fn rename_file(
    args: &serde_json::Value,
    workspace_dir: &std::path::Path,
) -> Result<String> {
    let from = args["from"].as_str().context("from is required")?;
    let to = args["to"].as_str().context("to is required")?;
    let from = resolve_path(from, workspace_dir)?;
    let to = resolve_path(to, workspace_dir)?;
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&from, &to).with_context(|| format!("failed to rename {from:?} to {to:?}"))?;
    Ok(format!("renamed {from:?} to {to:?}"))
}
pub(super) async fn git_status(
    runner: &dyn CommandRunner,
    _args: &serde_json::Value,
) -> Result<String> {
    runner
        .run("git", &["status".to_string(), "--short".to_string()])
        .await
}

pub(super) async fn git_diff(
    runner: &dyn CommandRunner,
    args: &serde_json::Value,
) -> Result<String> {
    let path = args["path"].as_str().unwrap_or_default();
    let mut cmd_args = vec!["diff".to_string()];
    if !path.is_empty() {
        cmd_args.push(path.to_string());
    }
    runner.run("git", &cmd_args).await
}

pub(super) async fn git_commit(
    runner: &dyn CommandRunner,
    args: &serde_json::Value,
) -> Result<String> {
    let message = args["message"].as_str().context("message is required")?;
    let files: Vec<String> = args["files"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if !files.is_empty() {
        let mut add_args = vec!["add".to_string()];
        add_args.extend(files);
        runner.run("git", &add_args).await?;
    } else {
        runner
            .run("git", &["add".to_string(), "-A".to_string()])
            .await?;
    }
    runner
        .run(
            "git",
            &["commit".to_string(), "-m".to_string(), message.to_string()],
        )
        .await
}

/// Matches a codebase search keeps. The result is read in a pane, so a search
/// answers with the first few hits rather than the whole tree's.
const MAX_MATCHES: usize = 50;

/// How many characters of a matched line the result carries. A minified line is
/// one line of tens of kilobytes, and the transcript draws what the tool
/// returns on every frame.
const MAX_MATCH_CHARS: usize = 240;

/// How many bytes of matches the result carries. Past this the walk stops and
/// the result says it was cut, so a common pattern's answer stays what a pane
/// can draw rather than what the tree holds.
const MAX_RESULT_BYTES: usize = 8 * 1024;

/// The largest file a search reads. A source file is far under this; a bundle, a
/// database or a build product is far over it and has nothing to show.
const MAX_FILE_BYTES: u64 = 1 << 20;

/// Directories a search never descends into: build output and dependencies. A
/// Rust workspace's `target/` alone runs to hundreds of gigabytes and hundreds
/// of thousands of files, and reading it per search is what made a scan take
/// longer than the turn that asked for it.
const SKIPPED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "target",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".cache",
    ".gradle",
    ".terraform",
    ".idea",
    ".vscode",
    "coverage",
];

pub(super) fn codebase_search(
    args: &serde_json::Value,
    workspace_dir: &std::path::Path,
) -> Result<String> {
    let pattern = args["pattern"].as_str().context("pattern is required")?;
    let search_path = args["path"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let search_path = if search_path.is_absolute() {
        search_path
    } else {
        workspace_dir.join(search_path)
    };
    let canonical_base = workspace_dir
        .canonicalize()
        .unwrap_or_else(|_| workspace_dir.to_path_buf());
    let canonical_search = search_path
        .canonicalize()
        .unwrap_or_else(|_| search_path.clone());
    if !canonical_search.starts_with(&canonical_base) {
        anyhow::bail!("search path escapes workspace directory");
    }
    let regex = regex_lite::Regex::new(pattern).context("invalid regex")?;
    let mut matches: Vec<String> = Vec::new();
    let mut bytes = 0usize;
    let mut cut = false;
    for entry in walkdir::WalkDir::new(&canonical_search)
        .into_iter()
        .filter_entry(keeps_scanning)
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(content) = read_text_file(entry.path()) else {
            continue;
        };
        for (i, line) in content.lines().enumerate() {
            if !regex.is_match(line) {
                continue;
            }
            let row = format!("{}:{}: {}", entry.path().display(), i + 1, cap_line(line));
            // The cap is checked before the row is kept, so the result never
            // runs past what a pane draws.
            if matches.len() == MAX_MATCHES || bytes + row.len() + 1 > MAX_RESULT_BYTES {
                cut = true;
                break;
            }
            bytes += row.len() + 1;
            matches.push(row);
        }
        if cut {
            break;
        }
    }
    let mut result = format!("{} matches\n{}", matches.len(), matches.join("\n"));
    if cut {
        result.push_str("\n… more matches not shown");
    }
    Ok(result)
}

/// Whether the walk descends into `entry`: the named build and dependency trees
/// are skipped, but a search pointed at one of them reads it as asked.
fn keeps_scanning(entry: &walkdir::DirEntry) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_dir() {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    !SKIPPED_DIRS.contains(&name.as_ref())
}

/// `path`'s text when it is a file a search reads: a product larger than
/// [`MAX_FILE_BYTES`] or one with a NUL byte in its head is no source, and is
/// skipped before it is read whole.
fn read_text_file(path: &std::path::Path) -> Option<String> {
    use std::io::Read as _;

    if std::fs::metadata(path).ok()?.len() > MAX_FILE_BYTES {
        return None;
    }
    let mut head = [0u8; 1024];
    let mut file = std::fs::File::open(path).ok()?;
    let read = file.read(&mut head).ok()?;
    if head[..read].contains(&0) {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// A matched line as the result carries it: trimmed, and cut at
/// [`MAX_MATCH_CHARS`] with `…` where it was cut.
fn cap_line(line: &str) -> String {
    let line = line.trim();
    match line.char_indices().nth(MAX_MATCH_CHARS) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_string(),
    }
}

pub(super) fn resolve_path(path: &str, workspace_dir: &std::path::Path) -> Result<PathBuf> {
    let p = PathBuf::from(path);
    let resolved = if p.is_absolute() {
        p
    } else {
        workspace_dir.join(p)
    };
    let canonical_base = workspace_dir
        .canonicalize()
        .unwrap_or_else(|_| workspace_dir.to_path_buf());
    let canonical_resolved = canonicalize_loose(&resolved);
    if !canonical_resolved.starts_with(&canonical_base) {
        anyhow::bail!("path {path:?} escapes workspace directory {canonical_base:?}");
    }
    Ok(canonical_resolved)
}

/// Canonicalize the deepest existing ancestor of `path` and re-append the
/// not-yet-existing tail. Without this, a target that is about to be created
/// (e.g. a `write_file` destination) would keep a non-canonical prefix, so a
/// symlinked workspace root (on macOS `/var` -> `/private/var`) would fail the
/// `starts_with` containment check and falsely reject a path inside the
/// workspace.
fn canonicalize_loose(path: &std::path::Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    while !existing.exists() {
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_os_string());
                existing = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
    let mut canon = existing
        .canonicalize()
        .unwrap_or_else(|_| existing.to_path_buf());
    for name in tail.iter().rev() {
        canon.push(name);
    }
    canon
}
