//! Runtime orchestration: decides where a conversation's turns execute.
//!
//! The harness engine lives in the daemon (`goble-daemon`, embedded in the GUI
//! via the desktop service). This app-owned module owns the *decision* — local
//! vs remote, from the persisted per-conversation routing — and drives it
//! through [`DaemonModel`], mirroring warp-new's
//! `app/src/ai/{local_harness_setup,remote_executor}` split where the app owns
//! the runtime setup and the crates own the engine.

use std::sync::Arc;

use goble_core::harness::PaneSession;
use goble_desktop_service::DesktopState;

use crate::daemon::{DaemonModel, TurnRequest};
use crate::ui::WorkspaceRouting;

/// Run a chat turn on the conversation's configured target.
///
/// The [`DaemonModel`] resolves the persisted routing: `local` (or unset) runs
/// on the embedded daemon the desktop service owns; a `remote` routing either
/// drives an attached remote client or errors with a clear message — it never
/// silently degrades to a local run.
///
/// `pane_session` is the pane's own shell when it has a terminal: the agent's
/// shell tool then runs there, in the user's own shell, instead of the sandbox.
/// A pane with no terminal passes `None` and keeps the sandboxed runner.
#[allow(clippy::too_many_arguments)]
pub fn run_turn(
    desktop: &Arc<DesktopState>,
    chat_id: &str,
    prompt: &str,
    provider: &str,
    model: &str,
    routing: Option<WorkspaceRouting>,
    medium_id: &str,
    project_id: &str,
    session_id: &str,
    cwd: &str,
    harness_id: Option<&str>,
    pane_session: Option<Arc<dyn PaneSession>>,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    run_turn_with_attachments(
        desktop,
        chat_id,
        prompt,
        provider,
        model,
        routing,
        medium_id,
        project_id,
        session_id,
        cwd,
        harness_id,
        pane_session,
        &[],
    )
}

/// [`run_turn`] carrying the files the composer attached to the draft.
///
/// The attachments are part of the turn's payload: the daemon seam turns them
/// into the context block the model receives (see [`with_attachments`]), and
/// the harness persists that block as the user message, so the transcript shows
/// exactly what was submitted. The LLM request itself is text-only
/// (`goble_core::llm::CompletionRequest` carries no file parts), so the block
/// names each path with its kind and size and the agent reads the contents with
/// its own file tools; binary/image contents are not handed to the provider.
#[allow(clippy::too_many_arguments)]
pub fn run_turn_with_attachments(
    desktop: &Arc<DesktopState>,
    chat_id: &str,
    prompt: &str,
    provider: &str,
    model: &str,
    routing: Option<WorkspaceRouting>,
    medium_id: &str,
    project_id: &str,
    session_id: &str,
    cwd: &str,
    harness_id: Option<&str>,
    pane_session: Option<Arc<dyn PaneSession>>,
    attachments: &[String],
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    // Run the harness in the pane's own working directory when one is set; a
    // pane with none gets a per-chat workspace under `~/.goble/worktrees`, so a
    // turn never writes into the directory the app was launched from.
    let workspace_dir = if cwd.is_empty() { None } else { Some(cwd) };
    DaemonModel::new(Arc::clone(desktop))
        .with_pane_session(pane_session)
        .run_turn(TurnRequest {
            chat_id,
            prompt,
            provider,
            model,
            routing,
            medium_id,
            project_id,
            session_id,
            workspace_dir,
            harness_id,
            attachments,
        })
}

/// The prompt a turn actually receives: the user's text plus the files the
/// composer attached to it.
///
/// The provider request is text-only, so each attachment is named by its path
/// with its kind and size. A file the app cannot stat is still named (marked
/// `missing`) rather than dropped: the payload records what the user attached.
/// An attachment-only draft still produces a prompt, so Send submits the files
/// instead of being a no-op.
pub(crate) fn with_attachments(prompt: &str, attachments: &[String]) -> String {
    if attachments.is_empty() {
        return prompt.to_string();
    }
    let mut block = String::new();
    for path in attachments {
        block.push_str("- ");
        block.push_str(&describe_attachment(path));
        block.push('\n');
    }
    let body = prompt.trim_end();
    if body.trim().is_empty() {
        format!("The user submitted these files with no message:\n{block}")
    } else {
        format!("{body}\n\nAttached files:\n{block}")
    }
}

/// One attachment line: its path, what it is and how big it is, so the model
/// can tell a text file it may read from a binary it cannot see.
fn describe_attachment(path: &str) -> String {
    let file = std::path::Path::new(path);
    match std::fs::metadata(file) {
        Ok(meta) if meta.is_dir() => format!("{path} (directory)"),
        Ok(meta) => format!(
            "{path} ({}, {})",
            attachment_kind(file),
            human_size(meta.len())
        ),
        Err(_) => format!("{path} (missing)"),
    }
}

/// Whether the file reads as text: valid UTF-8 with no NUL byte in its head. A
/// path that cannot be read is still named in the payload, so this only
/// decides the word after it.
fn attachment_kind(file: &std::path::Path) -> &'static str {
    const HEAD: usize = 4096;
    let Ok(mut handle) = std::fs::File::open(file) else {
        return "unreadable file";
    };
    use std::io::Read as _;
    let mut head = vec![0u8; HEAD];
    let Ok(read) = handle.read(&mut head) else {
        return "unreadable file";
    };
    let head = &head[..read];
    if head.contains(&0) || std::str::from_utf8(head).is_err() {
        "binary file"
    } else {
        "text file"
    }
}

fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_without_attachments_keeps_the_prompt_verbatim() {
        assert_eq!(with_attachments("hello", &[]), "hello");
        assert_eq!(with_attachments("", &[]), "");
    }

    #[test]
    fn a_draft_carries_each_attachment_as_a_named_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let text = dir.path().join("notes.md");
        std::fs::write(&text, "# notes\n").expect("write text file");
        let binary = dir.path().join("shot.png");
        std::fs::write(&binary, [0x89, b'P', b'N', b'G', 0, 1, 2]).expect("write binary file");
        let missing = dir.path().join("gone.txt");

        let prompt = with_attachments(
            "look at these",
            &[
                text.to_string_lossy().to_string(),
                binary.to_string_lossy().to_string(),
                missing.to_string_lossy().to_string(),
            ],
        );
        assert!(prompt.starts_with("look at these"), "{prompt}");
        assert!(prompt.contains("Attached files:"), "{prompt}");
        assert!(
            prompt.contains("notes.md (text file, 8 B)"),
            "a text file is named with its kind and size: {prompt}"
        );
        assert!(
            prompt.contains("shot.png (binary file, 7 B)"),
            "a binary is named as binary: {prompt}"
        );
        assert!(
            prompt.contains("gone.txt (missing)"),
            "a path that cannot be stat'ed is still recorded: {prompt}"
        );
        // Every attachment is its own line, so N files are N rows in the
        // payload the turn persists.
        assert_eq!(prompt.matches("\n- ").count(), 3, "{prompt}");
    }

    #[test]
    fn an_attachment_only_draft_still_makes_a_prompt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("only.txt");
        std::fs::write(&file, "only these").expect("write file");
        let prompt = with_attachments("   ", &[file.to_string_lossy().to_string()]);
        assert!(
            prompt.starts_with("The user submitted these files with no message:"),
            "{prompt}"
        );
        assert!(prompt.contains("only.txt (text file, 10 B)"), "{prompt}");
    }
}
