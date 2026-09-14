use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use futures::{Stream, StreamExt};

use crate::harness::{
    arc_to_sender_ref, command_candidates, execute_tool_call, harness_tool_definitions,
    ChatToolCall, HarnessEvent, ThinkingMode, ToolCallStatus, ToolTurnContext, WebSearchConfig,
    COMMAND_TOOL, HARNESS_SYSTEM_PROMPT,
};
use crate::llm::{
    CompletionRequest, CompletionStreamEvent, LlmProvider, LlmToolCall, Message, Role,
};
use crate::mcp_manager::McpManager;
use crate::protocol::DesktopMessage;
use crate::store::Store;
use crate::subagent_run::SubAgentHost;
use crate::worker::WorkerId;

use super::classify::{extract_ask_user, is_orchestration_goal};
use super::history::build_history;
use super::persist::{
    load_or_create_mission, persist_mission, persist_reasoning_step, persist_tool_call_records,
};
use super::prompts::{build_execution_prompt, build_reasoning_prompt};
use super::tools::build_reasoning_tools;
use super::types::{ReasoningDecision, ReasoningStep, MAX_EXECUTION_STEPS, MAX_REASONING_STEPS};

fn parse_reasoning_tool_calls(
    tool_calls: &[LlmToolCall],
    mode: &mut ThinkingMode,
) -> (ReasoningDecision, Vec<String>) {
    let mut decision = ReasoningDecision::Continue;
    let mut focus_notes = Vec::new();

    for call in tool_calls {
        match call.name.as_str() {
            "set_thinking_mode" => {
                if let Some(m) = call.arguments["mode"].as_str() {
                    if let Ok(new_mode) = m.parse() {
                        *mode = new_mode;
                    }
                }
            }
            "continue_thinking" => {
                decision = ReasoningDecision::Continue;
                if let Some(focus) = call.arguments["focus"].as_str() {
                    focus_notes.push(focus.to_string());
                }
            }
            "execute" => {
                decision = ReasoningDecision::Execute;
            }
            "ask_user" => {
                decision = ReasoningDecision::AskUser;
            }
            "create_mission" | "update_mission" => {
                decision = ReasoningDecision::Continue;
            }
            _ => {}
        }
    }

    (decision, focus_notes)
}

/// Run one harness turn. Crate-private: it takes the crate-private
/// [`SubAgentHost`] a `spawn_subagent` call registers its child with, and only
/// `Harness::run_turn` drives it.
pub(crate) fn run_mission_turn(
    store: Store,
    runner: Arc<dyn crate::harness::CommandRunner>,
    llm: Arc<dyn LlmProvider>,
    deploy_sender: Option<Arc<dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync>>,
    mcp_manager: McpManager,
    cancel: Arc<AtomicBool>,
    workspace_dir: std::path::PathBuf,
    docs_dir: std::path::PathBuf,
    chat_id: String,
    prompt: String,
    provider: String,
    model: String,
    reasoning_enabled: bool,
    auto_approve: bool,
    web_search: WebSearchConfig,
    subagents: SubAgentHost,
) -> Pin<Box<dyn Stream<Item = HarnessEvent> + Send>> {
    Box::pin(async_stream::stream! {
        let now = Utc::now().to_rfc3339();
        if let Err(e) = store.insert_chat_message(
            &uuid::Uuid::new_v4().to_string(),
            &chat_id,
            "user",
            &prompt,
            None,
            &now,
        ) {
            yield HarnessEvent::Error(e.to_string());
            return;
        }

        let mut mission = match load_or_create_mission(&store, &chat_id, &prompt) {
            Ok(m) => m,
            Err(e) => {
                yield HarnessEvent::Error(e.to_string());
                return;
            }
        };

        yield HarnessEvent::MissionUpdated {
            mission_id: mission.id.clone(),
            status: mission.status.clone(),
        };

        let mut mode = ThinkingMode::Direct;
        if !mission.reasoning_steps.is_empty() {
            mode = mission.reasoning_steps.last().unwrap().mode;
        }

        let mut tools = harness_tool_definitions();
        if let Ok(mcp_tools) = mcp_manager.refresh_from_store(&store) {
            tools.extend(mcp_tools);
        }
        tools.push(McpManager::generic_mcp_call_tool());

        let reasoning_tools = build_reasoning_tools();
        let enable_reasoning = reasoning_enabled && (is_orchestration_goal(&prompt) || !mission.reasoning_steps.is_empty() || mission.pending_ask.is_some());

        let mut reasoning_step = 0usize;
        let mut pending_decision = ReasoningDecision::Execute;

        if enable_reasoning {
            while reasoning_step < MAX_REASONING_STEPS {
                if cancel.load(Ordering::Relaxed) {
                    yield HarnessEvent::Error("cancelled".to_string());
                    return;
                }

                let history = build_history(&store, &chat_id, &mission, &reasoning_tools, &tools, mode).await;
                let request = CompletionRequest::new(provider.clone(), model.clone())
                    .with_system(HARNESS_SYSTEM_PROMPT)
                    .with_system(build_reasoning_prompt(mode, &mission.goal, &mission.reasoning_steps))
                    .with_messages(history)
                    .with_tools(reasoning_tools.clone());

                yield HarnessEvent::ReasoningStarted {
                    step: reasoning_step,
                    mode: mode.as_str().to_string(),
                };

                let mut stream = match llm.complete_stream(request).await {
                    Ok(s) => s,
                    Err(e) => {
                        yield HarnessEvent::Error(e.to_string());
                        return;
                    }
                };

                let mut content = String::new();
                let mut tool_calls = Vec::new();
                while let Some(event) = stream.next().await {
                    if cancel.load(Ordering::Relaxed) {
                        yield HarnessEvent::Error("cancelled".to_string());
                        return;
                    }
                    match event {
                        CompletionStreamEvent::AssistantDelta(delta) => {
                            content.push_str(&delta);
                            yield HarnessEvent::ReasoningDelta(delta);
                        }
                        CompletionStreamEvent::ToolCalls(calls) => {
                            tool_calls = calls;
                        }
                        CompletionStreamEvent::Usage(usage) => {
                            yield HarnessEvent::TokenUsage {
                                chat_id: chat_id.clone(),
                                input: usage.input,
                                cached: usage.cached,
                                output: usage.output,
                            };
                        }
                        CompletionStreamEvent::Done => break,
                        CompletionStreamEvent::Error(message) => {
                            yield HarnessEvent::Error(message);
                            return;
                        }
                    }
                }

                let (decision, _focus) = parse_reasoning_tool_calls(&tool_calls, &mut mode);
                let step = ReasoningStep {
                    step: reasoning_step,
                    mode,
                    content: content.clone(),
                    decision: decision.clone(),
                    tool_calls: tool_calls.clone(),
                };
                if let Err(e) = persist_reasoning_step(&store, &mission.id, &step) {
                    yield HarnessEvent::Error(e.to_string());
                    return;
                }
                mission.reasoning_steps.push(step);

                yield HarnessEvent::ReasoningDone {
                    step: reasoning_step,
                    mode: mode.as_str().to_string(),
                    content: content.clone(),
                    decision: serde_json::to_string(&decision).unwrap_or_default(),
                };

                if decision == ReasoningDecision::Execute || decision == ReasoningDecision::Done {
                    pending_decision = decision;
                    break;
                }

                if decision == ReasoningDecision::AskUser {
                    if let Some(ask) = extract_ask_user(&tool_calls) {
                        if auto_approve {
                            // Auto-approve: skip suspending on the question and
                            // record a synthetic answer so the next reasoning /
                            // execution step sees the user already responded.
                            let now = Utc::now().to_rfc3339();
                            if let Err(e) = store.insert_chat_message(
                                &uuid::Uuid::new_v4().to_string(),
                                &chat_id,
                                "user",
                                &format!("(auto-approved; no answer to: {})", ask.question),
                                None,
                                &now,
                            ) {
                                yield HarnessEvent::Error(e.to_string());
                                return;
                            }
                        } else {
                            let now = Utc::now().to_rfc3339();
                            if let Err(e) = store.insert_pending_ask(
                                &ask.id,
                                &chat_id,
                                Some(&mission.id),
                                &ask.question,
                                &ask.quick_replies.join("\n"),
                                "pending",
                                &now,
                                &now,
                            ) {
                                yield HarnessEvent::Error(e.to_string());
                                return;
                            }
                            mission.pending_ask = Some(ask.clone());
                            yield HarnessEvent::AskUser {
                                question: ask.question,
                                quick_replies: ask.quick_replies,
                            };
                            return;
                        }
                    }
                }

                reasoning_step += 1;
                if reasoning_step >= MAX_REASONING_STEPS {
                    pending_decision = ReasoningDecision::Execute;
                    break;
                }
            }

            if pending_decision == ReasoningDecision::Done {
                yield HarnessEvent::Done;
                return;
            }
        } // end if enable_reasoning

        // Execution phase
        // The main agent's conversation sits at depth 0 of the sub-agent tree;
        // a child it spawns runs deeper (S2).
        let turn_ctx = ToolTurnContext {
            chat_id: &chat_id,
            provider: &provider,
            model: &model,
            depth: 0,
            host: &subagents,
        };
        let mut execution_iteration = 0;
        let mut prev_tool_calls: Vec<LlmToolCall> = Vec::new();

        loop {
            if cancel.load(Ordering::Relaxed) {
                yield HarnessEvent::Error("cancelled".to_string());
                return;
            }
            if execution_iteration >= MAX_EXECUTION_STEPS {
                yield HarnessEvent::Error("too many execution iterations".to_string());
                return;
            }
            execution_iteration += 1;

            let mut history = build_history(&store, &chat_id, &mission, &reasoning_tools, &tools, mode).await;
            history.push(Message {
                role: Role::System,
                content: build_execution_prompt(&mission.goal, &mission.reasoning_steps),
                tool_calls: None,
                tool_call_id: None,
                tool_status: None,
            });

            let request = CompletionRequest::new(provider.clone(), model.clone())
                .with_system(HARNESS_SYSTEM_PROMPT)
                .with_messages(history)
                .with_tools(tools.clone());

            let mut stream = match llm.complete_stream(request).await {
                Ok(s) => s,
                Err(e) => {
                    yield HarnessEvent::Error(e.to_string());
                    return;
                }
            };

            let mut assistant_content = String::new();
            let mut tool_calls = Vec::new();
            // Stream the assistant reply into a single chat message row as deltas
            // arrive, so the renderer can show it progressively instead of only at
            // the end of the turn. The message id is kept to attach tool-call
            // metadata once the stream finishes.
            let mut assistant_msg_id: Option<String> = None;
            while let Some(event) = stream.next().await {
                if cancel.load(Ordering::Relaxed) {
                    yield HarnessEvent::Error("cancelled".to_string());
                    break;
                }
                match event {
                    CompletionStreamEvent::AssistantDelta(delta) => {
                        assistant_content.push_str(&delta);
                        match &assistant_msg_id {
                            Some(id) => {
                                if let Err(e) = store.append_chat_message_content(id, &delta) {
                                    yield HarnessEvent::Error(e.to_string());
                                    return;
                                }
                            }
                            None => {
                                let id = uuid::Uuid::new_v4().to_string();
                                if let Err(e) = store.insert_chat_message(
                                    &id,
                                    &chat_id,
                                    "assistant",
                                    &delta,
                                    None,
                                    &Utc::now().to_rfc3339(),
                                ) {
                                    yield HarnessEvent::Error(e.to_string());
                                    return;
                                }
                                assistant_msg_id = Some(id);
                            }
                        }
                        yield HarnessEvent::AssistantDelta(delta);
                    }
                    CompletionStreamEvent::ToolCalls(calls) => {
                        tool_calls = calls;
                    }
                    CompletionStreamEvent::Usage(usage) => {
                        yield HarnessEvent::TokenUsage {
                            chat_id: chat_id.clone(),
                            input: usage.input,
                            cached: usage.cached,
                            output: usage.output,
                        };
                    }
                    CompletionStreamEvent::Done => break,
                    CompletionStreamEvent::Error(message) => {
                        yield HarnessEvent::Error(message);
                        return;
                    }
                }
            }

            if tool_calls.is_empty() {
                break;
            }
            if assistant_content.is_empty() {
                if tool_calls == prev_tool_calls {
                    break;
                }
                prev_tool_calls = tool_calls.clone();
            }

            // Attach tool-call metadata to the streamed message, or create the
            // tool-call-only assistant message, so the next iteration's history
            // carries the calls. The records carry a lifecycle status that is
            // rewritten as each call runs, so a call is readable from the store
            // while the turn is still in flight instead of only once it ends.
            let mut call_records: Vec<ChatToolCall> =
                tool_calls.iter().map(ChatToolCall::planned).collect();
            let tool_calls_json = serde_json::to_string(&call_records).unwrap_or_default();
            match &assistant_msg_id {
                Some(id) => {
                    if let Err(e) = store.set_chat_message_tool_calls(id, &tool_calls_json) {
                        yield HarnessEvent::Error(e.to_string());
                        return;
                    }
                }
                None => {
                    let id = uuid::Uuid::new_v4().to_string();
                    if let Err(e) = store.insert_chat_message(
                        &id,
                        &chat_id,
                        "assistant",
                        "",
                        Some(&tool_calls_json),
                        &Utc::now().to_rfc3339(),
                    ) {
                        yield HarnessEvent::Error(e.to_string());
                        return;
                    }
                    assistant_msg_id = Some(id);
                }
            }

            for (index, call) in tool_calls.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    yield HarnessEvent::Error("cancelled".to_string());
                    break;
                }
                call_records[index].status = ToolCallStatus::Running;
                if let Some(id) = &assistant_msg_id {
                    if let Err(e) = persist_tool_call_records(&store, id, &call_records) {
                        yield HarnessEvent::Error(e.to_string());
                        return;
                    }
                }
                yield HarnessEvent::ToolCallStarted {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                };

                // A command tool suspends here, before it runs, until the user
                // approves, edits or rejects it. `auto_approve` is the single
                // switch that skips the gate and runs the command immediately.
                if !auto_approve && call.name == COMMAND_TOOL {
                    let candidates = command_candidates(call);
                    let cwd = workspace_dir.display().to_string();
                    let now = Utc::now().to_rfc3339();
                    let candidates_json = match serde_json::to_string(&candidates) {
                        Ok(json) => json,
                        Err(e) => {
                            yield HarnessEvent::Error(e.to_string());
                            return;
                        }
                    };
                    let message_id = assistant_msg_id.clone().unwrap_or_default();
                    if let Err(e) = store.insert_pending_command(
                        &call.id,
                        &chat_id,
                        &message_id,
                        &candidates_json,
                        &cwd,
                        "pending",
                        &now,
                        &now,
                    ) {
                        yield HarnessEvent::Error(e.to_string());
                        return;
                    }
                    yield HarnessEvent::CommandProposed {
                        id: call.id.clone(),
                        candidates,
                        cwd,
                    };
                    return;
                }

                let sender_ref = deploy_sender.as_ref().map(|f| arc_to_sender_ref(f));
                let result = execute_tool_call(&store, &*runner, sender_ref, &mcp_manager, &workspace_dir, &docs_dir, call, &web_search, &turn_ctx).await;
                match result {
                    Ok(value) => {
                        let tool_result_text = format!("{}\n{}", call.id, value);
                        if let Err(e) = store.insert_chat_message(
                            &uuid::Uuid::new_v4().to_string(),
                            &chat_id,
                            "tool",
                            &tool_result_text,
                            None,
                            &Utc::now().to_rfc3339(),
                        ) {
                            yield HarnessEvent::Error(e.to_string());
                            return;
                        }
                        call_records[index].status = ToolCallStatus::Finished;
                        call_records[index].result = Some(value.clone());
                        if let Some(id) = &assistant_msg_id {
                            if let Err(e) = persist_tool_call_records(&store, id, &call_records) {
                                yield HarnessEvent::Error(e.to_string());
                                return;
                            }
                        }
                        yield HarnessEvent::ToolCallFinished { id: call.id.clone(), result: value };
                    }
                    Err(e) => {
                        // The row carries the call id and the message only: the
                        // outcome is the call's `status`, persisted below, not a
                        // prefix in this text.
                        let error_text = format!("{}\n{}", call.id, e);
                        if let Err(e2) = store.insert_chat_message(
                            &uuid::Uuid::new_v4().to_string(),
                            &chat_id,
                            "tool",
                            &error_text,
                            None,
                            &Utc::now().to_rfc3339(),
                        ) {
                            yield HarnessEvent::Error(e2.to_string());
                            return;
                        }
                        call_records[index].status = ToolCallStatus::Error;
                        call_records[index].result = Some(e.to_string());
                        if let Some(id) = &assistant_msg_id {
                            if let Err(e2) = persist_tool_call_records(&store, id, &call_records) {
                                yield HarnessEvent::Error(e2.to_string());
                                return;
                            }
                        }
                        yield HarnessEvent::ToolCallError { id: call.id.clone(), message: e.to_string() };
                    }
                }
            }
        }

        mission.status = "done".to_string();
        if let Err(e) = persist_mission(&store, &mission) {
            yield HarnessEvent::Error(e.to_string());
            return;
        }
        yield HarnessEvent::MissionUpdated {
            mission_id: mission.id.clone(),
            status: mission.status.clone(),
        };
        yield HarnessEvent::Done;
    })
}
