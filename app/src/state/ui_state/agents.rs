use super::*;

impl UiState {
    /// Apply one live `chat:tool` event: a running call is held in the owning
    /// pane's in-flight map and overlaid on the transcript immediately, without
    /// re-reading the store; a finished/errored call clears its overlay entry,
    /// leaving the persisted terminal state to the next refresh.
    pub fn apply_tool_event(&mut self, call: &goble_desktop_service::ToolCallEvent) {
        let pane_id = self
            .pane_id_for_conversation(&call.chat_id)
            .unwrap_or(self.active_pane_id);
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
        let pane_id = self
            .pane_id_for_conversation(&event.chat_id)
            .unwrap_or(self.active_pane_id);
        let rt = self.pane_runtime.entry(pane_id).or_default();
        rt.pending_command = Some(CommandProposalUi::new(
            event.id.clone(),
            event.candidates.clone(),
            crate::state::display_path(&event.cwd),
        ));
        rt.command_selection = Some(Rc::new(RefCell::new(0)));
    }

    /// Clear `pane_id`'s pending command proposal, so the card disappears the
    /// moment a decision is submitted. A proposal id that no longer matches
    /// (a stale card) is left alone.
    pub fn clear_command_proposal(&mut self, pane_id: u64, proposal_id: &str) {
        if let Some(rt) = self.pane_runtime.get_mut(&pane_id) {
            if rt.pending_command.as_ref().map(|p| p.id.as_str()) == Some(proposal_id) {
                rt.pending_command = None;
                rt.command_selection = None;
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
    }

    /// Settle a finished turn on `pane_id`: the pane stops showing busy, its
    /// observed start time is dropped (the turn is over) and any command
    /// proposal it was waiting on is dropped (the store now carries the
    /// outcome). Called from the `chat:turn_finished` path.
    pub fn finish_turn(&mut self, pane_id: u64) {
        if let Some(rt) = self.pane_runtime.get_mut(&pane_id) {
            rt.busy = false;
            rt.turn_started_at = None;
            rt.pending_command = None;
            rt.command_selection = None;
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

    /// Apply one live `chat:reasoning` event: fold it into the owning pane's
    /// reasoning rows and overlay them on the transcript immediately, so the
    /// model's thinking appears as it streams instead of only after the store
    /// re-read that `chat:updated` triggers.
    pub fn apply_reasoning_event(&mut self, event: &goble_desktop_service::ReasoningEvent) {
        let pane_id = self
            .pane_id_for_conversation(&event.chat_id)
            .unwrap_or(self.active_pane_id);
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
        let pane_id = self
            .pane_id_for_conversation(&event.chat_id)
            .unwrap_or(self.active_pane_id);
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
        let pane_id = self
            .pane_id_for_conversation(&event.chat_id)
            .unwrap_or(self.active_pane_id);
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
        let pane_id = self
            .pane_id_for_conversation(&event.chat_id)
            .unwrap_or(self.active_pane_id);
        self.pane_runtime
            .entry(pane_id)
            .or_default()
            .apply_subagent_finished(event);
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

    /// Leave the child view open in `pane_id`, if it has one: the pane's filter
    /// goes back to the view it came from — the shell, or the parent's own agent
    /// view when the row was clicked from inside it — and both cards stay in the
    /// block list. Returns whether a child view was open.
    pub fn close_sub_agent_view(&mut self, pane_id: u64) -> bool {
        let Some(view) = self.sub_agent_views.remove(&pane_id) else {
            return false;
        };
        self.pane_controls_mut(pane_id).view = view.returned_to;
        true
    }

}
