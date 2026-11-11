//! The session ids harness CLIs keep on disk, and the command that reopens one.
//!
//! A TUI harness launched in a pane's pty owns its conversation inside its own
//! process; goble never sees a protocol event carrying a session id. What the
//! harness does leave behind is its session store, in a layout each CLI fixes:
//! transcript files under a directory named for the working directory, JSONL
//! rollouts, a storage tree, or a SQLite database. Reading the newest one for a
//! directory is how a sidebar click can reopen the conversation in the harness
//! that ran it, the way Paseo resumes through an agent's session id.
//!
//! Every lookup answers `None` when the store is not there or holds nothing
//! newer: the caller then launches the harness fresh instead of inventing an
//! id it cannot verify.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use goble_core::harness::HarnessSpec;
use serde_json::Value;

/// The newest session `harness` recorded for `cwd`, when its store can be read.
pub fn newest_session(harness: &str, cwd: &str) -> Option<String> {
    match harness {
        "antigravity" => antigravity_session(cwd),
        "claude" => claude_session(cwd),
        "codex" => codex_session(cwd),
        "grok" => grok_session(cwd),
        "pi" => pi_session(cwd),
        "omp" => omp_session(cwd),
        "opencode" => opencode_session(cwd),
        "hermes" => hermes_session(),
        _ => None,
    }
}

/// The line that reopens `harness` in `cwd`: its own resume command when a
/// session id was found, else its plain launch command.
pub fn resume_or_launch(spec: &HarnessSpec, cwd: &str) -> String {
    resume_or_launch_with(spec, cwd, newest_session)
}

/// [`resume_or_launch`] with the session lookup passed in, so the resume/fresh
/// choice is testable without a harness store on the machine.
fn resume_or_launch_with(
    spec: &HarnessSpec,
    cwd: &str,
    lookup: impl Fn(&str, &str) -> Option<String>,
) -> String {
    let resume = lookup(spec.id, cwd).and_then(|session| spec.resume_command(&session));
    resume.unwrap_or_else(|| spec.commands.first().copied().unwrap_or("").to_string())
}

/// The line that reopens `harness` in `cwd`: the session the conversation owns
/// (`session_id`) when it has one, else the newest session its store records,
/// else a plain launch.
///
/// A conversation that carried an id keeps that session even after a newer one
/// ran in the same directory; a conversation without one still reopens the way
/// [`resume_or_launch`] always has.
pub fn resume_command_for(spec: &HarnessSpec, cwd: &str, session_id: Option<&str>) -> String {
    let session = session_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| newest_session(spec.id, cwd));
    session
        .and_then(|session| spec.resume_command(&session))
        .unwrap_or_else(|| spec.commands.first().copied().unwrap_or("").to_string())
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The files under `root` whose name ends with `extension`, newest first, up to
/// `depth` levels of subdirectories.
fn files_newest_first(root: &Path, extension: &str, depth: usize) -> Vec<PathBuf> {
    files_with_times(root, extension, depth)
        .into_iter()
        .map(|(path, _)| path)
        .collect()
}

/// The same walk as [`files_newest_first`], keeping each file's time.
fn files_with_times(root: &Path, extension: &str, depth: usize) -> Vec<(PathBuf, SystemTime)> {
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
    let mut timed = Vec::new();
    walk(root, extension, depth, &mut timed);
    timed.sort_by(|a, b| b.0.cmp(&a.0));
    timed.into_iter().map(|(time, path)| (path, time)).collect()
}

/// The id a session file carries: the field at its head names it, else the uuid
/// in its name (a rollout), else its stem.
fn file_session_id(path: &Path) -> Option<String> {
    let head = head(path, 8 * 1024);
    ["sessionId", "session_id", "id"]
        .iter()
        .find_map(|key| json_value(&head, key).and_then(|value| non_empty(&value)))
        .or_else(|| {
            path.file_name()
                .and_then(|name| uuid_in(&name.to_string_lossy()))
        })
        .or_else(|| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
}

/// Every session a store of JSON files lists, newest first.
fn files_sessions(root: &Path, extension: &str, depth: usize) -> Vec<(String, SystemTime)> {
    let mut sessions: Vec<(String, SystemTime)> = files_with_times(root, extension, depth)
        .into_iter()
        .filter_map(|(path, time)| Some((file_session_id(&path)?, time)))
        .collect();
    newest_first(&mut sessions);
    sessions
}

/// Sort sessions newest first and drop a repeated id, keeping its newest entry.
fn newest_first(sessions: &mut Vec<(String, SystemTime)>) {
    sessions.sort_by(|a, b| b.1.cmp(&a.1));
    let mut seen = std::collections::HashSet::new();
    sessions.retain(|(id, _)| seen.insert(id.clone()));
}

/// A timestamp a session store wrote, as the clock the caller compares against.
///
/// The stores write RFC 3339 in UTC (`2026-11-10T23:47:07.406622Z`); SQLite
/// writes the same with a space instead of the `T`, which is the only shape
/// that has to be adapted.
fn session_time(stamp: &str) -> Option<SystemTime> {
    let stamp = stamp.trim();
    if stamp.is_empty() {
        return None;
    }
    let adapted = stamp.replacen(' ', "T", 1);
    chrono::DateTime::parse_from_rfc3339(&adapted)
        .ok()
        .map(SystemTime::from)
}

fn file_time(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// The sessions `harness` recorded for `cwd`, newest first, as `(id, time)`.
///
/// This is the list a conversation's own session is picked from: its id is
/// matched against the conversation's `updated_at`, so two conversations in one
/// directory keep two sessions instead of sharing the newest. A harness whose
/// store cannot be read answers with no sessions rather than with a guess.
pub fn sessions_newest_first(harness: &str, cwd: &str) -> Vec<(String, SystemTime)> {
    if cwd.trim().is_empty() {
        return Vec::new();
    }
    match harness {
        "antigravity" => antigravity_home()
            .map(|root| antigravity_sessions_at(&root, cwd))
            .unwrap_or_default(),
        "claude" => claude_config()
            .map(|config| claude_sessions_at(&config, cwd))
            .unwrap_or_default(),
        "codex" => codex_home()
            .map(|home| codex_sessions_at(&home.join("sessions")))
            .unwrap_or_default(),
        "grok" => grok_home()
            .map(|root| grok_sessions_at(&root, cwd))
            .unwrap_or_default(),
        "omp" => jsonl_sessions_at(&omp_roots()),
        "opencode" => data_home()
            .map(|data| opencode_sessions_at(&data))
            .unwrap_or_default(),
        "pi" => jsonl_sessions_at(&pi_roots()),
        "hermes" => hermes_home()
            .map(|base| hermes_sessions_at(&base))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The path segment Claude Code encodes a working directory to:
/// every non-alphanumeric character becomes `-`.
pub(super) fn claude_project_dir(cwd: &str) -> String {
    cwd.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

/// The root Claude Code keeps its projects under.
pub(super) fn claude_config() -> Option<PathBuf> {
    env_path("CLAUDE_CONFIG_DIR").or_else(|| home().map(|home| home.join(".claude")))
}

fn claude_session(cwd: &str) -> Option<String> {
    claude_session_at(&claude_config()?, cwd)
}

/// [`claude_session`] against a config root passed in: a project directory
/// scopes the lookup to the working directory, and the transcript's file name
/// is the session id (a UUID).
fn claude_session_at(config: &Path, cwd: &str) -> Option<String> {
    let project = config.join("projects").join(claude_project_dir(cwd));
    files_newest_first(&project, "jsonl", 0)
        .into_iter()
        .find_map(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
}

/// Every transcript in `cwd`'s project directory, newest first: one project
/// directory per working directory, so each file is a session of it.
fn claude_sessions_at(config: &Path, cwd: &str) -> Vec<(String, SystemTime)> {
    files_sessions(
        &config.join("projects").join(claude_project_dir(cwd)),
        "jsonl",
        0,
    )
}

fn uuid_in(name: &str) -> Option<String> {
    // A UUID is 36 characters: 8, 4, 4, 4 and 12 hex digits in five groups.
    let bytes = name.as_bytes();
    if bytes.len() < 36 || !bytes.is_ascii() {
        return None;
    }
    for start in (0..=bytes.len() - 36).rev() {
        let candidate = &bytes[start..start + 36];
        let parts: Vec<&[u8]> = candidate.split(|byte| *byte == b'-').collect();
        if parts.len() == 5
            && parts
                .iter()
                .zip([8, 4, 4, 4, 12])
                .all(|(part, len)| part.len() == len && part.iter().all(u8::is_ascii_hexdigit))
        {
            return Some(String::from_utf8_lossy(candidate).to_ascii_lowercase());
        }
    }
    None
}

fn head(path: &Path, bytes: usize) -> String {
    let Ok(file) = fs::File::open(path) else {
        return String::new();
    };
    use std::io::Read;
    let mut buffer = vec![0_u8; bytes];
    let read = file
        .take(bytes as u64)
        .read(&mut buffer)
        .unwrap_or_default();
    buffer.truncate(read);
    String::from_utf8_lossy(&buffer).into_owned()
}

/// The JSON string value of `key` in `text`, without pulling in a parser for
/// the few fields a session store exposes at the head of a file.
pub(super) fn json_value(text: &str, key: &str) -> Option<String> {
    for needle in [format!("\"{key}\":\""), format!("\"{key}\": \"")] {
        if let Some(index) = text.find(&needle) {
            let rest = &text[index + needle.len()..];
            let end = rest.find('"')?;
            return Some(rest[..end].to_string());
        }
    }
    None
}

/// The root Codex keeps its rollouts under.
pub(super) fn codex_home() -> Option<PathBuf> {
    env_path("CODEX_HOME").or_else(|| home().map(|home| home.join(".codex")))
}

fn codex_session(cwd: &str) -> Option<String> {
    codex_session_at(&codex_home()?.join("sessions"), cwd)
}

/// [`codex_session`] against a sessions root passed in.
fn codex_session_at(sessions: &Path, cwd: &str) -> Option<String> {
    let files = files_newest_first(sessions, "jsonl", 3);
    let candidate = files
        .iter()
        .find(|path| head(path, 8 * 1024).contains(cwd))
        .or_else(|| files.first())?;
    // `rollout-2026-01-01T00-00-00-<uuid>.jsonl`, or the id at the head of the
    // rollout when the file name carries another shape.
    let name = candidate.file_name()?.to_string_lossy();
    uuid_in(&name).or_else(|| {
        let head = head(candidate, 8 * 1024);
        json_value(&head, "id").filter(|id| id.len() >= 8)
    })
}

/// Every rollout under `sessions`, newest first.
fn codex_sessions_at(sessions: &Path) -> Vec<(String, SystemTime)> {
    files_sessions(sessions, "jsonl", 3)
}

/// The root Grok keeps its session store under.
pub(super) fn grok_home() -> Option<PathBuf> {
    env_path("GROK_HOME").or_else(|| home().map(|home| home.join(".grok")))
}

fn grok_session(cwd: &str) -> Option<String> {
    grok_session_at(&grok_home()?, cwd)
}

/// The newest session Grok recorded for `cwd`: a session directory under the
/// encoded working directory, named for the session id.
pub(super) fn grok_session_at(root: &Path, cwd: &str) -> Option<String> {
    // The directory's own name is the id, so nothing inside it has to be read
    // to reopen the conversation.
    newest_grok_session(&grok_sessions_dir(root, cwd))?
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

/// The directory Grok keeps `cwd`'s sessions and prompt history under:
/// `sessions/<percent-encoded cwd>`.
pub(super) fn grok_sessions_dir(root: &Path, cwd: &str) -> PathBuf {
    root.join("sessions")
        .join(percent_encode_path(&normalized_cwd(cwd)))
}

/// The newest session directory of one encoded working directory, if it has
/// any.
///
/// A session's `updated_at` is when it last ran, which is what tells a resumed
/// session from a new one; a directory's mtime only moves when a file is
/// created or removed inside it, so it is the fallback for a session whose
/// summary was never written.
pub(super) fn newest_grok_session(dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    let mut newest: Option<(String, PathBuf)> = None;
    let mut newest_by_mtime: Option<(SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if newest_by_mtime
            .as_ref()
            .is_none_or(|(best, _)| modified > *best)
        {
            newest_by_mtime = Some((modified, path.clone()));
        }
        let updated = fs::read_to_string(path.join("summary.json"))
            .ok()
            .and_then(|summary| grok_updated_at(&summary));
        if let Some(updated) = updated {
            if newest.as_ref().is_none_or(|(best, _)| updated > *best) {
                newest = Some((updated, path));
            }
        }
    }
    newest
        .map(|(_, path)| path)
        .or(newest_by_mtime.map(|(_, path)| path))
}

/// When a Grok session last ran, as its `summary.json` records it. The stamps
/// are one writer's RFC 3339 UTC strings, so they order as written.
fn grok_updated_at(summary: &str) -> Option<String> {
    let summary: Value = serde_json::from_str(summary).ok()?;
    ["updated_at", "last_active_at"]
        .iter()
        .find_map(|key| summary.get(key).and_then(Value::as_str))
        .map(str::to_string)
}

/// How close a main session has to be to a subagent's for the main one to lead:
/// a fork runs alongside the session it was spawned from.
const GROK_MAIN_WINDOW: std::time::Duration = std::time::Duration::from_secs(600);

/// Every session Grok ran in `cwd`, newest first.
///
/// A `subagent`/`subagent_resume` session is a fork of a run in the same
/// directory rather than a conversation the user started, so a main session
/// that ran within [`GROK_MAIN_WINDOW`] of the newest one is listed ahead of it.
/// The forks stay in the list: a conversation that owns one still reopens it.
fn grok_sessions_at(root: &Path, cwd: &str) -> Vec<(String, SystemTime)> {
    let dir = grok_sessions_dir(root, cwd);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut sessions: Vec<(String, SystemTime, bool)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let summary = fs::read_to_string(path.join("summary.json")).unwrap_or_default();
        let when = grok_updated_at(&summary)
            .and_then(|stamp| session_time(&stamp))
            .unwrap_or_else(|| file_time(&path));
        let main = !matches!(
            grok_session_kind(&summary).as_deref(),
            Some("subagent" | "subagent_resume")
        );
        sessions.push((entry.file_name().to_string_lossy().into_owned(), when, main));
    }
    sessions.sort_by(|a, b| b.1.cmp(&a.1));
    if let Some(newest) = sessions.first().map(|session| session.1) {
        // A stable sort keeps the newest-first order inside each group.
        sessions.sort_by_key(|(_, when, main)| {
            let close = newest.duration_since(*when).unwrap_or_default() <= GROK_MAIN_WINDOW;
            !(*main && close)
        });
    }
    sessions
        .into_iter()
        .map(|(id, when, _)| (id, when))
        .collect()
}

/// The kind of session Grok's `summary.json` records, when it records one.
fn grok_session_kind(summary: &str) -> Option<String> {
    let summary: Value = serde_json::from_str(summary).ok()?;
    summary
        .get("session_kind")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// The working directory a store names, freed of the shape the app's UI gives
/// it: `~` is the home directory again, and no path carries a trailing slash.
fn normalized_cwd(cwd: &str) -> String {
    let expanded = match cwd.trim() {
        "~" => home()
            .map(|home| home.to_string_lossy().into_owned())
            .unwrap_or_else(|| cwd.trim().to_string()),
        path => match path.strip_prefix("~/") {
            Some(rest) => home()
                .map(|home| format!("{}/{}", home.to_string_lossy().trim_end_matches('/'), rest))
                .unwrap_or_else(|| path.to_string()),
            None => path.to_string(),
        },
    };
    let trimmed = expanded.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

/// A working directory as a session store names its directory: the URL
/// unreserved set is kept and every other byte of the UTF-8 path is `%XX`.
/// `/Users/me/Projects/goble` becomes `%2FUsers%2Fme%2FProjects%2Fgoble`.
fn percent_encode_path(cwd: &str) -> String {
    const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut encoded = String::with_capacity(cwd.len());
    for byte in cwd.as_bytes() {
        if UNRESERVED.contains(byte) {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// The root Antigravity keeps its CLI profile under.
pub(super) fn antigravity_home() -> Option<PathBuf> {
    env_path("ANTIGRAVITY_HOME")
        .or_else(|| home().map(|home| home.join(".gemini").join("antigravity-cli")))
}

fn antigravity_session(cwd: &str) -> Option<String> {
    antigravity_session_at(&antigravity_home()?, cwd)
}

/// The conversation Antigravity last used for `cwd`.
///
/// `cache/last_conversations.json` records the conversation each directory was
/// last opened with; a directory the cache does not know is looked up in the
/// summaries database, whose rows carry the workspace as a list of `file://`
/// URIs.
pub(super) fn antigravity_session_at(root: &Path, cwd: &str) -> Option<String> {
    // An empty directory names no workspace: read as `/` it would match the
    // `file:///…` of every project in the summaries database.
    if cwd.trim().is_empty() {
        return None;
    }
    let cwd = normalized_cwd(cwd);
    if let Some(id) = antigravity_last_conversation(root, &cwd) {
        return Some(id);
    }
    let database = root.join("conversation_summaries.db");
    if !database.exists() {
        return None;
    }
    // `instr` is a plain substring test: the directory can hold `_` and `%`,
    // which a LIKE pattern would read as wildcards.
    let query = format!(
        "select conversation_id from conversation_summaries \
         where instr(workspace_uris, '{}') > 0 \
         order by last_modified_time desc limit 1;",
        sql_literal(&format!("file://{cwd}"))
    );
    first_line(&sqlite_query(&database, &query, true)?)
}

/// The conversation `cache/last_conversations.json` records for `cwd`.
fn antigravity_last_conversation(root: &Path, cwd: &str) -> Option<String> {
    let text = fs::read_to_string(root.join("cache").join("last_conversations.json")).ok()?;
    let conversations: Value = serde_json::from_str(&text).ok()?;
    conversations
        .get(cwd)
        .and_then(Value::as_str)
        .and_then(non_empty)
}

/// Every conversation Antigravity's summaries database records for `cwd`,
/// newest first.
///
/// The cache names the directory's last conversation in one column with no
/// timestamp of its own, so the record's mtime stands for it; the database
/// carries a row per conversation with `last_modified_time`, which is what
/// tells two conversations of one directory apart.
fn antigravity_sessions_at(root: &Path, cwd: &str) -> Vec<(String, SystemTime)> {
    if cwd.trim().is_empty() {
        return Vec::new();
    }
    let cwd = normalized_cwd(cwd);
    let mut sessions: Vec<(String, SystemTime)> = Vec::new();
    let cache = root.join("cache").join("last_conversations.json");
    if let Some(id) = antigravity_last_conversation(root, &cwd) {
        sessions.push((id, file_time(&cache)));
    }
    let database = root.join("conversation_summaries.db");
    if database.exists() {
        let query = format!(
            "select conversation_id, last_modified_time from conversation_summaries \
             where instr(workspace_uris, '{}') > 0 \
             order by last_modified_time desc;",
            sql_literal(&format!("file://{cwd}"))
        );
        if let Some(answer) = sqlite_query(&database, &query, true) {
            for line in answer.lines() {
                let mut columns = line.split('|');
                let (Some(id), Some(stamp)) = (columns.next(), columns.next()) else {
                    continue;
                };
                let Some(id) = non_empty(id) else { continue };
                let when = session_time(stamp).unwrap_or_else(|| file_time(&database));
                sessions.push((id, when));
            }
        }
    }
    newest_first(&mut sessions);
    sessions
}

fn non_empty(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn first_line(text: &str) -> Option<String> {
    text.lines().next().and_then(non_empty)
}

/// `text` as a single SQL string literal: a quote inside it is doubled, which
/// is how SQLite spells one.
fn sql_literal(text: &str) -> String {
    text.replace('\'', "''")
}

/// The first column of the first row `query` answers, `None` when the query
/// fails or answers nothing.
///
/// Reading a store another process owns passes `readonly`, so a lookup never
/// opens the CLI's database for writing — `sqlite3` recovers and checkpoints a
/// write connection, and goble is only a reader here. Neither mode links a
/// database engine: macOS ships `sqlite3` and the CLIs' own stores are SQLite.
pub(super) fn sqlite_query(database: &Path, query: &str, readonly: bool) -> Option<String> {
    let mut command = Command::new("sqlite3");
    if readonly {
        command.arg("-readonly");
    }
    let output = command.arg(database).arg(query).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn agent_sessions_roots(env_agent: &str, env_sessions: &str, default: PathBuf) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(dir) = env_path(env_sessions) {
        roots.push(dir);
    }
    if let Some(dir) = env_path(env_agent) {
        roots.push(dir.join("sessions"));
    }
    roots.push(default);
    roots
}

fn jsonl_session(root: &Path, cwd: &str) -> Option<String> {
    let files = files_newest_first(root, "jsonl", 2);
    let candidate = files
        .iter()
        .find(|path| head(path, 8 * 1024).contains(cwd))
        .or_else(|| files.first())?;
    let head = head(candidate, 8 * 1024);
    json_value(&head, "sessionId")
        .or_else(|| json_value(&head, "session_id"))
        .or_else(|| json_value(&head, "id"))
        .or_else(|| {
            candidate
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
}

fn pi_session(cwd: &str) -> Option<String> {
    pi_roots().iter().find_map(|root| jsonl_session(root, cwd))
}

fn omp_session(cwd: &str) -> Option<String> {
    omp_roots().iter().find_map(|root| jsonl_session(root, cwd))
}

/// The directories Pi keeps its session files under.
pub(super) fn pi_roots() -> Vec<PathBuf> {
    let default = home()
        .map(|home| home.join(".pi").join("agent").join("sessions"))
        .unwrap_or_default();
    agent_sessions_roots(
        "PI_CODING_AGENT_DIR",
        "PI_CODING_AGENT_SESSION_DIR",
        default,
    )
}

/// The directories OMP keeps its session files under.
pub(super) fn omp_roots() -> Vec<PathBuf> {
    let Some(home) = home() else {
        return Vec::new();
    };
    let mut roots = agent_sessions_roots(
        "PI_CODING_AGENT_DIR",
        "PI_CODING_AGENT_SESSION_DIR",
        home.join(".omp").join("agent").join("sessions"),
    );
    // OMP keeps its own profile root as well as the Pi-compatible one.
    roots.push(home.join(".omp").join("sessions"));
    roots
}

/// Every session file under the roots, newest first.
fn jsonl_sessions_at(roots: &[PathBuf]) -> Vec<(String, SystemTime)> {
    let mut sessions: Vec<(String, SystemTime)> = roots
        .iter()
        .flat_map(|root| files_sessions(root, "jsonl", 2))
        .collect();
    newest_first(&mut sessions);
    sessions
}

/// The data root OpenCode stores its sessions under.
pub(super) fn data_home() -> Option<PathBuf> {
    env_path("XDG_DATA_HOME").or_else(|| home().map(|home| home.join(".local").join("share")))
}

fn opencode_session(cwd: &str) -> Option<String> {
    opencode_session_at(&data_home()?, cwd)
}

/// [`opencode_session`] against a data root passed in.
fn opencode_session_at(data: &Path, cwd: &str) -> Option<String> {
    let root = data.join("opencode").join("storage").join("session");
    let files = files_newest_first(&root, "json", 2);
    let candidate = files
        .iter()
        .find(|path| head(path, 16 * 1024).contains(cwd))
        .or_else(|| files.first())?;
    candidate
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
}

/// Every session record under the storage tree, newest first.
fn opencode_sessions_at(data: &Path) -> Vec<(String, SystemTime)> {
    files_sessions(
        &data.join("opencode").join("storage").join("session"),
        "json",
        2,
    )
}

/// The root Hermes keeps its state database under.
pub(super) fn hermes_home() -> Option<PathBuf> {
    env_path("HERMES_HOME").or_else(|| home().map(|home| home.join(".hermes")))
}

fn hermes_session() -> Option<String> {
    hermes_session_at(&hermes_home()?)
}

/// [`hermes_session`] against a Hermes home passed in.
fn hermes_session_at(base: &Path) -> Option<String> {
    let database = base.join("state.db");
    if !database.exists() {
        return None;
    }
    // The store is SQLite; the CLI's own query keeps goble from linking a
    // second database engine just to read one column.
    let answer = sqlite_query(
        &database,
        "select id from sessions where source = 'cli' order by started_at desc limit 1;",
        false,
    )?;
    first_line(&answer)
}

/// Hermes's own newest CLI session. Its database answers one row per session,
/// and a card only needs the one the directory last ran, so the list is one
/// entry long.
fn hermes_sessions_at(base: &Path) -> Vec<(String, SystemTime)> {
    let database = base.join("state.db");
    if !database.exists() {
        return Vec::new();
    }
    let answer = sqlite_query(
        &database,
        "select id, started_at from sessions where source = 'cli' \
         order by started_at desc limit 1;",
        false,
    );
    let Some(answer) = answer else {
        return Vec::new();
    };
    let mut columns = answer.lines().next().unwrap_or_default().split('|');
    let (Some(id), started_at) = (columns.next().and_then(non_empty), columns.next()) else {
        return Vec::new();
    };
    let when = started_at
        .and_then(session_time)
        .unwrap_or_else(|| file_time(&database));
    vec![(id, when)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uuid_is_read_from_a_rollout_name() {
        assert_eq!(
            uuid_in("rollout-2026-01-02T03-04-05-0192f3a4-1111-2222-3333-444455556666.jsonl"),
            Some("0192f3a4-1111-2222-3333-444455556666".to_string())
        );
    }

    #[test]
    fn a_json_string_field_is_read_without_a_parser() {
        let text = r#"{"type":"session","id":"abc-123","sessionId": "s-9"}"#;
        assert_eq!(json_value(text, "sessionId").as_deref(), Some("s-9"));
        assert_eq!(json_value(text, "id").as_deref(), Some("abc-123"));
    }

    #[test]
    fn claude_project_dirs_encode_every_non_alphanumeric_character() {
        assert_eq!(
            claude_project_dir("/Users/me/Projects/goble"),
            "-Users-me-Projects-goble"
        );
    }

    /// A session file with a fixed mtime, so "newest" is the test's choice and
    /// not the filesystem's.
    fn session_file(path: &Path, contents: &str, modified: SystemTime) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create the session store directory");
        }
        fs::write(path, contents).expect("write the session file");
        fs::File::options()
            .write(true)
            .open(path)
            .expect("open the session file")
            .set_modified(modified)
            .expect("set the session file's mtime");
    }

    fn seconds_after_epoch(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(seconds)
    }

    // -- Claude Code --------------------------------------------------------

    #[test]
    fn claude_reads_the_newest_transcript_of_the_conversations_project() {
        let dir = tempfile::tempdir().expect("create a temp config dir");
        let config = dir.path();
        let cwd = "/Users/me/Projects/goble";
        let project = config.join("projects").join(claude_project_dir(cwd));
        session_file(
            &project.join("0192f3a4-1111-2222-3333-444455556666.jsonl"),
            "{}",
            seconds_after_epoch(1_000),
        );
        session_file(
            &project.join("aaaa1111-2222-3333-4444-555566667777.jsonl"),
            "{}",
            seconds_after_epoch(2_000),
        );
        // A newer transcript for another directory must not be resumed: the
        // project directory is what scopes the conversation.
        let other = config
            .join("projects")
            .join(claude_project_dir("/Users/me/other"));
        session_file(
            &other.join("bbbb1111-2222-3333-4444-555566667777.jsonl"),
            "{}",
            seconds_after_epoch(3_000),
        );

        assert_eq!(
            claude_session_at(config, cwd).as_deref(),
            Some("aaaa1111-2222-3333-4444-555566667777")
        );
    }

    #[test]
    fn claude_has_no_session_for_a_directory_it_never_ran_in() {
        let dir = tempfile::tempdir().expect("create a temp config dir");
        assert_eq!(claude_session_at(dir.path(), "/Users/me/other"), None);
    }

    // -- Codex --------------------------------------------------------------

    #[test]
    fn codex_prefers_the_rollout_that_names_the_working_directory() {
        let dir = tempfile::tempdir().expect("create a temp codex home");
        let sessions = dir.path().join("sessions");
        let cwd = "/Users/me/Projects/goble";
        session_file(
            &sessions
                .join("2026")
                .join("01")
                .join("01")
                .join("rollout-2026-01-01T00-00-00-0192f3a4-1111-2222-3333-444455556666.jsonl"),
            &format!(r#"{{"type":"session","cwd":"{cwd}"}}"#),
            seconds_after_epoch(1_000),
        );
        session_file(
            &sessions
                .join("rollout-2026-01-02T00-00-00-aaaa1111-2222-3333-4444-555566667777.jsonl"),
            r#"{"type":"session","cwd":"/tmp/elsewhere"}"#,
            seconds_after_epoch(2_000),
        );

        assert_eq!(
            codex_session_at(&sessions, cwd).as_deref(),
            Some("0192f3a4-1111-2222-3333-444455556666"),
            "the rollout that ran in the directory wins over a newer unrelated one"
        );
    }

    #[test]
    fn codex_falls_back_to_the_id_at_the_head_of_a_rollout_without_a_uuid_name() {
        let dir = tempfile::tempdir().expect("create a temp codex home");
        let sessions = dir.path().join("sessions");
        session_file(
            &sessions.join("rollout-latest.jsonl"),
            r#"{"type":"session","id":"sess-1234567890"}"#,
            seconds_after_epoch(1_000),
        );

        assert_eq!(
            codex_session_at(&sessions, "/Users/me/Projects/goble").as_deref(),
            Some("sess-1234567890")
        );
    }

    #[test]
    fn codex_has_no_session_without_a_rollout() {
        let dir = tempfile::tempdir().expect("create a temp codex home");
        assert_eq!(codex_session_at(&dir.path().join("sessions"), "/tmp"), None);
    }

    // -- Grok ---------------------------------------------------------------

    #[test]
    fn a_grok_working_directory_is_one_percent_encoded_path_segment() {
        assert_eq!(
            percent_encode_path("/Users/me/Projects/goble"),
            "%2FUsers%2Fme%2FProjects%2Fgoble"
        );
        // Bytes outside the URL unreserved set are encoded, and a space does not
        // split the directory in two.
        assert_eq!(
            percent_encode_path("/Users/me/My Notes/v1.2_x~"),
            "%2FUsers%2Fme%2FMy%20Notes%2Fv1.2_x~"
        );
    }

    #[test]
    fn grok_reads_the_newest_session_of_the_working_directory() {
        let dir = tempfile::tempdir().expect("create a temp grok home");
        let root = dir.path();
        let cwd = "/Users/me/Projects/goble";
        let sessions = grok_sessions_dir(root, cwd);
        let older = sessions.join("0192f3a4-1111-2222-3333-444455556666");
        let newer = sessions.join("aaaa1111-2222-3333-4444-555566667777");
        // The mtimes point the other way: what says when a session last ran is
        // its own `updated_at`, not when its directory was last touched.
        session_file(
            &older.join("summary.json"),
            r#"{"updated_at":"2026-01-01T00:00:00.000000Z"}"#,
            seconds_after_epoch(2_000),
        );
        session_file(
            &newer.join("summary.json"),
            r#"{"updated_at":"2026-01-02T00:00:00.000000Z"}"#,
            seconds_after_epoch(1_000),
        );
        // A session for another directory is newer still, and is not this one's.
        session_file(
            &grok_sessions_dir(root, "/Users/me/other")
                .join("bbbb1111-2222-3333-4444-555566667777/summary.json"),
            r#"{"updated_at":"2026-01-03T00:00:00.000000Z"}"#,
            seconds_after_epoch(3_000),
        );

        assert_eq!(
            grok_session_at(root, cwd).as_deref(),
            Some("aaaa1111-2222-3333-4444-555566667777"),
            "the session that ran last is the one to resume"
        );
    }

    #[test]
    fn grok_finds_a_session_whose_summary_was_never_written() {
        let dir = tempfile::tempdir().expect("create a temp grok home");
        let root = dir.path();
        let cwd = "/Users/me/Projects/goble";
        let session = grok_sessions_dir(root, cwd).join("0192f3a4-1111-2222-3333-444455556666");
        session_file(
            &session.join("chat_history.jsonl"),
            "{}",
            seconds_after_epoch(1_000),
        );

        assert_eq!(
            grok_session_at(root, cwd).as_deref(),
            Some("0192f3a4-1111-2222-3333-444455556666")
        );
    }

    #[test]
    fn grok_has_no_session_for_a_directory_it_never_ran_in() {
        let dir = tempfile::tempdir().expect("create a temp grok home");
        assert_eq!(
            grok_session_at(dir.path(), "/Users/me/Projects/goble"),
            None
        );
    }

    // -- Antigravity --------------------------------------------------------

    #[test]
    fn antigravity_reads_the_conversation_its_cache_records_for_the_directory() {
        let dir = tempfile::tempdir().expect("create a temp antigravity home");
        let root = dir.path();
        let cwd = "/Users/me/Projects/goble";
        session_file(
            &root.join("cache").join("last_conversations.json"),
            &format!(r#"{{"{cwd}": "conv-1", "/Users/me/other": "conv-2"}}"#),
            seconds_after_epoch(1_000),
        );

        assert_eq!(antigravity_session_at(root, cwd).as_deref(), Some("conv-1"));
        assert_eq!(
            antigravity_session_at(root, "/Users/me/other").as_deref(),
            Some("conv-2")
        );
        assert_eq!(antigravity_session_at(root, "/Users/me/third"), None);
        assert_eq!(
            antigravity_session_at(root, ""),
            None,
            "an unknown directory names no conversation"
        );
    }

    #[test]
    fn antigravity_falls_back_to_its_summaries_database() {
        if !sqlite3_available() {
            eprintln!("sqlite3 is not installed; skipping the antigravity database case");
            return;
        }
        let dir = tempfile::tempdir().expect("create a temp antigravity home");
        let root = dir.path();
        session_file(
            &root.join("cache").join("last_conversations.json"),
            "{}",
            seconds_after_epoch(1_000),
        );
        let database = root.join("conversation_summaries.db");
        let status = Command::new("sqlite3")
            .arg(&database)
            .arg(
                "create table conversation_summaries (conversation_id text primary key, \
                 title text, preview text, workspace_uris text, last_modified_time text);\
                 insert into conversation_summaries values ('conv-old', 'Old title', 'Old preview', \
                 '[\"file:///Users/me/Projects/goble\"]', '2026-01-01 00:00:00+00:00');\
                 insert into conversation_summaries values ('conv-new', 'New title', 'New preview', \
                 '[\"file:///Users/me/Projects/goble\"]', '2026-01-02 00:00:00+00:00');\
                 insert into conversation_summaries values ('conv-other', 'Other title', 'Other preview', \
                 '[\"file:///Users/me/Projects/other\"]', '2026-01-03 00:00:00+00:00');",
            )
            .status()
            .expect("run sqlite3");
        assert!(status.success(), "sqlite3 built the summaries store");

        assert_eq!(
            antigravity_session_at(root, "/Users/me/Projects/goble").as_deref(),
            Some("conv-new"),
            "the last conversation recorded for the directory is the one to resume"
        );
        assert_eq!(
            antigravity_session_at(root, "/Users/me/Projects/none"),
            None
        );
        assert_eq!(
            antigravity_session_at(root, ""),
            None,
            "an empty directory does not match every workspace in the database"
        );
    }

    // -- The JSONL stores (Pi and OMP) ---------------------------------------

    #[test]
    fn jsonl_session_reads_the_id_the_transcript_records() {
        let dir = tempfile::tempdir().expect("create a temp sessions root");
        let root = dir.path();
        let cwd = "/Users/me/Projects/goble";
        session_file(
            &root.join("2026").join("a.jsonl"),
            &format!(r#"{{"sessionId":"s-1","cwd":"{cwd}"}}"#),
            seconds_after_epoch(1_000),
        );
        // A newer transcript for another directory is not the conversation.
        session_file(
            &root.join("2026").join("b.jsonl"),
            r#"{"sessionId":"s-2","cwd":"/tmp/elsewhere"}"#,
            seconds_after_epoch(2_000),
        );

        assert_eq!(jsonl_session(root, cwd).as_deref(), Some("s-1"));
    }

    #[test]
    fn jsonl_session_also_reads_the_snake_case_field() {
        let dir = tempfile::tempdir().expect("create a temp sessions root");
        session_file(
            &dir.path().join("a.jsonl"),
            r#"{"session_id":"s-3"}"#,
            seconds_after_epoch(1_000),
        );

        assert_eq!(jsonl_session(dir.path(), "/tmp").as_deref(), Some("s-3"));
    }

    #[test]
    fn jsonl_session_falls_back_to_the_file_name() {
        let dir = tempfile::tempdir().expect("create a temp sessions root");
        session_file(
            &dir.path().join("abc-123.jsonl"),
            r#"{"type":"session"}"#,
            seconds_after_epoch(1_000),
        );

        assert_eq!(
            jsonl_session(dir.path(), "/tmp").as_deref(),
            Some("abc-123")
        );
    }

    // -- OpenCode -----------------------------------------------------------

    #[test]
    fn opencode_reads_the_newest_session_of_the_conversations_project() {
        let dir = tempfile::tempdir().expect("create a temp data dir");
        let data = dir.path();
        let root = data.join("opencode").join("storage").join("session");
        let cwd = "/Users/me/Projects/goble";
        session_file(
            &root.join("proj_a").join("ses_aaa.json"),
            &format!(r#"{{"directory":"{cwd}"}}"#),
            seconds_after_epoch(1_000),
        );
        session_file(
            &root.join("proj_b").join("ses_bbb.json"),
            r#"{"directory":"/tmp/elsewhere"}"#,
            seconds_after_epoch(2_000),
        );

        assert_eq!(
            opencode_session_at(data, cwd).as_deref(),
            Some("ses_aaa"),
            "the session whose file names the directory wins"
        );
    }

    #[test]
    fn opencode_has_no_session_without_its_storage_tree() {
        let dir = tempfile::tempdir().expect("create a temp data dir");
        assert_eq!(opencode_session_at(dir.path(), "/tmp"), None);
    }

    // -- Hermes -------------------------------------------------------------

    #[test]
    fn a_conversation_resumes_the_session_it_owns() {
        let spec = goble_core::harness::harness_by_id("grok").expect("grok is in the catalog");
        assert_eq!(
            resume_command_for(spec, "/nonexistent-goble-test-dir", Some("session-a")),
            "grok --resume session-a",
            "the conversation's own session is the one reopened"
        );
        assert_eq!(
            resume_command_for(spec, "/nonexistent-goble-test-dir", None),
            "grok",
            "a conversation with no session launches the harness"
        );
        assert_eq!(
            resume_command_for(spec, "/nonexistent-goble-test-dir", Some("   ")),
            "grok",
            "a blank id names no session"
        );
    }

    #[test]
    fn grok_lists_its_sessions_newest_first_and_leads_with_a_main_session() {
        let dir = tempfile::tempdir().expect("create a temp grok home");
        let root = dir.path();
        let cwd = "/Users/me/Projects/goble";
        let sessions = grok_sessions_dir(root, cwd);
        for (id, kind, updated) in [
            ("older-main", "main", "2026-11-10T22:07:00.000000Z"),
            ("main", "main", "2026-11-10T23:42:00.000000Z"),
            ("fork", "subagent_resume", "2026-11-10T23:47:00.000000Z"),
        ] {
            session_file(
                &sessions.join(id).join("summary.json"),
                &format!(
                    r#"{{"session_summary":"{id}","updated_at":"{updated}","session_kind":"{kind}"}}"#
                ),
                seconds_after_epoch(1_000),
            );
        }

        let listed = grok_sessions_at(root, cwd);
        assert_eq!(
            listed.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
            vec!["main", "fork", "older-main"],
            "a fork of the same run follows the main session it was spawned from"
        );
        assert_eq!(
            listed[0].1,
            session_time("2026-11-10T23:42:00.000000Z").expect("a parseable stamp"),
            "the time is the session's own updated_at"
        );
    }

    #[test]
    fn antigravity_lists_the_conversations_its_database_records_for_the_directory() {
        if !sqlite3_available() {
            eprintln!("sqlite3 is not installed; skipping the antigravity list case");
            return;
        }
        let dir = tempfile::tempdir().expect("create a temp antigravity home");
        let root = dir.path();
        let cwd = "/Users/me/Projects/goble";
        session_file(
            &root.join("cache").join("last_conversations.json"),
            &format!(r#"{{"{cwd}": "conv-b"}}"#),
            seconds_after_epoch(1_000),
        );
        let status = Command::new("sqlite3")
            .arg(root.join("conversation_summaries.db"))
            .arg(
                "create table conversation_summaries (conversation_id text primary key, \
                 title text, preview text, workspace_uris text, last_modified_time text);\
                 insert into conversation_summaries values ('conv-a', 'First', '', \
                 '[\"file:///Users/me/Projects/goble\"]', '2026-11-10 22:07:00.000000+00:00');\
                 insert into conversation_summaries values ('conv-b', 'Second', '', \
                 '[\"file:///Users/me/Projects/goble\"]', '2026-11-10 23:06:00.000000+00:00');",
            )
            .status()
            .expect("run sqlite3");
        assert!(status.success(), "sqlite3 built the summaries store");

        let listed = antigravity_sessions_at(root, cwd);
        assert_eq!(
            listed.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
            vec!["conv-b", "conv-a"],
            "both conversations of the directory are listed, newest first"
        );
    }

    fn sqlite3_available() -> bool {
        Command::new("sqlite3")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    #[test]
    fn hermes_has_no_session_without_its_database() {
        let dir = tempfile::tempdir().expect("create a temp hermes home");
        assert_eq!(hermes_session_at(dir.path()), None);
    }

    #[test]
    fn hermes_reads_the_newest_cli_session_from_its_database() {
        if !sqlite3_available() {
            eprintln!("sqlite3 is not installed; skipping the hermes database case");
            return;
        }
        let dir = tempfile::tempdir().expect("create a temp hermes home");
        let database = dir.path().join("state.db");
        let status = Command::new("sqlite3")
            .arg(&database)
            .arg(
                "create table sessions (id text, source text, started_at integer);\
                 insert into sessions values ('s-old', 'cli', 10);\
                 insert into sessions values ('s-new', 'cli', 20);\
                 insert into sessions values ('s-tui', 'tui', 30);",
            )
            .status()
            .expect("run sqlite3");
        assert!(status.success(), "sqlite3 built the session store");

        assert_eq!(
            hermes_session_at(dir.path()).as_deref(),
            Some("s-new"),
            "the newest cli session is the one to resume"
        );
    }

    // -- The resume/launch choice -------------------------------------------

    #[test]
    fn a_found_session_reopens_through_the_harness_template() {
        let spec = goble_core::harness::harness_by_id("claude").expect("claude is in the catalog");
        assert_eq!(
            resume_or_launch_with(spec, "/work", |_, _| Some("abc-123".to_string())),
            "claude --resume abc-123"
        );
    }

    #[test]
    fn a_missing_session_launches_the_harness_fresh() {
        let spec = goble_core::harness::harness_by_id("codex").expect("codex is in the catalog");
        assert_eq!(
            resume_or_launch_with(spec, "/work", |_, _| None),
            "codex",
            "there is nothing to resume, so the plain launch command runs"
        );
    }

    #[test]
    fn a_harness_without_a_resume_template_launches_even_when_an_id_is_found() {
        let spec = goble_core::harness::harness_by_id("gemini").expect("gemini is in the catalog");
        assert_eq!(
            resume_or_launch_with(spec, "/work", |_, _| Some("abc-123".to_string())),
            "gemini"
        );
    }

    #[test]
    fn a_harness_with_no_commands_has_nothing_to_run() {
        let spec = goble_core::harness::harness_by_id("goble").expect("goble is in the catalog");
        assert_eq!(resume_or_launch_with(spec, "/work", |_, _| None), "");
    }

    #[test]
    fn the_lookup_is_asked_about_the_harness_and_the_directory() {
        let spec = goble_core::harness::harness_by_id("hermes").expect("hermes is in the catalog");
        let asked = std::cell::RefCell::new(Vec::new());
        let line = resume_or_launch_with(spec, "/work", |harness, cwd| {
            asked
                .borrow_mut()
                .push((harness.to_string(), cwd.to_string()));
            None
        });
        assert_eq!(line, "hermes");
        assert_eq!(
            asked.borrow().as_slice(),
            [("hermes".to_string(), "/work".to_string())]
        );
    }
}
