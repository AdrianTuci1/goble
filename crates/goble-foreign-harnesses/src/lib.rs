//! Read-only discovery of foreign harness sessions.
//!
//! Goble hosts a bring-your-own-harness model: a project's work may actually
//! happen in Claude Code, Codex or Cursor. This crate discovers those sessions
//! on disk **without writing anything**, and surfaces them as a normalized
//! [`ForeignSession`] list so the GUI can show "what did my other agents do in
//! this project?" (per-project observability) before replaying them through a
//! harness adapter.
//!
//! Claude Code and Codex store transcripts as JSON-lines (`.jsonl`) files under
//! `~/.claude/projects` and `~/.codex/sessions` respectively; the parser here is
//! tolerant of the shapes those tools emit. Cursor's transcript store is a
//! SQLite database (not JSONL), so its sessions are surfaced as metadata-only
//! entries; parsing that store is future work.
//!
//! The crate is framework-agnostic (no `goble-core`, no UI) and depends only on
//! `serde`/`dirs`. It never mutates the discovered files.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A third-party agent harness whose sessions Goble can observe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForeignTool {
    Claude,
    Codex,
    Cursor,
}

impl ForeignTool {
    pub fn as_str(&self) -> &'static str {
        match self {
            ForeignTool::Claude => "claude",
            ForeignTool::Codex => "codex",
            ForeignTool::Cursor => "cursor",
        }
    }

    /// The directory (relative to the home root) where this tool keeps its
    /// sessions.
    fn session_dir_rel(&self) -> &'static str {
        match self {
            ForeignTool::Claude => ".claude/projects",
            ForeignTool::Codex => ".codex/sessions",
            ForeignTool::Cursor => ".cursor",
        }
    }
}

/// One message extracted from a foreign transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignMessage {
    pub role: String,
    pub content: String,
    pub created_at: Option<String>,
}

/// A discovered foreign session (read-only view).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForeignSession {
    pub tool: ForeignTool,
    pub session_id: String,
    pub path: PathBuf,
    pub updated_at: Option<String>,
    pub messages: Vec<ForeignMessage>,
}

impl ForeignSession {
    /// A short preview of the last message, for list rendering.
    pub fn preview(&self, max_chars: usize) -> String {
        let text = self
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let text = text.trim();
        if text.is_empty() {
            return String::new();
        }
        let mut out = text.chars().take(max_chars).collect::<String>();
        if text.chars().count() > max_chars {
            out.push('…');
        }
        out
    }
}

/// Discover foreign sessions using the current user's home directory.
pub fn discover_foreign_sessions() -> Vec<ForeignSession> {
    match dirs::home_dir() {
        Some(home) => discover_at(&home),
        None => Vec::new(),
    }
}

/// Discover foreign sessions below `root` (a home dir, or an injected path in
/// tests). Returns sessions sorted by most recently updated first, dropping
/// `.jsonl` files that contain no parseable transcript.
pub fn discover_at(root: &Path) -> Vec<ForeignSession> {
    let mut sessions = Vec::new();
    for (tool, rel) in [
        (ForeignTool::Claude, ForeignTool::Claude.session_dir_rel()),
        (ForeignTool::Codex, ForeignTool::Codex.session_dir_rel()),
        (ForeignTool::Cursor, ForeignTool::Cursor.session_dir_rel()),
    ] {
        let dir = root.join(rel);
        if !dir.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        collect_jsonl(&dir, &mut files);
        for path in files {
            let session = parse_session(tool, &path);
            if let Some(session) = session {
                sessions.push(session);
            }
        }
    }
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    sessions
}

fn parse_session(tool: ForeignTool, path: &Path) -> Option<ForeignSession> {
    let session_id = path.file_stem()?.to_string_lossy().to_string();
    if session_id.is_empty() {
        return None;
    }
    let messages = parse_jsonl(path);
    if messages.is_empty() {
        // A `.jsonl` with no transcript is config/state noise, not a session.
        return None;
    }
    let updated_at = messages
        .iter()
        .rev()
        .find_map(|m| m.created_at.clone())
        .or_else(|| file_mtime(path));
    Some(ForeignSession {
        tool,
        session_id,
        path: path.to_path_buf(),
        updated_at,
        messages,
    })
}

/// Read a `.jsonl` file and extract its messages, tolerantly.
fn parse_jsonl(path: &Path) -> Vec<ForeignMessage> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines().filter_map(parse_line).collect()
}

/// Extract a [`ForeignMessage`] from one JSON-lines record, if it carries text.
fn parse_line(line: &str) -> Option<ForeignMessage> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let content = extract_text(&v)?;
    let role = extract_role(&v).unwrap_or_else(|| "unknown".to_string());
    Some(ForeignMessage {
        role,
        content,
        created_at: extract_timestamp(&v),
    })
}

/// The role of a record, from any of the shapes the tools use.
fn extract_role(v: &Value) -> Option<String> {
    let candidates = [Some(v), v.get("message"), v.get("payload")];
    // Prefer an explicit `role` field over a block `type`.
    for c in candidates.iter().flatten() {
        if let Some(s) = c.get("role").and_then(Value::as_str) {
            return Some(normalize_role(s));
        }
    }
    for c in candidates.iter().flatten() {
        if let Some(s) = c.get("type").and_then(Value::as_str) {
            if is_role_token(s) {
                return Some(normalize_role(s));
            }
        }
    }
    None
}

/// The text content of a record, from its `content` string/array or a nested
/// `message`/`payload`.
fn extract_text(v: &Value) -> Option<String> {
    if let Some(s) = v.get("content").and_then(Value::as_str) {
        return Some(s.to_string());
    }
    if let Some(blocks) = v.get("content").and_then(Value::as_array) {
        let texts: Vec<String> = blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str).map(String::from))
            .collect();
        if !texts.is_empty() {
            return Some(texts.join("\n"));
        }
    }
    for key in ["message", "payload"] {
        if let Some(nested) = v.get(key) {
            if let Some(s) = extract_text(nested) {
                return Some(s);
            }
        }
    }
    v.get("text").and_then(Value::as_str).map(String::from)
}

/// A timestamp for a record, from the typical fields.
fn extract_timestamp(v: &Value) -> Option<String> {
    for key in ["timestamp", "created_at", "ts", "time"] {
        if let Some(s) = v.get(key).and_then(str_or_num) {
            return Some(s);
        }
    }
    for container in ["message", "payload"] {
        if let Some(nested) = v.get(container) {
            for key in ["timestamp", "created_at", "ts", "time"] {
                if let Some(s) = nested.get(key).and_then(str_or_num) {
                    return Some(s);
                }
            }
        }
    }
    None
}

fn str_or_num(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn normalize_role(raw: &str) -> String {
    match raw.to_lowercase().as_str() {
        "human" | "user" | "input" | "input_text" => "user".to_string(),
        "assistant" | "ai" | "output" | "output_text" => "assistant".to_string(),
        "tool" | "tool_result" | "function_call" | "function" => "tool".to_string(),
        _ => raw.to_lowercase(),
    }
}

fn is_role_token(raw: &str) -> bool {
    matches!(
        raw.to_lowercase().as_str(),
        "user" | "human" | "assistant" | "ai" | "tool" | "system" | "input" | "output"
    )
}

fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

fn file_mtime(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let dt: chrono::DateTime<chrono::Utc> = modified.into();
    Some(dt.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn write(path: PathBuf, content: &str) -> PathBuf {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn claude_jsonl_is_discovered() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".claude/projects/backend");
        let transcript = concat!(
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"fix the bug\"}]}}\n",
            "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"done\"}]}}\n",
        );
        let path = write(dir.join("abc-123.jsonl"), transcript);

        let sessions = discover_at(root.path());
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert_eq!(s.tool, ForeignTool::Claude);
        assert_eq!(s.session_id, "abc-123");
        assert_eq!(s.path, path);
        assert_eq!(s.messages.len(), 2);
        assert_eq!(s.messages[0].role, "user");
        assert_eq!(s.messages[0].content, "fix the bug");
        assert_eq!(s.messages[1].role, "assistant");
        assert_eq!(s.messages[1].content, "done");
        assert_eq!(s.preview(100), "done");
    }

    #[test]
    fn codex_response_items_are_discovered() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".codex/sessions/2024/08/25");
        let transcript = concat!(
            "{\"type\":\"session_meta\",\"timestamp\":\"2024-08-25T10:00:00Z\"}\n",
            "{\"type\":\"response_item\",\"timestamp\":\"2024-08-25T10:00:01Z\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"add tests\"}]}}\n",
            "{\"type\":\"response_item\",\"timestamp\":\"2024-08-25T10:00:02Z\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"tests added\"}]}}\n",
        );
        let _path = write(dir.join("rollout-2024-08-25-abc.jsonl"), transcript);

        let sessions = discover_at(root.path());
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert_eq!(s.tool, ForeignTool::Codex);
        assert_eq!(s.session_id, "rollout-2024-08-25-abc");
        assert_eq!(s.messages.len(), 2);
        assert_eq!(s.messages[0].role, "user");
        assert_eq!(s.messages[0].content, "add tests");
        assert_eq!(s.messages[1].content, "tests added");
        // session_meta line has no text -> dropped.
        assert_eq!(s.updated_at.as_deref(), Some("2024-08-25T10:00:02Z"));
    }

    #[test]
    fn cursor_dir_is_scanned_read_only() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".cursor");
        let transcript = "{\"role\":\"user\",\"content\":\"hello cursor\"}\n";
        let _path = write(dir.join("ws-1.jsonl"), transcript);

        let sessions = discover_at(root.path());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].tool, ForeignTool::Cursor);
        assert_eq!(sessions[0].preview(100), "hello cursor");
    }

    #[test]
    fn empty_or_non_transcript_jsonl_is_skipped() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".claude/projects/x");
        // Schema-like JSON lines with no parseable text are noise.
        write(dir.join("settings.jsonl"), "{\"model\":\"claude-3-5\"}\n");
        write(dir.join("meta.jsonl"), "not json at all\n");

        let sessions = discover_at(root.path());
        assert!(sessions.is_empty());
    }

    #[test]
    fn discovery_sorts_by_most_recent() {
        let root = tempfile::tempdir().unwrap();
        write(
            root.path().join(".codex/sessions/old.jsonl"),
            "{\"role\":\"user\",\"content\":\"a\",\"timestamp\":\"2024-01-01T00:00:00Z\"}\n",
        );
        write(
            root.path().join(".codex/sessions/new.jsonl"),
            "{\"role\":\"user\",\"content\":\"b\",\"timestamp\":\"2024-02-01T00:00:00Z\"}\n",
        );
        let sessions = discover_at(root.path());
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "new");
        assert_eq!(sessions[1].session_id, "old");
    }
}
