//! The title a CLI harness recorded for the conversation it ran in a directory.
//!
//! A TUI harness keeps its conversation inside its own process and its own
//! session store, so nothing goble reads through the chat store says what that
//! conversation was about: its sidebar card would stay named for the harness.
//! Each CLI does write a title, a summary or the first prompt of the session
//! where it keeps its sessions, so the newest one for a working directory is
//! read the way [`super::harness_session`] reads its id, and the card takes the
//! name the conversation itself carries.
//!
//! Every lookup answers `None` when the store is missing, holds no session for
//! the directory, or writes nothing a title can be read from: the caller then
//! keeps the name the card already has. No lookup invents a title, and none of
//! them fails.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use super::harness_session::{
    antigravity_home, antigravity_session_at, claude_config, claude_project_dir, codex_home,
    data_home, grok_home, grok_sessions_dir, hermes_home, json_value, newest_grok_session,
    omp_roots, pi_roots, sqlite_query,
};

/// The longest title a card shows before the ellipsis.
const MAX_TITLE_CHARS: usize = 60;

/// How much of a transcript a title is looked for in. A tool result can be a
/// whole file inline, so the read is bounded rather than taking the file.
const MAX_SCAN_BYTES: u64 = 8 * 1024 * 1024;

/// A line longer than this is a tool result, not a prompt or a summary.
const MAX_LINE_BYTES: usize = 1024 * 1024;

/// How much of a file the directory match reads.
const HEAD_BYTES: usize = 8 * 1024;

/// The title `harness` recorded for its newest session in `cwd`, when its store
/// can be read.
pub fn session_title(harness: &str, cwd: &str) -> Option<String> {
    // A store is scoped by working directory, so a lookup without one would
    // answer with whichever session happened to be newest anywhere.
    if cwd.trim().is_empty() {
        return None;
    }
    match harness {
        "antigravity" => antigravity_title_at(&antigravity_home()?, cwd),
        "claude" => claude_title_at(&claude_config()?, cwd),
        "codex" => codex_title_at(&codex_home()?, cwd),
        "grok" => grok_title_at(&grok_home()?, cwd),
        "pi" => pi_roots().iter().find_map(|root| jsonl_title_at(root, cwd)),
        "omp" => omp_roots()
            .iter()
            .find_map(|root| jsonl_title_at(root, cwd)),
        "opencode" => opencode_title_at(&data_home()?, cwd),
        "hermes" => hermes_title_at(&hermes_home()?),
        _ => None,
    }
}

/// The title the harness recorded for the session `session_id` in `cwd`.
///
/// A conversation owns one session, and this reads that session's own title:
/// an id the store does not hold answers `None` rather than another session's
/// name, so a card never borrows the title of a session it does not resume.
pub fn session_title_for(harness: &str, cwd: &str, session_id: &str) -> Option<String> {
    let session_id = session_id.trim();
    if session_id.is_empty() || cwd.trim().is_empty() {
        return None;
    }
    match harness {
        "antigravity" => antigravity_title_for(&antigravity_home()?, session_id),
        "claude" => claude_title_for(&claude_config()?, cwd, session_id),
        "codex" => codex_title_for(&codex_home()?, session_id),
        "grok" => grok_title_for(&grok_home()?, cwd, session_id),
        "omp" => omp_roots()
            .iter()
            .find_map(|root| jsonl_title_for(root, session_id)),
        "opencode" => opencode_title_for(&data_home()?, session_id),
        "pi" => pi_roots()
            .iter()
            .find_map(|root| jsonl_title_for(root, session_id)),
        "hermes" => hermes_title_for(&hermes_home()?, session_id),
        _ => None,
    }
}

/// The files under `root` whose name ends with `extension`, newest first, up to
/// `depth` levels of subdirectories.
fn files_newest_first(root: &Path, extension: &str, depth: usize) -> Vec<PathBuf> {
    fn walk(dir: &Path, extension: &str, depth: usize, out: &mut Vec<(SystemTime, PathBuf)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth > 0 {
                    walk(&path, extension, depth - 1, out);
                }
                continue;
            }
            if !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
            {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            out.push((modified, path));
        }
    }
    let mut files = Vec::new();
    walk(root, extension, depth, &mut files);
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.into_iter().map(|(_, path)| path).collect()
}

/// The file of `files` that belongs to `cwd`: the newest one that names the
/// directory, else the newest one there is.
fn newest_for<'a>(files: &'a [PathBuf], cwd: &str) -> Option<&'a Path> {
    files
        .iter()
        .find(|path| head(path, HEAD_BYTES).contains(cwd))
        .or_else(|| files.first())
        .map(PathBuf::as_path)
}

/// The file under `root` that holds the session `session_id`: its stem, a uuid
/// in its name, or the id at its head names it.
fn session_file(root: &Path, extension: &str, depth: usize, session_id: &str) -> Option<PathBuf> {
    files_newest_first(root, extension, depth)
        .into_iter()
        .find(|path| names_session(path, session_id))
}

fn names_session(path: &Path, session_id: &str) -> bool {
    if path
        .file_stem()
        .is_some_and(|stem| stem.to_string_lossy() == session_id)
    {
        return true;
    }
    if path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().contains(session_id))
    {
        return true;
    }
    let head = head(path, HEAD_BYTES);
    ["sessionId", "session_id", "id"]
        .iter()
        .any(|key| json_value(&head, key).as_deref() == Some(session_id))
}

fn head(path: &Path, bytes: usize) -> String {
    let Ok(file) = fs::File::open(path) else {
        return String::new();
    };
    let mut buffer = vec![0_u8; bytes];
    let read = file
        .take(bytes as u64)
        .read(&mut buffer)
        .unwrap_or_default();
    buffer.truncate(read);
    String::from_utf8_lossy(&buffer).into_owned()
}

/// Call `visit` on each JSON row of `path`, until it answers `false`. Both the
/// bytes read and the length of one row are bounded, so a transcript carrying a
/// tool result inline is still a cheap read.
fn scan_rows(path: &Path, mut visit: impl FnMut(&Value) -> bool) {
    let Ok(file) = fs::File::open(path) else {
        return;
    };
    let reader = BufReader::new(file.take(MAX_SCAN_BYTES));
    for line in reader.lines() {
        let Ok(line) = line else {
            return;
        };
        if line.len() > MAX_LINE_BYTES {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if !visit(&value) {
            return;
        }
    }
}

fn claude_title_at(config: &Path, cwd: &str) -> Option<String> {
    // The project directory is the working directory's own, so the newest
    // transcript in it is this conversation's and no other's.
    let project = config.join("projects").join(claude_project_dir(cwd));
    files_newest_first(&project, "jsonl", 0)
        .iter()
        .find_map(|path| claude_title_in(path))
}

/// The title of Claude's own transcript for the session `session`.
fn claude_title_for(config: &Path, cwd: &str, session: &str) -> Option<String> {
    let project = config.join("projects").join(claude_project_dir(cwd));
    claude_title_in(&session_file(&project, "jsonl", 0, session)?)
}

/// The title Claude wrote for one transcript: the summary it generated, else
/// the first prompt the user actually typed.
fn claude_title_in(path: &Path) -> Option<String> {
    let mut summary: Option<String> = None;
    let mut first_prompt: Option<String> = None;
    scan_rows(path, |row| {
        match row.get("type").and_then(Value::as_str) {
            Some("summary") => {
                if summary.is_none() {
                    summary = row
                        .get("summary")
                        .and_then(Value::as_str)
                        .and_then(clean_title);
                }
            }
            Some("user") => {
                if first_prompt.is_none() {
                    first_prompt = row
                        .get("message")
                        .and_then(|message| message.get("content"))
                        .and_then(content_text)
                        .filter(|text| !is_machine_text(text))
                        .and_then(|text| clean_title(&text));
                }
            }
            _ => {}
        }
        // A summary is the name Claude itself gave the session; once both are
        // known nothing later can improve on it.
        summary.is_none() || first_prompt.is_none()
    });
    summary.or(first_prompt)
}

fn codex_title_at(home: &Path, cwd: &str) -> Option<String> {
    let files = files_newest_first(&home.join("sessions"), "jsonl", 3);
    codex_title_in(newest_for(&files, cwd)?)
}

/// The title of Codex's own rollout for the session `session`.
fn codex_title_for(home: &Path, session: &str) -> Option<String> {
    codex_title_in(&session_file(&home.join("sessions"), "jsonl", 3, session)?)
}

/// The title one rollout carries: what the user typed, the way Codex wrote it.
fn codex_title_in(path: &Path) -> Option<String> {
    let mut title: Option<String> = None;
    scan_rows(path, |row| {
        let payload = row.get("payload");
        match row.get("type").and_then(Value::as_str) {
            Some("event_msg") => {
                if payload
                    .and_then(|payload| payload.get("type"))
                    .and_then(Value::as_str)
                    == Some("user_message")
                {
                    title = payload
                        .and_then(|payload| payload.get("message"))
                        .and_then(Value::as_str)
                        .filter(|text| !is_machine_text(text))
                        .and_then(clean_title);
                }
            }
            // The rollout also records the message itself; the `event_msg`
            // carries the text the user typed, this one the turn's payload.
            Some("response_item") => {
                if title.is_none() {
                    let user = payload.filter(|payload| {
                        payload.get("role").and_then(Value::as_str) == Some("user")
                    });
                    title = user
                        .and_then(|payload| payload.get("content"))
                        .and_then(content_text)
                        .filter(|text| !is_machine_text(text))
                        .and_then(|text| clean_title(&text));
                }
            }
            _ => {}
        }
        title.is_none()
    });
    title
}

fn jsonl_title_at(root: &Path, cwd: &str) -> Option<String> {
    let files = files_newest_first(root, "jsonl", 2);
    jsonl_title_in(newest_for(&files, cwd)?)
}

/// The title of the session file `root` holds for `session`.
fn jsonl_title_for(root: &Path, session: &str) -> Option<String> {
    jsonl_title_in(&session_file(root, "jsonl", 2, session)?)
}

/// The title a JSONL transcript carries: the `title` it records, else the first
/// user turn in it.
fn jsonl_title_in(path: &Path) -> Option<String> {
    let mut title: Option<String> = None;
    let mut first_prompt: Option<String> = None;
    scan_rows(path, |row| {
        if title.is_none() {
            title = row
                .get("title")
                .and_then(Value::as_str)
                .and_then(clean_title);
        }
        if first_prompt.is_none() {
            first_prompt = user_message_text(row)
                .filter(|text| !is_machine_text(text))
                .and_then(|text| clean_title(&text));
        }
        title.is_none() || first_prompt.is_none()
    });
    title.or(first_prompt)
}

fn opencode_title_at(data: &Path, cwd: &str) -> Option<String> {
    let root = data.join("opencode").join("storage").join("session");
    let files = files_newest_first(&root, "json", 2);
    opencode_title_in(newest_for(&files, cwd)?)
}

/// The title of OpenCode's own record for `session`.
fn opencode_title_for(data: &Path, session: &str) -> Option<String> {
    let root = data.join("opencode").join("storage").join("session");
    opencode_title_in(&session_file(&root, "json", 2, session)?)
}

/// The title a session record holds.
fn opencode_title_in(path: &Path) -> Option<String> {
    // A session record is its metadata, so the whole of it is small; a file
    // past the read's bound simply names nothing.
    let session = read_json(path)?;
    session
        .get("title")
        .and_then(Value::as_str)
        .and_then(clean_title)
}

/// The title Grok recorded for the newest session it ran in `cwd`: the name the
/// session's `summary.json` carries, else the first prompt the session holds.
fn grok_title_at(root: &Path, cwd: &str) -> Option<String> {
    let session = newest_grok_session(&grok_sessions_dir(root, cwd))?;
    let id = session.file_name()?.to_string_lossy().into_owned();
    grok_title_for(root, cwd, &id)
}

/// The title Grok recorded for its own session `session` in `cwd`: the name its
/// `summary.json` carries, else the first prompt it holds.
fn grok_title_for(root: &Path, cwd: &str, session: &str) -> Option<String> {
    let sessions = grok_sessions_dir(root, cwd);
    let dir = sessions.join(session);
    if !dir.is_dir() {
        return None;
    }
    grok_session_summary(&dir.join("summary.json"))
        .or_else(|| first_user_message(&dir.join("chat_history.jsonl")))
        .or_else(|| first_recorded_prompt(&sessions.join("prompt_history.jsonl"), session))
}

/// The name Grok gave the session, which its `summary.json` carries as
/// `session_summary` once the session has been summarised.
fn grok_session_summary(path: &Path) -> Option<String> {
    let summary = read_json(path)?;
    summary
        .get("session_summary")
        .and_then(Value::as_str)
        .and_then(clean_title)
}

/// The first prompt the user typed into `session`, from the working
/// directory's shared `prompt_history.jsonl`. A shell command is a turn in the
/// session, not a name for it, so an `is_bash` row names nothing.
fn first_recorded_prompt(path: &Path, session: &str) -> Option<String> {
    let mut prompt = None;
    scan_rows(path, |row| {
        let same_session = row.get("session_id").and_then(Value::as_str) == Some(session);
        let is_bash = row.get("is_bash").and_then(Value::as_bool).unwrap_or(false);
        if same_session && !is_bash {
            prompt = row
                .get("prompt")
                .and_then(Value::as_str)
                .filter(|text| !is_machine_text(text))
                .and_then(clean_title);
        }
        prompt.is_none()
    });
    prompt
}

/// The title Antigravity recorded for the conversation it ran in `cwd`: the
/// summaries database's title, else the preview it wrote for the same row.
fn antigravity_title_at(root: &Path, cwd: &str) -> Option<String> {
    antigravity_title_for(root, &antigravity_session_at(root, cwd)?)
}

/// The title Antigravity's summaries database records for `conversation`: its
/// title, else the preview it wrote for the same row.
fn antigravity_title_for(root: &Path, conversation: &str) -> Option<String> {
    let database = root.join("conversation_summaries.db");
    if !database.exists() {
        return None;
    }
    let query = format!(
        "select coalesce(nullif(trim(title), ''), nullif(trim(preview), '')) \
         from conversation_summaries where conversation_id = '{}' limit 1;",
        conversation.replace('\'', "''")
    );
    clean_title(&sqlite_query(&database, &query, true)?)
}

/// Hermes keeps its sessions in SQLite. The CLI's own query answers the newest
/// row: linking a database engine just to read one column is not worth it, and
/// a machine without `sqlite3` simply keeps the name the card has.
fn hermes_title_at(base: &Path) -> Option<String> {
    let database = base.join("state.db");
    if !database.exists() {
        return None;
    }
    let schema = hermes_schema(&database)?;
    let filter = schema.filter();
    let query = format!(
        "select nullif(trim({}), '') from sessions{filter} order by {} desc limit 1;",
        schema.value, schema.order
    );
    clean_title(&sqlite_query(&database, &query, false)?)
}

/// The title Hermes recorded for its own session `session`.
fn hermes_title_for(base: &Path, session: &str) -> Option<String> {
    let database = base.join("state.db");
    if !database.exists() {
        return None;
    }
    let schema = hermes_schema(&database)?;
    let mut conditions = vec![format!("id = '{}'", session.replace('\'', "''"))];
    if schema.has_source() {
        conditions.push("source = 'cli'".to_string());
    }
    let query = format!(
        "select nullif(trim({}), '') from sessions where {} limit 1;",
        schema.value,
        conditions.join(" and ")
    );
    clean_title(&sqlite_query(&database, &query, false)?)
}

/// The columns a Hermes store names a session by, read from its own schema: the
/// CLI writes its table once, and the title column is whichever of the known
/// names it chose.
struct HermesSchema {
    names: Vec<String>,
    value: String,
    order: String,
}

impl HermesSchema {
    fn has_source(&self) -> bool {
        self.names
            .iter()
            .any(|name| name.eq_ignore_ascii_case("source"))
    }

    fn filter(&self) -> &'static str {
        if self.has_source() {
            " where source = 'cli'"
        } else {
            ""
        }
    }
}

fn hermes_schema(database: &Path) -> Option<HermesSchema> {
    let columns = sqlite_query(database, "pragma table_info(sessions);", false)?;
    let names: Vec<String> = columns
        .lines()
        .filter_map(|line| line.split('|').nth(1).map(str::to_string))
        .collect();
    let pick = |candidates: &[&str]| {
        candidates
            .iter()
            .find(|candidate| {
                names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(candidate))
            })
            .map(|candidate| candidate.to_string())
    };
    let value = pick(&[
        "title",
        "summary",
        "subject",
        "name",
        "first_message",
        "first_user_message",
    ])?;
    let order = pick(&["started_at", "created_at", "updated_at", "last_active"])
        .unwrap_or_else(|| "rowid".to_string());
    Some(HermesSchema {
        names,
        value,
        order,
    })
}

/// A store's small JSON record, read from the head of the file: a session
/// record or a summary, never a transcript.
fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&head(path, 64 * 1024)).ok()
}

/// The first turn in `path` the user actually typed.
fn first_user_message(path: &Path) -> Option<String> {
    let mut message = None;
    scan_rows(path, |row| {
        message = user_message_text(row)
            .filter(|text| !is_machine_text(text))
            .and_then(|text| clean_title(&text));
        message.is_none()
    });
    message
}

/// The text of a user turn written as a plain JSONL row: the role may sit on
/// the row or in its `message`, and the text may be a string or a list of parts.
fn user_message_text(row: &Value) -> Option<String> {
    let message = row.get("message").unwrap_or(row);
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .or_else(|| row.get("role").and_then(Value::as_str))
        .or_else(|| (row.get("type").and_then(Value::as_str) == Some("user")).then_some("user"))?;
    if role != "user" {
        return None;
    }
    content_text(message.get("content").or_else(|| message.get("text"))?)
        .or_else(|| row.get("text").and_then(Value::as_str).map(str::to_string))
}

/// The text of a message `content`: a bare string, or the text parts of a list.
fn content_text(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let texts: Vec<&str> = parts
                .iter()
                .filter(|part| {
                    matches!(
                        part.get("type").and_then(Value::as_str),
                        Some("text" | "input_text")
                    )
                })
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect();
            (!texts.is_empty()).then(|| texts.join(" "))
        }
        _ => None,
    }
}

/// Whether a stored user turn is the harness's own context rather than
/// something the user typed: a command it ran (`<command-name>`), the output it
/// printed back (`<local-command-stdout>`), or the instructions it prepends to
/// the first turn. All of it is written as markup, so a turn that opens with a
/// tag names nothing.
fn is_machine_text(text: &str) -> bool {
    text.trim_start().starts_with('<')
}

/// The title `text` reads as: one line, without markup or control noise,
/// trimmed to [`MAX_TITLE_CHARS`] with an ellipsis.
///
/// A title is a name, not the message: the first line that says anything is the
/// one a card can show, and everything a harness writes as markup around a
/// turn (`<command-name>`, a control sequence) names nothing.
fn clean_title(text: &str) -> Option<String> {
    let first = text
        .lines()
        // A line a harness wrote about a command it ran is not something the
        // user said, whatever the tag wraps.
        .filter(|line| !is_machine_text(line))
        .map(strip_tags)
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .find(|line| !line.is_empty())?;
    if !first.chars().any(char::is_alphanumeric) {
        return None;
    }
    Some(capitalize_first(&truncate(first)))
}

/// `text` without the XML-like tags a harness surrounds a turn with. A `<` that
/// does not open a short tag (`a < b`) is text and stays.
fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let opens_tag = after
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '/' || ch == '!' || ch == '?');
        let close = after.find('>').filter(|end| *end < 64);
        match (opens_tag, close) {
            (true, Some(end)) => {
                out.push_str(&rest[..start]);
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str(&rest[..start + 1]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn truncate(mut title: String) -> String {
    if title.chars().count() <= MAX_TITLE_CHARS {
        return title;
    }
    let mut cut: String = title.chars().take(MAX_TITLE_CHARS).collect();
    if let Some(space) = cut.rfind(' ').filter(|space| *space > 0) {
        cut.truncate(space);
    }
    title = cut.trim_end().to_string();
    title.push('…');
    title
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("session directory");
        }
        fs::write(path, contents).expect("transcript");
    }

    /// A card names itself from the transcript it owns, whichever one ran last
    /// in the same directory.
    #[test]
    fn a_named_session_is_read_from_its_own_file() {
        let home = tempfile::tempdir().expect("temp store");
        let cwd = "/Users/me/Projects/goble";

        let project = home.path().join("projects").join(claude_project_dir(cwd));
        for (id, summary) in [
            ("session-a", "First summary"),
            ("session-b", "Second summary"),
        ] {
            write(
                &project.join(format!("{id}.jsonl")),
                &format!(r#"{{"type":"summary","summary":"{summary}"}}"#),
            );
        }
        assert_eq!(
            claude_title_for(home.path(), cwd, "session-a").as_deref(),
            Some("First summary"),
            "the transcript the card owns names it"
        );
        assert_eq!(
            claude_title_for(home.path(), cwd, "session-b").as_deref(),
            Some("Second summary")
        );
        assert_eq!(claude_title_for(home.path(), cwd, "session-c"), None);

        let rollout = home
            .path()
            .join("sessions")
            .join("2026")
            .join("rollout-2026-01-02T03-04-05-0192f3a4-1111-2222-3333-444455556666.jsonl");
        write(
            &rollout,
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"Codex named this"}}"#,
        );
        assert_eq!(
            codex_title_for(home.path(), "0192f3a4-1111-2222-3333-444455556666").as_deref(),
            Some("Codex named this")
        );
        assert_eq!(
            codex_title_for(home.path(), "0192f3a4-0000-0000-0000-000000000000"),
            None
        );

        let pi = home.path().join("pi");
        write(
            &pi.join("a.jsonl"),
            r#"{"sessionId":"pi-1","title":"Pi named this"}"#,
        );
        assert_eq!(
            jsonl_title_for(&pi, "pi-1").as_deref(),
            Some("Pi named this")
        );
        assert_eq!(jsonl_title_for(&pi, "pi-2"), None);

        let opencode = home.path().join("data");
        write(
            &opencode
                .join("opencode")
                .join("storage")
                .join("session")
                .join("ses_1.json"),
            r#"{"title":"OpenCode named this"}"#,
        );
        assert_eq!(
            opencode_title_for(&opencode, "ses_1").as_deref(),
            Some("OpenCode named this")
        );
        assert_eq!(opencode_title_for(&opencode, "ses_2"), None);
    }

    /// Claude's own summary names the session; the transcript's first prompt is
    /// the fallback, and a local-command line is not a prompt.
    #[test]
    fn a_claude_session_is_named_by_its_summary_then_by_its_first_prompt() {
        let config = tempfile::tempdir().expect("temp claude config");
        let cwd = "/Users/me/Projects/goble";
        let transcript = config
            .path()
            .join("projects")
            .join(claude_project_dir(cwd))
            .join("0a1b.jsonl");
        write(
            &transcript,
            concat!(
                r#"{"type":"user","message":{"role":"user","content":"<command-name>/clear</command-name>"}}"#,
                "\n",
                r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Arata un text colorat in chat"}]}}"#,
                "\n",
            ),
        );
        assert_eq!(
            claude_title_at(config.path(), cwd).as_deref(),
            Some("Arata un text colorat in chat")
        );

        // A summary written later is the name Claude itself gave the session.
        write(
            &transcript,
            concat!(
                r#"{"type":"summary","summary":"sidebar titles for harness chat"}"#,
                "\n",
                r#"{"type":"user","message":{"role":"user","content":"Arata un text colorat in chat"}}"#,
                "\n",
            ),
        );
        assert_eq!(
            claude_title_at(config.path(), cwd).as_deref(),
            Some("Sidebar titles for harness chat")
        );
    }

    /// A summary alone still names the conversation, and a store with no
    /// transcript for the directory names nothing.
    #[test]
    fn a_claude_store_without_a_session_names_nothing() {
        let config = tempfile::tempdir().expect("temp claude config");
        assert_eq!(
            claude_title_at(config.path(), "/Users/me/Projects/goble"),
            None
        );
    }

    #[test]
    fn a_codex_rollout_is_named_by_its_user_message() {
        let home = tempfile::tempdir().expect("temp codex home");
        let cwd = "/Users/me/Projects/goble";
        write(
            &home.path().join("sessions/2026/01/02/rollout-abc.jsonl"),
            concat!(
                r#"{"type":"session_meta","payload":{"id":"abc","cwd":"/Users/me/Projects/goble"}}"#,
                "\n",
                r#"{"type":"event_msg","payload":{"type":"user_message","message":"Goble to become an enterprise ready product"}}"#,
                "\n",
            ),
        );
        assert_eq!(
            codex_title_at(home.path(), cwd).as_deref(),
            Some("Goble to become an enterprise ready product")
        );
    }

    #[test]
    fn an_opencode_session_carries_its_title() {
        let data = tempfile::tempdir().expect("temp data home");
        let cwd = "/Users/me/Projects/goble";
        write(
            &data
                .path()
                .join("opencode/storage/session/global/ses_1.json"),
            r#"{"id":"ses_1","directory":"/Users/me/Projects/goble","title":"Fix the sidebar titles"}"#,
        );
        assert_eq!(
            opencode_title_at(data.path(), cwd).as_deref(),
            Some("Fix the sidebar titles")
        );
    }

    /// Grok names a session in its `summary.json`; a session it never
    /// summarised is named by the first prompt the user typed into it.
    #[test]
    fn a_grok_session_is_named_by_its_summary_then_by_its_first_prompt() {
        let home = tempfile::tempdir().expect("temp grok home");
        let cwd = "/Users/me/Projects/goble";
        let session =
            grok_sessions_dir(home.path(), cwd).join("0192f3a4-1111-2222-3333-444455556666");
        write(
            &session.join("summary.json"),
            r#"{"info":{"cwd":"/Users/me/Projects/goble"},"session_summary":"Fix CLI harness sidebar titles using session store titles","updated_at":"2026-01-02T00:00:00.000000Z"}"#,
        );
        assert_eq!(
            grok_title_at(home.path(), cwd).as_deref(),
            Some("Fix CLI harness sidebar titles using session store titles")
        );

        // A summary Grok has not filled in yet leaves the transcript to name
        // the session: a system reminder is not the user's prompt, the line
        // after it is.
        write(
            &session.join("summary.json"),
            r#"{"session_summary":"","updated_at":"2026-01-02T00:00:00.000000Z"}"#,
        );
        write(
            &session.join("chat_history.jsonl"),
            concat!(
                r#"{"type":"system","content":"You are Grok."}"#,
                "\n",
                r#"{"type":"user","content":[{"type":"text","text":"<system-reminder>skills</system-reminder>"}]}"#,
                "\n",
                r#"{"type":"user","content":[{"type":"text","text":"cand folosim harness custom trebuie afisat titlul corect"}]}"#,
                "\n",
            ),
        );
        assert_eq!(
            grok_title_at(home.path(), cwd).as_deref(),
            Some("Cand folosim harness custom trebuie afisat titlul corect")
        );

        // A named conversation reads its own session, even when the directory
        // holds another one that ran later.
        let newer =
            grok_sessions_dir(home.path(), cwd).join("0192f3a4-9999-8888-7777-666655554444");
        write(
            &newer.join("summary.json"),
            r#"{"session_summary":"Someone else's session","updated_at":"2026-01-03T00:00:00.000000Z"}"#,
        );
        assert_eq!(
            grok_title_for(home.path(), cwd, "0192f3a4-1111-2222-3333-444455556666").as_deref(),
            Some("Cand folosim harness custom trebuie afisat titlul corect"),
            "the session the conversation owns names it"
        );
        assert_eq!(
            grok_title_at(home.path(), cwd).as_deref(),
            Some("Someone else's session"),
            "the newest session is another conversation's"
        );
        assert_eq!(
            grok_title_for(home.path(), cwd, "0192f3a4-0000-0000-0000-000000000000"),
            None,
            "an id the store does not hold names nothing"
        );
    }

    /// A session with no transcript of its own is named by the prompt history
    /// the working directory keeps, read for that session only.
    #[test]
    fn a_grok_session_without_a_transcript_is_named_by_the_prompt_history() {
        let home = tempfile::tempdir().expect("temp grok home");
        let cwd = "/Users/me/Projects/goble";
        let sessions = grok_sessions_dir(home.path(), cwd);
        write(
            &sessions.join("aaaa1111-2222-3333-4444-555566667777/summary.json"),
            r#"{"session_summary":"","updated_at":"2026-01-02T00:00:00.000000Z"}"#,
        );
        write(
            &sessions.join("prompt_history.jsonl"),
            concat!(
                r#"{"session_id":"bbbb1111-2222-3333-4444-555566667777","prompt":"another conversation","is_bash":false}"#,
                "\n",
                r#"{"session_id":"aaaa1111-2222-3333-4444-555566667777","prompt":"cargo test","is_bash":true}"#,
                "\n",
                r#"{"session_id":"aaaa1111-2222-3333-4444-555566667777","prompt":"Arata un text colorat in chat","is_bash":false}"#,
                "\n",
            ),
        );

        assert_eq!(
            grok_title_at(home.path(), cwd).as_deref(),
            Some("Arata un text colorat in chat"),
            "the session's own first prompt names it, and a shell command does not"
        );
    }

    /// Antigravity names the conversation in its summaries database: the title
    /// it generated, else the preview of the same row.
    #[test]
    fn an_antigravity_conversation_is_named_by_its_database_row() {
        if !sqlite3_available() {
            eprintln!("sqlite3 is not installed; skipping the antigravity title case");
            return;
        }
        let home = tempfile::tempdir().expect("temp antigravity home");
        let cwd = "/Users/me/Projects/goble";
        let database = home.path().join("conversation_summaries.db");
        let status = std::process::Command::new("sqlite3")
            .arg(&database)
            .arg(
                "create table conversation_summaries (conversation_id text primary key, \
                 title text, preview text, workspace_uris text, last_modified_time text);\
                 insert into conversation_summaries values ('conv-1', 'Session Termination Request', \
                 'Session Termination Request', '[\"file:///Users/me/Projects/goble\"]', \
                 '2026-01-02 00:00:00+00:00');\
                 insert into conversation_summaries values ('conv-2', '', \
                 'Add a title to the sidebar cards', '[\"file:///Users/me/Projects/other\"]', \
                 '2026-01-03 00:00:00+00:00');",
            )
            .status()
            .expect("run sqlite3");
        assert!(status.success(), "sqlite3 built the summaries store");
        write(
            &home.path().join("cache").join("last_conversations.json"),
            &format!(r#"{{"{cwd}": "conv-1", "/Users/me/Projects/other": "conv-2"}}"#),
        );

        assert_eq!(
            antigravity_title_at(home.path(), cwd).as_deref(),
            Some("Session Termination Request")
        );
        assert_eq!(
            antigravity_title_at(home.path(), "/Users/me/Projects/other").as_deref(),
            Some("Add a title to the sidebar cards"),
            "a row with no title is named by its preview"
        );
        assert_eq!(
            antigravity_title_at(home.path(), "/Users/me/Projects/none"),
            None
        );
        assert_eq!(
            antigravity_title_for(home.path(), "conv-1").as_deref(),
            Some("Session Termination Request"),
            "the row the conversation owns names it"
        );
        assert_eq!(
            antigravity_title_for(home.path(), "conv-2").as_deref(),
            Some("Add a title to the sidebar cards"),
            "another conversation's row names that conversation"
        );
        assert_eq!(
            antigravity_title_for(home.path(), "conv-unknown"),
            None,
            "an id the database does not hold names nothing"
        );
    }

    /// A store that is not there, a harness that keeps none, and a lookup with
    /// no directory all name nothing — the caller keeps the name it has.
    #[test]
    fn nothing_is_named_without_a_store_or_a_directory() {
        let empty = tempfile::tempdir().expect("temp home");
        assert_eq!(hermes_title_at(empty.path()), None);
        assert_eq!(
            grok_title_at(empty.path(), "/Users/me/Projects/goble"),
            None
        );
        assert_eq!(
            antigravity_title_at(empty.path(), "/Users/me/Projects/goble"),
            None
        );
        assert_eq!(session_title("goble", "/Users/me/Projects/goble"), None);
        assert_eq!(session_title("claude", "  "), None);
    }

    fn sqlite3_available() -> bool {
        std::process::Command::new("sqlite3")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    #[test]
    fn a_title_is_one_short_clean_line() {
        assert_eq!(
            clean_title("  Deci pentru o persoana solo sa\nmai vorbim  ").as_deref(),
            Some("Deci pentru o persoana solo sa")
        );
        assert_eq!(clean_title("<command-name>/clear</command-name>"), None);
        assert_eq!(clean_title("   "), None);
        let long = "please refactor the renderer so the sidebar title column wraps without \
                    truncating the timestamp of every conversation in the list";
        let title = clean_title(long).expect("a title");
        assert!(title.chars().count() <= MAX_TITLE_CHARS + 1, "{title}");
        assert!(title.ends_with('…'), "{title}");
    }
}
