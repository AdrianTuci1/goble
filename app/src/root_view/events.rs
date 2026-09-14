//! Draining the backend event bus into the app state before each rebuild.

use super::RootView;

impl RootView {
    /// Poll the event bus and refresh state from the backend when something
    /// changed (chats, messages, workflows, agents). Called on every frame
    /// before the tree is rebuilt, so backend updates show up live.
    pub(super) fn drain_events(&mut self) {
        let Some(bus) = self.event_bus.clone() else {
            return;
        };
        let events = bus.take_events();
        if events.is_empty() {
            return;
        }
        let Some(desktop) = self.desktop.clone() else {
            return;
        };
        let mut state = self.state.borrow_mut();
        for (name, payload) in events {
            match name.as_str() {
                "chats:updated" | "chat:updated" => state.refresh_conversations(&desktop),
                "chat:turn_finished" => {
                    // Route the finished turn to the pane that owns its
                    // conversation (falling back to the active pane).
                    let pane_id = payload
                        .get("chat_id")
                        .and_then(|v| v.as_str())
                        .and_then(|cid| state.pane_id_for_conversation(cid))
                        .unwrap_or(state.active_pane_id);
                    let conv = state.pane_conversation_id(pane_id);
                    state.finish_turn(pane_id);
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
                // The agent suspended to ask the user a question; render the
                // inline ask card in the transcript.
                "chat:ask_user" => {
                    let pane_id = payload
                        .get("chat_id")
                        .and_then(|v| v.as_str())
                        .and_then(|cid| state.pane_id_for_conversation(cid))
                        .unwrap_or(state.active_pane_id);
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
                        if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                            rt.pending_ask = Some(goble_ui::AskUserUi::new(
                                question.to_string(),
                                quick,
                            ));
                        }
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
                // The agent handed the desktop to a remote screen: open the
                // screen panel, refresh the source list (so the fresh remote
                // source shows up), then select it. The same source is marked
                // as this conversation's inline handoff, so its live frame
                // renders inside the chat's harness area too.
                "screen:handoff" => {
                    if let Some(source) = payload.get("source").and_then(|v| v.as_str()) {
                        let mut s = self.screen_state.borrow_mut();
                        s.open = true;
                        s.refresh_sources(&desktop);
                        s.select_source(source);
                        let pane_id = payload
                            .get("chat_id")
                            .and_then(|v| v.as_str())
                            .and_then(|cid| state.pane_id_for_conversation(cid))
                            .unwrap_or(state.active_pane_id);
                        if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                            rt.inline_screen_source = Some(source.to_string());
                        }
                    }
                }
                _ => {}
            }
        }

        // A clicked BYOH handoff link: open the screen sheet, selecting the
        // source derived from the URI. The root owns the screen sheet, so the
        // action only stashes the URI here.
        if let Some(link) = state.pending_screen_link_open.take() {
            let source = crate::state::screen_source_from_link(&link);
            let mut s = self.screen_state.borrow_mut();
            s.open = true;
            s.refresh_sources(&desktop);
            if let Some(src) = source {
                s.select_source(&src);
            }
        }
    }
}
