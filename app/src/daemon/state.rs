//! The app's handle to the daemon: a local-embedded or remote [`DaemonClient`].
//!
//! The GUI is a thin client, so it never reaches into the daemon's internals.
//! It holds a [`DaemonModel`] that owns the *decision* of where a turn runs:
//! conversations routed `local` run on the embedded daemon the desktop service
//! owns (which also builds and registers the internal harness), while a
//! `remote` routing needs a genuinely remote client. There is no remote
//! transport wired yet, so a remote turn errors with a clear message **instead
//! of silently degrading to local** — surfacing the gap rather than pretending
//! the user got remote execution.

use std::sync::Arc;

use anyhow::Result;
use goble_core::harness::PaneSession;
use goble_daemon_client::DaemonClient;
use goble_desktop_service::DesktopState;
use goble_harness_types::{HarnessId, HarnessTurn, MediumId, ProjectId, SessionId};

use crate::features::{feature_enabled, FeatureFlag};
use crate::ui::WorkspaceRouting;

/// Routing decision + the client that backs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Local,
    Remote,
}

impl Route {
    /// Resolve a persisted routing choice to an execution route. `None` (the
    /// user has not chosen yet) defaults to local.
    fn resolve(routing: Option<WorkspaceRouting>) -> Self {
        match routing {
            Some(WorkspaceRouting::Remote) if feature_enabled(FeatureFlag::Daemon) => {
                Route::Remote
            }
            _ => Route::Local,
        }
    }
}

/// App-owned daemon seam.
///
/// `remote` is the client used when a conversation is routed `remote`. It is
/// `None` until a remote transport is wired, so `Route::Remote` turns fail
/// loudly instead of running on the embedded daemon. The embedded daemon is not
/// stored here: the desktop service owns it and its harness construction, so
/// local turns are delegated to [`DesktopState::run_chat_turn`].
pub struct DaemonModel {
    desktop: Arc<DesktopState>,
    remote: Option<Arc<dyn DaemonClient>>,
    /// The pane's own shell, when the turn's pane has a terminal. Passed to the
    /// desktop service so the harness's shell tool runs in that session instead
    /// of the sandbox; `None` keeps the sandboxed runner.
    pane_session: Option<Arc<dyn PaneSession>>,
}

impl DaemonModel {
    /// Wrap the desktop service (the composition root of the embedded daemon).
    /// The remote client is `None` until [`Self::with_remote`] supplies one.
    pub fn new(desktop: Arc<DesktopState>) -> Self {
        Self {
            desktop,
            remote: None,
            pane_session: None,
        }
    }

    /// Attach the pane's own shell for a turn running in an agent + terminal
    /// pane, so the agent's commands run in it (visibly, with the user's own
    /// environment) instead of the sandbox.
    pub fn with_pane_session(mut self, pane: Option<Arc<dyn PaneSession>>) -> Self {
        self.pane_session = pane;
        self
    }

    /// Attach a remote daemon client for `remote`-routed conversations.
    ///
    /// Only meaningful once a remote transport (e.g. the mTLS WebSocket codec
    /// behind the `daemon` cargo feature) is wired. Until then, callers that do
    /// not call this get a clear error on a remote turn.
    pub fn with_remote(mut self, remote: Arc<dyn DaemonClient>) -> Self {
        self.remote = Some(remote);
        self
    }

    /// The remote client, if one has been attached.
    pub fn remote_client(&self) -> Option<&Arc<dyn DaemonClient>> {
        self.remote.as_ref()
    }

    /// Run one chat turn on the conversation's configured route.
    ///
    /// Local/unspecified routes run on the embedded daemon via the desktop
    /// service. A remote route either drives the attached remote client (sending
    /// a turn for a harness the remote daemon registers) or, when no client is
    /// attached, errors — never silently running locally. `harness_id` names the
    /// harness the turn runs on (`None`/`internal` = the native goble harness).
    pub fn run_turn(
        &self,
        chat_id: &str,
        prompt: &str,
        provider: &str,
        model: &str,
        routing: Option<WorkspaceRouting>,
        medium_id: &str,
        project_id: &str,
        session_id: &str,
        workspace_dir: Option<&str>,
        harness_id: Option<&str>,
    ) -> Result<tokio::task::JoinHandle<()>> {
        match Route::resolve(routing) {
            Route::Local => self.desktop.run_chat_turn_with_pane_session(
                chat_id,
                prompt,
                provider,
                model,
                medium_id,
                project_id,
                session_id,
                workspace_dir,
                harness_id,
                self.pane_session.clone(),
            ),
            Route::Remote => {
                let client = self.remote.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "conversation {chat_id} is routed remote, but no remote daemon \
                         client is configured"
                    )
                })?;
                let turn = build_remote_turn(
                    chat_id,
                    prompt,
                    medium_id,
                    project_id,
                    session_id,
                    harness_id,
                );
                client.run(turn)?;
                Ok(spawn_waiter(client, SessionId::new(session_id)))
            }
        }
    }
}

/// Spawn a task that completes when the daemon streams `TraceFinished` for the
/// session. Mirrors the waiter the desktop service spawns for local turns so
/// both routes return a handle that resolves when the turn is done.
fn spawn_waiter(client: &Arc<dyn DaemonClient>, session_id: SessionId) -> tokio::task::JoinHandle<()> {
    use goble_daemon_protocol::DaemonEvent;
    let mut rx = client.subscribe();
    tokio::spawn(async move {
        while let Ok(ev) = rx.recv().await {
            if let DaemonEvent::TraceFinished { session_id: sid, .. } = &ev {
                if sid == &session_id {
                    break;
                }
            }
        }
    })
}

/// Build the harness turn for a remote-routed run, carrying the selected medium
/// and the project/session it is scoped to instead of the `local`/`default`
/// defaults. The session id is the tree-selected session (falls back to the
/// chat when the user has not picked one).
fn build_remote_turn(
    chat_id: &str,
    prompt: &str,
    medium_id: &str,
    project_id: &str,
    session_id: &str,
    harness_id: Option<&str>,
) -> HarnessTurn {
    let hid = harness_id
        .map(|id| HarnessId::new(id.to_string()))
        .unwrap_or_else(|| HarnessId::new(format!("internal-{chat_id}")));
    let mut turn = HarnessTurn::new(hid, SessionId::new(session_id), prompt.to_string());
    turn.medium_id = MediumId::new(medium_id);
    turn.project_id = ProjectId::new(project_id);
    turn
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_desktop_service::ThreadStore;

    fn desktop() -> Arc<DesktopState> {
        let dir = tempfile::tempdir().expect("tempdir");
        DesktopState::new(
            goble_core::store::Store::open_in_memory().expect("store"),
            ThreadStore::new(dir.path().to_path_buf()).expect("thread store"),
        )
    }

    #[test]
    fn remote_without_client_errors() {
        // Remote is attempted (daemon feature flag is on by default) and there
        // is no attached client, so it errors instead of running locally.
        let model = DaemonModel::new(desktop());
        let err = model.run_turn(
            "c1",
            "hi",
            "mock",
            "",
            Some(WorkspaceRouting::Remote),
            "local",
            "default",
            "c1",
            None,
            None,
        );
        assert!(err.is_err(), "remote with no client must not run locally");
    }

    #[test]
    fn remote_turn_carries_selected_medium_project_and_session() {
        // The turn the daemon receives for a remote run is built with the
        // selected medium/project/session identity (not the `local`/`default`
        // defaults) and the harness the user selected.
        let turn = build_remote_turn("c1", "hi", "vm", "projects/vm", "s-vm", Some("cli-grok"));
        assert_eq!(turn.medium_id, MediumId::new("vm"));
        assert_eq!(turn.project_id, ProjectId::new("projects/vm"));
        assert_eq!(turn.session_id, SessionId::new("s-vm"));
        assert_eq!(turn.harness_id, HarnessId::new("cli-grok"));
    }

    #[tokio::test]
    async fn local_runs_on_the_embedded_daemon() {
        // A local turn builds + registers an internal harness on the desktop's
        // embedded daemon and resolves the mock provider deterministically, so
        // its handle completes on `TraceFinished`.
        let state = desktop();
        let model = DaemonModel::new(Arc::clone(&state));
        let handle = model
            .run_turn(
                "c1",
                "Say hi",
                "mock",
                "",
                Some(WorkspaceRouting::Local),
                "local",
                "default",
                "c1",
                None,
                None,
            )
            .expect("local run should start");
        handle.await.expect("local turn should finish");
    }

    #[test]
    fn remote_client_is_exposed_after_with_remote() {
        let state = desktop();
        let client = state.daemon_client();
        let model = DaemonModel::new(Arc::clone(&state)).with_remote(client);
        assert!(model.remote_client().is_some());
    }
}
