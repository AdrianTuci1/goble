use std::sync::Arc;

use super::DesktopState;

/// Convert goble-core's command decision into the wire decision the daemon
/// protocol carries.
fn command_decision_to_wire(
    decision: goble_core::harness::CommandDecision,
) -> goble_harness_types::CommandDecision {
    match decision {
        goble_core::harness::CommandDecision::Approve(text) => {
            goble_harness_types::CommandDecision::Approve(text)
        }
        goble_core::harness::CommandDecision::Edit(text) => {
            goble_harness_types::CommandDecision::Edit(text)
        }
        goble_core::harness::CommandDecision::Reject(reason) => {
            goble_harness_types::CommandDecision::Reject(reason)
        }
    }
}

/// Build the harness turn for a chat run, carrying the selected medium and the
/// project it is scoped to instead of the `local`/`default` defaults.
pub(super) fn build_chat_turn(
    harness_id: goble_harness_types::HarnessId,
    session_id: goble_harness_types::SessionId,
    prompt: &str,
    medium_id: &str,
    project_id: &str,
) -> goble_harness_types::HarnessTurn {
    let mut turn = goble_harness_types::HarnessTurn::new(harness_id, session_id, prompt);
    turn.medium_id = goble_harness_types::MediumId::new(medium_id);
    turn.project_id = goble_harness_types::ProjectId::new(project_id);
    turn
}

impl DesktopState {
    /// Run one conversational turn for a chat through the daemon and return a
    /// handle that completes when the turn has finished.
    ///
    /// Resolves the provider/model from the configured LLM setting (falling
    /// back to a deterministic `MockProvider` when a `mock` provider or no key
    /// is configured). For `None`/`"internal"` it wraps a real
    /// [`goble_core::harness::Harness`] behind the internal-harness seam,
    /// registers it in the daemon's harness registry, and drives the daemon —
    /// which owns the execution ledger, cancellation and event streaming. For
    /// any other `harness_id` it resolves an already-registered BYOH harness
    /// (registered via [`Self::register_harness`]) and runs the turn on it
    /// unchanged. The harness persists the user/assistant/tool messages into
    /// the store itself, and the daemon translator emits `chat:updated` after
    /// each event so the native UI re-reads the transcript.
    pub fn run_chat_turn(
        self: &Arc<Self>,
        chat_id: &str,
        prompt: &str,
        provider: &str,
        model: &str,
        medium_id: &str,
        project_id: &str,
        session_id: &str,
        workspace_dir: Option<&str>,
        harness_id: Option<&str>,
    ) -> anyhow::Result<tokio::task::JoinHandle<()>> {
        self.run_chat_turn_with_pane_session(
            chat_id,
            prompt,
            provider,
            model,
            medium_id,
            project_id,
            session_id,
            workspace_dir,
            harness_id,
            None,
        )
    }

    /// [`Self::run_chat_turn`] with the pane's own shell attached.
    ///
    /// When the app passes a `pane_session` — an agent + terminal pane's own
    /// session — the agent's shell tool runs there, in the user's own shell with
    /// the user's environment and credentials, instead of in the sandbox. That
    /// is a deliberate change of security posture (`agent-terminal-bridge.md`
    /// §5); a pane with no terminal passes `None` and keeps
    /// [`goble_core::harness::SandboxedCommandRunner`], so the sandbox path
    /// stays reachable.
    #[allow(clippy::too_many_arguments)]
    pub fn run_chat_turn_with_pane_session(
        self: &Arc<Self>,
        chat_id: &str,
        prompt: &str,
        provider: &str,
        model: &str,
        medium_id: &str,
        project_id: &str,
        session_id: &str,
        workspace_dir: Option<&str>,
        harness_id: Option<&str>,
        pane_session: Option<Arc<dyn goble_core::harness::PaneSession>>,
    ) -> anyhow::Result<tokio::task::JoinHandle<()>> {
        let auto_approve = self.get_auto_approve();
        let web_search = self.get_web_search_setting();
        // Resolve the harness the turn runs on. `None`/`internal` builds and
        // registers the native goble harness (historical behavior); any other id
        // names an already-registered BYOH harness the daemon drives unchanged.
        let resolved_id: goble_harness_types::HarnessId = match harness_id {
            None | Some("internal") => {
                let (llm, model_name) = self.resolve_llm_provider(provider, model);
                let store = self.store.lock().clone();
                let hid = goble_harness_types::HarnessId::new(format!("internal-{chat_id}"));
                let sandbox = Arc::new(
                    goble_core::harness::SandboxedCommandRunner::default_tools()
                        .with_sandbox(goble_core::harness::harness_sandbox()),
                );
                let runner = Arc::new(
                    goble_core::harness::RoutedCommandRunner::new(sandbox)
                        .with_pane_session(pane_session),
                );
                let mut internal = goble_harness_internal::InternalHarness::new(store)
                    .with_llm(llm)
                    .with_provider(provider)
                    .with_model(&model_name)
                    .with_reasoning(true)
                    .with_auto_approve(auto_approve)
                    .with_web_search(web_search)
                    .with_runner(runner)
                    .with_id(hid.clone());
                // Run the harness with the pane's own working directory when
                // provided (the per-session cwd), so commands execute there.
                if let Some(dir) = workspace_dir {
                    internal = internal.with_workspace_dir(dir);
                }
                self.daemon_state.register(Arc::new(internal));
                hid
            }
            Some(id) => {
                let hid = goble_harness_types::HarnessId::new(id);
                if !self.daemon.list_harnesses().iter().any(|h| h == &hid) {
                    anyhow::bail!("harness {id} is not registered");
                }
                hid
            }
        };
        self.ensure_translator();

        let sid = goble_harness_types::SessionId::new(session_id);
        self.daemon.run(build_chat_turn(
            resolved_id,
            sid.clone(),
            prompt,
            medium_id,
            project_id,
        ))?;

        let mut rx = self.daemon.subscribe();
        let handle = tokio::spawn(async move {
            while let Ok(ev) = rx.recv().await {
                if let goble_daemon_protocol::DaemonEvent::TraceFinished { session_id, .. } = &ev {
                    if session_id == &sid {
                        break;
                    }
                }
            }
        });
        Ok(handle)
    }

    /// Resume a chat turn that suspended waiting on a user answer. The daemon
    /// finds the still-live session for the chat, invokes the same harness's
    /// resume path (which resolves the pending ask in the store and re-runs the
    /// mission), and streams the same events as [`run_chat_turn`] so the UI
    /// keeps rendering inline. Provider/model are accepted for call-site
    /// compatibility; the resumed harness already carries its resolved LLM.
    pub fn resume_chat_turn(
        self: &Arc<Self>,
        chat_id: &str,
        response: &str,
        credential: Option<(String, String)>,
        _provider: &str,
        _model: &str,
    ) -> anyhow::Result<tokio::task::JoinHandle<()>> {
        self.ensure_translator();
        let sid = goble_harness_types::SessionId::new(chat_id);
        self.daemon.resume(&sid, response, credential)?;

        let mut rx = self.daemon.subscribe();
        let handle = tokio::spawn(async move {
            while let Ok(ev) = rx.recv().await {
                if let goble_daemon_protocol::DaemonEvent::TraceFinished { session_id, .. } = &ev {
                    if session_id == &sid {
                        break;
                    }
                }
            }
        });
        Ok(handle)
    }

    /// Answer a chat turn that suspended on a command proposal.
    ///
    /// The daemon finds the still-live session for the chat and runs the
    /// harness's command-resume path (approve/edit runs the chosen text; reject
    /// fails the call), streaming the same events as [`Self::run_chat_turn`] so
    /// the UI keeps rendering inline. The returned handle resolves when the
    /// resumed turn settles (its `TraceFinished`), so awaiting it cannot leave
    /// the session busy on a suspended command.
    pub fn resume_command_chat_turn(
        self: &Arc<Self>,
        chat_id: &str,
        decision: goble_core::harness::CommandDecision,
    ) -> anyhow::Result<tokio::task::JoinHandle<()>> {
        self.ensure_translator();
        let sid = goble_harness_types::SessionId::new(chat_id);
        // Subscribe before issuing the resume: a broadcast receiver only sees
        // events sent after it is created, so this ordering guarantees the
        // resumed turn's `TraceFinished` cannot slip past the listener. Issuing
        // the resume first would leave a fast turn free to settle before
        // `subscribe()`, and the handle would then wait forever.
        let mut rx = self.daemon.subscribe();
        self.daemon
            .resume_command(&sid, command_decision_to_wire(decision))?;

        let handle = tokio::spawn(async move {
            while let Ok(ev) = rx.recv().await {
                if let goble_daemon_protocol::DaemonEvent::TraceFinished { session_id, .. } = &ev {
                    if session_id == &sid {
                        break;
                    }
                }
            }
        });
        Ok(handle)
    }

    /// Request cancellation of a running chat turn started by [`run_chat_turn`].
    ///
    /// The daemon sets the session's cancel flag so the running turn
    /// yields/terminates, and returns whether a turn was actually in flight for
    /// this chat (i.e. the daemon still had a live session to cancel).
    pub fn cancel_chat_turn(&self, chat_id: &str) -> bool {
        self.daemon
            .cancel(&goble_harness_types::SessionId::new(chat_id))
            .is_ok()
    }
}
