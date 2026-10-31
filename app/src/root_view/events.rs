//! Draining the backend event bus into the app state before each rebuild.

use super::RootView;

impl RootView {
    /// Poll the event bus and refresh state from the backend when something
    /// changed (chats, messages, workflows, agents). Called on every frame
    /// before the tree is rebuilt, so backend updates show up live.
    pub(super) fn drain_events(&mut self) {
        let Some(desktop) = self.desktop.clone() else {
            return;
        };
        let Some(bus) = self.event_bus.clone() else {
            return;
        };
        let events = bus.take_events();
        if events.is_empty() {
            return;
        }
        let mut state = self.state.borrow_mut();
        // One refresh for the whole batch. A streaming turn emits an event per
        // delta, and re-reading every conversation (and every pane's
        // transcript) once per event made the sidebar redraw faster than the
        // eye can follow. The batch is drained per frame, so this is one
        // refresh per frame at most.
        let mut conversations_dirty = false;
        for (name, payload) in events {
            match name.as_str() {
                "chats:updated" | "chat:updated" => conversations_dirty = true,
                "chat:turn_finished" => {
                    // The conversation names the pane that started the turn and
                    // is authoritative even after the focus moved. Only a
                    // finish event that carries no conversation id at all has
                    // nothing to resolve and falls back to the active pane; one
                    // naming a conversation no pane owns is not attributed to
                    // whichever pane happens to be focused.
                    let pane_id = match payload.get("chat_id").and_then(|v| v.as_str()) {
                        Some(chat_id) => state.pane_for_chat_event(chat_id),
                        None => Some(state.active_pane_id),
                    };
                    let Some(pane_id) = pane_id else {
                        continue;
                    };
                    let conv = state.pane_conversation_id(pane_id);
                    let outcome = match payload.get("status").and_then(|v| v.as_str()) {
                        Some("success") => crate::state::TurnOutcome::Success,
                        Some("failed") => crate::state::TurnOutcome::Failed(None),
                        Some(other) => {
                            crate::state::TurnOutcome::Failed(Some(other.to_string()))
                        }
                        None => crate::state::TurnOutcome::Unknown,
                    };
                    state.finish_turn(pane_id, outcome);
                    // The turn ended: the card reads the finished reply (and
                    // the subject it names) now, before a queued prompt starts
                    // the next turn and makes the conversation live again.
                    state.refresh_conversations(&desktop);
                    log::warn!(
                        "chat:turn_finished for {}: {}",
                        payload.get("chat_id").and_then(|v| v.as_str()).unwrap_or(""),
                        payload.get("status").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    // A prompt queued while the agent was running is submitted
                    // automatically now that the turn finished (warp-new model).
                    let queued = state
                        .pane_runtime
                        .get_mut(&pane_id)
                        .and_then(|rt| rt.queued_prompt.take());
                    if let (Some(prompt), Some(conv)) = (queued, conv) {
                        let model = if state.selected_model.trim().is_empty() {
                            state.settings_llm_model.clone()
                        } else {
                            state.selected_model.clone()
                        };
                        let (medium_id, project_id, session_id) = {
                            let media_b = self.media_state.borrow();
                            let session_id = if media_b.selected_session_id().is_empty() {
                                conv.clone()
                            } else {
                                media_b.selected_session_id().to_string()
                            };
                            (
                                media_b.selected_medium_id().to_string(),
                                media_b.selected_project_id().to_string(),
                                session_id,
                            )
                        };
                        let cwd = state
                            .pane_sessions
                            .get(&pane_id)
                            .map(|s| s.path.clone())
                            .unwrap_or_default();
                        let pane_session = state.pane_session(pane_id, &conv);
                        if let Err(e) = crate::runtime::run_turn(
                            &desktop,
                            &conv,
                            &prompt,
                            &state.settings_llm_provider,
                            &model,
                            state.workspace_routing,
                            &medium_id,
                            &project_id,
                            &session_id,
                            &cwd,
                            Some(state.selected_harness.as_str()),
                            pane_session,
                        ) {
                            log::warn!("auto-submit queued prompt failed: {e}");
                        } else {
                            // A new turn was just issued, so the app observes
                            // its start (used for the chrome's elapsed time).
                            state.begin_turn(pane_id);
                        }
                    }
                    state.sync_active_view();
                }
                // A terminal error from the harness: the turn failed with the
                // message the event carries. The following `turn_finished`
                // settles the same turn and dedupes the inbox item.
                "chat:error" => {
                    let chat_id = payload
                        .get("chat_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let message = payload
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    if !chat_id.is_empty() {
                        log::warn!("chat:error for {chat_id}: {message}");
                        state.apply_turn_error(chat_id, message);
                    }
                    state.sync_active_view();
                }
                "chat:ask_user" => {
                    if let Some(question) = payload.get("question").and_then(|v| v.as_str()) {
                        let quick: Vec<String> = payload
                            .get("quick_replies")
                            .and_then(|q| q.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let chat_id = payload
                            .get("chat_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        state.apply_ask_user(&chat_id, question, quick);
                    }
                    state.refresh_messages(&desktop);
                }
                // The harness suspended before running a command and is waiting
                // on the user's approval; hold the proposal on the pane so the
                // composer draws A5's approve/edit/reject card.
                "chat:command_proposed" => {
                    if let Ok(proposal) = serde_json::from_value::<
                        goble_desktop_service::CommandProposedEvent,
                    >(payload)
                    {
                        state.apply_command_proposed(&proposal);
                        // The pane's approval mode decides whether the turn
                        // suspends at all: always-approve runs the first
                        // candidate at once, plan refuses it, and the two
                        // review modes leave the card for the user.
                        if let Some(pane_id) = state.pane_for_chat_event(&proposal.chat_id) {
                            let decision = match state.pane_controls(pane_id).approval_mode {
                                goble_ui::ApprovalMode::AlwaysApprove => proposal
                                    .candidates
                                    .first()
                                    .cloned()
                                    .map(goble_core::harness::CommandDecision::Approve),
                                goble_ui::ApprovalMode::Plan => Some(
                                    goble_core::harness::CommandDecision::Reject(
                                        "Plan mode: command execution is disabled.".to_string(),
                                    ),
                                ),
                                goble_ui::ApprovalMode::Manual
                                | goble_ui::ApprovalMode::AutoReview => None,
                            };
                            if let Some(decision) = decision {
                                match desktop.resume_command_chat_turn(&proposal.chat_id, decision) {
                                    Ok(_) => {
                                        state.resume_command_decision(pane_id, &proposal.id);
                                        state.agent_busy = true;
                                    }
                                    Err(e) => {
                                        log::warn!("resume_command_chat_turn failed: {e}")
                                    }
                                }
                            }
                        }
                    }
                    state.refresh_messages(&desktop);
                }
                // A live tool-call transition from the harness: hold a running
                // call in the pane's in-flight map and clear it on finish/error.
                // The call is overlaid on the transcript immediately, so it is
                // visible before the turn ends instead of only after the store
                // re-read that `chat:updated` triggers.
                "chat:tool" => {
                    if let Ok(call) =
                        serde_json::from_value::<goble_desktop_service::ToolCallEvent>(payload)
                    {
                        state.apply_tool_event(&call);
                    }
                }
                // A live reasoning transition: fold it into the owning pane's
                // thinking rows and overlay them on the transcript, so the
                // model's reasoning renders as it streams.
                "chat:reasoning" => {
                    if let Ok(event) =
                        serde_json::from_value::<goble_desktop_service::ReasoningEvent>(payload)
                    {
                        state.apply_reasoning_event(&event);
                    }
                }
                // One model call's token accounting, reported by the provider
                // itself. Folded into the conversation's running total, which
                // the transcript's usage affordance shows.
                "chat:usage" => {
                    if let Ok(event) =
                        serde_json::from_value::<crate::state::TokenUsagePayload>(payload)
                    {
                        state.apply_token_usage(&event);
                    }
                }
                // A sub-agent child's live record, straight from S4: the spawn
                // opens the parent transcript's row (S5), progress carries its
                // status, activity, counters and ticking elapsed time, and the
                // finish carries its outcome and duration. Nothing is re-read
                // from the store — the row is the record, not the tool result.
                "chat:subagent_spawned" => {
                    if let Ok(event) = serde_json::from_value::<
                        goble_desktop_service::SubAgentSpawnedEvent,
                    >(payload)
                    {
                        state.apply_subagent_spawned(&event);
                    }
                }
                "chat:subagent_progress" => {
                    if let Ok(event) = serde_json::from_value::<
                        goble_desktop_service::SubAgentProgressEvent,
                    >(payload)
                    {
                        state.apply_subagent_progress(&event);
                    }
                }
                "chat:subagent_finished" => {
                    if let Ok(event) = serde_json::from_value::<
                        goble_desktop_service::SubAgentFinishedEvent,
                    >(payload)
                    {
                        state.apply_subagent_finished(&event);
                    }
                }
                // A worker agent's live lifecycle. `started`/`finished` carry
                // the running set the chrome reads; `state_update`,
                // `tool_result` and `log` update the execution's own record.
                // `executions:updated` below still re-reads the service's
                // authoritative pages, so the running set is available from
                // these events without extending that arm.
                "agent:started" => {
                    let worker = payload.get("worker_id").and_then(|v| v.as_str());
                    let trace = payload.get("trace_id").and_then(|v| v.as_str());
                    let agent = payload.get("agent_id").and_then(|v| v.as_str());
                    if let (Some(worker), Some(trace), Some(agent)) = (worker, trace, agent) {
                        let started_at = payload
                            .get("started_at")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        state.apply_agent_started(worker, trace, agent, started_at);
                    }
                }
                "agent:finished" => {
                    if let Some(trace) = payload.get("trace_id").and_then(|v| v.as_str()) {
                        state.apply_agent_finished(trace);
                    }
                }
                // A worker's own report of itself. A report that says it can
                // still serve work means the channel is live, so the viewer
                // panes on it re-join the sessions they run on — the run lives
                // on the worker, so a drop and a return is a re-attach, not a
                // new session. Any other report is that connection gone, so the
                // panes on it detach and say so. Neither turns a viewer pane
                // into a terminal — there is no local shell behind one.
                "worker:status" => {
                    let worker = payload.get("worker_id").and_then(|v| v.as_str());
                    let status = payload.get("status").and_then(|v| v.as_str());
                    if let (Some(worker), Some(status)) = (worker, status) {
                        if worker_status_serving(status) {
                            state.worker_status_serving(worker, &desktop);
                        } else {
                            state.worker_connection_dropped(
                                worker,
                                &format!("worker {worker} reported {status}"),
                            );
                        }
                    }
                }
                "agent:state_update" => {
                    if let (Some(trace), Some(reported)) = (
                        payload.get("trace_id").and_then(|v| v.as_str()),
                        payload.get("state"),
                    ) {
                        state.apply_agent_state_update(trace, reported.clone());
                    }
                }
                "agent:tool_result" => {
                    let trace = payload.get("trace_id").and_then(|v| v.as_str());
                    let step = payload.get("step_id").and_then(|v| v.as_str());
                    let name = payload.get("name").and_then(|v| v.as_str());
                    let result = payload.get("result").and_then(|v| v.as_str());
                    if let (Some(trace), Some(step), Some(name), Some(result)) =
                        (trace, step, name, result)
                    {
                        state.apply_agent_tool_result(trace, step, name, result);
                    }
                }
                "agent:log" => {
                    if let (Some(trace), Some(level), Some(message)) = (
                        payload.get("trace_id").and_then(|v| v.as_str()),
                        payload.get("level").and_then(|v| v.as_str()),
                        payload.get("message").and_then(|v| v.as_str()),
                    ) {
                        state.apply_agent_log(trace, level, message);
                    }
                }
                "workflows:updated" => {
                    state.refresh_crons(&desktop);
                    state.refresh_observability(&desktop);
                }
                "agents:updated" => state.refresh_agent_name(&desktop),
                "vault:updated" => self.ai_state.borrow_mut().refresh_vault(&desktop),
                "executions:updated" => {
                    self.projects_state.borrow_mut().refresh(&desktop);
                    // Sessions/projects come from the store; re-derive the tree
                    // so a new session shows up in the environment selector.
                    self.media_state.borrow_mut().refresh(&desktop);
                    state.refresh_observability(&desktop);
                }
                _ => {}
            }
        }
        if conversations_dirty {
            state.refresh_conversations(&desktop);
        }
    }
}

/// Whether a `worker:status` report says the worker is still serving sessions.
///
/// The desktop service sends the status as [`goble_core::worker::WorkerStatus`]'s
/// own form (its debug words), so it parses back here; the two states that can
/// run work are `Online` and `Idle` (`WorkerSnapshot::is_available`). Anything
/// else — `Offline`, `Unknown`, `Pairing`, `Error(..)`, or a report this build
/// cannot parse — is a connection a viewer pane has lost, since the pane may
/// only claim a live channel when the worker reported one.
fn worker_status_serving(status: &str) -> bool {
    use goble_core::worker::WorkerStatus;
    matches!(
        serde_json::from_value::<WorkerStatus>(serde_json::Value::String(status.to_string())),
        Ok(WorkerStatus::Online | WorkerStatus::Idle)
    )
}
