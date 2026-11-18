//! Gateway run-event → ACP `session/update` mapping.
//!
//! The gateway relays events from the worker/agent (the dsh-remote reference
//! streams the same vocabulary). The bridge converts them into the exact ACP
//! update frames captured from a real `dsh --profile acp` server; the fixtures
//! in `docs/acp-frames.md` upstream are the source of truth for the shapes.
//!
//! | Gateway event | ACP update |
//! |---|---|
//! | `text` | `agent_message_chunk` |
//! | `thinking` | `agent_thought_chunk` (empty chunk skipped) |
//! | `tool_call` | `tool_call` (`kind: "other"`, `status: "in_progress"`, `rawInput`) |
//! | `tool_result` | `tool_call_update` (`completed`/`failed`, text `content`) |
//! | `session` | `session_info_update` (with `_meta.bridge`) |
//! | `usage` | `usage_update` (only with both `used` and `size`) |
//! | `status` | ignored (no ACP equivalent) |
//! | `final` / `end` | settles the prompt (`end_turn`/`cancelled`/error) |
//!
//! Parsing is deliberately tolerant: the Goble gateway is written in parallel
//! with this bridge, and worker frames use different field names
//! (`AssistantDelta`/`ToolCallStarted`/…). Every known alias is accepted, an
//! unknown event is ignored (forward compatible), and a terminal event is never
//! guessed from silence.

use serde_json::{json, Value};

use crate::gateway::RunTerminal;

/// One mapped gateway event: zero or more ACP updates plus, at most, the
/// terminal reason that settles the prompt request.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MappedEvent {
    pub updates: Vec<Value>,
    pub terminal: Option<RunTerminal>,
}

impl MappedEvent {
    fn ignored() -> Self {
        Self::default()
    }

    fn updates(updates: Vec<Value>) -> Self {
        Self {
            updates,
            terminal: None,
        }
    }
}

/// Stateful per-run mapper. It owns the stable `messageId`s (one for assistant
/// text, one for reasoning) and whether assistant text already arrived, so a
/// `final.text` fallback can never duplicate streamed output.
pub struct UpdateMapper {
    session_id: String,
    run_id: String,
    gateway_session_id: String,
    message_id: String,
    thought_message_id: String,
    text_emitted: bool,
}

impl UpdateMapper {
    /// Build a mapper for one prompt turn.
    ///
    /// `session_id` is the ACP session id (echoed in `session/update` params),
    /// `gateway_session_id` is the gateway's id (reported in `_meta.bridge`).
    pub fn new(session_id: impl Into<String>, run_id: impl Into<String>) -> Self {
        let run_id = run_id.into();
        let message_id = format!("msg-{run_id}");
        let thought_message_id = format!("thought-{run_id}");
        Self {
            session_id: session_id.into(),
            gateway_session_id: String::new(),
            run_id,
            message_id,
            thought_message_id,
            text_emitted: false,
        }
    }

    /// Attach the gateway session id reported in `session_info_update._meta`.
    pub fn with_gateway_session(mut self, gateway_session_id: impl Into<String>) -> Self {
        self.gateway_session_id = gateway_session_id.into();
        self
    }

    /// The ACP session id this mapper emits updates for.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The run id this mapper is settling.
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Translate one gateway event.
    pub fn map(&mut self, event: &Value) -> MappedEvent {
        let mut kind = event
            .get("type")
            .or_else(|| event.get("kind"))
            .or_else(|| event.get("event"))
            .and_then(Value::as_str)
            .unwrap_or("");
        // The gateway wraps forward-compatible/future worker frames as
        // `{"type":"event","kind":"<worker frame kind>"}`; unwrap one level so
        // aliases keep working when the worker protocol grows.
        if kind == "event" {
            kind = event.get("kind").and_then(Value::as_str).unwrap_or("");
        }
        match kind {
            "text" | "assistant" | "assistant_text" | "assistant_delta" => self.text(event),
            "thinking" | "thought" | "agent_thought" | "agent_thought_chunk" | "reasoning" => {
                self.thinking(event)
            }
            "tool_call" | "toolcall" | "tool_call_started" => self.tool_call(event),
            "tool_result" | "toolcall_result" | "tool_call_finished" | "tool_call_result" => {
                self.tool_result(event)
            }
            "tool_call_error" => self.tool_call_error(event),
            "session" | "session_info" => self.session_info(event),
            "usage" | "usage_update" => self.usage(event),
            "final" | "done" | "turn_end" => self.final_event(event),
            "end" | "stream_end" => self.end_event(event),
            "gateway" => self.gateway_event(event),
            "error" | "run_error" => MappedEvent {
                updates: Vec::new(),
                terminal: Some(RunTerminal::Failed {
                    message: text_field(event, &["message", "error", "detail"])
                        .unwrap_or_else(|| "run failed".to_string()),
                }),
            },
            // `status` is bookkeeping only: ACP has no equivalent, and the
            // gateway's status carries no context-window size, so no
            // `usage_update` is invented here.
            "status" | "ping" | "keepalive" | "" => MappedEvent::ignored(),
            _ => MappedEvent::ignored(),
        }
    }

    fn text(&mut self, event: &Value) -> MappedEvent {
        let mut updates = Vec::new();
        if let Some(message_id) = self.message_id_for(event) {
            if !message_id.is_empty() {
                updates.push(self.update(json!({
                    "sessionUpdate": "agent_message_chunk",
                    "messageId": message_id,
                    "content": {"type": "text", "text": self.chunk_text(event)},
                })));
                self.text_emitted = true;
            }
        }
        MappedEvent::updates(updates)
    }

    fn thinking(&mut self, event: &Value) -> MappedEvent {
        let text = self.chunk_text(event);
        if text.is_empty() {
            // The reference server skips empty thinking events.
            return MappedEvent::ignored();
        }
        let message_id = self
            .str_field(event, &["messageId", "message_id", "id"])
            .unwrap_or(&self.thought_message_id)
            .to_string();
        MappedEvent::updates(vec![self.update(json!({
            "sessionUpdate": "agent_thought_chunk",
            "messageId": message_id,
            "content": {"type": "text", "text": text},
        }))])
    }

    fn tool_call(&mut self, event: &Value) -> MappedEvent {
        let Some(call_id) = self.str_field(event, &["callId", "call_id", "toolCallId", "id"])
        else {
            return MappedEvent::ignored();
        };
        let title = self
            .str_field(event, &["tool", "name", "title", "toolName"])
            .unwrap_or("tool");
        let raw_input = event
            .get("input")
            .or_else(|| event.get("arguments"))
            .or_else(|| event.get("rawInput"))
            .cloned()
            .unwrap_or(Value::Null);
        MappedEvent::updates(vec![self.update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": call_id,
            "title": title,
            "kind": "other",
            "status": "in_progress",
            "rawInput": raw_input,
        }))])
    }

    fn tool_result(&mut self, event: &Value) -> MappedEvent {
        let Some(call_id) = self.str_field(event, &["callId", "call_id", "toolCallId", "id"])
        else {
            return MappedEvent::ignored();
        };
        let status = self
            .str_field(event, &["status"])
            .map(tool_status)
            .unwrap_or("completed");
        let result =
            text_field(event, &["result", "output", "content", "text"]).unwrap_or_default();
        MappedEvent::updates(vec![self.tool_update(call_id, status, &result)])
    }

    fn tool_call_error(&mut self, event: &Value) -> MappedEvent {
        let Some(call_id) = self.str_field(event, &["callId", "call_id", "toolCallId", "id"])
        else {
            return MappedEvent::ignored();
        };
        let message = text_field(event, &["message", "error", "result"]).unwrap_or_default();
        MappedEvent::updates(vec![self.tool_update(call_id, "failed", &message)])
    }

    fn tool_update(&self, call_id: &str, status: &str, result: &str) -> Value {
        // The reference server carries tool output in a separate
        // `tool_call_update` whose content is a list of `content` parts.
        self.update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": call_id,
            "status": status,
            "content": [{
                "type": "content",
                "content": {"type": "text", "text": result},
            }],
        }))
    }

    fn session_info(&mut self, event: &Value) -> MappedEvent {
        let gateway_session_id = self
            .str_field(event, &["sessionId", "session_id"])
            .unwrap_or(&self.gateway_session_id)
            .to_string();
        if self.gateway_session_id.is_empty() {
            self.gateway_session_id.clone_from(&gateway_session_id);
        }
        MappedEvent::updates(vec![self.update(json!({
            "sessionUpdate": "session_info_update",
            "_meta": {"bridge": {
                "gatewaySessionId": gateway_session_id,
                "runId": self.run_id,
            }},
        }))])
    }

    fn usage(&mut self, event: &Value) -> MappedEvent {
        // ACP `usage_update` requires both numbers; inventing a context size
        // would be worse than omitting the frame.
        let used = event
            .get("used")
            .or_else(|| event.get("usedTokens"))
            .or_else(|| event.get("totalTokens"))
            .and_then(Value::as_u64);
        let size = event
            .get("size")
            .or_else(|| event.get("contextSize"))
            .or_else(|| event.get("contextWindow"))
            .and_then(Value::as_u64);
        match (used, size) {
            (Some(used), Some(size)) => MappedEvent::updates(vec![self.update(json!({
                "sessionUpdate": "usage_update",
                "used": used,
                "size": size,
            }))]),
            _ => MappedEvent::ignored(),
        }
    }

    fn final_event(&mut self, event: &Value) -> MappedEvent {
        let mut updates = Vec::new();
        // `final.text` repeats the whole turn; only emit it when no text event
        // preceded it, so output is never duplicated.
        if !self.text_emitted {
            if let Some(text) = text_field(event, &["text", "final_text", "finalText"]) {
                if !text.trim().is_empty() {
                    updates.push(self.update(json!({
                        "sessionUpdate": "agent_message_chunk",
                        "messageId": self.message_id.clone(),
                        "content": {"type": "text", "text": text},
                    })));
                    self.text_emitted = true;
                }
            }
        }
        MappedEvent {
            updates,
            terminal: Some(terminal_from_reason(event)),
        }
    }

    fn end_event(&mut self, event: &Value) -> MappedEvent {
        let status = self
            .str_field(event, &["status", "reason", "state"])
            .unwrap_or("")
            .to_ascii_lowercase();
        let terminal = match status.as_str() {
            "done" | "completed" | "complete" | "succeeded" | "success" | "ok" | "end_turn" => {
                Some(RunTerminal::Completed)
            }
            "cancelled" | "canceled" | "stopped" => Some(RunTerminal::Cancelled),
            "failed" | "error" | "timeout" | "timed_out" => Some(RunTerminal::Failed {
                message: text_field(event, &["error", "message", "detail"])
                    .unwrap_or_else(|| format!("run ended with status {status:?}")),
            }),
            // A non-terminal status in an `end` frame is not a settlement; the
            // caller polls `GET /api/v1/runs/{id}` instead of guessing.
            "" | "running" | "queued" | "pending" | "in_progress" => None,
            _ => Some(RunTerminal::Failed {
                message: format!("run ended with unknown status {status:?}"),
            }),
        };
        MappedEvent {
            updates: Vec::new(),
            terminal,
        }
    }

    fn gateway_event(&mut self, event: &Value) -> MappedEvent {
        // The gateway appends its own bookkeeping event when a run settles.
        let status = self
            .str_field(event, &["status"])
            .unwrap_or("")
            .to_ascii_lowercase();
        MappedEvent {
            updates: Vec::new(),
            terminal: match status.as_str() {
                "done" | "completed" => Some(RunTerminal::Completed),
                "failed" => Some(RunTerminal::Failed {
                    message: text_field(event, &["reason", "error", "message"])
                        .unwrap_or_else(|| "run failed".to_string()),
                }),
                "cancelled" | "canceled" => Some(RunTerminal::Cancelled),
                _ => None,
            },
        }
    }

    /// Build a `session/update` notification for this session.
    fn update(&self, update: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {"sessionId": self.session_id, "update": update},
        })
    }

    fn chunk_text(&self, event: &Value) -> String {
        if let Some(text) = event.get("text").and_then(Value::as_str) {
            return text.to_string();
        }
        if let Some(delta) = event.get("delta").and_then(Value::as_str) {
            return delta.to_string();
        }
        if let Some(content) = event.get("content") {
            if let Some(text) = content.as_str() {
                return text.to_string();
            }
            if let Some(text) = content.get("text").and_then(Value::as_str) {
                return text.to_string();
            }
        }
        String::new()
    }

    fn message_id_for(&self, event: &Value) -> Option<String> {
        self.str_field(event, &["messageId", "message_id", "id"])
            .map(str::to_string)
            .or_else(|| Some(self.message_id.clone()))
    }

    fn str_field<'a>(&'a self, event: &'a Value, names: &[&str]) -> Option<&'a str> {
        names
            .iter()
            .find_map(|name| event.get(*name).and_then(Value::as_str))
    }
}

/// Normalise a gateway tool status to ACP `tool_call_update.status`.
fn tool_status(raw: &str) -> &'static str {
    match raw.to_ascii_lowercase().as_str() {
        "completed" | "complete" | "done" | "success" | "succeeded" | "ok" => "completed",
        "failed" | "error" | "cancelled" | "canceled" | "timeout" => "failed",
        _ => "completed",
    }
}

/// Read the terminal reason out of a `final` event.
fn terminal_from_reason(event: &Value) -> RunTerminal {
    let reason = event.get("reason");
    let kind = reason
        .and_then(|reason| {
            reason.as_str().map(str::to_string).or_else(|| {
                reason
                    .get("kind")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        })
        .or_else(|| {
            event
                .get("status")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
    match kind.as_str() {
        "completed" | "done" | "end_turn" | "success" | "succeeded" | "ok" | "" => {
            RunTerminal::Completed
        }
        "cancelled" | "canceled" | "stopped" => RunTerminal::Cancelled,
        "error" | "failed" | "timeout" | "timed_out" => RunTerminal::Failed {
            message: reason
                .and_then(|reason| {
                    reason
                        .get("error")
                        .and_then(|error| error.get("message").or(Some(error)))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .or_else(|| text_field(event, &["error", "message", "detail"]))
                .unwrap_or_else(|| "run failed".to_string()),
        },
        other => RunTerminal::Failed {
            message: format!("run finished with unknown reason {other:?}"),
        },
    }
}

/// Find a text-ish field, flattening `{"message": "…"}` and `{"error": {…}}`
/// shapes so gateway error objects render as readable JSON-RPC messages.
fn text_field(event: &Value, names: &[&str]) -> Option<String> {
    for name in names {
        let Some(value) = event.get(*name) else {
            continue;
        };
        if let Some(text) = value.as_str() {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        } else if let Some(text) = value.get("message").and_then(Value::as_str) {
            return Some(text.to_string());
        } else if value.is_object() {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapper() -> UpdateMapper {
        UpdateMapper::new("acp-1", "run-1").with_gateway_session("gw-1")
    }

    fn updates(mapped: &MappedEvent) -> Vec<Value> {
        mapped
            .updates
            .iter()
            .map(|notification| notification["params"]["update"].clone())
            .collect()
    }

    #[test]
    fn text_event_becomes_agent_message_chunk() {
        let mut mapper = mapper();
        let mapped = mapper.map(&json!({"type": "text", "text": "bridge-probe"}));
        assert_eq!(
            updates(&mapped),
            vec![json!({
                "sessionUpdate": "agent_message_chunk",
                "messageId": "msg-run-1",
                "content": {"type": "text", "text": "bridge-probe"},
            })]
        );
        assert_eq!(mapped.terminal, None);
        // The notification envelope echoes the ACP session id.
        assert_eq!(mapped.updates[0]["params"]["sessionId"], json!("acp-1"));
    }

    #[test]
    fn consecutive_text_deltas_share_one_message_id() {
        let mut mapper = mapper();
        for delta in ["Hel", "lo"] {
            mapper.map(&json!({"type": "text", "text": delta}));
        }
        assert!(mapper.text_emitted);
    }

    #[test]
    fn empty_thinking_is_skipped_but_text_is_not() {
        let mut mapper = mapper();
        assert!(mapper
            .map(&json!({"type": "thinking", "text": ""}))
            .updates
            .is_empty());
        let mapped = mapper.map(&json!({"type": "thinking", "text": "hmm"}));
        assert_eq!(
            updates(&mapped),
            vec![json!({
                "sessionUpdate": "agent_thought_chunk",
                "messageId": "thought-run-1",
                "content": {"type": "text", "text": "hmm"},
            })]
        );
    }

    #[test]
    fn tool_call_matches_the_captured_acp_frame() {
        let mut mapper = mapper();
        let mapped = mapper.map(&json!({
            "type": "tool_call",
            "callId": "call_1",
            "tool": "bash",
            "input": {"description": "Run echo probe command", "command": "echo bridge-acp-probe"},
        }));
        assert_eq!(
            updates(&mapped),
            vec![json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "call_1",
                "title": "bash",
                "kind": "other",
                "status": "in_progress",
                "rawInput": {
                    "description": "Run echo probe command",
                    "command": "echo bridge-acp-probe",
                },
            })]
        );
    }

    #[test]
    fn tool_result_matches_the_captured_acp_frame() {
        let mut mapper = mapper();
        let mapped = mapper.map(&json!({
            "type": "tool_result",
            "callId": "call_1",
            "status": "completed",
            "result": "bridge-acp-probe\n",
        }));
        assert_eq!(
            updates(&mapped),
            vec![json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call_1",
                "status": "completed",
                "content": [{
                    "type": "content",
                    "content": {"type": "text", "text": "bridge-acp-probe\n"},
                }],
            })]
        );
    }

    #[test]
    fn failed_tool_result_is_marked_failed() {
        let mut mapper = mapper();
        let mapped = mapper.map(&json!({
            "type": "tool_result",
            "callId": "call_2",
            "status": "error",
            "result": "permission denied",
        }));
        assert_eq!(updates(&mapped)[0]["status"], json!("failed"));
    }

    #[test]
    fn session_event_emits_session_info_update_with_bridge_meta() {
        let mut mapper = mapper();
        let mapped = mapper.map(&json!({"type": "session", "sessionId": "native-1"}));
        assert_eq!(
            updates(&mapped),
            vec![json!({
                "sessionUpdate": "session_info_update",
                "_meta": {"bridge": {"gatewaySessionId": "native-1", "runId": "run-1"}},
            })]
        );
    }

    #[test]
    fn usage_update_requires_both_used_and_size() {
        let mut mapper = mapper();
        assert!(mapper
            .map(&json!({"type": "usage", "used": 100}))
            .updates
            .is_empty());
        let mapped = mapper.map(&json!({"type": "usage", "used": 6061, "size": 1048576}));
        assert_eq!(
            updates(&mapped),
            vec![json!({"sessionUpdate": "usage_update", "used": 6061, "size": 1048576})]
        );
    }

    #[test]
    fn status_events_are_bookkeeping_only() {
        let mut mapper = mapper();
        let mapped = mapper.map(&json!({"type": "status", "phase": "turn_start", "turn": 1}));
        assert!(mapped.updates.is_empty());
        assert_eq!(mapped.terminal, None);
    }

    #[test]
    fn final_completed_settles_end_turn_without_duplicating_text() {
        let mut mapper = mapper();
        mapper.map(&json!({"type": "text", "text": "done"}));
        let mapped = mapper.map(&json!({
            "type": "final",
            "reason": {"kind": "completed"},
            "text": "done",
        }));
        assert!(mapped.updates.is_empty(), "streamed text must not repeat");
        assert_eq!(mapped.terminal, Some(RunTerminal::Completed));
    }

    #[test]
    fn final_text_is_emitted_when_no_text_event_preceded_it() {
        let mut mapper = mapper();
        let mapped = mapper.map(&json!({
            "type": "final",
            "reason": {"kind": "completed"},
            "text": "only in final",
        }));
        assert_eq!(mapped.updates.len(), 1);
        assert_eq!(
            updates(&mapped)[0],
            json!({
                "sessionUpdate": "agent_message_chunk",
                "messageId": "msg-run-1",
                "content": {"type": "text", "text": "only in final"},
            })
        );
        assert_eq!(mapped.terminal, Some(RunTerminal::Completed));
    }

    #[test]
    fn final_error_carries_the_message() {
        let mut mapper = mapper();
        let mapped = mapper.map(&json!({
            "type": "final",
            "reason": {
                "kind": "error",
                "error": {"message": "ACCOUNT_SIGN_IN_REQUIRED", "code": "ACCOUNT_SIGN_IN_REQUIRED"},
            },
            "sessionId": "native-1",
        }));
        assert_eq!(
            mapped.terminal,
            Some(RunTerminal::Failed {
                message: "ACCOUNT_SIGN_IN_REQUIRED".to_string()
            })
        );
    }

    #[test]
    fn final_cancelled_settles_cancelled() {
        let mut mapper = mapper();
        assert_eq!(
            mapper
                .map(&json!({"type": "final", "reason": {"kind": "cancelled"}}))
                .terminal,
            Some(RunTerminal::Cancelled)
        );
    }

    #[test]
    fn end_event_maps_status() {
        let mut mapper = mapper();
        assert_eq!(
            mapper
                .map(&json!({"type": "end", "status": "done"}))
                .terminal,
            Some(RunTerminal::Completed)
        );
        assert_eq!(
            mapper
                .map(&json!({"type": "end", "status": "failed", "error": "boom"}))
                .terminal,
            Some(RunTerminal::Failed {
                message: "boom".to_string()
            })
        );
        assert_eq!(
            mapper
                .map(&json!({"type": "end", "status": "cancelled"}))
                .terminal,
            Some(RunTerminal::Cancelled)
        );
        assert_eq!(
            mapper
                .map(&json!({"type": "end", "status": "running"}))
                .terminal,
            None
        );
    }

    #[test]
    fn worker_frame_aliases_are_accepted() {
        let mut mapper = mapper();
        let mapped = mapper.map(&json!({
            "type": "assistant_delta",
            "delta": "hel",
        }));
        assert_eq!(updates(&mapped)[0]["content"]["text"], json!("hel"));

        let mapped = mapper.map(&json!({
            "type": "tool_call_started",
            "id": "call_9",
            "name": "bash",
            "arguments": {"command": "ls"},
        }));
        assert_eq!(updates(&mapped)[0]["toolCallId"], json!("call_9"));
        assert_eq!(updates(&mapped)[0]["rawInput"], json!({"command": "ls"}));

        let mapped = mapper.map(&json!({
            "type": "tool_call_finished",
            "id": "call_9",
            "result": "ok",
        }));
        assert_eq!(updates(&mapped)[0]["status"], json!("completed"));
    }

    #[test]
    fn unknown_events_are_ignored_for_forward_compatibility() {
        let mut mapper = mapper();
        let mapped = mapper.map(&json!({"type": "brand_new_event", "x": 1}));
        assert!(mapped.updates.is_empty());
        assert_eq!(mapped.terminal, None);
    }

    #[test]
    fn gateway_settle_event_settles_the_turn() {
        let mut mapper = mapper();
        assert_eq!(
            mapper
                .map(&json!({"type": "gateway", "status": "done"}))
                .terminal,
            Some(RunTerminal::Completed)
        );
        assert_eq!(
            mapper
                .map(&json!({"type": "gateway", "status": "failed", "reason": "watchdog"}))
                .terminal,
            Some(RunTerminal::Failed {
                message: "watchdog".to_string()
            })
        );
    }
}
