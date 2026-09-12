use std::sync::Arc;

use goble_core::harness::ToolCallStatus;

use super::{
    CommandProposedEvent, DesktopState, ReasoningEvent, ReasoningPhase, SubAgentFinishedEvent,
    SubAgentProgressEvent, SubAgentSpawnedEvent, TokenUsageEvent, ToolCallEvent,
};

/// The `session_id` scoping a daemon event. Every [`DaemonEvent`] variant
/// carries one, so the desktop turn entry points can correlate an event to the
/// chat turn that produced it without matching each variant by hand.
fn daemon_session_id(event: &goble_daemon_protocol::DaemonEvent) -> goble_harness_types::SessionId {
    use goble_daemon_protocol::DaemonEvent as DE;
    match event {
        DE::AssistantDelta { session_id, .. }
        | DE::ToolCallStarted { session_id, .. }
        | DE::ToolCallFinished { session_id, .. }
        | DE::ToolCallError { session_id, .. }
        | DE::AskUser { session_id, .. }
        | DE::CommandProposed { session_id, .. }
        | DE::SubAgentSpawned { session_id, .. }
        | DE::SubAgentProgress { session_id, .. }
        | DE::SubAgentFinished { session_id, .. }
        | DE::MissionUpdated { session_id, .. }
        | DE::TokenUsage { session_id, .. }
        | DE::ReasoningStarted { session_id, .. }
        | DE::ReasoningDelta { session_id, .. }
        | DE::ReasoningDone { session_id, .. }
        | DE::Done { session_id }
        | DE::Error { session_id, .. }
        | DE::TraceStarted { session_id, .. }
        | DE::TraceFinished { session_id, .. }
        | DE::ScreenHandoff { session_id, .. } => session_id.clone(),
    }
}

impl DesktopState {
    /// Spawn the daemon-event -> `chat:*` translator, once.
    ///
    /// Subscribes synchronously *before* the turn runs so no live event is
    /// missed, then fans the daemon wire events into the `chat:updated`,
    /// `chat:ask_user`, `chat:mission`, `chat:tool` and `chat:turn_finished`
    /// events the native UI listens for. Only called from the harness turn
    /// entry points, which run inside a tokio runtime — `new()` does not, so it
    /// cannot spawn.
    pub(super) fn ensure_translator(self: &Arc<Self>) {
        if self
            .translator_spawned
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        let mut rx = self.daemon.subscribe();
        let this = Arc::clone(self);
        tokio::spawn(async move {
            while let Ok(ev) = rx.recv().await {
                this.translate_daemon_event(ev);
            }
        });
    }

    /// Translate one daemon event into the `chat:*` events the native UI reacts
    /// to, updating the per-chat in-flight tool-call map for tool lifecycle
    /// events. Kept separate from the subscriber loop so it can be driven
    /// directly in tests, where no runtime task is spawnable.
    pub(super) fn translate_daemon_event(&self, ev: goble_daemon_protocol::DaemonEvent) {
        use goble_daemon_protocol::DaemonEvent as DE;
        let session_id = daemon_session_id(&ev);
        self.emit("chat:updated", serde_json::json!({ "chat_id": session_id.0 }));
        match ev {
            DE::AskUser {
                session_id,
                question,
                quick_replies,
            } => {
                self.emit(
                    "chat:ask_user",
                    serde_json::json!({
                        "chat_id": session_id.0,
                        "question": question,
                        "quick_replies": quick_replies,
                    }),
                );
            }
            DE::CommandProposed {
                session_id,
                id,
                candidates,
                cwd,
            } => {
                self.emit(
                    "chat:command_proposed",
                    CommandProposedEvent {
                        chat_id: session_id.0,
                        id,
                        candidates,
                        cwd,
                    },
                );
            }
            DE::SubAgentSpawned {
                session_id: _,
                chat_id,
                subagent_id,
                subagent_type,
                description,
                parent_call_id,
                run_in_background,
            } => {
                self.emit(
                    "chat:subagent_spawned",
                    SubAgentSpawnedEvent {
                        chat_id,
                        subagent_id,
                        subagent_type,
                        description,
                        parent_call_id,
                        run_in_background,
                    },
                );
            }
            DE::SubAgentProgress {
                session_id: _,
                chat_id,
                subagent_id,
                status,
                activity,
                turns,
                tool_calls,
                tokens,
                duration_ms,
            } => {
                self.emit(
                    "chat:subagent_progress",
                    SubAgentProgressEvent {
                        chat_id,
                        subagent_id,
                        status,
                        activity,
                        turns,
                        tool_calls,
                        tokens,
                        duration_ms,
                    },
                );
            }
            DE::SubAgentFinished {
                session_id: _,
                chat_id,
                subagent_id,
                status,
                output,
                error,
                duration_ms,
                turns,
                tool_calls,
                tokens,
            } => {
                self.emit(
                    "chat:subagent_finished",
                    SubAgentFinishedEvent {
                        chat_id,
                        subagent_id,
                        status,
                        output,
                        error,
                        duration_ms,
                        turns,
                        tool_calls,
                        tokens,
                    },
                );
            }
            DE::MissionUpdated {
                session_id,
                mission_id,
                status,
            } => {
                self.emit(
                    "chat:mission",
                    serde_json::json!({
                        "chat_id": session_id.0,
                        "mission_id": mission_id,
                        "status": status,
                    }),
                );
            }
            DE::TokenUsage {
                session_id: _,
                chat_id,
                input,
                cached,
                output,
            } => {
                // The chat id rides along so the app can total the tokens of
                // this conversation's own turns, not the whole workspace.
                self.emit(
                    "chat:usage",
                    serde_json::json!({
                        "chat_id": chat_id,
                        "usage": TokenUsageEvent { input, cached, output },
                    }),
                );
            }
            DE::ToolCallStarted {
                session_id,
                id,
                name,
                arguments,
            } => {
                self.record_tool_call(
                    &session_id.0,
                    &id,
                    Some(name),
                    Some(arguments),
                    ToolCallStatus::Running,
                    None,
                );
            }
            DE::ToolCallFinished {
                session_id,
                id,
                result,
            } => {
                self.record_tool_call(
                    &session_id.0,
                    &id,
                    None,
                    None,
                    ToolCallStatus::Finished,
                    Some(result),
                );
            }
            DE::ToolCallError {
                session_id,
                id,
                message,
            } => {
                self.record_tool_call(
                    &session_id.0,
                    &id,
                    None,
                    None,
                    ToolCallStatus::Error,
                    Some(message),
                );
            }
            DE::ReasoningStarted {
                session_id,
                step,
                mode,
            } => {
                self.emit(
                    "chat:reasoning",
                    ReasoningEvent {
                        chat_id: session_id.0,
                        step: Some(step),
                        mode,
                        delta: String::new(),
                        content: None,
                        decision: None,
                        phase: ReasoningPhase::Started,
                    },
                );
            }
            DE::ReasoningDelta { session_id, delta } => {
                self.emit(
                    "chat:reasoning",
                    ReasoningEvent {
                        chat_id: session_id.0,
                        step: None,
                        mode: String::new(),
                        delta,
                        content: None,
                        decision: None,
                        phase: ReasoningPhase::Delta,
                    },
                );
            }
            DE::ReasoningDone {
                session_id,
                step,
                mode,
                content,
                decision,
            } => {
                self.emit(
                    "chat:reasoning",
                    ReasoningEvent {
                        chat_id: session_id.0,
                        step: Some(step),
                        mode,
                        delta: String::new(),
                        content: Some(content),
                        decision: Some(decision),
                        phase: ReasoningPhase::Done,
                    },
                );
            }
            DE::TraceFinished { session_id, .. } => {
                self.emit(
                    "chat:turn_finished",
                    serde_json::json!({ "chat_id": session_id.0 }),
                );
            }
            DE::ScreenHandoff { session_id, config } => {
                match self.open_remote_screen(config) {
                    Ok(source) => {
                        self.add_log(format!("opened remote desktop {source}"));
                        self.emit(
                            "screen:handoff",
                            serde_json::json!({
                                "chat_id": session_id.0,
                                "source": source,
                            }),
                        );
                    }
                    Err(e) => {
                        self.add_log(format!("screen handoff failed: {e:#}"));
                    }
                }
            }
            _ => {}
        }
    }

    /// Apply one tool-call transition to the per-chat in-flight map and emit the
    /// structured `chat:tool` event. `Started` inserts the running call;
    /// `Finished`/`Error` remove it, since the persisted row now carries the
    /// terminal state. A finished/errored wire event carries no name or
    /// arguments, so they are read back from the entry being cleared.
    fn record_tool_call(
        &self,
        chat_id: &str,
        id: &str,
        name: Option<String>,
        arguments: Option<serde_json::Value>,
        status: ToolCallStatus,
        result: Option<String>,
    ) {
        let event = {
            let mut in_flight = self.tool_in_flight.lock();
            let calls = in_flight.entry(chat_id.to_string()).or_default();
            let (name, arguments) = match (name, arguments) {
                (Some(name), Some(arguments)) => (name, arguments),
                _ => calls
                    .get(id)
                    .map(|call| (call.name.clone(), call.arguments.clone()))
                    .unwrap_or((String::new(), serde_json::Value::Null)),
            };
            let event = ToolCallEvent {
                chat_id: chat_id.to_string(),
                id: id.to_string(),
                name,
                arguments,
                status,
                result,
            };
            if status == ToolCallStatus::Running {
                calls.insert(id.to_string(), event.clone());
            } else {
                calls.remove(id);
            }
            if calls.is_empty() {
                in_flight.remove(chat_id);
            }
            event
        };
        self.emit("chat:tool", &event);
    }

    /// The tool calls still running for `chat_id`. The app overlays these on the
    /// persisted rows, so a running call is visible before the turn ends.
    pub fn in_flight_tool_calls(&self, chat_id: &str) -> Vec<ToolCallEvent> {
        self.tool_in_flight
            .lock()
            .get(chat_id)
            .map(|calls| calls.values().cloned().collect())
            .unwrap_or_default()
    }
}
