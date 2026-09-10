//! Runtime orchestration: decides where a conversation's turns execute.
//!
//! The harness engine lives in the daemon (`goble-daemon`, embedded in the GUI
//! via the desktop service). This app-owned module owns the *decision* — local
//! vs remote, from the persisted per-conversation routing — and drives it
//! through [`DaemonModel`], mirroring warp-new's
//! `app/src/ai/{local_harness_setup,remote_executor}` split where the app owns
//! the runtime setup and the crates own the engine.

use std::sync::Arc;

use goble_desktop_service::DesktopState;

use crate::daemon::DaemonModel;
use crate::ui::WorkspaceRouting;

/// Run a chat turn on the conversation's configured target.
///
/// The [`DaemonModel`] resolves the persisted routing: `local` (or unset) runs
/// on the embedded daemon the desktop service owns; a `remote` routing either
/// drives an attached remote client or errors with a clear message — it never
/// silently degrades to a local run.
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
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    // Run the harness in the pane's own working directory when one is set,
    // else fall back to the harness default (process cwd).
    let workspace_dir = if cwd.is_empty() { None } else { Some(cwd) };
    DaemonModel::new(Arc::clone(desktop)).run_turn(
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
    )
}
