//! Viewer sessions: the pane a conversation runs on a remote worker gets.
//!
//! A conversation routed to a worker is not a terminal this machine owns — its
//! shell is the worker's. The pane that shows it is therefore a viewer pane
//! ([`crate::ui::PaneKind::Worker`]) with nothing local behind it, and its
//! connection state ([`PaneAttach`], held per pane in
//! [`UiState::pane_workers`]) is the whole of what the pane has to report. The
//! rule this module exists to keep: **a viewer pane never becomes a shell**. Not
//! when it is opened (the leaf is built as a conversation, and any local pty it
//! had is dropped), not when a command is typed at it, and not when the
//! connection drops.
//!
//! The second rule lives here too: the run outlives this client, so a pane that
//! opens on a conversation with a session — or one whose connection came back —
//! **re-attaches** to that session and replays its transcript
//! ([`UiState::attach_worker_session`]) rather than starting a fresh turn or
//! drawing an empty one. The transcript it draws is the session's own, and a
//! conversation with no session says so.

use goble_desktop_service::SessionAttach;

use super::*;

/// Whether a routing choice is one the app actually runs a turn on a worker
/// for.
///
/// The daemon seam is the only thing that honours `remote` — with its feature
/// flag off a `remote` choice runs locally (see `crate::daemon::DaemonModel`'s
/// own `Route::resolve`) — so a pane is only shaped as a viewer when a turn on
/// that conversation really runs on a worker. Mirroring the rule here keeps the
/// pane's shape and the turn's route from disagreeing.
fn routes_to_worker(routing: Option<WorkspaceRouting>) -> bool {
    matches!(routing, Some(WorkspaceRouting::Remote))
        && crate::features::feature_enabled(crate::features::FeatureFlag::Daemon)
}

impl UiState {
    /// The environment (medium) `pane_id`'s conversation runs its turns on,
    /// when that conversation is the one that chooses it.
    ///
    /// A conversation routed to a worker picks its environment where the prompt
    /// is submitted, so the pane draws the control beside the send affordance
    /// and the submitted turn carries the same medium. `None` for every other
    /// pane: a local conversation has no environment of its own — no control is
    /// drawn for it — and runs on the window's selected environment, which the
    /// caller already holds.
    pub fn pane_environment(&self, pane_id: u64) -> Option<String> {
        let environment = self.pane_controls.get(&pane_id)?.environment.clone();
        (!environment.is_empty()).then_some(environment)
    }

    /// Adopt the environment a conversation's turns run on.
    ///
    /// The conversation's own row is the choice ([`DesktopState::get_chat_medium`],
    /// written by the composer's environment control); while no choice has been
    /// made the pane holds the medium its routing implies, which is the one the
    /// turn carries either way, so the control never names an environment the
    /// work would not run on. A conversation that does not run on a worker keeps
    /// no environment: it has no control to choose with, and its turns run on
    /// the window's selected environment.
    pub fn adopt_environment(
        &mut self,
        pane_id: u64,
        conversation: &str,
        routing: Option<WorkspaceRouting>,
        desktop: &DesktopState,
    ) {
        let environment = if routes_to_worker(routing) && !conversation.is_empty() {
            desktop
                .get_chat_medium(conversation)
                .ok()
                .flatten()
                .filter(|medium| !medium.trim().is_empty())
                .unwrap_or_else(|| {
                    crate::media::medium_for_routing(WorkspaceRouting::Remote).to_string()
                })
        } else {
            String::new()
        };
        self.pane_controls_mut(pane_id).environment = environment;
    }

    /// Whether `pane_id` is a viewer pane: a conversation whose shell does not
    /// exist locally.
    pub fn pane_is_viewer(&self, pane_id: u64) -> bool {
        self.spaces
            .iter()
            .any(|space| matches!(space.leaf_kind(pane_id), Some(PaneKind::Worker)))
    }

    /// The viewer session of `pane_id`, if the pane is one.
    pub fn pane_worker(&self, pane_id: u64) -> Option<&WorkerSession> {
        self.pane_workers.get(&pane_id)
    }

    /// How `pane_id`'s connection stands, if the pane is a viewer pane.
    pub fn pane_attach(&self, pane_id: u64) -> Option<&PaneAttach> {
        self.pane_workers.get(&pane_id).map(|w| &w.attach)
    }

    /// Adopt the pane shape a conversation's routing asks for.
    ///
    /// A conversation routed to a worker is a viewer pane (`PaneKind::Worker`)
    /// and one routed locally is an ordinary chat pane. Only the conversation
    /// surfaces are shaped this way — a chat, terminal or viewer leaf; a file
    /// view or the settings tab is not a conversation and is left exactly as it
    /// is.
    ///
    /// This is the one path into (and out of) viewer shape, and it is where the
    /// worker a routing resolves to is resolved — once, on the way in. A pane
    /// that already holds a viewer session is left alone, so this is cheap to
    /// call on every refresh and never re-resolves (or reconnects) a pane that
    /// has one. A viewer leaf with no session — a layout restored from disk,
    /// whose connection state is runtime data and was never persisted — is
    /// resolved again.
    ///
    /// It is called where the app learns what a pane's conversation is routed
    /// to: the per-pane refresh ([`UiState::refresh_pane`]).
    ///
    /// Leaving viewer shape — the routing went back to local — gives the pane
    /// [`PaneKind::Chat`], never [`PaneKind::Terminal`]: the pane never had a
    /// shell of its own to return to, and a re-routed conversation is a local
    /// conversation, not a local terminal.
    pub fn adopt_routing(
        &mut self,
        pane_id: u64,
        routing: Option<WorkspaceRouting>,
        desktop: &DesktopState,
    ) {
        let kind = self
            .spaces
            .iter()
            .find_map(|space| space.leaf_kind(pane_id).cloned());
        let Some(kind) = kind else {
            return;
        };
        if !matches!(kind, PaneKind::Chat | PaneKind::Terminal | PaneKind::Worker) {
            return;
        }
        if routes_to_worker(routing) {
            if !self.pane_workers.contains_key(&pane_id) {
                // Which worker the routing resolves to is the pool's answer,
                // never this pane's guess. The routing is only `local` or
                // `remote` — no worker id rides with it and no tag is chosen
                // here — so this is the `any` target the app's other
                // pool-resolved runs use (`goble-desktop-native`'s scheduled
                // runs): round-robin over the paired workers. No worker is not a
                // local fallback: the pane is a viewer that says it could not
                // connect.
                match desktop.resolve_worker_for_target("any", None, None) {
                    Ok(worker_id) => {
                        self.open_worker_pane(pane_id, &worker_id.to_string());
                        // Opening the pane on a conversation that already has a
                        // session rejoins it and replays its transcript, instead
                        // of showing an empty one as if the work had not
                        // happened. Once per open: a pane that holds a viewer
                        // session is left alone, so a refresh is not a reconnect.
                        self.attach_worker_session(pane_id, desktop);
                    }
                    Err(e) => {
                        self.worker_failed(pane_id, &e.to_string());
                    }
                }
            }
        } else if matches!(kind, PaneKind::Worker) {
            self.close_worker_pane(pane_id);
        }
    }

    /// Open `pane_id` as a viewer session connected to `worker_id`.
    ///
    /// Nothing local is left behind: any shell the pane had is dropped here (its
    /// pty dies with it), the pane's harness switch is cleared and the pane
    /// reads no local cwd. A pane that is already a viewer keeps the session it
    /// has — the same pane refreshed is not a reconnect.
    pub fn open_worker_pane(&mut self, pane_id: u64, worker_id: &str) -> bool {
        if !self.viewer_shape(pane_id) {
            return false;
        }
        match self.pane_workers.get_mut(&pane_id) {
            Some(session) => {
                if session.worker_id.trim().is_empty() {
                    session.worker_id = worker_id.to_string();
                }
            }
            None => {
                self.pane_workers.insert(
                    pane_id,
                    WorkerSession {
                        worker_id: worker_id.to_string(),
                        attach: PaneAttach::Connecting,
                        session: None,
                    },
                );
            }
        }
        true
    }

    /// Open `pane_id` as a viewer session that could not connect: a `message`
    /// says why (no paired worker for the routing, or a refused connect).
    pub fn worker_failed(&mut self, pane_id: u64, message: &str) -> bool {
        if !self.viewer_shape(pane_id) {
            return false;
        }
        let worker_id = self
            .pane_workers
            .get(&pane_id)
            .map(|session| session.worker_id.clone())
            .unwrap_or_default();
        let session = self
            .pane_workers
            .get(&pane_id)
            .and_then(|session| session.session.clone());
        self.pane_workers.insert(
            pane_id,
            WorkerSession {
                worker_id,
                attach: PaneAttach::Failed {
                    message: message.to_string(),
                },
                session,
            },
        );
        true
    }

    /// Take `pane_id` out of viewer shape and back into a plain chat pane, and
    /// forget its session. Used when a conversation's routing goes back to
    /// local; the pane is a conversation surface again, never a terminal.
    pub fn close_worker_pane(&mut self, pane_id: u64) -> bool {
        if !self.pane_is_viewer(pane_id) {
            return false;
        }
        let mut shaped = false;
        for space in &mut self.spaces {
            if space.leaf_kind(pane_id) == Some(&PaneKind::Worker) {
                shaped |= space.set_leaf_kind(pane_id, PaneKind::Chat);
            }
        }
        self.pane_workers.remove(&pane_id);
        shaped
    }

    /// A viewer pane's worker answered: its panes rejoin the sessions they run
    /// on. A drop does not lose the run — the worker drives its own daemon — so
    /// the report is the re-attach path.
    ///
    /// A pane that never connected keeps its failure: a worker report is not a
    /// routing decision, and only [`UiState::adopt_routing`] resolves one.
    ///
    /// Returns how many panes ended up attached.
    pub fn worker_status_serving(&mut self, worker_id: &str, desktop: &DesktopState) -> usize {
        let panes = self.worker_panes(worker_id);
        let mut attached = 0;
        for pane_id in panes {
            // A pane that never connected keeps its failure: a worker report
            // does not resolve a routing this pane could not resolve.
            let before = self.pane_workers.get(&pane_id).map(|s| s.attach.clone());
            if matches!(before, Some(PaneAttach::Failed { .. }) | None) {
                continue;
            }
            // The session is what makes the pane a live view of the worker's
            // run: rejoin it (replaying the turns this pane missed) before the
            // pane claims the channel is up.
            self.attach_worker_session(pane_id, desktop);
            if let Some(session) = self.pane_workers.get_mut(&pane_id) {
                if matches!(
                    session.attach,
                    PaneAttach::Connecting | PaneAttach::Detached { .. }
                ) {
                    session.attach = PaneAttach::Attached;
                }
                if session.attach.is_attached() && before != Some(PaneAttach::Attached) {
                    attached += 1;
                }
            }
        }
        attached
    }

    /// Rejoin the session `pane_id`'s conversation runs on the worker.
    ///
    /// This is what a viewer pane does instead of starting over: the run lives
    /// on the worker, so opening the pane (or coming back after a drop) asks the
    /// service for the session's own state and replays the turns this pane has
    /// not seen — from the session's own transcript, never from a locally
    /// reconstructed one ([`goble_desktop_service::SessionAttach`]). A
    /// conversation with no session on the worker is recorded as
    /// [`WorkerPaneSession::NoSession`] and the pane says so; no turn is started
    /// here.
    ///
    /// Returns how many turns this attach replayed.
    pub fn attach_worker_session(&mut self, pane_id: u64, desktop: &DesktopState) -> usize {
        let Some(conversation) = self.pane_conversation_id(pane_id) else {
            return 0;
        };
        if conversation.is_empty() {
            return 0;
        }
        let Some(session) = self.pane_workers.get(&pane_id) else {
            return 0;
        };
        // What this pane already holds: the turn count the last (re-)attach
        // reported. A pane that has never attached (or one whose conversation
        // had no session) starts from the session's beginning.
        let from_turn = match &session.session {
            Some(WorkerPaneSession::Attached { turns, .. }) => *turns,
            _ => 0,
        };
        match desktop.attach_chat_session(&conversation, from_turn) {
            SessionAttach::Attached(attached) => {
                let replayed = attached.messages.len();
                let rt = self.pane_runtime.entry(pane_id).or_default();
                // The span handed back is added to what the pane already
                // replayed and the whole transcript is re-resolved, so the rows
                // the pane draws are the session's own, in session order. A span
                // starting at the session's beginning *is* the whole transcript,
                // so it replaces whatever the pane held rather than stacking on
                // it (a pane back on a conversation it had closed, say).
                if attached.from_turn == 0 {
                    rt.session_rows = attached.messages;
                } else {
                    rt.session_rows.extend(attached.messages);
                }
                rt.messages = rt.parse_cache.resolve(&rt.session_rows);
                if let Some(session) = self.pane_workers.get_mut(&pane_id) {
                    session.session = Some(WorkerPaneSession::Attached {
                        session_id: attached.session_id,
                        turns: attached.turns,
                    });
                    // The session came back: the channel to it is up, whatever
                    // the pane reported before the attach.
                    session.attach = PaneAttach::Attached;
                }
                replayed
            }
            SessionAttach::NoSession { .. } => {
                if let Some(session) = self.pane_workers.get_mut(&pane_id) {
                    session.session = Some(WorkerPaneSession::NoSession);
                }
                0
            }
        }
    }

    /// The connection to `worker_id` is gone: every viewer pane on it reports
    /// the drop, naming `reason` when the pane was attached (see
    /// [`UiState::worker_connection_lost`]). The session it re-attached to is
    /// not lost — it is on the worker, and a serving report rejoins it. Returns
    /// how many panes moved.
    pub fn worker_connection_dropped(&mut self, worker_id: &str, reason: &str) -> usize {
        let panes = self.worker_panes(worker_id);
        let mut moved = 0;
        for pane_id in panes {
            if self.worker_connection_lost(pane_id, reason).is_some() {
                moved += 1;
            }
        }
        moved
    }

    /// `pane_id`'s worker could not be reached, or stopped answering: a pane
    /// that was attached detaches with `reason`, one that was still connecting
    /// fails with it, and one already detached or failed keeps the state it has
    /// (the first report is the one that says why).
    ///
    /// The pane stays exactly the pane it was: a viewer. A drop has no shell to
    /// fall back to, which is the whole point of the kind.
    ///
    /// Returns the state the pane ends in, or `None` when it is not a viewer.
    pub fn worker_connection_lost(&mut self, pane_id: u64, reason: &str) -> Option<PaneAttach> {
        let session = self.pane_workers.get_mut(&pane_id)?;
        session.attach = match &session.attach {
            PaneAttach::Attached => PaneAttach::Detached {
                reason: reason.to_string(),
            },
            PaneAttach::Connecting => PaneAttach::Failed {
                message: reason.to_string(),
            },
            current => current.clone(),
        };
        Some(session.attach.clone())
    }

    /// The viewer panes sitting on `worker_id`.
    fn worker_panes(&self, worker_id: &str) -> Vec<u64> {
        if worker_id.trim().is_empty() {
            return Vec::new();
        }
        self.pane_workers
            .iter()
            .filter(|(_, session)| session.worker_id == worker_id)
            .map(|(pane_id, _)| *pane_id)
            .collect()
    }

    /// Give `pane_id` the shape of a viewer pane: a `Worker` leaf, no live
    /// shell, an order to the harness switch, no local cwd.
    ///
    /// Returns whether the leaf was there to shape.
    fn viewer_shape(&mut self, pane_id: u64) -> bool {
        let mut present = false;
        for space in &mut self.spaces {
            let kind = space.leaf_kind(pane_id).cloned();
            if let Some(kind) = kind {
                present = true;
                if kind != PaneKind::Worker {
                    space.set_leaf_kind(pane_id, PaneKind::Worker);
                }
            }
        }
        if !present {
            return false;
        }
        // Nothing local backs the pane: the shell (and its pty) goes with the
        // shape, and the harness switch belongs to a shell surface the pane no
        // longer has.
        self.terminal.borrow_mut().drop_pane(pane_id);
        let controls = self.pane_controls_mut(pane_id);
        controls.harness_mode = false;
        controls.harness_refusal = None;
        true
    }
}
