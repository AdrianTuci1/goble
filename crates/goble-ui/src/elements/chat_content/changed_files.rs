//! The changed-files summary a completed turn's tool calls state.
//!
//! A turn has no separate change ledger on this side of the harness seam. What
//! it does carry is each mutation's recorded unified diff (`ToolCall::diff`), so
//! the card counts exactly the files those diffs name and exactly the lines they
//! add and remove. A call with no path or no diff contributes nothing: the card
//! does not invent a file or a zero.

use std::collections::BTreeMap;

use super::{ChatMessage, ChatRole, ToolCall};

/// One file the turn changed, with the counts its recorded diff states.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub added: u64,
    pub removed: u64,
}

/// The turn's changed files, deduplicated by path (a file edited twice keeps the
/// last recorded counts, not the sum of both diffs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangedFiles {
    pub files: Vec<ChangedFile>,
}

impl ChangedFiles {
    pub fn files_changed(&self) -> usize {
        self.files.len()
    }

    pub fn added(&self) -> u64 {
        self.files.iter().map(|file| file.added).sum()
    }

    pub fn removed(&self) -> u64 {
        self.files.iter().map(|file| file.removed).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// The files the last assistant turn changed, read from the diffs its tool calls
/// recorded. `None` when the turn mutated nothing (no card at all).
pub fn changed_files_for_message(message: &ChatMessage) -> Option<ChangedFiles> {
    if message.role != ChatRole::Assistant {
        return None;
    }
    let mut by_path: BTreeMap<String, ChangedFile> = BTreeMap::new();
    for call in &message.tool_calls {
        let Some(diff) = call.diff.as_deref() else {
            continue;
        };
        let Some(path) = diff_path(diff).or_else(|| argument_path(call)) else {
            continue;
        };
        let (mut added, mut removed) = (0u64, 0u64);
        for hunk in crate::elements::parse_unified_diff(diff) {
            added += hunk.added() as u64;
            removed += hunk.removed() as u64;
        }
        if added == 0 && removed == 0 {
            continue;
        }
        by_path.insert(
            path.clone(),
            ChangedFile {
                path,
                added,
                removed,
            },
        );
    }
    if by_path.is_empty() {
        return None;
    }
    Some(ChangedFiles {
        files: by_path.into_values().collect(),
    })
}

/// The changed path a unified diff's own headers name. `+++ b/path` is the
/// target; a deletion's `/dev/null` target falls back to its `--- a/path`.
fn diff_path(diff: &str) -> Option<String> {
    let mut old = None;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ ") {
            let path = path.trim();
            if let Some(path) = path.strip_prefix("b/") {
                return Some(path.to_string());
            }
            if !path.is_empty() && path != "/dev/null" {
                return Some(path.to_string());
            }
        }
        if let Some(path) = line.strip_prefix("--- ") {
            let path = path.trim();
            if let Some(path) = path.strip_prefix("a/") {
                old = Some(path.to_string());
            }
        }
    }
    old
}

/// The path a call's arguments name, for a diff without file headers.
fn argument_path(call: &ToolCall) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(&call.arguments).ok()?;
    for key in [
        "path",
        "file_path",
        "filePath",
        "target_file",
        "notebook_path",
    ] {
        if let Some(path) = value.get(key).and_then(|value| value.as_str()) {
            if !path.trim().is_empty() {
                return Some(path.trim().to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::chat_content::{ChatFragment, ChatRole};
    use goble_core::harness::ToolCallStatus;

    fn call(name: &str, args: &str, diff: Option<&str>) -> ToolCall {
        ToolCall {
            id: name.to_string(),
            name: name.to_string(),
            arguments: args.to_string(),
            status: ToolCallStatus::Finished,
            result: None,
            diff: diff.map(str::to_string),
        }
    }

    fn message(calls: Vec<ToolCall>) -> ChatMessage {
        ChatMessage::new(ChatRole::Assistant, vec![ChatFragment::text("done")])
            .with_tool_calls(calls)
    }

    const READ_DIFF: &str =
        "--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,2 +1,4 @@\n one\n-two\n+two\n+three\n+four\n";

    #[test]
    fn a_turn_counts_the_files_and_lines_its_recorded_diffs_name() {
        let summary = changed_files_for_message(&message(vec![
            call("edit_file", r#"{"path":"src/a.rs"}"#, Some(READ_DIFF)),
            call(
                "write_file",
                r#"{"path":"src/b.rs"}"#,
                Some("--- a/src/b.rs\n+++ b/src/b.rs\n@@ -1 +1 @@\n-old\n+new\n"),
            ),
        ]))
        .expect("two changed files");
        assert_eq!(summary.files_changed(), 2);
        assert_eq!(summary.added(), 4);
        assert_eq!(summary.removed(), 2);
        assert_eq!(summary.files[0].path, "src/a.rs");
        assert_eq!(summary.files[1].path, "src/b.rs");
    }

    #[test]
    fn a_file_edited_twice_keeps_one_row_with_the_last_counts() {
        let summary = changed_files_for_message(&message(vec![
            call("edit_file", r#"{"path":"src/a.rs"}"#, Some(READ_DIFF)),
            call(
                "edit_file",
                r#"{"path":"src/a.rs"}"#,
                Some("--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@\n-old\n+new\n"),
            ),
        ]))
        .expect("one file");
        assert_eq!(summary.files_changed(), 1);
        assert_eq!((summary.added(), summary.removed()), (1, 1));
    }

    #[test]
    fn a_deletion_takes_its_path_from_the_old_side() {
        let summary = changed_files_for_message(&message(vec![call(
            "delete_file",
            r#"{"path":"src/gone.rs"}"#,
            Some("--- a/src/gone.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-old\n"),
        )]))
        .expect("a deletion");
        assert_eq!(summary.files[0].path, "src/gone.rs");
        assert_eq!((summary.added(), summary.removed()), (0, 1));
    }

    #[test]
    fn no_recorded_diff_means_no_card() {
        assert!(changed_files_for_message(&message(vec![call("read_file", "{}", None)])).is_none());
        assert!(changed_files_for_message(&ChatMessage::new(
            ChatRole::User,
            vec![ChatFragment::text("hi")]
        ))
        .is_none());
    }
}
