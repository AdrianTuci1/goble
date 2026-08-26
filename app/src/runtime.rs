//! Runtime orchestration: decides where a conversation's turns execute.
//!
//! The harness engine lives in `goble-core` (via `goble-desktop-service`); this
//! app-owned module owns the *decision* — local vs remote, from the persisted
//! per-conversation routing — and drives it, mirroring warp-new's
//! `app/src/ai/{local_harness_setup,remote_executor}` split where the app owns
//! the runtime setup and the crates own the engine.

use std::sync::Arc;

use goble_desktop_service::DesktopState;

use crate::ui::WorkspaceRouting;

/// Run a chat turn on the conversation's configured target.
///
/// Only the local harness is wired today (`DesktopState::run_chat_turn`). A
/// `Remote` routing is persisted but remote chat execution is not implemented
/// yet, so it degrades to local and the gap is logged explicitly.
pub fn run_turn(
    desktop: &Arc<DesktopState>,
    chat_id: &str,
    prompt: &str,
    provider: &str,
    model: &str,
    routing: Option<WorkspaceRouting>,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    if routing == Some(WorkspaceRouting::Remote) {
        log::warn!(
            "conversation {chat_id} is routed remote, but remote chat execution is not wired yet; running locally"
        );
    }
    desktop.run_chat_turn(chat_id, prompt, provider, model)
}
