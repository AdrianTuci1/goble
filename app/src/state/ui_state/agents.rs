use super::*;

/// What a pane's agent is doing, as the real events reported it.
///
/// [`Self::Unknown`] is the honest default: a pane whose agent has not reported
/// anything yet is not assumed to be idle. The state is event-derived (see
/// [`UiState::begin_turn`], [`UiState::apply_command_proposed`],
/// [`UiState::apply_ask_user`] and [`UiState::finish_turn`]); the one observed
/// exception is a TUI agent, whose terminal mode and running command are the
/// signal ([`UiState::refresh_tui_agent_states`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaneAgentState {
    #[default]
    Unknown,
    Idle,
    Working,
    NeedsInput,
    Done,
    Errored,
}

/// How a turn the app is settling actually ended, read off the terminal event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnOutcome {
    Success,
    /// A failed turn, with the terminal error's own message when one was
    /// reported.
    Failed(Option<String>),
    /// The finish event carried no status. The turn is over, but no state is
    /// invented for it.
    Unknown,
}

impl UiState {
    /// The pane a live `chat:*` event belongs to, from the conversation it
    /// names.
    ///
    /// The conversation-to-pane binding is authoritative: focus can move after
    /// a turn starts, and the event still describes the pane that started it.
    /// A conversation an event names but no pane owns is not attributed to the
    /// focused pane — an empty id is the only case with nothing to resolve, and
    /// it falls back to the active pane. A sub-agent's own conversation resolves
    /// to the pane that spawned that child, so a nested child's events stay with
    /// the transcript that drew its parent row.
    pub fn pane_for_chat_event(&self, conversation_id: &str) -> Option<u64> {
        if conversation_id.is_empty() {
            return Some(self.active_pane_id);
        }
        self.pane_id_for_conversation(conversation_id)
            .or_else(|| self.pane_id_owning_sub_agent(conversation_id))
    }

    /// The pane whose live sub-agent set holds `child_id` as a spawned child.
    fn pane_id_owning_sub_agent(&self, child_id: &str) -> Option<u64> {
        self.pane_runtime.iter().find_map(|(pane_id, rt)| {
            if rt.sub_agents.contains_key(child_id) {
                Some(*pane_id)
            } else {
                None
            }
        })
    }

    /// Apply one live `chat:tool` event: a running call is held in the owning
    /// pane's in-flight map and overlaid on the transcript immediately, without
    /// re-reading the store; a finished/errored call clears its overlay entry,
    /// leaving the persisted terminal state to the next refresh.
    pub fn apply_tool_event(&mut self, call: &goble_desktop_service::ToolCallEvent) {
        let Some(pane_id) = self.pane_for_chat_event(&call.chat_id) else {
            return;
        };
        {
            let rt = self.pane_runtime.entry(pane_id).or_default();
            if call.status == ToolCallStatus::Running {
                rt.in_flight_tools
                    .insert(call.id.clone(), tool_call_from_event(call));
            } else {
                rt.in_flight_tools.remove(&call.id);
            }
            overlay_in_flight(&mut rt.messages, &rt.in_flight_tools);
        }
        if pane_id == self.active_pane_id {
            self.sync_active_view();
        }
    }

    /// Apply one live `chat:command_proposed` event: hold the command the
    /// harness suspended on the owning pane, so the composer draws A5's
    /// approval card. The pane stays busy — the turn is suspended, not finished.
    pub fn apply_command_proposed(&mut self, event: &goble_desktop_service::CommandProposedEvent) {
        let Some(pane_id) = self.pane_for_chat_event(&event.chat_id) else {
            return;
        };
        let detail = event
            .candidates
            .first()
            .cloned()
            .unwrap_or_else(|| event.cwd.clone());
        {
            let rt = self.pane_runtime.entry(pane_id).or_default();
            rt.pending_command = Some(CommandProposalUi::new(
                event.id.clone(),
                event.candidates.clone(),
                crate::state::display_path(&event.cwd),
            ));
            rt.command_selection = Some(Rc::new(RefCell::new(0)));
            *rt.command_scroll.borrow_mut() = goble_ui::ScrollState::default();
            // The card opens on the first candidate, ready to run or edit.
            let first = event.candidates.first().cloned().unwrap_or_default();
            *rt.command_caret.borrow_mut() = first.chars().count();
            *rt.command_draft.borrow_mut() = first;
            *rt.command_editor_scroll.borrow_mut() = goble_ui::ScrollState::default();
            rt.agent_state = PaneAgentState::NeedsInput;
        }
        // The card takes the keyboard when it appears: Enter approves the
        // command until the user clicks the composer to queue a follow-up.
        if pane_id == self.active_pane_id {
            self.composer_focused = false;
        }
        self.raise_inbox(
            pane_id,
            event.chat_id.clone(),
            InboxKind::Approval,
            "Command needs approval",
            detail,
        );
    }

    /// Apply one live `chat:ask_user` event: hold the question on the pane, mark
    /// the agent as needing input, and raise the question in the global inbox.
    pub fn apply_ask_user(&mut self, chat_id: &str, question: &str, quick_replies: Vec<String>) {
        let Some(pane_id) = self.pane_for_chat_event(chat_id) else {
            return;
        };
        {
            let rt = self.pane_runtime.entry(pane_id).or_default();
            rt.pending_ask = Some(AskUserUi::new(question.to_string(), quick_replies));
            rt.agent_state = PaneAgentState::NeedsInput;
        }
        self.raise_inbox(
            pane_id,
            chat_id.to_string(),
            InboxKind::Question,
            "Agent asked a question",
            question,
        );
    }

    /// Settle a terminal error reported by the harness (`chat:error`). The turn
    /// is over and failed; the message is the one the event carried.
    pub fn apply_turn_error(&mut self, chat_id: &str, message: &str) {
        let Some(pane_id) = self.pane_for_chat_event(chat_id) else {
            return;
        };
        if let Some(rt) = self.pane_runtime.get_mut(&pane_id) {
            rt.turn_error = Some(message.to_string());
        }
        self.finish_turn(pane_id, TurnOutcome::Failed(Some(message.to_string())));
    }

    /// Settle a command decision the user made: the resumed turn is running, so
    /// the pane goes back to Working and the card is gone. The busy flag is
    /// recorded before the proposal is cleared, so the pane derives the same
    /// state the live event path would (`clear_command_proposal`).
    pub fn resume_command_decision(&mut self, pane_id: u64, proposal_id: &str) {
        if let Some(rt) = self.pane_runtime.get_mut(&pane_id) {
            rt.busy = true;
        }
        self.clear_command_proposal(pane_id, proposal_id);
    }

    /// Settle a skipped question: the resumed turn is running, so the pane
    /// reports Working and the card is gone.
    pub fn skip_pending_ask(&mut self, pane_id: u64) {
        if let Some(rt) = self.pane_runtime.get_mut(&pane_id) {
            rt.pending_ask = None;
            rt.busy = true;
            rt.agent_state = PaneAgentState::Working;
        }
    }

    /// Stop a pane's turn. Cancellation has no terminal turn event of its own,
    /// so the pane reports Idle directly; the suspended approval belongs to the
    /// turn that is no longer running and goes with it.
    pub fn stop_turn(&mut self, pane_id: u64) {
        let rt = self.pane_runtime.entry(pane_id).or_default();
        rt.busy = false;
        rt.turn_started_at = None;
        rt.reset_command_edit();
        rt.agent_state = PaneAgentState::Idle;
    }

    /// Clear `pane_id`'s pending command proposal, so the card disappears the
    /// moment a decision is submitted. A proposal id that no longer matches
    /// (a stale card) is left alone.
    pub fn clear_command_proposal(&mut self, pane_id: u64, proposal_id: &str) {
        if let Some(rt) = self.pane_runtime.get_mut(&pane_id) {
            if rt.pending_command.as_ref().map(|p| p.id.as_str()) == Some(proposal_id) {
                rt.reset_command_edit();
                // The decision resumes the suspended turn; without a further
                // signal the pane is simply running its turn again.
                if rt.agent_state == PaneAgentState::NeedsInput {
                    rt.agent_state = if rt.busy {
                        PaneAgentState::Working
                    } else {
                        PaneAgentState::Unknown
                    };
                }
            }
        }
    }

    /// Mark `pane_id`'s turn as running and record the instant the app issued
    /// it, so the chrome can show elapsed time. The app observes this itself
    /// (it starts the turn); no wire event carries a turn start.
    pub fn begin_turn(&mut self, pane_id: u64) {
        let rt = self.pane_runtime.entry(pane_id).or_default();
        rt.busy = true;
        rt.turn_started_at = Some(std::time::Instant::now());
        rt.agent_state = PaneAgentState::Working;
        // The previous turn's failure belongs to that turn, not to this one.
        rt.turn_error = None;
    }

    /// Settle a finished turn on `pane_id`: the pane stops showing busy, its
    /// observed start time is dropped (the turn is over), and the terminal
    /// verdict becomes [`PaneAgentState::Done`] or [`PaneAgentState::Errored`].
    /// A pending approval or question outlives the finished turn: the pane
    /// stays [`PaneAgentState::NeedsInput`], the card is left standing, and no
    /// terminal inbox item is raised over it. Called from the
    /// `chat:turn_finished` / `chat:error` paths.
    pub fn finish_turn(&mut self, pane_id: u64, outcome: TurnOutcome) {
        let conversation_id = self.pane_conversation_id(pane_id).unwrap_or_default();
        let still_pending;
        if let Some(rt) = self.pane_runtime.get_mut(&pane_id) {
            rt.busy = false;
            rt.turn_started_at = None;
            still_pending = rt.pending_command.is_some() || rt.pending_ask.is_some();
            if still_pending {
                rt.agent_state = PaneAgentState::NeedsInput;
            } else {
                rt.reset_command_edit();
                rt.agent_state = match &outcome {
                    TurnOutcome::Success => {
                        rt.turn_error = None;
                        PaneAgentState::Done
                    }
                    TurnOutcome::Failed(detail) => {
                        // `chat:error` already left the message here; a bare
                        // failed status that follows it keeps that message.
                        if let Some(detail) = detail {
                            rt.turn_error = Some(detail.clone());
                        }
                        PaneAgentState::Errored
                    }
                    TurnOutcome::Unknown => PaneAgentState::Unknown,
                };
            }
        } else {
            return;
        }
        if still_pending {
            return;
        }
        match outcome {
            TurnOutcome::Success => self.raise_inbox(
                pane_id,
                conversation_id,
                InboxKind::Done,
                "Agent turn finished",
                "The turn completed.",
            ),
            TurnOutcome::Failed(detail) => self.raise_inbox(
                pane_id,
                conversation_id,
                InboxKind::Error,
                "Agent turn failed",
                detail.unwrap_or_else(|| "The turn ended in an error.".to_string()),
            ),
            TurnOutcome::Unknown => String::new(),
        };
    }

    /// The state stored for `pane_id`'s agent, or [`PaneAgentState::Unknown`]
    /// for a pane that has reported nothing.
    pub fn pane_agent_state(&self, pane_id: u64) -> PaneAgentState {
        self.pane_runtime
            .get(&pane_id)
            .map(|rt| rt.agent_state)
            .unwrap_or(PaneAgentState::Unknown)
    }

    /// Refresh the state of every pane whose terminal is running a TUI agent.
    /// The registry's mode and its running-command flag are the signal: a
    /// command under way is [`PaneAgentState::Working`], a prompt is
    /// [`PaneAgentState::Idle`]. A shell pane is left untouched — it has no
    /// agent state to report.
    pub fn refresh_tui_agent_states(&mut self) {
        let terminal = self.terminal.borrow();
        for (pane_id, rt) in self.pane_runtime.iter_mut() {
            if let Some(state) =
                tui_agent_state(&terminal.mode(*pane_id), terminal.running_a_command(*pane_id))
            {
                rt.agent_state = state;
            }
        }
    }

    /// Fold one provider-reported token count for a model call into the
    /// conversation's running total. The conversation is the event's `chat_id`,
    /// so a pane that shows a different conversation than the active one still
    /// totals the right turn.
    pub fn apply_token_usage(&mut self, payload: &TokenUsagePayload) {
        crate::state::fold_token_usage(
            self.conversation_usage
                .entry(payload.chat_id.clone())
                .or_default(),
            &payload.usage,
        );
    }

    /// Seed a conversation's token total from the store: what its model calls
    /// reported before this app run, which is what a restored conversation shows
    /// in its footer. A conversation that already holds counts from live
    /// `chat:usage` events keeps them — the store's figure is that same spend,
    /// so seeding on top of the running total would count it twice.
    pub fn seed_conversation_usage(
        &mut self,
        conversation_id: &str,
        usage: Option<goble_ui::TokenUsage>,
    ) {
        if self.conversation_usage.contains_key(conversation_id) {
            return;
        }
        if let Some(usage) = usage {
            self.conversation_usage
                .insert(conversation_id.to_string(), usage);
        }
    }

    /// Apply one live `chat:reasoning` event: fold it into the owning pane's
    /// reasoning rows and overlay them on the transcript immediately, so the
    /// model's thinking appears as it streams instead of only after the store
    /// re-read that `chat:updated` triggers.
    pub fn apply_reasoning_event(&mut self, event: &goble_desktop_service::ReasoningEvent) {
        let Some(pane_id) = self.pane_for_chat_event(&event.chat_id) else {
            return;
        };
        {
            let rt = self.pane_runtime.entry(pane_id).or_default();
            rt.apply_reasoning(event);
            overlay_reasoning(&mut rt.messages, &rt.reasoning);
        }
        if pane_id == self.active_pane_id {
            self.sync_active_view();
        }
    }

    /// Apply one live `chat:subagent_spawned` event: open the child's record on
    /// the pane that owns the parent conversation, so its row appears in the
    /// parent transcript at once.
    pub fn apply_subagent_spawned(&mut self, event: &goble_desktop_service::SubAgentSpawnedEvent) {
        let Some(pane_id) = self.pane_for_chat_event(&event.chat_id) else {
            return;
        };
        self.pane_runtime
            .entry(pane_id)
            .or_default()
            .apply_subagent_spawned(event);
    }

    /// Apply one live `chat:subagent_progress` event: the child's status,
    /// activity label, counters and ticking elapsed time, which the parent's row
    /// draws from. The child's own conversation is not overlaid on the parent.
    pub fn apply_subagent_progress(
        &mut self,
        event: &goble_desktop_service::SubAgentProgressEvent,
    ) {
        let Some(pane_id) = self.pane_for_chat_event(&event.chat_id) else {
            return;
        };
        self.pane_runtime
            .entry(pane_id)
            .or_default()
            .apply_subagent_progress(event);
    }

    /// Apply one live `chat:subagent_finished` event: the child's terminal
    /// status, its outcome and the time it took.
    pub fn apply_subagent_finished(
        &mut self,
        event: &goble_desktop_service::SubAgentFinishedEvent,
    ) {
        let Some(pane_id) = self.pane_for_chat_event(&event.chat_id) else {
            return;
        };
        self.pane_runtime
            .entry(pane_id)
            .or_default()
            .apply_subagent_finished(event);
        if event.status == "failed" {
            let detail = event
                .error
                .clone()
                .unwrap_or_else(|| event.status.clone());
            self.raise_inbox(
                pane_id,
                event.chat_id.clone(),
                InboxKind::Agent,
                "Sub-agent failed",
                detail,
            );
        }
    }

    /// The live sub-agent rows for `pane_id`, keyed by the id of the parent tool
    /// call that spawned each child — the key the transcript's rows look them
    /// up by. Empty for a pane that has spawned nothing.
    pub fn sub_agent_rows(&self, pane_id: u64) -> HashMap<String, SubAgentRow> {
        let Some(rt) = self.pane_runtime.get(&pane_id) else {
            return HashMap::new();
        };
        rt.sub_agents
            .values()
            .map(|record| (record.parent_call_id.clone(), record.row.clone()))
            .collect()
    }

    /// How many of `pane_id`'s spawned sub-agent children are still running. A
    /// child that completed, failed or was cancelled has ended and is not
    /// counted, so the pane's still-running line names only live work.
    pub fn running_sub_agent_count(&self, pane_id: u64) -> usize {
        self.pane_runtime
            .get(&pane_id)
            .map(|rt| {
                rt.sub_agents
                    .values()
                    .filter(|record| record.row.is_running())
                    .count()
            })
            .unwrap_or(0)
    }

    /// Record one worker agent execution as running, from `agent:started`. The
    /// start time is the service's own, carried on the event; a run already
    /// tracked is refreshed rather than duplicated.
    pub fn apply_agent_started(
        &mut self,
        worker_id: &str,
        trace_id: &str,
        agent_id: &str,
        started_at: &str,
    ) {
        self.live_executions.insert(
            trace_id.to_string(),
            LiveExecution {
                trace_id: trace_id.to_string(),
                agent_id: agent_id.to_string(),
                worker_id: worker_id.to_string(),
                status: "running".to_string(),
                started_at: started_at.to_string(),
                runtime_state: None,
                last_activity: None,
            },
        );
    }

    /// Mark one worker agent execution finished (from `agent:finished`): it is
    /// no longer in flight, so it leaves the live set. Its terminal record is
    /// still re-read from the service by `executions:updated`.
    pub fn apply_agent_finished(&mut self, trace_id: &str) {
        self.live_executions.remove(trace_id);
    }

    /// Record a running execution's agent runtime state, from
    /// `agent:state_update`.
    pub fn apply_agent_state_update(&mut self, trace_id: &str, state: serde_json::Value) {
        if let Some(execution) = self.live_executions.get_mut(trace_id) {
            execution.runtime_state = Some(state);
        }
    }

    /// Record the last tool result a running execution reported, from
    /// `agent:tool_result`.
    pub fn apply_agent_tool_result(
        &mut self,
        trace_id: &str,
        step_id: &str,
        name: &str,
        result: &str,
    ) {
        if let Some(execution) = self.live_executions.get_mut(trace_id) {
            execution.last_activity = Some(format!("{name} {step_id}: {result}"));
        }
    }

    /// Record the last log line a running execution reported, from `agent:log`.
    /// The logs page has its own store; this is the live chrome's view of it.
    pub fn apply_agent_log(&mut self, trace_id: &str, level: &str, message: &str) {
        if let Some(execution) = self.live_executions.get_mut(trace_id) {
            execution.last_activity = Some(format!("{level}: {message}"));
        }
    }

    /// The one answer to "what work is in flight right now": every worker agent
    /// execution the app has seen start and not yet seen finish, the active
    /// pane's in-flight tool calls and busy flag, and whether that pane is
    /// waiting on an approval or a question.
    pub fn live_work(&self) -> LiveWork {
        let pane_id = self.active_pane_id;
        let runtime = self.pane_runtime.get(&pane_id);
        let mut executions: Vec<LiveExecution> = self.live_executions.values().cloned().collect();
        executions.sort_by(|a, b| a.trace_id.cmp(&b.trace_id));
        let mut tool_calls: Vec<ToolCall> = runtime
            .map(|rt| rt.in_flight_tools.values().cloned().collect())
            .unwrap_or_default();
        tool_calls.sort_by(|a, b| a.id.cmp(&b.id));
        LiveWork {
            executions,
            tool_calls,
            turn_busy: runtime.map(|rt| rt.busy).unwrap_or(false),
            pending_approval: runtime
                .and_then(|rt| rt.pending_command.as_ref().map(|p| p.id.clone())),
            pending_question: runtime
                .and_then(|rt| rt.pending_ask.as_ref().map(|a| a.question.clone())),
            turn_started_at: runtime.and_then(|rt| rt.turn_started_at),
        }
    }

    /// The background work `pane_id` alone can be charged with, one row per
    /// item: the tool calls its turn started and has not seen finish, and the
    /// sub-agent children it spawned that are still running. Empty for a pane
    /// with nothing in flight, which is the pane header's chip inert state.
    ///
    /// The app-wide work is deliberately absent. [`UiState::live_executions`]
    /// is keyed by trace id and carries no pane, and the scheduled tasks and
    /// workflows are registered globally, so none of them can be attributed to
    /// the pane a chip sits in — they stay on the app-level cue.
    pub fn pane_live_work(&self, pane_id: u64) -> Vec<PaneWorkItem> {
        let Some(rt) = self.pane_runtime.get(&pane_id) else {
            return Vec::new();
        };
        // The two sources are hash maps, so they are sorted to keep the count
        // and the tooltip's lines stable frame to frame.
        let mut calls: Vec<&ToolCall> = rt.in_flight_tools.values().collect();
        calls.sort_by(|a, b| a.id.cmp(&b.id));
        let mut children: Vec<&SubAgentRecord> = rt.sub_agents.values().collect();
        children.sort_by(|a, b| a.row.child_id.cmp(&b.row.child_id));
        calls
            .into_iter()
            .map(|call| PaneWorkItem {
                kind: PaneWorkKind::Task,
                description: call.name.clone(),
            })
            .chain(
                children
                    .into_iter()
                    .filter(|record| record.row.is_running())
                    .map(|record| PaneWorkItem {
                        kind: PaneWorkKind::SubAgent,
                        description: format!(
                            "{} · {}",
                            record.row.subagent_type, record.row.description
                        ),
                    }),
            )
            .collect()
    }

    /// The live record's row for a child spawned by `pane_id`, if that pane
    /// still holds the record.
    pub(crate) fn live_sub_agent_row(&self, pane_id: u64, child_id: &str) -> Option<SubAgentRow> {
        self.pane_runtime
            .get(&pane_id)
            .and_then(|rt| rt.sub_agents.get(child_id))
            .map(|record| record.row.clone())
    }

    /// Enter a sub-agent child's own conversation in `pane_id` (S6): the view
    /// the parent transcript's row opens.
    ///
    /// It is the agent-view mechanism (A7) pointed at the child's conversation:
    /// the child's card goes into the pane's block list — the way back at the
    /// shell — and the pane's filter points at the child. One card per
    /// conversation, so a round trip through the shell leaves the child's card
    /// rather than a stack of them. The rows come from the store under the
    /// child's chat id, the conversation S2 gave the child, and are parsed
    /// through the same cache the pane's own transcript uses.
    ///
    /// A7 filters a pane to one conversation at a time, so entering a child from
    /// inside the parent's own agent view necessarily switches the pane to the
    /// child. That is deliberate: the pane remembers the view it came from
    /// ([`UiState::close_sub_agent_view`]) and Esc restores it, parent's card and
    /// all, so neither conversation is ever stranded.
    pub fn open_sub_agent(&mut self, pane_id: u64, child_id: &str, desktop: Option<&DesktopState>) {
        let row = self.live_sub_agent_row(pane_id, child_id);
        // The card's label names the child by what it is doing, the way the
        // transcript's row does; with no live record the conversation's own name
        // is all there is.
        let label = match &row {
            Some(row) => format!("{} · {}", row.subagent_type, row.description),
            None => self.conversation_name(child_id),
        };
        let returned_to = match self.sub_agent_views.get(&pane_id) {
            // Entering another child from inside a child view keeps the way back
            // at the conversation the user actually came from, rather than
            // stacking one child view on the next.
            Some(view) => view.returned_to.clone(),
            None => self.pane_view(pane_id),
        };
        self.enter_agent_view(pane_id, child_id, &label);
        let mut view = SubAgentChildView {
            child_id: child_id.to_string(),
            row,
            messages: Vec::new(),
            returned_to,
            parse_cache: MessageParseCache::default(),
        };
        if let Some(desktop) = desktop {
            let row = view.row.clone();
            view.refresh(row, desktop);
        }
        self.sub_agent_views.insert(pane_id, view);
    }

    /// Leave the child view open in `pane_id`, if it has one: the pane goes
    /// back to the view it came from — the shell, or the parent's own agent
    /// view when the row was clicked from inside it — and both cards stay in the
    /// block list. Returns whether a child view was open.
    ///
    /// The surface the pane goes back to is decided the way it always is: a
    /// child view is entered through [`Self::enter_agent_view`], so the harness
    /// switch is on while it is open, and returning to the shell turns it off
    /// with the filter ([`Self::leave_agent_view`]) rather than leaving the pane
    /// painting an agent surface its own view calls the shell.
    pub fn close_sub_agent_view(&mut self, pane_id: u64) -> bool {
        let Some(view) = self.sub_agent_views.remove(&pane_id) else {
            return false;
        };
        match view.returned_to {
            // Back to the parent's agent view: still the pane's agent surface,
            // so the harness switch stays on with the filter.
            BlockView::Agent { .. } => self.pane_controls_mut(pane_id).view = view.returned_to,
            // Back to the shell: the switch goes off with the filter.
            BlockView::Terminal => self.leave_agent_view(pane_id),
        }
        true
    }

}

/// The state a pane's TUI agent reports through its terminal: `Some(Working)`
/// while a command runs, `Some(Idle)` at a prompt, and `None` for a shell pane
/// that has no agent to report.
pub(super) fn tui_agent_state(
    mode: &crate::terminal::TerminalMode,
    running: bool,
) -> Option<PaneAgentState> {
    match mode {
        crate::terminal::TerminalMode::Agent(_) => Some(if running {
            PaneAgentState::Working
        } else {
            PaneAgentState::Idle
        }),
        crate::terminal::TerminalMode::Shell => None,
    }
}
