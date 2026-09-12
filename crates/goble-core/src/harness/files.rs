use std::path::PathBuf;

use anyhow::{Context as _, Result};

use super::CommandRunner;

pub(super) fn read_file(
    args: &serde_json::Value,
    workspace_dir: &std::path::Path,
) -> Result<String> {
    let path = args["path"].as_str().context("path is required")?;
    let path = resolve_path(path, workspace_dir)?;
    std::fs::read_to_string(&path).with_context(|| format!("failed to read {path:?}"))
}

pub(super) fn write_file(
    args: &serde_json::Value,
    workspace_dir: &std::path::Path,
) -> Result<String> {
    let path = args["path"].as_str().context("path is required")?;
    let content = args["content"].as_str().context("content is required")?;
    let path = resolve_path(path, workspace_dir)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content).with_context(|| format!("failed to write {path:?}"))?;
    Ok(format!("wrote {path:?}"))
}

pub(super) fn edit_file(
    args: &serde_json::Value,
    workspace_dir: &std::path::Path,
) -> Result<String> {
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
    std::fs::write(&path, replaced).with_context(|| format!("failed to write {path:?}"))?;
    Ok(format!("edited {path:?}"))
}

pub(super) fn delete_file(
    args: &serde_json::Value,
    workspace_dir: &std::path::Path,
) -> Result<String> {
    let path = args["path"].as_str().context("path is required")?;
    let path = resolve_path(path, workspace_dir)?;
    std::fs::remove_file(&path).with_context(|| format!("failed to delete {path:?}"))?;
    Ok(format!("deleted {path:?}"))
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
    let mut matches = Vec::new();
    for entry in walkdir::WalkDir::new(&canonical_search)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            for (i, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    matches.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                    if matches.len() >= 50 {
                        break;
                    }
                }
            }
        }
        if matches.len() >= 50 {
            break;
        }
    }
    Ok(format!("{} matches\n{}", matches.len(), matches.join("\n")))
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
